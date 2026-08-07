use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::transcription::Transcript;

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "wav", "flac"];
const ELEVENLABS_MAX_FILE_SIZE: u64 = 5_000_000_000;

#[derive(Default)]
pub(crate) struct AudioSelectionState {
    path: Mutex<Option<PathBuf>>,
    transcribing: AtomicBool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedAudioFile {
    name: String,
    size_bytes: u64,
}

struct TranscriptionGuard<'a>(&'a AtomicBool);

impl Drop for TranscriptionGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn validate_audio_file(path: &Path) -> Result<u64, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "音声ファイルの拡張子を確認できませんでした。".to_string())?;

    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err("MP3、M4A、WAV、FLACのいずれかを選択してください。".to_string());
    }

    let metadata = std::fs::metadata(path).map_err(|error| {
        eprintln!("Could not inspect selected audio file: {error:?}");
        "選択した音声ファイルを読み込めませんでした。".to_string()
    })?;

    if !metadata.is_file() {
        return Err("音声ファイルを選択してください。".to_string());
    }

    if metadata.len() == 0 {
        return Err("選択した音声ファイルが空です。".to_string());
    }

    if metadata.len() >= ELEVENLABS_MAX_FILE_SIZE {
        return Err("音声ファイルは5GB未満にしてください。".to_string());
    }

    Ok(metadata.len())
}

#[tauri::command]
pub(crate) async fn select_audio_file(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
) -> Result<Option<SelectedAudioFile>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Audio", AUDIO_EXTENSIONS)
        .blocking_pick_file();

    let Some(selected) = selected else {
        return Ok(None);
    };

    let path = selected
        .into_path()
        .map_err(|_| "選択したファイルのパスを取得できませんでした。".to_string())?;
    let size_bytes = validate_audio_file(&path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("選択した音声ファイル")
        .to_string();

    *state
        .path
        .lock()
        .map_err(|_| "選択したファイルの状態を更新できませんでした。".to_string())? = Some(path);

    Ok(Some(SelectedAudioFile { name, size_bytes }))
}

#[tauri::command]
pub(crate) async fn transcribe_selected_audio(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
) -> Result<Transcript, String> {
    if state
        .transcribing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("文字起こしはすでに実行中です。".to_string());
    }
    let _guard = TranscriptionGuard(&state.transcribing);

    let path = state
        .path
        .lock()
        .map_err(|_| "選択したファイルの状態を取得できませんでした。".to_string())?
        .clone()
        .ok_or_else(|| "先に音声ファイルを選択してください。".to_string())?;

    validate_audio_file(&path)?;
    let api_key = crate::commands::api_key::load_api_key(&app)?;

    crate::transcription::elevenlabs::transcribe(&path, &api_key).await
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use super::validate_audio_file;

    #[test]
    fn rejects_unsupported_file_extension() {
        let path = std::env::temp_dir().join("mutsuna-echo-unsupported.txt");
        let mut file = File::create(&path).expect("create fixture");
        file.write_all(b"not audio").expect("write fixture");

        let result = validate_audio_file(&path);
        let _ = std::fs::remove_file(path);

        assert_eq!(
            result.expect_err("unsupported extension should fail"),
            "MP3、M4A、WAV、FLACのいずれかを選択してください。"
        );
    }
}
