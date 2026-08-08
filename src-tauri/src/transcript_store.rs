use std::{
    collections::HashSet,
    fmt::Write as FmtWrite,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::transcription::{Transcript, TranscriptionProvider};

const SCHEMA_VERSION: u8 = 3;
const PREVIOUS_SCHEMA_VERSION: u8 = 2;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscript {
    schema_version: u8,
    #[serde(default)]
    meeting_id: Option<String>,
    #[serde(rename = "savedAt")]
    _saved_at: String,
    transcript: Transcript,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscriptRef<'a> {
    schema_version: u8,
    meeting_id: &'a str,
    saved_at: String,
    transcript: &'a Transcript,
}

#[derive(Default)]
pub(crate) struct TranscriptIndex {
    meeting_providers: HashSet<String>,
    legacy_file_names: HashSet<String>,
}

impl TranscriptIndex {
    pub(crate) fn load(app: &AppHandle) -> Result<Self, String> {
        let mut index = Self::default();
        let meetings = crate::meeting_store::meetings_directory(app)?;
        if let Ok(entries) = fs::read_dir(meetings) {
            for entry in entries.filter_map(Result::ok) {
                let Some(meeting_id) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if crate::meeting_store::validate_meeting_id(&meeting_id).is_err() {
                    continue;
                }
                let transcripts = entry.path().join("transcripts");
                let Ok(files) = fs::read_dir(transcripts) else {
                    continue;
                };
                for file in files.filter_map(Result::ok) {
                    let Some(provider) = transcript_provider_from_file_name(&file.file_name())
                    else {
                        continue;
                    };
                    index
                        .meeting_providers
                        .insert(meeting_provider_key(&meeting_id, &provider));
                }
            }
        }

        let legacy = legacy_transcripts_directory(app)?;
        if let Ok(entries) = fs::read_dir(legacy) {
            index.legacy_file_names = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
        }
        Ok(index)
    }

    pub(crate) fn providers_for_meeting(
        &self,
        meeting_id: &str,
        legacy_audio_path: Option<&Path>,
    ) -> Vec<String> {
        let legacy_key = legacy_audio_path.and_then(|path| audio_key(path).ok());
        TranscriptionProvider::ALL
            .into_iter()
            .filter(|provider| {
                self.meeting_providers
                    .contains(&meeting_provider_key(meeting_id, provider.id()))
                    || legacy_key.as_ref().is_some_and(|key| {
                        let primary = format!("{key}.{}.json", provider.id());
                        self.legacy_file_names.contains(&primary)
                            || self
                                .legacy_file_names
                                .contains(&format!("{primary}.backup"))
                            || (*provider == TranscriptionProvider::ElevenLabs
                                && (self.legacy_file_names.contains(&format!("{key}.json"))
                                    || self
                                        .legacy_file_names
                                        .contains(&format!("{key}.json.backup"))))
                    })
            })
            .map(|provider| provider.id().to_string())
            .collect()
    }
}

pub(crate) fn save(
    app: &AppHandle,
    meeting_id: &str,
    transcript: &Transcript,
) -> Result<(), String> {
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    save_in(&directory, meeting_id, transcript)
}

pub(crate) fn load(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    if let Some(transcript) = load_current_in(&directory, meeting_id, provider)? {
        return Ok(Some(transcript));
    }

    let legacy_directory = legacy_transcripts_directory(app)?;
    let Some(transcript) = load_legacy_in(&legacy_directory, audio_path, provider)? else {
        return Ok(None);
    };
    // 旧形式は残したまま、新しいMeeting配下へコピーして段階的に移行する。
    save_in(&directory, meeting_id, &transcript)?;
    Ok(Some(transcript))
}

fn legacy_transcripts_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("transcripts"))
        .map_err(|error| format!("文字起こしの保存先を取得できませんでした: {error}"))
}

fn transcript_path_in(
    directory: &Path,
    meeting_id: &str,
    provider_id: &str,
) -> Result<PathBuf, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    validate_provider_id(provider_id)?;
    Ok(directory.join(format!("{provider_id}.json")))
}

fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    if provider_id.is_empty()
        || !provider_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err("文字起こしプロバイダーIDが不正です。".to_string());
    }
    Ok(())
}

fn legacy_transcript_path(directory: &Path, audio_path: &Path) -> Result<PathBuf, String> {
    Ok(directory.join(format!("{}.json", audio_key(audio_path)?)))
}

