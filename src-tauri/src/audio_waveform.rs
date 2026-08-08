use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const MIN_POINTS: u16 = 64;
const MAX_POINTS: u16 = 512;
const CACHE_VERSION: u8 = 1;
pub(crate) const DEFAULT_POINTS: u16 = 320;
const PROGRESS_POINT_INTERVAL: usize = 4;
const WAVEFORM_SAMPLE_STRIDE: usize = 8;

#[derive(Debug, Clone, Copy, Default)]
struct BinStats {
    sum_of_squares: f64,
    transient: f32,
    count: u64,
}

impl BinStats {
    fn accept(&mut self, sample: f32) {
        let amplitude = sample.abs().min(1.0);
        self.sum_of_squares += f64::from(amplitude) * f64::from(amplitude);
        self.transient = self.transient.max(amplitude);
        self.count = self.count.saturating_add(1);
    }

    fn merge(&mut self, other: Self) {
        self.sum_of_squares += other.sum_of_squares;
        self.transient = self.transient.max(other.transient);
        self.count = self.count.saturating_add(other.count);
    }

    fn amplitude(self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            let rms = (self.sum_of_squares / self.count as f64).sqrt() as f32;
            rms * 0.85 + self.transient * 0.15
        }
    }
}

pub(crate) struct LiveWaveformAccumulator {
    bucket_frames: usize,
    current: BinStats,
    buckets: Vec<BinStats>,
}

impl LiveWaveformAccumulator {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            bucket_frames: (sample_rate as usize / 10).max(1),
            current: BinStats::default(),
            buckets: Vec::new(),
        }
    }

    pub(crate) fn accept(&mut self, samples: &[f32]) {
        for sample in samples {
            self.current.accept(*sample);
            if self.current.count as usize >= self.bucket_frames {
                self.buckets.push(std::mem::take(&mut self.current));
            }
        }
    }

    fn finish(mut self, points: usize) -> Vec<f32> {
        if self.current.count > 0 {
            self.buckets.push(self.current);
        }
        let mut bins = vec![BinStats::default(); points];
        let bucket_count = self.buckets.len();
        if bucket_count <= points {
            for (index, target) in bins.iter_mut().enumerate() {
                if let Some(bucket) = self.buckets.get(index * bucket_count / points) {
                    *target = *bucket;
                }
            }
        } else {
            for (index, bucket) in self.buckets.into_iter().enumerate() {
                let target = (index * points / bucket_count).min(points.saturating_sub(1));
                bins[target].merge(bucket);
            }
        }
        peaks_from_bins(&bins)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioWaveform {
    meeting_id: String,
    points: u16,
    peaks: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioWaveformProgress {
    meeting_id: String,
    peaks: Vec<f32>,
    completed_points: usize,
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
    let progress_app = app.clone();
    let progress_meeting_id = meeting_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        if let Some(cached) = read_cache(&cache_path, &cached_meeting_id, points) {
            return Ok(cached);
        }
        if cache_path.is_file() {
            let _ = fs::remove_file(&cache_path);
        }
        let peaks = extract_peaks(
            &audio_path,
            duration_ms,
            points as usize,
            |peaks, completed_points| {
                if crate::commands::transcribe::selected_audio_for_waveform(
                    &progress_app,
                    &progress_meeting_id,
                )
                .is_err()
                {
                    return false;
                }
                let _ = progress_app.emit(
                    "audio-waveform-progress",
                    AudioWaveformProgress {
                        meeting_id: progress_meeting_id.clone(),
                        peaks,
                        completed_points,
                    },
                );
                true
            },
        )?;
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

pub(crate) fn cache_recorded_waveform(
    app: &AppHandle,
    meeting_id: &str,
    accumulator: LiveWaveformAccumulator,
) -> Result<(), String> {
    let points = DEFAULT_POINTS;
    let waveform = AudioWaveform {
        meeting_id: meeting_id.to_string(),
        points,
        peaks: accumulator.finish(points as usize),
    };
    write_cache(&cache_path(app, meeting_id, points)?, &waveform)
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

fn extract_peaks(
    path: &Path,
    duration_ms: u64,
    points: usize,
    mut on_progress: impl FnMut(Vec<f32>, usize) -> bool,
) -> Result<Vec<f32>, String> {
    if duration_ms == 0 {
        return Err("音声の長さを取得できないため波形を生成できません。".into());
    }
    let mut bins = vec![BinStats::default(); points];
    let mut next_progress_point = PROGRESS_POINT_INTERVAL;
    crate::transcription::audio_decode::decode_mono_sampled(
        path,
        WAVEFORM_SAMPLE_STRIDE,
        |sample_rate, packet_frame_offset, samples| {
            let expected_frames = duration_ms
                .saturating_mul(sample_rate as u64)
                .saturating_add(999)
                / 1_000;
            for (offset, sample) in samples.iter().enumerate() {
                let frame =
                    packet_frame_offset.saturating_add((offset * WAVEFORM_SAMPLE_STRIDE) as u64);
                let index = ((frame as u128 * points as u128) / expected_frames.max(1) as u128)
                    .min(points.saturating_sub(1) as u128) as usize;
                bins[index].accept(*sample);
            }
            let decoded_through = packet_frame_offset
                .saturating_add((samples.len().saturating_mul(WAVEFORM_SAMPLE_STRIDE)) as u64);
            let completed_points = ((decoded_through as u128 * points as u128)
                / expected_frames.max(1) as u128)
                .min(points as u128) as usize;
            if next_progress_point <= points && completed_points >= next_progress_point {
                if !on_progress(peaks_from_bins(&bins), completed_points) {
                    return Err("別の音声が選択されたため波形生成を中止しました。".into());
                }
                next_progress_point = completed_points.saturating_add(PROGRESS_POINT_INTERVAL);
            }
            Ok(())
        },
    )?;
    Ok(peaks_from_bins(&bins))
}

fn peaks_from_bins(bins: &[BinStats]) -> Vec<f32> {
    let mut peaks: Vec<f32> = bins.iter().copied().map(BinStats::amplitude).collect();
    normalize_peaks(&mut peaks);
    peaks
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
    use super::{normalize_peaks, validate_points, LiveWaveformAccumulator};

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

    #[test]
    fn live_accumulator_downsamples_without_redecoding() {
        let mut accumulator = LiveWaveformAccumulator::new(100);
        accumulator.accept(&[0.1; 10]);
        accumulator.accept(&[0.8; 10]);
        let peaks = accumulator.finish(8);
        assert_eq!(peaks.len(), 8);
        assert!(peaks.iter().all(|peak| (0.0..=1.0).contains(peak)));
        assert!(peaks.iter().any(|peak| *peak > 0.0));
    }
}
