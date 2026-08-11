use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

const MIN_POINTS: u16 = 64;
const MAX_POINTS: u16 = 512;
const CACHE_VERSION: u8 = 2;
const STORAGE_POINTS: u16 = 512;
const PREVIEW_POINTS: usize = 64;
const FULL_DECODE_THRESHOLD_MS: u64 = 30_000;
const MAX_WINDOW_MS: u64 = 250;
const MIN_WINDOW_MS: u64 = 40;
const FINGERPRINT_SAMPLE_BYTES: usize = 4_096;

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
        downsample_bins(&self.buckets, points)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceFingerprint {
    size_bytes: u64,
    modified_at_unix_ms: u64,
    duration_ms: u64,
    edge_sha256: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredWaveform {
    version: u8,
    meeting_id: String,
    source: SourceFingerprint,
    points: u16,
    peaks: Vec<f32>,
}

#[derive(Default)]
struct JobSnapshot {
    preview: Option<Vec<f32>>,
    result: Option<Result<StoredWaveform, String>>,
}

struct WaveformJob {
    snapshot: Mutex<JobSnapshot>,
    changed: Condvar,
    cancelled: AtomicBool,
}

impl WaveformJob {
    fn new() -> Self {
        Self {
            snapshot: Mutex::new(JobSnapshot::default()),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
        }
    }
}

static JOBS: OnceLock<Mutex<HashMap<String, Arc<WaveformJob>>>> = OnceLock::new();

#[tauri::command]
pub(crate) async fn get_selected_audio_waveform(
    app: AppHandle,
    meeting_id: String,
    points: u16,
) -> Result<AudioWaveform, String> {
    validate_points(points)?;
    let (audio_path, duration_ms) =
        crate::commands::transcribe::selected_audio_for_waveform(&app, &meeting_id)?;
    let fingerprint = source_fingerprint(&audio_path, duration_ms)?;
    let worker_app = app.clone();
    let worker_meeting_id = meeting_id.clone();
    let stored = tauri::async_runtime::spawn_blocking(move || {
        load_or_generate(
            &worker_app,
            &worker_meeting_id,
            &audio_path,
            duration_ms,
            fingerprint,
            true,
        )
    })
    .await
    .map_err(|error| format!("音声波形の生成処理を完了できませんでした: {error}"))??;
    Ok(AudioWaveform {
        meeting_id,
        points,
        peaks: resample_peaks(&stored.peaks, points as usize),
    })
}

/// Starts cache generation as soon as an audio file is registered. Calls from
/// the player join this same job rather than decoding the source a second time.
pub(crate) fn schedule_waveform_generation(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    duration_ms: u64,
) {
    let app = app.clone();
    let meeting_id = meeting_id.to_string();
    let audio_path = audio_path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let result = source_fingerprint(&audio_path, duration_ms).and_then(|fingerprint| {
            load_or_generate(
                &app,
                &meeting_id,
                &audio_path,
                duration_ms,
                fingerprint,
                false,
            )
        });
        if let Err(error) = result {
            if !error.contains("中止") {
                eprintln!("Could not prepare audio waveform: {error}");
            }
        }
    });
}

pub(crate) fn cache_recorded_waveform(
    app: &AppHandle,
    meeting_id: &str,
    accumulator: LiveWaveformAccumulator,
) -> Result<(), String> {
    let (audio_path, duration_ms) =
        crate::commands::transcribe::selected_audio_for_waveform(app, meeting_id)?;
    cache_peaks(
        app,
        meeting_id,
        &audio_path,
        duration_ms,
        accumulator.finish(STORAGE_POINTS as usize),
    )
}

#[cfg(target_os = "android")]
pub(crate) fn cache_external_recorded_waveform(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    duration_ms: u64,
    peaks: Vec<f32>,
) -> Result<(), String> {
    if peaks.is_empty() || peaks.iter().any(|peak| !peak.is_finite()) {
        return Err("録音中に生成した波形データが不正です。".into());
    }
    cache_peaks(
        app,
        meeting_id,
        audio_path,
        duration_ms,
        resample_peaks(&peaks, STORAGE_POINTS as usize),
    )
}

