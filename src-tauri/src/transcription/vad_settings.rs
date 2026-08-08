use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";
const MAX_SETTINGS_BYTES: u64 = 8 * 1024;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum VadPreset {
    SoftVoice,
    #[default]
    Standard,
    NoiseReduction,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VadParameters {
    pub(crate) threshold: f32,
    pub(crate) min_silence_duration: f32,
    pub(crate) min_speech_duration: f32,
    pub(crate) max_speech_duration: f32,
}

impl VadPreset {
    pub(crate) const fn parameters(self) -> VadParameters {
        match self {
            Self::SoftVoice => VadParameters {
                threshold: 0.15,
                min_silence_duration: 0.6,
                min_speech_duration: 0.15,
                max_speech_duration: 30.0,
            },
            Self::Standard => VadParameters {
                threshold: 0.25,
                min_silence_duration: 0.5,
                min_speech_duration: 0.25,
                max_speech_duration: 30.0,
            },
            Self::NoiseReduction => VadParameters {
                threshold: 0.5,
                min_silence_duration: 0.4,
                min_speech_duration: 0.3,
                max_speech_duration: 30.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VadSettings {
    preset: VadPreset,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("local-stt").join("vad").join(SETTINGS_FILE))
        .map_err(|error| format!("VAD設定の保存先を取得できませんでした: {error}"))
}

pub(crate) fn current_preset(app: &AppHandle) -> Result<VadPreset, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(VadPreset::default());
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("VAD設定を確認できませんでした: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_SETTINGS_BYTES
    {
        eprintln!("Ignoring invalid VAD settings file at {}", path.display());
        return Ok(VadPreset::default());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("VAD設定を読み込めませんでした: {error}"))?;
    match serde_json::from_slice::<VadSettings>(&bytes) {
        Ok(settings) => Ok(settings.preset),
        Err(error) => {
            eprintln!(
                "Ignoring corrupt VAD settings at {}: {error}",
                path.display()
            );
            Ok(VadPreset::default())
        }
    }
}

#[tauri::command]
pub(crate) fn get_vad_preset(app: AppHandle) -> Result<VadPreset, String> {
    current_preset(&app)
}

#[tauri::command]
pub(crate) fn set_vad_preset(app: AppHandle, preset: VadPreset) -> Result<(), String> {
    let path = settings_path(&app)?;
    let parent = path.parent().ok_or("VAD設定の保存先が不正です。")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("VAD設定の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(".{SETTINGS_FILE}.{}", uuid::Uuid::now_v7()));
    let bytes = serde_json::to_vec_pretty(&VadSettings { preset })
        .map_err(|error| format!("VAD設定を作成できませんでした: {error}"))?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("VAD設定を保存できませんでした: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("VAD設定を保存できませんでした: {error}"))?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("VAD設定を更新できませんでした: {error}"))?;
        }
        fs::rename(&temporary, &path)
            .map_err(|error| format!("VAD設定を確定できませんでした: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::VadPreset;

    #[test]
    fn presets_order_sensitivity_as_documented() {
        let soft = VadPreset::SoftVoice.parameters();
        let standard = VadPreset::Standard.parameters();
        let noise = VadPreset::NoiseReduction.parameters();
        assert!(soft.threshold < standard.threshold);
        assert!(standard.threshold < noise.threshold);
        assert_eq!(standard.max_speech_duration, 30.0);
    }
}
