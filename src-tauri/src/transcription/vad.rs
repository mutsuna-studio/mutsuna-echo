use std::path::Path;

use serde::{Deserialize, Serialize};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::{
    audio_decode::{decode_mono, StreamingAreaResampler},
    vad_settings::{VadParameters, VadPreset},
};

pub(crate) const SAMPLE_RATE: u32 = 16_000;
const REGION_PADDING_MS: u64 = 1_000;
const REGION_PADDING_SAMPLES: u64 = REGION_PADDING_MS * SAMPLE_RATE as u64 / 1_000;
// Rejoin detector-level forced splits and short false-negative gaps before
// recognition. The latter commonly occurs around quiet fillers or speech
// partially masked by another speaker; keeping both sides in one recognition
// window prevents those samples from being discarded outright.
pub(crate) const MISSED_SPEECH_RECOVERY_GAP_MS: u64 = 2_500;
const FORCED_SPLIT_MAX_GAP_MS: u64 = 100;
const START_OF_AUDIO_RECOVERY_MS: u64 = 5_000;
// ReazonSpeech can skip words even inside a single 20-to-30-second VAD turn.
// Shorter windows keep attention local; the existing 300 ms padding on both
// sides gives each artificial boundary enough context and preserves quiet
// lead-ins that Silero places just outside its detected speech span.
const MAX_RECOGNITION_SPEECH_MS: u64 = 15_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechRegion {
    /// Start of the padded recognition window on the original timeline.
    pub(crate) start_ms: u64,
    pub(crate) speech_start_ms: u64,
    pub(crate) speech_end_ms: u64,
    pub(crate) end_ms: u64,
}

type VadAnalysisResult = (u64, Vec<SpeechRegion>, Vec<u64>, Vec<SpeechRegion>);

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
    mut on_resampled_audio: impl FnMut(&[f32]) -> Result<(), String>,
) -> Result<VadAnalysisResult, String> {
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
        on_resampled_audio(&output)?;
        detector.accept_waveform(&output);
        resampled_samples = resampled_samples.saturating_add(output.len() as u64);
        collect_detected(&detector, &mut detected);
        decoded_frames = decoded_frames.saturating_add(samples.len() as u64);
        on_progress(decoded_frames.saturating_mul(1_000) / sample_rate.max(1) as u64)
    })?;
    detector.flush();
    collect_detected(&detector, &mut detected);
    let mut regions: Vec<SpeechRegion> = detected
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
    if let Some(first) = regions
        .first_mut()
        .filter(|first| first.speech_start_ms <= START_OF_AUDIO_RECOVERY_MS)
    {
        first.start_ms = 0;
        first.speech_start_ms = 0;
    }
    let utterances = coalesce_display_forced_splits(regions.clone());
    let utterance_starts_ms = display_utterance_starts(&utterances);
    let regions = split_long_regions(coalesce_short_detector_gaps(
        regions,
        seconds_to_ms(parameters.max_speech_duration),
    ));
    Ok((duration_ms, regions, utterance_starts_ms, utterances))
}

fn split_long_regions(regions: Vec<SpeechRegion>) -> Vec<SpeechRegion> {
    let mut split = Vec::with_capacity(regions.len());
    for region in regions {
        let mut speech_start_ms = region.speech_start_ms;
        while region.speech_end_ms.saturating_sub(speech_start_ms) > MAX_RECOGNITION_SPEECH_MS {
            let speech_end_ms = speech_start_ms.saturating_add(MAX_RECOGNITION_SPEECH_MS);
            split.push(SpeechRegion {
                start_ms: if speech_start_ms == region.speech_start_ms {
                    region.start_ms
                } else {
                    speech_start_ms.saturating_sub(REGION_PADDING_MS)
                },
                speech_start_ms,
                speech_end_ms,
                end_ms: speech_end_ms
                    .saturating_add(REGION_PADDING_MS)
                    .min(region.end_ms),
            });
            speech_start_ms = speech_end_ms;
        }
        split.push(SpeechRegion {
            start_ms: if speech_start_ms == region.speech_start_ms {
                region.start_ms
            } else {
                speech_start_ms.saturating_sub(REGION_PADDING_MS)
            },
            speech_start_ms,
            speech_end_ms: region.speech_end_ms,
            end_ms: region.end_ms,
        });
    }
    split
}

fn coalesce_display_forced_splits(regions: Vec<SpeechRegion>) -> Vec<SpeechRegion> {
    let mut utterances: Vec<SpeechRegion> = Vec::with_capacity(regions.len());
    for next in regions {
        let should_merge = utterances.last().is_some_and(|current| {
            next.speech_start_ms.saturating_sub(current.speech_end_ms) <= FORCED_SPLIT_MAX_GAP_MS
                && next.speech_end_ms.saturating_sub(current.speech_start_ms)
                    <= MAX_RECOGNITION_SPEECH_MS
        });
        if should_merge {
            let current = utterances.last_mut().expect("a previous utterance exists");
            current.speech_end_ms = current.speech_end_ms.max(next.speech_end_ms);
            current.end_ms = current.end_ms.max(next.end_ms);
        } else {
            utterances.push(next);
        }
    }
    utterances
}

fn display_utterance_starts(regions: &[SpeechRegion]) -> Vec<u64> {
    regions
        .windows(2)
        .filter_map(|pair| {
            (pair[1]
                .speech_start_ms
                .saturating_sub(pair[0].speech_end_ms)
                > FORCED_SPLIT_MAX_GAP_MS)
                .then_some(pair[1].speech_start_ms)
        })
        .collect()
}

