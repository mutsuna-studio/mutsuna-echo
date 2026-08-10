use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use chrono::Local;
use tauri::{AppHandle, Manager};

use super::{
    manifest::{remove_session, RecordingManifest},
    types::{RecordedAudioSummary, RecoverableRecording},
};

const HISTORY_LIMIT: usize = 100;

pub(super) struct RecordingPaths {
    pub(super) session_id: String,
    pub(super) directory: PathBuf,
    pub(super) microphone: PathBuf,
    pub(super) system: PathBuf,
    pub(super) mixed: PathBuf,
    pub(super) final_file: PathBuf,
}

impl RecordingPaths {
    pub(super) fn create(app: &AppHandle) -> Result<Self, String> {
        let now = Local::now();
        let base_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let session_id = format!("{}-{}", now.format("%Y%m%d%H%M%S%3f"), std::process::id());
        let directory = recordings_root(app)?.join("in-progress").join(&session_id);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("録音用の一時フォルダーを作成できませんでした: {error}"))?;

        let output_directory = completed_recordings_directory(app)?;
        fs::create_dir_all(&output_directory)
            .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;

        Ok(Self {
            session_id,
            microphone: directory.join("microphone.partial.m4a"),
            system: directory.join("system.partial.m4a"),
            mixed: directory.join("meeting.partial.m4a"),
            final_file: unique_output_path(&output_directory, &base_name),
            directory,
        })
    }
}

fn unique_output_path(directory: &Path, base_name: &str) -> PathBuf {
    let initial = directory.join(format!("{base_name}.m4a"));
    if !initial.exists() {
        return initial;
    }
    (2..=999)
        .map(|suffix| directory.join(format!("{base_name}_{suffix}.m4a")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| directory.join(format!("{base_name}_{}.m4a", std::process::id())))
}

fn recordings_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("アプリデータフォルダーを取得できませんでした: {error}"))?
        .join("recordings"))
}

fn completed_recordings_directory(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    let directory = app
        .path()
        .cache_dir()
        .map_err(|error| format!("録音履歴フォルダーを取得できませんでした: {error}"))?
        .join("recordings");
    #[cfg(not(target_os = "android"))]
    let directory = app
        .path()
        .audio_dir()
        .map_err(|error| format!("ミュージックフォルダーを取得できませんでした: {error}"))?
        .join("Mutsuna Echo");

    fs::create_dir_all(&directory)
        .map_err(|error| format!("録音履歴フォルダーを準備できませんでした: {error}"))?;
    Ok(directory)
}

pub(super) fn completed_recordings_with_paths(
    app: &AppHandle,
) -> Result<Vec<(RecordedAudioSummary, PathBuf)>, String> {
    let directory = completed_recordings_directory(app)?;
    completed_recording_entries_in(&directory)
}

fn completed_recording_entries_in(
    directory: &Path,
) -> Result<Vec<(RecordedAudioSummary, PathBuf)>, String> {
    let mut recordings = fs::read_dir(directory)
        .map_err(|error| format!("過去の録音を確認できませんでした: {error}"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("m4a"))
            {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            let file_name = entry.file_name().into_string().ok()?;
            let recorded_at_unix_ms = metadata
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis()
                .try_into()
                .ok()?;
            Some((
                RecordedAudioSummary {
                    id: file_name.clone(),
                    meeting_id: String::new(),
                    file_name,
                    size_bytes: metadata.len(),
                    recorded_at_unix_ms,
                    transcript_providers: Vec::new(),
                },
                path,
            ))
        })
        .collect::<Vec<_>>();
    recordings.sort_by_key(|(recording, _)| std::cmp::Reverse(recording.recorded_at_unix_ms));
    recordings.truncate(HISTORY_LIMIT);
    Ok(recordings)
}

pub(super) fn completed_recording_path(
    app: &AppHandle,
    file_name: &str,
) -> Result<PathBuf, String> {
    validate_completed_file_name(file_name)?;
    let path = completed_recordings_directory(app)?.join(file_name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| {
        "選択した録音ファイルが見つかりません。履歴を更新してください。".to_string()
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("選択した録音ファイルを安全に開けませんでした。".to_string());
    }
    Ok(path)
}

fn validate_completed_file_name(file_name: &str) -> Result<(), String> {
    let path = Path::new(file_name);
    if path.file_name().and_then(|name| name.to_str()) != Some(file_name)
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("m4a"))
    {
        return Err("録音ファイル名が不正です。".to_string());
    }
    Ok(())
}

