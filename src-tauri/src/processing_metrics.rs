use std::time::{Duration, Instant};

/// Lightweight structured timing used by both desktop and Android builds.
/// Timings deliberately stay out of transcript data and are observable through
/// the normal application log (`logcat` on Android).
pub(crate) struct StageTimer {
    pipeline: &'static str,
    stage: &'static str,
    audio_ms: Option<u64>,
    started: Instant,
}

impl StageTimer {
    pub(crate) fn start(
        pipeline: &'static str,
        stage: &'static str,
        audio_ms: Option<u64>,
    ) -> Self {
        Self {
            pipeline,
            stage,
            audio_ms,
            started: Instant::now(),
        }
    }

    pub(crate) fn finish(self) -> Duration {
        let elapsed = self.started.elapsed();
        log_timing(self.pipeline, self.stage, elapsed, self.audio_ms);
        elapsed
    }
}

pub(crate) fn log_timing(
    pipeline: &'static str,
    stage: &'static str,
    elapsed: Duration,
    audio_ms: Option<u64>,
) {
    let elapsed_ms = elapsed.as_millis();
    if let Some(audio_ms) = audio_ms.filter(|value| *value > 0) {
        let realtime_factor = elapsed.as_secs_f64() / (audio_ms as f64 / 1_000.0);
        eprintln!(
            "processing_timing pipeline={pipeline} stage={stage} elapsed_ms={elapsed_ms} audio_ms={audio_ms} realtime_factor={realtime_factor:.4}"
        );
    } else {
        eprintln!("processing_timing pipeline={pipeline} stage={stage} elapsed_ms={elapsed_ms}");
    }
}

#[cfg(test)]
mod tests {
    use super::StageTimer;

    #[test]
    fn timer_finishes_without_panicking_at_clock_resolution_boundaries() {
        let _elapsed = StageTimer::start("test", "noop", Some(1_000)).finish();
    }
}
