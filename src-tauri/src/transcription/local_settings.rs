use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "recognition.json";

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LocalRecognitionMode {
    #[default]
    Fast,
    Accurate,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalRecognitionSettings {
    pub(crate) mode: LocalRecognitionMode,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("local-stt").join(SETTINGS_FILE))
        .map_err(|error| format!("ローカル文字起こし設定の保存先を取得できませんでした: {error}"))
}

pub(crate) fn current(app: &AppHandle) -> Result<LocalRecognitionSettings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(LocalRecognitionSettings::default());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("ローカル文字起こし設定を読み込めませんでした: {error}"))?;
    serde_json::from_slice(&bytes).or_else(|error| {
        eprintln!(
            "Ignoring invalid local transcription settings at {}: {error}",
            path.display()
        );
        Ok(LocalRecognitionSettings::default())
    })
}

#[tauri::command]
pub(crate) fn get_local_recognition_settings(
    app: AppHandle,
) -> Result<LocalRecognitionSettings, String> {
    current(&app)
}

#[tauri::command]
pub(crate) fn set_local_recognition_settings(
    app: AppHandle,
    settings: LocalRecognitionSettings,
) -> Result<LocalRecognitionSettings, String> {
    let path = settings_path(&app)?;
    let parent = path
        .parent()
        .ok_or("ローカル文字起こし設定の保存先が不正です。")?;
    fs::create_dir_all(parent).map_err(|error| {
        format!("ローカル文字起こし設定の保存先を作成できませんでした: {error}")
    })?;
    let temporary = parent.join(format!(".{SETTINGS_FILE}.{}", uuid::Uuid::now_v7()));
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("ローカル文字起こし設定を作成できませんでした: {error}"))?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("ローカル文字起こし設定を保存できませんでした: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| {
                format!("ローカル文字起こし設定を安全に保存できませんでした: {error}")
            })?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                format!("ローカル文字起こし設定を更新できませんでした: {error}")
            })?;
        }
        fs::rename(&temporary, &path)
            .map_err(|error| format!("ローカル文字起こし設定を確定できませんでした: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map(|()| settings)
}

#[cfg(test)]
mod tests {
    use super::{LocalRecognitionMode, LocalRecognitionSettings};

    #[test]
    fn fast_mode_is_the_compatible_default() {
        assert_eq!(
            LocalRecognitionSettings::default().mode,
            LocalRecognitionMode::Fast
        );
    }
}
