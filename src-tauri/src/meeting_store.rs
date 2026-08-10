use std::{
    fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const SCHEMA_VERSION: u8 = 1;
const MEETING_FILE: &str = "meeting.json";
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeetingDocument {
    schema_version: u8,
    id: String,
    created_at: String,
    updated_at: String,
    title: String,
    audio: MeetingAudio,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeetingAudio {
    content_sha256: String,
    file_name: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalMeetingState {
    schema_version: u8,
    meeting_id: String,
    audio_path: PathBuf,
    size_bytes: u64,
    modified_at_unix_ms: u64,
    linked_at: String,
    last_seen_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredMeetingSummary {
    pub(crate) meeting_id: String,
    pub(crate) title: String,
    pub(crate) file_name: String,
    pub(crate) size_bytes: u64,
    pub(crate) updated_at_unix_ms: u64,
    pub(crate) audio_available: bool,
}

pub(crate) fn resolve_or_create(app: &AppHandle, audio_path: &Path) -> Result<String, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingの保存処理を開始できませんでした。".to_string())?;
    let root = meetings_directory(app)?;
    let local_root = local_meetings_directory(app)?;
    resolve_or_create_in(&root, &local_root, audio_path)
}

fn resolve_or_create_in(
    root: &Path,
    local_root: &Path,
    audio_path: &Path,
) -> Result<String, String> {
    let canonical = canonical_audio_path(audio_path)?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("音声ファイルの情報を取得できませんでした: {error}"))?;
    let modified_at_unix_ms = modified_at_unix_ms(&metadata);
    fs::create_dir_all(root)
        .map_err(|error| format!("Meetingの保存先を作成できませんでした: {error}"))?;
    fs::create_dir_all(local_root)
        .map_err(|error| format!("Meetingのローカル保存先を作成できませんでした: {error}"))?;

    if let Some(document) = find_by_local_path(
        root,
        local_root,
        &canonical,
        metadata.len(),
        modified_at_unix_ms,
    )? {
        touch_local_state(local_root, &document.id, &canonical, &metadata)?;
        return Ok(document.id);
    }

    let content_sha256 = content_sha256(&canonical)?;
    if let Some(document) = find_by_content_hash(root, &content_sha256, metadata.len())? {
        touch_local_state(local_root, &document.id, &canonical, &metadata)?;
        return Ok(document.id);
    }

    let id = Uuid::now_v7().to_string();
    create_document(root, local_root, &id, &canonical, &metadata, content_sha256)?;
    Ok(id)
}

pub(crate) fn link_existing(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingの保存処理を開始できませんでした。".to_string())?;
    validate_meeting_id(meeting_id)?;
    let canonical = canonical_audio_path(audio_path)?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("音声ファイルの情報を取得できませんでした: {error}"))?;
    let root = meetings_directory(app)?;
    let local_root = local_meetings_directory(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("Meetingの保存先を作成できませんでした: {error}"))?;
    let document_path = meeting_directory_in(&root, meeting_id)?.join(MEETING_FILE);
    let hash = content_sha256(&canonical)?;
    if document_path.exists() {
        let document: MeetingDocument = read_json(&document_path)?;
        if document.audio.content_sha256 != hash || document.audio.size_bytes != metadata.len() {
            return Err("選択した音声はこのMeetingに登録された音声と一致しません。".into());
        }
        touch_local_state(&local_root, meeting_id, &canonical, &metadata)
    } else {
        create_document(&root, &local_root, meeting_id, &canonical, &metadata, hash)
    }
}

pub(crate) fn meeting_directory(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    meeting_directory_in(&meetings_directory(app)?, meeting_id)
}

pub(crate) fn local_audio_path(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    validate_meeting_id(meeting_id)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingのローカル情報を確認できませんでした。".to_string())?;
    let primary = local_meetings_directory(app)?.join(format!("{meeting_id}.json"));
    let backup = primary.with_extension("json.backup");
    let path = if primary.exists() {
        primary
    } else if backup.exists() {
        backup
    } else {
        return Err(
            "この端末にはMeetingの音声ファイルがありません。音声を選び直してください。".into(),
        );
    };
    let local: LocalMeetingState = read_json(&path)?;
    if local.schema_version != SCHEMA_VERSION || local.meeting_id != meeting_id {
        return Err("Meetingのローカル情報が一致しません。".into());
    }
    let canonical = canonical_audio_path(&local.audio_path)?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("Meetingの音声ファイルを確認できませんでした: {error}"))?;
    if metadata.len() != local.size_bytes
        || modified_at_unix_ms(&metadata) != local.modified_at_unix_ms
    {
        return Err("Meetingの音声ファイルが変更されています。音声を選び直してください。".into());
    }
    Ok(canonical)
}