fn legacy_provider_path(
    directory: &Path,
    audio_path: &Path,
    provider_id: &str,
) -> Result<PathBuf, String> {
    validate_provider_id(provider_id)?;
    Ok(directory.join(format!("{}.{provider_id}.json", audio_key(audio_path)?)))
}

fn audio_key(audio_path: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(audio_path)
        .map_err(|error| format!("音声ファイルの保存識別子を作成できませんでした: {error}"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("音声ファイルの情報を取得できませんでした: {error}"))?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified_nanos.to_le_bytes());
    let digest = hasher.finalize();
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut key, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(key)
}

fn save_in(directory: &Path, meeting_id: &str, transcript: &Transcript) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("文字起こしの保存先を作成できませんでした: {error}"))?;
    let path = transcript_path_in(directory, meeting_id, &transcript.provider)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    let backup = path.with_extension("json.backup");
    let stored = StoredTranscriptRef {
        schema_version: SCHEMA_VERSION,
        meeting_id,
        saved_at: chrono::Utc::now().to_rfc3339(),
        transcript,
    };

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("文字起こしを書き込めませんでした: {error}"))?;
    if let Err(error) = serde_json::to_writer(&mut file, &stored) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!("文字起こしをJSONへ変換できませんでした: {error}"));
    }
    let written_bytes = file
        .metadata()
        .map_err(|error| format!("文字起こしの保存サイズを確認できませんでした: {error}"))?
        .len();
    if written_bytes > MAX_TRANSCRIPT_BYTES {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err("文字起こしが大きすぎるため保存できませんでした。".to_string());
    }
    file.sync_all()
        .map_err(|error| format!("文字起こしを安全に書き込めませんでした: {error}"))?;
    drop(file);
    replace_with_backup(&path, &temporary, &backup)
}

fn replace_with_backup(path: &Path, temporary: &Path, backup: &Path) -> Result<(), String> {
    if path.exists() {
        if backup.exists() {
            fs::remove_file(backup).map_err(|error| {
                format!("古い文字起こしのバックアップを削除できませんでした: {error}")
            })?;
        }
        fs::rename(path, backup)
            .map_err(|error| format!("文字起こしを更新用に退避できませんでした: {error}"))?;
    }
    if let Err(error) = fs::rename(temporary, path) {
        if backup.exists() {
            let _ = fs::rename(backup, path);
        }
        return Err(format!("文字起こしの保存を確定できませんでした: {error}"));
    }
    if backup.exists() {
        fs::remove_file(backup).map_err(|error| {
            format!("文字起こし保存後のバックアップを削除できませんでした: {error}")
        })?;
    }
    Ok(())
}

fn load_current_in(
    directory: &Path,
    meeting_id: &str,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let primary = transcript_path_in(directory, meeting_id, provider.id())?;
    let candidates = [primary.clone(), primary.with_extension("json.backup")];
    let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
        return Ok(None);
    };
    let stored = read_stored_transcript(&path)?;
    if !matches!(
        stored.schema_version,
        SCHEMA_VERSION | PREVIOUS_SCHEMA_VERSION
    ) || stored.meeting_id.as_deref() != Some(meeting_id)
    {
        return Err("保存済みの文字起こし形式またはMeeting IDが一致しません。".into());
    }
    validate_stored_provider(&stored, provider)?;
    Ok(Some(stored.transcript))
}

fn load_legacy_in(
    directory: &Path,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let primary = legacy_provider_path(directory, audio_path, provider.id())?;
    let mut candidates = vec![primary.clone(), primary.with_extension("json.backup")];
    if provider == TranscriptionProvider::ElevenLabs {
        let legacy = legacy_transcript_path(directory, audio_path)?;
        candidates.push(legacy.clone());
        candidates.push(legacy.with_extension("json.backup"));
    }
    let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
        return Ok(None);
    };
    let stored = read_stored_transcript(&path)?;
    if stored.schema_version != 1 {
        return Err("保存済みの旧文字起こし形式に対応していません。".into());
    }
    validate_stored_provider(&stored, provider)?;
    Ok(Some(stored.transcript))
}

fn read_stored_transcript(path: &Path) -> Result<StoredTranscript, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("保存済みの文字起こしを確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err("保存済みの文字起こしファイルが不正です。".to_string());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("保存済みの文字起こしを読み込めませんでした: {error}"))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("保存済みの文字起こしが壊れています: {error}"))
}

