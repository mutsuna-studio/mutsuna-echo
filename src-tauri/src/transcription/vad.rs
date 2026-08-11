use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::{
    audio_decode::decode_mono,
    vad_settings::{VadParameters, VadPreset},
};

pub(crate) const SAMPLE_RATE: u32 = 16_000;
const REGION_PADDING_MS: u64 = 300;
const REGION_PADDING_SAMPLES: u64 = REGION_PADDING_MS * SAMPLE_RATE as u64 / 1_000;
// Silero finalizes a region when max_speech_duration is reached even when no
// silence was detected. Rejoin those artificial boundaries before recognition
// so a word cannot be decoded independently on either side of a 30-second cut.
const FORCED_SPLIT_MAX_GAP_MS: u64 = 100;
// Keep recognition bounded on mobile while allowing normal continuous speech
// to pass through several detector-level safety cuts as one context window.
const MAX_RECOGNITION_REGION_MS: u64 = 180_000;

pub(crate) struct SpeechRegion {
    /// Start of the padded recognition window on the original timeline.
    pub(crate) start_ms: u64,
    pub(crate) speech_start_ms: u64,
    pub(crate) speech_end_ms: u64,
    pub(crate) end_ms: u64,
}

impl SpeechRegion {
    pub(crate) fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Detects speech boundaries without retaining source audio. Recognition reads
/// each padded region directly from the file after this pass, so long speech or
/// delayed VAD finalization can never evict the beginning of a region.
pub(crate) fn visit_speech_regions(
    audio_path: &Path,
    model_path: &Path,
    preset: VadPreset,
    mut on_progress: impl FnMut(u64) -> Result<(), String>,
) -> Result<(u64, Vec<SpeechRegion>), String> {
    let parameters = preset.parameters();
    let detector = create_detector(model_path, parameters)?;
    let mut resampler: Option<StreamingAreaResampler> = None;
    let mut detected = Vec::new();
    let mut decoded_frames = 0u64;
    let mut resampled_samples = 0u64;
    let duration_ms = decode_mono(audio_path, |sample_rate, samples| {
        let resampler =
            resampler.get_or_insert_with(|| StreamingAreaResampler::new(sample_rate, SAMPLE_RATE));
        if resampler.source_rate != sample_rate {
            return Err("途中でサンプルレートが変わる音声には対応していません。".into());
        }
        let output = resampler.process(samples);
        detector.accept_waveform(&output);
        resampled_samples = resampled_samples.saturating_add(output.len() as u64);
        collect_detected(&detector, &mut detected);
        decoded_frames = decoded_frames.saturating_add(samples.len() as u64);
        on_progress(decoded_frames.saturating_mul(1_000) / sample_rate.max(1) as u64)
    })?;
    detector.flush();
    collect_detected(&detector, &mut detected);
    let regions = detected
        .into_iter()
        .map(|region| SpeechRegion {
            start_ms: samples_to_ms(region.start_sample.saturating_sub(REGION_PADDING_SAMPLES)),
            speech_start_ms: samples_to_ms(region.start_sample),
            speech_end_ms: samples_to_ms(region.end_sample),
            end_ms: samples_to_ms(
                region
                    .end_sample
                    .saturating_add(REGION_PADDING_SAMPLES)
                    .min(resampled_samples),
            ),
        })
        .collect();
    let regions = coalesce_forced_split_regions(regions);
    Ok((duration_ms, regions))
}

fn coalesce_forced_split_regions(regions: Vec<SpeechRegion>) -> Vec<SpeechRegion> {
    let mut coalesced: Vec<SpeechRegion> = Vec::with_capacity(regions.len());
    for next in regions {
        let should_merge = coalesced.last().is_some_and(|current| {
            next.speech_start_ms.saturating_sub(current.speech_end_ms) <= FORCED_SPLIT_MAX_GAP_MS
                && next.speech_end_ms.saturating_sub(current.speech_start_ms)
                    <= MAX_RECOGNITION_REGION_MS
        });
        if should_merge {
            let current = coalesced
                .last_mut()
                .expect("the merge predicate requires an existing region");
            current.speech_end_ms = current.speech_end_ms.max(next.speech_end_ms);
            current.end_ms = current.end_ms.max(next.end_ms);
        } else {
            coalesced.push(next);
        }
    }
    coalesced
}

fn create_detector(
    model_path: &Path,
    parameters: VadParameters,
) -> Result<VoiceActivityDetector, String> {
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model_path.to_string_lossy().into_owned()),
            threshold: parameters.threshold,
            min_silence_duration: parameters.min_silence_duration,
            min_speech_duration: parameters.min_speech_duration,
            window_size: 512,
            max_speech_duration: parameters.max_speech_duration,
        },
        sample_rate: SAMPLE_RATE as i32,
        num_threads: 1,
        provider: Some("cpu".into()),
        debug: false,
        ..Default::default()
    };
    let detector = VoiceActivityDetector::create(&config, 60.0).ok_or_else(|| {
        "VAD推論エンジンを初期化できませんでした。VADモデルを再インストールしてください。"
            .to_string()
    })?;
    Ok(detector)
}