pub(crate) fn list_stored_meetings(app: &AppHandle) -> Result<Vec<StoredMeetingSummary>, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meeting一覧の確認を開始できませんでした。".to_string())?;
    list_stored_meetings_in(&meetings_directory(app)?, &local_meetings_directory(app)?)
}

pub(crate) fn mark_updated(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingの更新処理を開始できませんでした。".to_string())?;
    validate_meeting_id(meeting_id)?;
    let path = meeting_directory_in(&meetings_directory(app)?, meeting_id)?.join(MEETING_FILE);
    let mut document: MeetingDocument = read_json(&path)?;
    validate_document(&document)?;
    document.updated_at = chrono::Utc::now().to_rfc3339();
    write_json_atomic(&path, &document)
}

pub(crate) fn rename_audio_metadata(
    app: &AppHandle,
    meeting_id: &str,
    file_name: &str,
) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meeting名の更新処理を開始できませんでした。".to_string())?;
    validate_meeting_id(meeting_id)?;
    let path = meeting_directory_in(&meetings_directory(app)?, meeting_id)?.join(MEETING_FILE);
    let mut document: MeetingDocument = read_json(&path)?;
    validate_document(&document)?;
    document.title = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .to_string();
    document.audio.file_name = file_name.to_string();
    document.updated_at = chrono::Utc::now().to_rfc3339();
    write_json_atomic(&path, &document)
}

pub(crate) fn detach_audio(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingの音声情報を削除できませんでした。".to_string())?;
    validate_meeting_id(meeting_id)?;
    remove_local_state_in(&local_meetings_directory(app)?, meeting_id)
}

pub(crate) fn delete_meeting(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Meetingの削除処理を開始できませんでした。".to_string())?;
    validate_meeting_id(meeting_id)?;
    delete_meeting_in(
        &meetings_directory(app)?,
        &local_meetings_directory(app)?,
        meeting_id,
    )
}

fn delete_meeting_in(root: &Path, local_root: &Path, meeting_id: &str) -> Result<(), String> {
    validate_meeting_id(meeting_id)?;
    let directory = meeting_directory_in(root, meeting_id)?;
    if directory.exists() {
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|error| format!("Meetingの保存情報を確認できませんでした: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Meetingの保存先を安全に削除できませんでした。".into());
        }
        fs::remove_dir_all(&directory)
            .map_err(|error| format!("Meetingの関連データを削除できませんでした: {error}"))?;
    }
    remove_local_state_in(local_root, meeting_id)
}

pub(crate) fn meetings_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("meetings"))
        .map_err(|error| format!("Meetingの保存先を取得できませんでした: {error}"))
}

fn local_meetings_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("local").join("meetings"))
        .map_err(|error| format!("Meetingのローカル保存先を取得できませんでした: {error}"))
}

fn remove_local_state_in(local_root: &Path, meeting_id: &str) -> Result<(), String> {
    for path in [
        local_root.join(format!("{meeting_id}.json")),
        local_root.join(format!("{meeting_id}.json.backup")),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Meetingの音声リンクを削除できませんでした: {error}"
                ))
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_meeting_id(meeting_id: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(meeting_id).map_err(|_| "Meeting IDが不正です。".to_string())?;
    if parsed.get_version_num() != 7 {
        return Err("Meeting IDの形式に対応していません。".into());
    }
    Ok(())
}

fn create_document(
    root: &Path,
    local_root: &Path,
    meeting_id: &str,
    audio_path: &Path,
    metadata: &fs::Metadata,
    content_sha256: String,
) -> Result<(), String> {
    validate_meeting_id(meeting_id)?;
    let now = chrono::Utc::now().to_rfc3339();
    let file_name = audio_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio")
        .to_string();
    let title = audio_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Meeting")
        .to_string();
    let directory = meeting_directory_in(root, meeting_id)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Meetingの保存先を作成できませんでした: {error}"))?;
    let document = MeetingDocument {
        schema_version: SCHEMA_VERSION,
        id: meeting_id.to_string(),
        created_at: now.clone(),
        updated_at: now,
        title,
        audio: MeetingAudio {
            content_sha256,
            file_name,
            size_bytes: metadata.len(),
        },
    };
    write_json_atomic(&directory.join(MEETING_FILE), &document)?;
    touch_local_state(local_root, meeting_id, audio_path, metadata)
}

