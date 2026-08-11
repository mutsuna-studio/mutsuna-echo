use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComputeProfile {
    pub(crate) stt_threads: i32,
    pub(crate) diarization_threads: i32,
    pub(crate) vad_threads: i32,
    pub(crate) max_stt_batch: usize,
    pub(crate) stt_batch_memory_bytes: u64,
}

static PROFILE: OnceLock<ComputeProfile> = OnceLock::new();
static COMBINED_INFERENCE: AtomicBool = AtomicBool::new(false);

pub(crate) struct CombinedInferenceGuard;

impl CombinedInferenceGuard {
    pub(crate) fn enter() -> Self {
        COMBINED_INFERENCE.store(true, Ordering::Release);
        Self
    }
}

impl Drop for CombinedInferenceGuard {
    fn drop(&mut self) {
        COMBINED_INFERENCE.store(false, Ordering::Release);
    }
}

pub(crate) fn profile() -> ComputeProfile {
    let mut profile = *PROFILE.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2);
        let profile = profile_for(cores, cfg!(mobile));
        eprintln!(
            "processing_tuning cores={cores} mobile={} stt_threads={} diarization_threads={} vad_threads={} max_stt_batch={} stt_batch_memory_bytes={}",
            cfg!(mobile),
            profile.stt_threads,
            profile.diarization_threads,
            profile.vad_threads,
            profile.max_stt_batch,
            profile.stt_batch_memory_bytes
        );
        profile
    });
    if !cfg!(mobile) && COMBINED_INFERENCE.load(Ordering::Acquire) {
        let cores = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2);
        let workers_per_model = (cores.saturating_sub(4) / 2).clamp(1, 8) as i32;
        profile.stt_threads = workers_per_model;
        profile.diarization_threads = workers_per_model;
        profile.max_stt_batch = profile.max_stt_batch.min(4);
    }
    profile
}

pub(crate) fn stt_batch_size(max_region_ms: u64) -> usize {
    let profile = profile();
    let bytes_per_stream = max_region_ms
        .max(1)
        .saturating_mul(crate::transcription::vad::SAMPLE_RATE as u64)
        .saturating_mul(std::mem::size_of::<f32>() as u64)
        / 1_000;
    let memory_limited = profile.stt_batch_memory_bytes / bytes_per_stream.max(1);
    profile
        .max_stt_batch
        .min(usize::try_from(memory_limited).unwrap_or(usize::MAX))
        .max(1)
}

fn profile_for(cores: usize, mobile: bool) -> ComputeProfile {
    let cores = if cores == 0 { 1 } else { cores };
    if mobile {
        let threads = if cores >= 8 {
            4
        } else if cores >= 4 {
            3
        } else {
            cores as i32
        };
        ComputeProfile {
            stt_threads: threads,
            diarization_threads: threads,
            vad_threads: 1,
            max_stt_batch: if cores >= 6 { 2 } else { 1 },
            stt_batch_memory_bytes: 48 * 1024 * 1024,
        }
    } else {
        let workers = cores.saturating_sub(2).clamp(1, 12) as i32;
        ComputeProfile {
            stt_threads: workers,
            diarization_threads: workers,
            vad_threads: if cores >= 12 {
                4
            } else if cores >= 6 {
                2
            } else {
                1
            },
            max_stt_batch: if cores >= 16 {
                6
            } else if cores >= 12 {
                4
            } else if cores >= 6 {
                3
            } else {
                2
            },
            stt_batch_memory_bytes: if cores >= 16 {
                384 * 1024 * 1024
            } else {
                192 * 1024 * 1024
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::profile_for;

    #[test]
    fn mobile_profile_stays_within_memory_and_core_limits() {
        let low = profile_for(2, true);
        let high = profile_for(8, true);
        assert_eq!(low.stt_threads, 2);
        assert_eq!(low.max_stt_batch, 1);
        assert_eq!(high.stt_threads, 4);
        assert_eq!(high.max_stt_batch, 2);
    }

    #[test]
    fn desktop_profile_leaves_capacity_for_io_and_ui() {
        let profile = profile_for(16, false);
        assert_eq!(profile.stt_threads, 12);
        assert_eq!(profile.diarization_threads, 12);
        assert_eq!(profile.vad_threads, 4);
        assert_eq!(profile.max_stt_batch, 6);
    }
}