pub(crate) struct LiveVoiceActivityDetector {
    detector: VoiceActivityDetector,
    resampler: StreamingAreaResampler,
}

impl LiveVoiceActivityDetector {
    pub(crate) fn create(
        model_path: &Path,
        source_sample_rate: u32,
        preset: VadPreset,
    ) -> Result<Self, String> {
        Ok(Self {
            detector: create_detector(model_path, preset.parameters())?,
            resampler: StreamingAreaResampler::new(source_sample_rate, SAMPLE_RATE),
        })
    }

    pub(crate) fn accept_waveform(&mut self, samples: &[f32]) -> bool {
        let output = self.resampler.process(samples);
        if !output.is_empty() {
            self.detector.accept_waveform(&output);
        }
        let detected = self.detector.detected();
        while !self.detector.is_empty() {
            self.detector.pop();
        }
        detected
    }
}

#[derive(Debug, Clone, Copy)]
struct DetectedRegion {
    start_sample: u64,
    end_sample: u64,
}

fn collect_detected(detector: &VoiceActivityDetector, detected: &mut Vec<DetectedRegion>) {
    while let Some(segment) = detector.front() {
        let start_sample = segment.start().max(0) as u64;
        detected.push(DetectedRegion {
            start_sample,
            end_sample: start_sample.saturating_add(segment.samples().len() as u64),
        });
        detector.pop();
    }
}

const fn samples_to_ms(samples: u64) -> u64 {
    samples.saturating_mul(1_000) / SAMPLE_RATE as u64
}

/// Streaming box-filter resampler. It bounds memory to one decoded packet and
/// avoids aliasing when common 44.1/48 kHz meeting audio is reduced to 16 kHz.
struct StreamingAreaResampler {
    source_rate: u32,
    target_rate: u32,
    output_remaining: u64,
    weighted_sum: f64,
}

pub(crate) fn resample_mono(source_rate: u32, samples: &[f32]) -> Vec<f32> {
    StreamingAreaResampler::new(source_rate, SAMPLE_RATE).process(samples)
}

impl StreamingAreaResampler {
    fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            source_rate,
            target_rate,
            output_remaining: source_rate as u64,
            weighted_sum: 0.0,
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let expected = input.len().saturating_mul(self.target_rate as usize)
            / self.source_rate.max(1) as usize
            + 2;
        let mut output = Vec::with_capacity(expected);
        for &sample in input {
            let mut input_remaining = self.target_rate as u64;
            while input_remaining > 0 {
                let overlap = input_remaining.min(self.output_remaining);
                self.weighted_sum += sample as f64 * overlap as f64;
                input_remaining -= overlap;
                self.output_remaining -= overlap;
                if self.output_remaining == 0 {
                    output.push((self.weighted_sum / self.source_rate as f64) as f32);
                    self.output_remaining = self.source_rate as u64;
                    self.weighted_sum = 0.0;
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{coalesce_forced_split_regions, SpeechRegion, StreamingAreaResampler};

    fn region(speech_start_ms: u64, speech_end_ms: u64) -> SpeechRegion {
        SpeechRegion {
            start_ms: speech_start_ms.saturating_sub(300),
            speech_start_ms,
            speech_end_ms,
            end_ms: speech_end_ms.saturating_add(300),
        }
    }

    #[test]
    fn resamples_48khz_in_streaming_chunks_without_drift() {
        let mut resampler = StreamingAreaResampler::new(48_000, 16_000);
        let mut output = resampler.process(&vec![1.0; 24_001]);
        output.extend(resampler.process(&vec![1.0; 23_999]));
        assert_eq!(output.len(), 16_000);
        assert!(output
            .iter()
            .all(|sample| (*sample - 1.0).abs() < f32::EPSILON));
    }

    #[test]
    fn supports_44khz_and_low_rate_inputs() {
        let mut down = StreamingAreaResampler::new(44_100, 16_000);
        assert_eq!(down.process(&vec![0.5; 44_100]).len(), 16_000);
        let mut up = StreamingAreaResampler::new(8_000, 16_000);
        assert_eq!(up.process(&vec![0.5; 8_000]).len(), 16_000);
    }

    #[test]
    fn rejoins_detector_max_duration_boundaries_without_silence() {
        let regions = coalesce_forced_split_regions(vec![
            region(1_000, 31_000),
            region(31_032, 61_032),
            region(61_064, 72_000),
        ]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].speech_start_ms, 1_000);
        assert_eq!(regions[0].speech_end_ms, 72_000);
        assert_eq!(regions[0].end_ms, 72_300);
    }

    #[test]
    fn preserves_real_silence_boundaries() {
        let regions =
            coalesce_forced_split_regions(vec![region(1_000, 31_000), region(31_500, 50_000)]);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn keeps_continuous_recognition_windows_bounded() {
        let regions =
            coalesce_forced_split_regions(vec![region(0, 90_000), region(90_032, 180_032)]);
        assert_eq!(regions.len(), 2);
    }
}