fn touch_local_state(
    local_root: &Path,
    meeting_id: &str,
    audio_path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), String> {
    validate_meeting_id(meeting_id)?;
    fs::create_dir_all(local_root)
        .map_err(|error| format!("Meetingのローカル情報を作成できませんでした: {error}"))?;
    let path = local_root.join(format!("{meeting_id}.json"));
    let now = chrono::Utc::now().to_rfc3339();
    let linked_at = read_json::<LocalMeetingState>(&path)
        .ok()
        .filter(|state| state.meeting_id == meeting_id)
        .map_or_else(|| now.clone(), |state| state.linked_at);
    write_json_atomic(
        &path,
        &LocalMeetingState {
            schema_version: SCHEMA_VERSION,
            meeting_id: meeting_id.to_string(),
            audio_path: audio_path.to_path_buf(),
            size_bytes: metadata.len(),
            modified_at_unix_ms: modified_at_unix_ms(metadata),
            linked_at,
            last_seen_at: now,
        },
    )
}

fn find_by_local_path(
    root: &Path,
    local_root: &Path,
    audio_path: &Path,
    size_bytes: u64,
    modified_at_unix_ms: u64,
) -> Result<Option<MeetingDocument>, String> {
    let entries = match fs::read_dir(local_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Meetingのローカル一覧を読み込めませんでした: {error}"
            ))
        }
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(local) = read_json::<LocalMeetingState>(&entry.path()) else {
            continue;
        };
        if local.audio_path == audio_path
            && local.size_bytes == size_bytes
            && local.modified_at_unix_ms == modified_at_unix_ms
        {
            let document: MeetingDocument =
                read_json(&meeting_directory_in(root, &local.meeting_id)?.join(MEETING_FILE))?;
            validate_document(&document)?;
            return Ok(Some(document));
        }
    }
    Ok(None)
}

fn find_by_content_hash(
    root: &Path,
    content_sha256: &str,
    size_bytes: u64,
) -> Result<Option<MeetingDocument>, String> {
    for directory in meeting_directories(root)? {
        let Ok(document) = read_json::<MeetingDocument>(&directory.join(MEETING_FILE)) else {
            continue;
        };
        validate_document(&document)?;
        if document.audio.content_sha256 == content_sha256
            && document.audio.size_bytes == size_bytes
        {
            return Ok(Some(document));
        }
    }
    Ok(None)
}

fn meeting_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Meeting一覧を読み込めませんでした: {error}")),
    };
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect())
}

fn list_stored_meetings_in(
    root: &Path,
    local_root: &Path,
) -> Result<Vec<StoredMeetingSummary>, String> {
    let mut meetings = Vec::new();
    for directory in meeting_directories(root)? {
        let document_path = directory.join(MEETING_FILE);
        let Ok(document) = read_json::<MeetingDocument>(&document_path) else {
            continue;
        };
        if validate_document(&document).is_err() {
            continue;
        }
        let local =
            read_json::<LocalMeetingState>(&local_root.join(format!("{}.json", document.id)))
                .ok()
                .filter(|state| {
                    state.schema_version == SCHEMA_VERSION && state.meeting_id == document.id
                });
        let audio_available = local.as_ref().is_some_and(|state| {
            fs::metadata(&state.audio_path).is_ok_and(|metadata| {
                metadata.is_file()
                    && metadata.len() == state.size_bytes
                    && modified_at_unix_ms(&metadata) == state.modified_at_unix_ms
            })
        });
        meetings.push(StoredMeetingSummary {
            meeting_id: document.id,
            title: document.title,
            file_name: document.audio.file_name,
            size_bytes: document.audio.size_bytes,
            updated_at_unix_ms: rfc3339_unix_ms(&document.updated_at),
            audio_available,
        });
    }
    meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.updated_at_unix_ms));
    Ok(meetings)
}

fn rfc3339_unix_ms(value: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|timestamp| timestamp.timestamp_millis().try_into().ok())
        .unwrap_or(0)
}

fn validate_document(document: &MeetingDocument) -> Result<(), String> {
    if document.schema_version != SCHEMA_VERSION {
        return Err("保存済みMeetingの形式に対応していません。".into());
    }
    validate_meeting_id(&document.id)
}

fn meeting_directory_in(root: &Path, meeting_id: &str) -> Result<PathBuf, String> {
    validate_meeting_id(meeting_id)?;
    Ok(root.join(meeting_id))
}

fn canonical_audio_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("音声ファイルのローカルパスを確認できませんでした: {error}"))
}

fn modified_at_unix_ms(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_millis().try_into().ok())
        .unwrap_or(0)
}