fn coalesce_short_detector_gaps(
    regions: Vec<SpeechRegion>,
    max_speech_duration_ms: u64,
) -> Vec<SpeechRegion> {
    // Preserve short false-negative gaps only while the resulting speech span
    // remains safe for the recognizer. Longer windows can return tokens at both
    // ends while silently omitting speech in the middle.
    let recognition_speech_limit_ms = max_speech_duration_ms.min(MAX_RECOGNITION_SPEECH_MS);
    let mut coalesced: Vec<SpeechRegion> = Vec::with_capacity(regions.len());
    for next in regions {
        let should_merge = coalesced.last().is_some_and(|current| {
            let gap_ms = next.speech_start_ms.saturating_sub(current.speech_end_ms);
            let combined_duration_ms = next.speech_end_ms.saturating_sub(current.speech_start_ms);
            gap_ms <= MISSED_SPEECH_RECOVERY_GAP_MS
                && combined_duration_ms <= recognition_speech_limit_ms
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
        num_threads: crate::compute_tuning::profile().vad_threads,
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
    _runtime: crate::local_ai_runtime::RuntimeUseGuard,
    detector: VoiceActivityDetector,
    resampler: StreamingAreaResampler,
}

impl LiveVoiceActivityDetector {
    pub(crate) fn create(
        app: &tauri::AppHandle,
        model_path: &Path,
        source_sample_rate: u32,
        preset: VadPreset,
    ) -> Result<Self, String> {
        let runtime = crate::local_ai_runtime::begin_use(app)?;
        Ok(Self {
            _runtime: runtime,
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

fn seconds_to_ms(seconds: f32) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1_000.0).round() as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        coalesce_short_detector_gaps, display_utterance_starts, split_long_regions, SpeechRegion,
        REGION_PADDING_MS,
    };

    fn region(speech_start_ms: u64, speech_end_ms: u64) -> SpeechRegion {
        SpeechRegion {
            start_ms: speech_start_ms.saturating_sub(REGION_PADDING_MS),
            speech_start_ms,
            speech_end_ms,
            end_ms: speech_end_ms.saturating_add(REGION_PADDING_MS),
        }
    }

    #[test]
    fn keeps_detector_max_duration_boundaries_split_for_recognition() {
        let regions = coalesce_short_detector_gaps(
            vec![
                region(1_000, 31_000),
                region(31_032, 61_032),
                region(61_064, 72_000),
            ],
            30_000,
        );
        assert_eq!(regions.len(), 3);
        assert_eq!(regions[0].speech_start_ms, 1_000);
        assert_eq!(regions[0].speech_end_ms, 31_000);
        assert_eq!(regions[0].end_ms, 32_000);
    }

    #[test]
    fn preserves_real_silence_boundaries() {
        let regions = coalesce_short_detector_gaps(
            vec![region(1_000, 31_000), region(34_000, 50_000)],
            30_000,
        );
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn keeps_continuous_recognition_windows_bounded() {
        let regions =
            coalesce_short_detector_gaps(vec![region(0, 90_000), region(90_032, 180_032)], 30_000);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn recovers_short_detector_gaps_that_can_hide_quiet_words() {
        let regions = coalesce_short_detector_gaps(
            vec![region(41_022, 45_996), region(47_710, 52_620)],
            30_000,
        );
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].speech_start_ms, 41_022);
        assert_eq!(regions[0].speech_end_ms, 52_620);
    }

    #[test]
    fn does_not_chain_short_conversational_pauses_into_huge_regions() {
        let regions = coalesce_short_detector_gaps(
            vec![
                region(0, 30_000),
                region(31_000, 60_000),
                region(61_000, 90_000),
            ],
            30_000,
        );
        assert_eq!(regions.len(), 3);
        assert!(regions
            .iter()
            .all(|region| region.speech_end_ms - region.speech_start_ms <= 30_000));
    }

    #[test]
    fn does_not_merge_a_meeting_turn_into_a_long_lossy_window() {
        let regions = coalesce_short_detector_gaps(
            vec![
                region(70_878, 71_532),
                region(72_670, 78_380),
                region(79_134, 94_380),
                region(96_382, 99_884),
                region(100_510, 105_388),
                region(106_622, 109_196),
                region(109_886, 114_828),
            ],
            30_000,
        );
        assert!(regions.len() >= 2);
        assert!(regions
            .iter()
            .all(|region| region.speech_end_ms - region.speech_start_ms <= 30_000));
    }

    #[test]
    fn splits_the_observed_long_opening_turn_with_boundary_context() {
        let regions = split_long_regions(vec![SpeechRegion {
            start_ms: 8_054,
            speech_start_ms: 9_054,
            speech_end_ms: 35_980,
            end_ms: 36_980,
        }]);
        assert_eq!(regions.len(), 2);
        assert_eq!(
            (
                regions[0].start_ms,
                regions[0].speech_start_ms,
                regions[0].speech_end_ms,
                regions[0].end_ms,
            ),
            (8_054, 9_054, 24_054, 25_054)
        );
        assert_eq!(
            (
                regions[1].start_ms,
                regions[1].speech_start_ms,
                regions[1].speech_end_ms,
                regions[1].end_ms,
            ),
            (23_054, 24_054, 35_980, 36_980)
        );
    }

    #[test]
    fn display_boundaries_keep_real_pauses_but_ignore_forced_splits() {
        let regions = vec![
            region(0, 30_000),
            region(30_032, 60_032),
            region(61_000, 90_000),
        ];
        assert_eq!(display_utterance_starts(&regions), vec![61_000]);
    }
}