pub(super) fn atomic_copy_to_output(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "録音の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(
        ".{}.partial",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    fs::copy(source, &temporary)
        .map_err(|error| format!("録音ファイルを保存先へコピーできませんでした: {error}"))?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("録音ファイルを安全に書き込めませんでした: {error}"))?;
    fs::rename(&temporary, destination)
        .map_err(|error| format!("録音ファイルを保存先へ確定できませんでした: {error}"))
}

pub(super) fn recoverable_recordings(app: &AppHandle) -> Result<Vec<RecoverableRecording>, String> {
    let root = recordings_root(app)?.join("in-progress");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut recordings = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("復旧可能な録音を確認できませんでした: {error}"))?
    {
        let directory = match entry {
            Ok(entry) if entry.path().is_dir() => entry.path(),
            _ => continue,
        };
        let manifest = match RecordingManifest::load(&directory) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !manifest.finalized && manifest.mixed_file.exists() {
            recordings.push(RecoverableRecording {
                session_id: manifest.session_id,
                started_at: manifest.started_at,
                duration_ms: manifest.duration_ms,
                microphone: manifest.microphone,
                system_audio: manifest.system_audio,
            });
        }
    }
    recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(recordings)
}

pub(super) fn recover(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    validate_session_id(session_id)?;
    let directory = recordings_root(app)?.join("in-progress").join(session_id);
    let manifest = RecordingManifest::load(&directory)?;
    if !manifest.mixed_file.exists() {
        return Err("復旧できる音声フラグメントが見つかりません。".into());
    }
    crate::commands::transcribe::validate_audio_path(&manifest.mixed_file)
        .map_err(|error| format!("録音フラグメントを再生可能なM4Aとして復旧できませんでした。元データは破棄していません: {error}"))?;
    atomic_copy_to_output(&manifest.mixed_file, &manifest.final_file)?;
    let selected =
        crate::commands::transcribe::set_selected_audio_path(app, manifest.final_file.clone())?;
    crate::meeting_store::store_recording_tracks(
        app,
        selected.meeting_id(),
        manifest
            .microphone_file
            .as_deref()
            .filter(|path| path.exists()),
        manifest.system_file.as_deref().filter(|path| path.exists()),
    )?;
    remove_session(&directory)?;
    Ok(manifest.final_file)
}

pub(super) fn discard(app: &AppHandle, session_id: &str) -> Result<(), String> {
    validate_session_id(session_id)?;
    remove_session(&recordings_root(app)?.join("in-progress").join(session_id))
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("録音セッションIDが不正です。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        completed_recording_entries_in, unique_output_path, validate_completed_file_name,
        validate_session_id,
    };

    #[test]
    fn session_id_cannot_escape_recording_root() {
        assert!(validate_session_id("../../secret").is_err());
        assert!(validate_session_id("20260808-42").is_ok());
    }

    #[test]
    fn output_path_uses_m4a_extension() {
        let path = unique_output_path(&std::env::temp_dir(), "2026-08-08_12-00-00");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("m4a")
        );
    }

    #[test]
    fn completed_recording_name_cannot_escape_output_directory() {
        assert!(validate_completed_file_name("2026-08-08_12-00-00.m4a").is_ok());
        assert!(validate_completed_file_name("../secret.m4a").is_err());
        assert!(validate_completed_file_name("meeting.mp3").is_err());
    }

    #[test]
    fn completed_recordings_include_only_m4a_files() {
        let directory = std::env::temp_dir().join(format!(
            "mutsuna-completed-recordings-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create history fixture directory");
        std::fs::write(directory.join("meeting.m4a"), b"audio").expect("write M4A fixture");
        std::fs::write(directory.join("notes.txt"), b"not audio").expect("write non-audio fixture");

        let recordings =
            completed_recording_entries_in(&directory).expect("list completed recordings");
        assert_eq!(recordings.len(), 1);
        assert_eq!(recordings[0].0.id, "meeting.m4a");
        assert_eq!(recordings[0].0.file_name, "meeting.m4a");
        assert_eq!(recordings[0].0.size_bytes, 5);

        let _ = std::fs::remove_dir_all(directory);
    }
}