fn content_sha256(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("音声ファイルの内容識別子を作成できませんでした: {error}"))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("音声ファイルの内容を確認できませんでした: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Meeting情報を読み込めませんでした: {error}"))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("Meeting情報が壊れています: {error}"))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Meetingの保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Meetingの保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::now_v7()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Meeting情報を書き込めませんでした: {error}"))?;
    if let Err(error) = serde_json::to_writer_pretty(&mut file, value)
        .and_then(|_| file.flush().map_err(serde_json::Error::io))
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!("Meeting情報をJSONへ変換できませんでした: {error}"));
    }
    file.sync_all()
        .map_err(|error| format!("Meeting情報を安全に書き込めませんでした: {error}"))?;
    drop(file);
    let backup = path.with_extension("json.backup");
    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("古いMeeting情報を削除できませんでした: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Meeting情報を更新用に退避できませんでした: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Meeting情報の保存を確定できませんでした: {error}"));
    }
    if backup.exists() {
        fs::remove_file(backup)
            .map_err(|error| format!("Meeting情報のバックアップを削除できませんでした: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        delete_meeting_in, list_stored_meetings_in, meeting_directory_in, read_json,
        remove_local_state_in, resolve_or_create_in, validate_meeting_id, LocalMeetingState,
    };

    #[test]
    fn uuid_v7_is_required_for_meeting_paths() {
        let id = uuid::Uuid::now_v7().to_string();
        assert!(validate_meeting_id(&id).is_ok());
        assert!(validate_meeting_id("../meeting").is_err());
        assert!(meeting_directory_in(std::path::Path::new("meetings"), &id).is_ok());
    }

    #[test]
    fn meeting_id_and_local_link_survive_rename() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-meeting-content-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let audio_root = root.join("audio");
        let meetings = root.join("meetings");
        let local = root.join("local").join("meetings");
        std::fs::create_dir_all(&audio_root).expect("create fixture");
        let first = audio_root.join("first.m4a");
        let renamed = audio_root.join("renamed.m4a");
        std::fs::write(&first, b"same audio bytes").expect("write fixture");
        let meeting_id = resolve_or_create_in(&meetings, &local, &first).expect("create meeting");
        let sync_document = std::fs::read_to_string(
            meeting_directory_in(&meetings, &meeting_id)
                .expect("meeting directory")
                .join("meeting.json"),
        )
        .expect("read sync document");
        assert!(!sync_document.contains("audioPath"));
        std::fs::rename(&first, &renamed).expect("rename fixture");
        assert_eq!(
            meeting_id,
            resolve_or_create_in(&meetings, &local, &renamed).expect("resolve renamed meeting")
        );
        let local_state: LocalMeetingState =
            read_json(&local.join(format!("{meeting_id}.json"))).expect("read local link");
        assert_eq!(
            local_state.audio_path,
            std::fs::canonicalize(&renamed).expect("canonical renamed path")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn audio_link_can_be_removed_without_deleting_transcripts() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-meeting-delete-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let audio_root = root.join("audio");
        let meetings = root.join("meetings");
        let local = root.join("local").join("meetings");
        std::fs::create_dir_all(&audio_root).expect("create fixture");
        let audio = audio_root.join("meeting.m4a");
        std::fs::write(&audio, b"audio bytes").expect("write audio fixture");
        let meeting_id =
            resolve_or_create_in(&meetings, &local, &audio).expect("create meeting fixture");
        let transcripts = meetings.join(&meeting_id).join("transcripts");
        std::fs::create_dir_all(&transcripts).expect("create transcript directory");
        std::fs::write(transcripts.join("index.json"), b"{}").expect("write transcript fixture");

        remove_local_state_in(&local, &meeting_id).expect("detach audio");

        assert!(!local.join(format!("{meeting_id}.json")).exists());
        assert!(transcripts.join("index.json").exists());
        assert!(meetings.join(&meeting_id).join("meeting.json").exists());

        delete_meeting_in(&meetings, &local, &meeting_id).expect("delete complete meeting");
        assert!(!meetings.join(&meeting_id).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lists_stored_meetings_with_local_audio_availability() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-meeting-list-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let audio_root = root.join("audio");
        let meetings = root.join("meetings");
        let local = root.join("local").join("meetings");
        std::fs::create_dir_all(&audio_root).expect("create fixture");
        let audio = audio_root.join("imported.wav");
        std::fs::write(&audio, b"audio bytes").expect("write fixture");
        let meeting_id =
            resolve_or_create_in(&meetings, &local, &audio).expect("create imported meeting");

        let listed = list_stored_meetings_in(&meetings, &local).expect("list meetings");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].meeting_id, meeting_id);
        assert_eq!(listed[0].file_name, "imported.wav");
        assert!(listed[0].audio_available);

        std::fs::remove_file(&audio).expect("remove linked audio");
        let missing = list_stored_meetings_in(&meetings, &local).expect("list missing audio");
        assert!(!missing[0].audio_available);
        let _ = std::fs::remove_dir_all(root);
    }
}
