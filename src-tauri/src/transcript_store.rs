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

const SCHEMA_VERSION: u8 = 1;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscript {
    schema_version: u8,
    #[serde(rename = "savedAt")]
    _saved_at: String,
    transcript: Transcript,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscriptRef<'a> {
    schema_version: u8,
    saved_at: String,
    transcript: &'a Transcript,
}

#[derive(Default)]
pub(crate) struct TranscriptIndex {
    file_names: HashSet<String>,
}

impl TranscriptIndex {
    pub(crate) fn load(app: &AppHandle) -> Result<Self, String> {
        let directory = transcripts_directory(app)?;
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => {
                return Err(format!(
                    "保存済みの文字起こし一覧を確認できませんでした: {error}"
                ))
            }
        };
        let file_names = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        Ok(Self { file_names })
    }

    pub(crate) fn providers_for_audio(&self, audio_path: &Path) -> Vec<String> {
        let Ok(key) = audio_key(audio_path) else {
            return Vec::new();
        };
        TranscriptionProvider::ALL
            .into_iter()
            .filter(|provider| {
                let primary = format!("{key}.{}.json", provider.id());
                self.file_names.contains(&primary)
                    || self.file_names.contains(&format!("{primary}.backup"))
                    || (*provider == TranscriptionProvider::ElevenLabs
                        && (self.file_names.contains(&format!("{key}.json"))
                            || self.file_names.contains(&format!("{key}.json.backup"))))
            })
            .map(|provider| provider.id().to_string())
            .collect()
    }
}

pub(crate) fn save(
    app: &AppHandle,
    audio_path: &Path,
    transcript: &Transcript,
) -> Result<(), String> {
    let directory = transcripts_directory(app)?;
    save_in(&directory, audio_path, transcript)
}

pub(crate) fn load(
    app: &AppHandle,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let directory = transcripts_directory(app)?;
    load_in(&directory, audio_path, provider)
}

fn transcripts_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("transcripts"))
        .map_err(|error| format!("文字起こしの保存先を取得できませんでした: {error}"))
}

fn transcript_path_in(
    directory: &Path,
    audio_path: &Path,
    provider_id: &str,
) -> Result<PathBuf, String> {
    if provider_id.is_empty()
        || !provider_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err("文字起こしプロバイダーIDが不正です。".to_string());
    }
    Ok(directory.join(format!("{}.{provider_id}.json", audio_key(audio_path)?)))
}