fn cache_peaks(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    duration_ms: u64,
    mut peaks: Vec<f32>,
) -> Result<(), String> {
    normalize_peaks(&mut peaks);
    let stored = StoredWaveform {
        version: CACHE_VERSION,
        meeting_id: meeting_id.to_string(),
        source: source_fingerprint(audio_path, duration_ms)?,
        points: STORAGE_POINTS,
        peaks,
    };
    write_cache(&cache_path(app, meeting_id)?, &stored)
}

fn load_or_generate(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    duration_ms: u64,
    fingerprint: SourceFingerprint,
    emit_preview: bool,
) -> Result<StoredWaveform, String> {
    let cache_path = cache_path(app, meeting_id)?;
    if let Some(cached) = read_cache(&cache_path, meeting_id, &fingerprint) {
        return Ok(cached);
    }

    let key = format!(
        "{meeting_id}:{}:{}:{}",
        fingerprint.size_bytes, fingerprint.modified_at_unix_ms, fingerprint.duration_ms
    );
    let jobs = JOBS.get_or_init(|| Mutex::new(HashMap::new()));
    let (job, leader) = {
        let mut active = jobs
            .lock()
            .map_err(|_| "波形生成の実行状態を取得できませんでした。".to_string())?;
        for (active_key, active_job) in active.iter() {
            if active_key != &key {
                active_job.cancelled.store(true, Ordering::Release);
            }
        }
        if let Some(job) = active.get(&key) {
            (job.clone(), false)
        } else {
            let job = Arc::new(WaveformJob::new());
            active.insert(key.clone(), job.clone());
            (job, true)
        }
    };

    if leader {
        let result = generate_stored_waveform(
            app,
            meeting_id,
            audio_path,
            duration_ms,
            fingerprint,
            &job,
            emit_preview,
        )
        .and_then(|stored| {
            write_cache(&cache_path, &stored)?;
            Ok(stored)
        });
        if let Ok(mut snapshot) = job.snapshot.lock() {
            snapshot.result = Some(result.clone());
            job.changed.notify_all();
        }
        if let Ok(mut active) = jobs.lock() {
            active.remove(&key);
        }
        result
    } else {
        wait_for_job(app, meeting_id, &job, emit_preview)
    }
}

fn wait_for_job(
    app: &AppHandle,
    meeting_id: &str,
    job: &WaveformJob,
    emit_preview: bool,
) -> Result<StoredWaveform, String> {
    let mut preview_emitted = false;
    let mut snapshot = job
        .snapshot
        .lock()
        .map_err(|_| "波形生成の進捗を取得できませんでした。".to_string())?;
    loop {
        if emit_preview && !preview_emitted {
            if let Some(preview) = snapshot.preview.clone() {
                emit_waveform_progress(app, meeting_id, preview);
                preview_emitted = true;
            }
        }
        if let Some(result) = snapshot.result.clone() {
            return result;
        }
        snapshot = job
            .changed
            .wait(snapshot)
            .map_err(|_| "波形生成の完了を待機できませんでした。".to_string())?;
    }
}

fn generate_stored_waveform(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    duration_ms: u64,
    fingerprint: SourceFingerprint,
    job: &WaveformJob,
    emit_preview: bool,
) -> Result<StoredWaveform, String> {
    let peaks = if duration_ms <= FULL_DECODE_THRESHOLD_MS {
        let peaks =
            extract_peaks_sequential(audio_path, duration_ms, STORAGE_POINTS as usize, job)?;
        publish_preview(
            app,
            meeting_id,
            job,
            resample_peaks(&peaks, PREVIEW_POINTS),
            emit_preview,
        );
        peaks
    } else {
        match extract_peaks_sparse(audio_path, duration_ms, PREVIEW_POINTS, job) {
            Ok(preview) => {
                publish_preview(app, meeting_id, job, preview, emit_preview);
                match extract_peaks_sparse(audio_path, duration_ms, STORAGE_POINTS as usize, job) {
                    Ok(peaks) => peaks,
                    Err(error) if error.contains("中止") => return Err(error),
                    Err(_) => extract_peaks_sequential(
                        audio_path,
                        duration_ms,
                        STORAGE_POINTS as usize,
                        job,
                    )?,
                }
            }
            Err(error) if error.contains("中止") => return Err(error),
            Err(_) => {
                let peaks = extract_peaks_sequential(
                    audio_path,
                    duration_ms,
                    STORAGE_POINTS as usize,
                    job,
                )?;
                publish_preview(
                    app,
                    meeting_id,
                    job,
                    resample_peaks(&peaks, PREVIEW_POINTS),
                    emit_preview,
                );
                peaks
            }
        }
    };
    Ok(StoredWaveform {
        version: CACHE_VERSION,
        meeting_id: meeting_id.to_string(),
        source: fingerprint,
        points: STORAGE_POINTS,
        peaks,
    })
}

