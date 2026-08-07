use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use lofty::{config::ParseOptions, file::AudioFile, probe::Probe};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::transcription::Transcript;

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "wav", "flac"];
const ELEVENLABS_MAX_FILE_SIZE: u64 = 5_000_000_000;
const ELEVENLABS_STT_USD_PER_HOUR: f64 = 0.22;
const PRICING_VERIFIED_ON: &str = "2026-08-08";

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
    duration_ms: u64,
    estimated_cost_usd: f64,
    pricing_rate_usd_per_hour: f64,
    pricing_verified_on: &'static str,
}

pub(crate) fn describe_audio_path(path: &Path) -> Result<SelectedAudioFile, String> {
    let size_bytes = validate_audio_file(path)?;
    let estimate = estimate_audio_cost(path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("選択した音声ファイル")
        .to_string();
    Ok(SelectedAudioFile {
        name,
        size_bytes,
        duration_ms: estimate.duration_ms,
        estimated_cost_usd: estimate.estimated_cost_usd,
        pricing_rate_usd_per_hour: ELEVENLABS_STT_USD_PER_HOUR,
        pricing_verified_on: PRICING_VERIFIED_ON,
    })
}

pub(crate) fn set_selected_audio_path(
    app: &AppHandle,
    path: PathBuf,
) -> Result<SelectedAudioFile, String> {
    let selected = describe_audio_path(&path)?;
    let state = app.state::<AudioSelectionState>();
    *state
        .path
        .lock()
        .map_err(|_| "選択したファイルの状態を更新できませんでした。".to_string())? = Some(path);
    Ok(selected)
}

struct AudioEstimate {
    duration_ms: u64,
    estimated_cost_usd: f64,
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

fn estimate_audio_cost(path: &Path) -> Result<AudioEstimate, String> {
    let tagged_file = Probe::open(path)
        .and_then(|probe| probe.options(ParseOptions::new().read_tags(false)).read())
        .map_err(|error| {
            eprintln!("Could not read audio duration: {error:?}");
            "音声の再生時間を取得できませんでした。ファイル内容を確認してください。".to_string()
        })?;
    let duration = tagged_file.properties().duration();

    if duration.is_zero() {
        return Err("音声の再生時間が0秒です。別のファイルを選択してください。".to_string());
    }

    let duration_ms = u64::try_from(duration.as_millis())
        .map_err(|_| "音声の再生時間が長すぎます。".to_string())?;
    let estimated_cost_usd = duration.as_secs_f64() / 3600.0 * ELEVENLABS_STT_USD_PER_HOUR;

    Ok(AudioEstimate {
        duration_ms,
        estimated_cost_usd,
    })
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

    let path = selected_file_path(selected)?;
    let selected = describe_audio_path(&path)?;
    *state
        .path
        .lock()
        .map_err(|_| "選択したファイルの状態を更新できませんでした。".to_string())? = Some(path);
    Ok(Some(selected))
}

fn selected_file_path(selected: FilePath) -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    if let FilePath::Url(url) = &selected {
        if url.scheme() == "content" {
            return crate::recording::android::copy_content_uri(url.as_str());
        }
    }
    selected
        .into_path()
        .map_err(|_| "選択したファイルのパスを取得できませんでした。".to_string())
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
    use std::{fs::File, io::Write, path::Path};

    use super::{estimate_audio_cost, validate_audio_file, ELEVENLABS_STT_USD_PER_HOUR};

    fn write_one_second_wav(path: &Path) {
        const SAMPLE_RATE: u32 = 8_000;
        const CHANNELS: u16 = 1;
        const BITS_PER_SAMPLE: u16 = 16;
        let data_size = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8);
        let mut file = File::create(path).expect("create WAV fixture");

        file.write_all(b"RIFF").expect("write RIFF");
        file.write_all(&(36 + data_size).to_le_bytes())
            .expect("write RIFF size");
        file.write_all(b"WAVEfmt ").expect("write WAVE fmt");
        file.write_all(&16_u32.to_le_bytes())
            .expect("write fmt size");
        file.write_all(&1_u16.to_le_bytes())
            .expect("write PCM format");
        file.write_all(&CHANNELS.to_le_bytes())
            .expect("write channels");
        file.write_all(&SAMPLE_RATE.to_le_bytes())
            .expect("write sample rate");
        file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())
            .expect("write byte rate");
        file.write_all(&2_u16.to_le_bytes())
            .expect("write block align");
        file.write_all(&BITS_PER_SAMPLE.to_le_bytes())
            .expect("write bits per sample");
        file.write_all(b"data").expect("write data marker");
        file.write_all(&data_size.to_le_bytes())
            .expect("write data size");
        file.write_all(&vec![0_u8; data_size as usize])
            .expect("write PCM samples");
    }

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

    #[test]
    fn estimates_cost_from_local_audio_duration() {
        let path =
            std::env::temp_dir().join(format!("mutsuna-echo-duration-{}.wav", std::process::id()));
        write_one_second_wav(&path);

        let estimate = estimate_audio_cost(&path).expect("estimate WAV cost");
        let _ = std::fs::remove_file(path);

        assert_eq!(estimate.duration_ms, 1_000);
        assert!((estimate.estimated_cost_usd - ELEVENLABS_STT_USD_PER_HOUR / 3600.0).abs() < 1e-9);
    }
}
