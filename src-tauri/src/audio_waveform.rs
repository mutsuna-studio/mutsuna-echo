use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MIN_POINTS: u16 = 64;
const MAX_POINTS: u16 = 512;
const CACHE_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioWaveform {
    meeting_id: String,
    points: u16,
    peaks: Vec<f32>,
}

#[tauri::command]
pub(crate) async fn get_selected_audio_waveform(
    app: AppHandle,
    meeting_id: String,
    points: u16,
) -> Result<AudioWaveform, String> {
    validate_points(points)?;
    let (audio_path, duration_ms) =
        crate::commands::transcribe::selected_audio_for_waveform(&app, &meeting_id)?;
    let cache_path = cache_path(&app, &meeting_id, points)?;
    let cached_meeting_id = meeting_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        if let Some(cached) = read_cache(&cache_path, &cached_meeting_id, points) {
            return Ok(cached);
        }
        if cache_path.is_file() {
            let _ = fs::remove_file(&cache_path);
        }
        let peaks = extract_peaks(&audio_path, duration_ms, points as usize)?;
        let waveform = AudioWaveform {
            meeting_id,
            points,
            peaks,
        };
        if let Err(error) = write_cache(&cache_path, &waveform) {
            eprintln!("Could not cache audio waveform: {error}");
        }
        Ok(waveform)
    })
    .await
    .map_err(|error| format!("音声波形の生成処理を完了できませんでした: {error}"))?
}

fn validate_points(points: u16) -> Result<(), String> {
    if (MIN_POINTS..=MAX_POINTS).contains(&points) {
        Ok(())
    } else {
        Err(format!(
            "波形の解像度は{MIN_POINTS}〜{MAX_POINTS}の範囲で指定してください。"
        ))
    }
}

fn cache_path(app: &AppHandle, meeting_id: &str, points: u16) -> Result<PathBuf, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("波形キャッシュの保存先を取得できませんでした: {error}"))?
        .join("waveforms");
    Ok(directory.join(format!("{meeting_id}-{points}-v{CACHE_VERSION}.json")))
}

fn read_cache(path: &Path, meeting_id: &str, points: u16) -> Option<AudioWaveform> {
    if fs::metadata(path).ok()?.len() > 32 * 1_024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let waveform: AudioWaveform = serde_json::from_slice(&bytes).ok()?;
    (waveform.meeting_id == meeting_id
        && waveform.points == points
        && waveform.peaks.len() == points as usize
        && waveform
            .peaks
            .iter()
            .all(|peak| peak.is_finite() && (0.0..=1.0).contains(peak)))
    .then_some(waveform)
}

fn write_cache(path: &Path, waveform: &AudioWaveform) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| "波形キャッシュの保存先が不正です。".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("波形キャッシュの保存先を作成できませんでした: {error}"))?;
    let bytes = serde_json::to_vec(waveform)
        .map_err(|error| format!("音声波形を保存用に変換できませんでした: {error}"))?;
    let temporary = directory.join(format!(".{}.tmp", uuid::Uuid::now_v7()));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("音声波形の一時キャッシュを保存できませんでした: {error}"))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if !path.is_file() {
            return Err(format!(
                "音声波形のキャッシュを確定できませんでした: {error}"
            ));
        }
    }
    Ok(())
}

fn extract_peaks(path: &Path, duration_ms: u64, points: usize) -> Result<Vec<f32>, String> {
    if duration_ms == 0 {
        return Err("音声の長さを取得できないため波形を生成できません。".into());
    }
    let mut energy = vec![0.0f64; points];
    let mut transients = vec![0.0f32; points];
    let mut counts = vec![0u64; points];
    let mut frame_offset = 0u64;
    crate::transcription::audio_decode::decode_mono(path, |sample_rate, samples| {
        let expected_frames = duration_ms
            .saturating_mul(sample_rate as u64)
            .saturating_add(999)
            / 1_000;
        for (offset, sample) in samples.iter().enumerate() {
            let frame = frame_offset.saturating_add(offset as u64);
            let index = ((frame as u128 * points as u128) / expected_frames.max(1) as u128)
                .min(points.saturating_sub(1) as u128) as usize;
            let amplitude = sample.abs().min(1.0);
            energy[index] += f64::from(amplitude) * f64::from(amplitude);
            transients[index] = transients[index].max(amplitude);
            counts[index] = counts[index].saturating_add(1);
        }
        frame_offset = frame_offset.saturating_add(samples.len() as u64);
        Ok(())
    })?;
    let mut peaks: Vec<f32> = energy
        .into_iter()
        .zip(transients)
        .zip(counts)
        .map(|((sum_of_squares, transient), count)| {
            if count == 0 {
                0.0
            } else {
                let rms = (sum_of_squares / count as f64).sqrt() as f32;
                rms * 0.85 + transient * 0.15
            }
        })
        .collect();
    normalize_peaks(&mut peaks);
    Ok(peaks)
}

fn normalize_peaks(peaks: &mut [f32]) {
    let mut audible: Vec<f32> = peaks.iter().copied().filter(|peak| *peak > 0.001).collect();
    if audible.is_empty() {
        return;
    }
    audible.sort_by(f32::total_cmp);
    let reference_index = ((audible.len() - 1) as f32 * 0.95).round() as usize;
    let reference = audible[reference_index].max(0.05);
    for peak in peaks {
        *peak = (*peak / reference).clamp(0.0, 1.0).sqrt();
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_peaks, validate_points};

    #[test]
    fn accepts_only_bounded_waveform_resolutions() {
        assert!(validate_points(64).is_ok());
        assert!(validate_points(320).is_ok());
        assert!(validate_points(512).is_ok());
        assert!(validate_points(63).is_err());
        assert!(validate_points(513).is_err());
    }

    #[test]
    fn normalizes_peaks_without_exceeding_one() {
        let mut peaks = vec![0.0, 0.05, 0.2, 0.5, 1.0];
        normalize_peaks(&mut peaks);
        assert_eq!(peaks[0], 0.0);
        assert!(peaks.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(peaks.iter().all(|peak| (0.0..=1.0).contains(peak)));
        assert_eq!(peaks[4], 1.0);
    }
}