fn publish_preview(
    app: &AppHandle,
    meeting_id: &str,
    job: &WaveformJob,
    preview: Vec<f32>,
    emit_preview: bool,
) {
    if let Ok(mut snapshot) = job.snapshot.lock() {
        snapshot.preview = Some(preview.clone());
        job.changed.notify_all();
    }
    if emit_preview {
        emit_waveform_progress(app, meeting_id, preview);
    }
}

fn emit_waveform_progress(app: &AppHandle, meeting_id: &str, peaks: Vec<f32>) {
    let completed_points = peaks.len();
    let _ = app.emit(
        "audio-waveform-progress",
        AudioWaveformProgress {
            meeting_id: meeting_id.to_string(),
            peaks,
            completed_points,
        },
    );
}

fn extract_peaks_sparse(
    path: &Path,
    duration_ms: u64,
    points: usize,
    job: &WaveformJob,
) -> Result<Vec<f32>, String> {
    let windows = sample_windows(duration_ms, points);
    let mut bins = vec![BinStats::default(); points];
    crate::transcription::audio_decode::decode_mono_windows(
        path,
        &windows,
        |index, _, samples| {
            check_cancelled(job)?;
            for sample in samples {
                bins[index].accept(*sample);
            }
            Ok(())
        },
    )?;
    check_cancelled(job)?;
    Ok(peaks_from_bins(&bins))
}

fn extract_peaks_sequential(
    path: &Path,
    duration_ms: u64,
    points: usize,
    job: &WaveformJob,
) -> Result<Vec<f32>, String> {
    if duration_ms == 0 {
        return Err("音声の長さを取得できないため波形を生成できません。".into());
    }
    let mut bins = vec![BinStats::default(); points];
    crate::transcription::audio_decode::decode_mono_sampled(
        path,
        8,
        |sample_rate, packet_frame_offset, samples| {
            check_cancelled(job)?;
            let expected_frames = duration_ms
                .saturating_mul(sample_rate as u64)
                .saturating_add(999)
                / 1_000;
            for (offset, sample) in samples.iter().enumerate() {
                let frame = packet_frame_offset.saturating_add((offset * 8) as u64);
                let index = ((frame as u128 * points as u128) / expected_frames.max(1) as u128)
                    .min(points.saturating_sub(1) as u128) as usize;
                bins[index].accept(*sample);
            }
            Ok(())
        },
    )?;
    Ok(peaks_from_bins(&bins))
}

fn check_cancelled(job: &WaveformJob) -> Result<(), String> {
    if job.cancelled.load(Ordering::Acquire) {
        Err("別の音声が選択されたため波形生成を中止しました。".into())
    } else {
        Ok(())
    }
}