fn legacy_transcript_path(directory: &Path, audio_path: &Path) -> Result<PathBuf, String> {
    Ok(directory.join(format!("{}.json", audio_key(audio_path)?)))
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

fn save_in(directory: &Path, audio_path: &Path, transcript: &Transcript) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("文字起こしの保存先を作成できませんでした: {error}"))?;
    let path = transcript_path_in(directory, audio_path, &transcript.provider)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    let backup = path.with_extension("json.backup");
    let stored = StoredTranscriptRef {
        schema_version: SCHEMA_VERSION,
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

    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!("古い文字起こしのバックアップを削除できませんでした: {error}")
            })?;
        }
        fs::rename(&path, &backup)
            .map_err(|error| format!("文字起こしを更新用に退避できませんでした: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
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

fn load_in(
    directory: &Path,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let primary = transcript_path_in(directory, audio_path, provider.id())?;
    let mut candidates = vec![primary.clone(), primary.with_extension("json.backup")];
    if provider == TranscriptionProvider::ElevenLabs {
        let legacy = legacy_transcript_path(directory, audio_path)?;
        candidates.push(legacy.clone());
        candidates.push(legacy.with_extension("json.backup"));
    }
    let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
        return Ok(None);
    };
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("保存済みの文字起こしを確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err("保存済みの文字起こしファイルが不正です。".to_string());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("保存済みの文字起こしを読み込めませんでした: {error}"))?;
    let stored: StoredTranscript = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("保存済みの文字起こしが壊れています: {error}"))?;
    if stored.schema_version != SCHEMA_VERSION {
        return Err("保存済みの文字起こし形式に対応していません。".to_string());
    }
    if stored.transcript.provider != provider.id() {
        return Err("保存済みの文字起こしプロバイダーが一致しません。".to_string());
    }
    Ok(Some(stored.transcript))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs};

    use super::{audio_key, load_in, save_in, transcript_path_in, TranscriptIndex};
    use crate::transcription::{Transcript, TranscriptSegment, TranscriptionProvider};

    fn fixture_transcript() -> Transcript {
        Transcript {
            provider: "elevenlabs".into(),
            model: "scribe_v2".into(),
            language: "ja".into(),
            segments: vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 100,
                end_ms: 500,
                text: "テストです。".into(),
            }],
        }
    }

    #[test]
    fn transcript_round_trips_without_storing_beside_audio() {
        let root =
            std::env::temp_dir().join(format!("mutsuna-transcript-store-{}", std::process::id()));
        let audio = root.join("audio").join("meeting.m4a");
        let store = root.join("store");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(audio.parent().expect("audio parent")).expect("create audio directory");
        fs::write(&audio, b"audio fixture").expect("write audio fixture");

        let transcript = fixture_transcript();
        save_in(&store, &audio, &transcript).expect("save transcript");
        assert_eq!(
            load_in(&store, &audio, TranscriptionProvider::ElevenLabs).expect("load transcript"),
            Some(transcript.clone())
        );
        assert!(!audio.with_extension("json").exists());

        let primary = transcript_path_in(&store, &audio, "elevenlabs").expect("transcript path");
        fs::rename(&primary, primary.with_extension("json.backup"))
            .expect("simulate interrupted update");
        assert_eq!(
            load_in(&store, &audio, TranscriptionProvider::ElevenLabs)
                .expect("load backup transcript"),
            Some(transcript.clone())
        );

        let provider_backup = primary.with_extension("json.backup");
        let legacy = store.join(format!("{}.json", audio_key(&audio).expect("legacy key")));
        fs::rename(provider_backup, legacy).expect("move transcript to legacy path");
        assert_eq!(
            load_in(&store, &audio, TranscriptionProvider::ElevenLabs)
                .expect("load legacy transcript"),
            Some(transcript)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audio_changes_get_a_new_storage_key() {
        let root =
            std::env::temp_dir().join(format!("mutsuna-transcript-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture directory");
        let audio = root.join("meeting.m4a");
        fs::write(&audio, b"first").expect("write first fixture");
        let first = audio_key(&audio).expect("first key");
        fs::write(&audio, b"second version").expect("write second fixture");
        let second = audio_key(&audio).expect("second key");

        assert_ne!(first, second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn providers_get_distinct_storage_paths() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-provider-key-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture directory");
        let audio = root.join("meeting.m4a");
        fs::write(&audio, b"audio fixture").expect("write audio fixture");

        let elevenlabs = transcript_path_in(&root, &audio, "elevenlabs")
            .expect("create ElevenLabs transcript path");
        let assemblyai = transcript_path_in(&root, &audio, "assemblyai")
            .expect("create AssemblyAI transcript path");
        assert_ne!(elevenlabs, assemblyai);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn transcript_index_matches_provider_and_legacy_files_without_rescanning() {
        let root =
            std::env::temp_dir().join(format!("mutsuna-transcript-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture directory");
        let audio = root.join("meeting.m4a");
        fs::write(&audio, b"audio fixture").expect("write audio fixture");
        let key = audio_key(&audio).expect("audio key");

        let provider_index = TranscriptIndex {
            file_names: HashSet::from([format!("{key}.elevenlabs.json.backup")]),
        };
        assert_eq!(
            provider_index.providers_for_audio(&audio),
            vec!["elevenlabs"]
        );

        let legacy_index = TranscriptIndex {
            file_names: HashSet::from([format!("{key}.json")]),
        };
        assert_eq!(legacy_index.providers_for_audio(&audio), vec!["elevenlabs"]);

        let _ = fs::remove_dir_all(root);
    }
}