fn validate_stored_provider(
    stored: &StoredTranscript,
    provider: TranscriptionProvider,
) -> Result<(), String> {
    if stored.transcript.provider != provider.id() {
        return Err("保存済みの文字起こしプロバイダーが一致しません。".to_string());
    }
    Ok(())
}

fn transcript_provider_from_file_name(name: &std::ffi::OsStr) -> Option<String> {
    let name = name.to_str()?;
    let provider = name
        .strip_suffix(".json")
        .or_else(|| name.strip_suffix(".json.backup"))?;
    validate_provider_id(provider).ok()?;
    Some(provider.to_string())
}

fn meeting_provider_key(meeting_id: &str, provider_id: &str) -> String {
    format!("{meeting_id}:{provider_id}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{audio_key, load_current_in, load_legacy_in, save_in, transcript_path_in};
    use crate::transcription::{
        TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
        TranscriptionProvider,
    };

    fn fixture_transcript() -> Transcript {
        Transcript {
            provider: "elevenlabs".into(),
            model: "scribe_v2".into(),
            language: "ja".into(),
            tokens: vec![TranscriptToken {
                text: "テストです。".into(),
                start_ms: Some(100),
                end_ms: Some(500),
                start_time_source: Some(TokenTimeSource::Provider),
                end_time_source: Some(TokenTimeSource::Provider),
                speaker: Some("Speaker 1".into()),
                speaker_source: Some(TokenSpeakerSource::Provider),
                confidence: None,
            }],
            segments: vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 100,
                end_ms: 500,
                text: "テストです。".into(),
            }],
        }
    }

    #[test]
    fn transcript_round_trips_by_meeting_id() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-store-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        let transcript = fixture_transcript();
        save_in(&directory, &meeting_id, &transcript).expect("save transcript");
        assert_eq!(
            load_current_in(&directory, &meeting_id, TranscriptionProvider::ElevenLabs)
                .expect("load transcript"),
            Some(transcript)
        );
        let path =
            transcript_path_in(&directory, &meeting_id, "elevenlabs").expect("transcript path");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("elevenlabs.json")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn providers_get_distinct_storage_paths() {
        let root = std::path::Path::new("transcripts");
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let elevenlabs = transcript_path_in(root, &meeting_id, "elevenlabs").expect("path");
        let assemblyai = transcript_path_in(root, &meeting_id, "assemblyai").expect("path");
        assert_ne!(elevenlabs, assemblyai);
    }

    #[test]
    fn schema_v2_without_tokens_remains_readable() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-v2-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        fs::create_dir_all(&directory).expect("create store");
        let stored = serde_json::json!({
            "schemaVersion": 2,
            "meetingId": meeting_id,
            "savedAt": "2026-08-08T00:00:00Z",
            "transcript": {
                "provider": "elevenlabs",
                "model": "scribe_v2",
                "language": "ja",
                "segments": [{
                    "speaker": "Speaker 1",
                    "startMs": 100,
                    "endMs": 500,
                    "text": "旧データ"
                }]
            }
        });
        fs::write(
            directory.join("elevenlabs.json"),
            serde_json::to_vec(&stored).expect("serialize transcript"),
        )
        .expect("write transcript");
        let loaded = load_current_in(&directory, &meeting_id, TranscriptionProvider::ElevenLabs)
            .expect("load v2 transcript")
            .expect("stored transcript");
        assert!(loaded.tokens.is_empty());
        assert_eq!(loaded.segments[0].text, "旧データ");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_v1_transcript_remains_readable_for_migration() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-legacy-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let audio = root.join("meeting.m4a");
        let legacy = root.join("transcripts");
        fs::create_dir_all(&legacy).expect("create legacy store");
        fs::write(&audio, b"legacy audio").expect("write audio");
        let transcript = fixture_transcript();
        let stored = serde_json::json!({
            "schemaVersion": 1,
            "savedAt": "2026-08-08T00:00:00Z",
            "transcript": transcript
        });
        let key = audio_key(&audio).expect("legacy audio key");
        fs::write(
            legacy.join(format!("{key}.elevenlabs.json")),
            serde_json::to_vec(&stored).expect("serialize legacy transcript"),
        )
        .expect("write legacy transcript");
        assert_eq!(
            load_legacy_in(&legacy, &audio, TranscriptionProvider::ElevenLabs)
                .expect("load legacy transcript"),
            Some(fixture_transcript())
        );
        let _ = fs::remove_dir_all(root);
    }
}