fn sample_windows(duration_ms: u64, points: usize) -> Vec<(u64, u64)> {
    let bin_ms = (duration_ms / points.max(1) as u64).max(1);
    let window_ms = bin_ms
        .clamp(MIN_WINDOW_MS, MAX_WINDOW_MS)
        .min(duration_ms.max(1));
    (0..points)
        .map(|index| {
            let start = duration_ms.saturating_mul(index as u64) / points as u64;
            let end = duration_ms.saturating_mul((index + 1) as u64) / points as u64;
            let position =
                start.saturating_add(end.saturating_sub(start).saturating_sub(window_ms) / 2);
            (position, window_ms)
        })
        .collect()
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

fn cache_path(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    Ok(crate::meeting_store::meeting_directory(app, meeting_id)?
        .join("derived")
        .join(format!("waveform-v{CACHE_VERSION}.json")))
}

fn source_fingerprint(path: &Path, duration_ms: u64) -> Result<SourceFingerprint, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("波形元の音声情報を取得できませんでした: {error}"))?;
    let modified_at_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default();
    let mut file = fs::File::open(path)
        .map_err(|error| format!("波形元の音声を確認できませんでした: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; FINGERPRINT_SAMPLE_BYTES.min(metadata.len() as usize)];
    file.read_exact(&mut buffer)
        .map_err(|error| format!("波形元の音声先頭を確認できませんでした: {error}"))?;
    hasher.update(&buffer);
    if metadata.len() > FINGERPRINT_SAMPLE_BYTES as u64 {
        file.seek(SeekFrom::End(-(FINGERPRINT_SAMPLE_BYTES as i64)))
            .map_err(|error| format!("波形元の音声末尾へ移動できませんでした: {error}"))?;
        let mut tail = vec![0u8; FINGERPRINT_SAMPLE_BYTES];
        file.read_exact(&mut tail)
            .map_err(|error| format!("波形元の音声末尾を確認できませんでした: {error}"))?;
        hasher.update(&tail);
    }
    Ok(SourceFingerprint {
        size_bytes: metadata.len(),
        modified_at_unix_ms,
        duration_ms,
        edge_sha256: hasher.finalize().into(),
    })
}

fn read_cache(
    path: &Path,
    meeting_id: &str,
    fingerprint: &SourceFingerprint,
) -> Option<StoredWaveform> {
    if fs::metadata(path).ok()?.len() > 64 * 1_024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let waveform: StoredWaveform = serde_json::from_slice(&bytes).ok()?;
    (waveform.version == CACHE_VERSION
        && waveform.meeting_id == meeting_id
        && &waveform.source == fingerprint
        && waveform.points == STORAGE_POINTS
        && waveform.peaks.len() == STORAGE_POINTS as usize
        && valid_peaks(&waveform.peaks))
    .then_some(waveform)
}

fn write_cache(path: &Path, waveform: &StoredWaveform) -> Result<(), String> {
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
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("古い音声波形キャッシュを更新できませんでした: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("音声波形のキャッシュを確定できませんでした: {error}")
    })
}

fn downsample_bins(source: &[BinStats], points: usize) -> Vec<f32> {
    if source.is_empty() {
        return vec![0.0; points];
    }
    let mut bins = vec![BinStats::default(); points];
    for (index, bucket) in source.iter().copied().enumerate() {
        let target = (index * points / source.len()).min(points.saturating_sub(1));
        bins[target].merge(bucket);
    }
    peaks_from_bins(&bins)
}

fn resample_peaks(source: &[f32], points: usize) -> Vec<f32> {
    if source.is_empty() {
        return vec![0.0; points];
    }
    (0..points)
        .map(|index| {
            let from = index * source.len() / points;
            let to = ((index + 1) * source.len() / points).max(from + 1);
            source[from..to.min(source.len())]
                .iter()
                .copied()
                .fold(0.0, f32::max)
        })
        .collect()
}

fn peaks_from_bins(bins: &[BinStats]) -> Vec<f32> {
    let mut peaks: Vec<f32> = bins.iter().copied().map(BinStats::amplitude).collect();
    normalize_peaks(&mut peaks);
    peaks
}

fn valid_peaks(peaks: &[f32]) -> bool {
    peaks
        .iter()
        .all(|peak| peak.is_finite() && (0.0..=1.0).contains(peak))
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
    use super::{
        normalize_peaks, resample_peaks, sample_windows, validate_points, LiveWaveformAccumulator,
    };

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

    #[test]
    fn sparse_windows_cover_the_whole_duration_without_full_scan() {
        let windows = sample_windows(3_600_000, 320);
        assert_eq!(windows.len(), 320);
        assert!(windows.iter().all(|(_, length)| *length == 250));
        assert!(windows.last().unwrap().0 < 3_600_000);
    }

    #[test]
    fn cached_resolution_can_be_resampled_for_the_player() {
        let source: Vec<f32> = (0..512).map(|index| index as f32 / 511.0).collect();
        let result = resample_peaks(&source, 320);
        assert_eq!(result.len(), 320);
        assert!(result.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
