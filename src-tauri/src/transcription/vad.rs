use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::{
    audio_decode::decode_mono,
    vad_settings::{VadParameters, VadPreset},
};

pub(crate) const SAMPLE_RATE: u32 = 16_000;

pub(crate) struct SpeechRegion {
    pub(crate) start_ms: u64,
    pub(crate) samples: Vec<f32>,
}

impl SpeechRegion {
    pub(crate) fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / SAMPLE_RATE as u64
    }
}

/// Detects speech without changing the source timeline. Each callback receives
/// 16 kHz mono samples and their absolute offset in the original audio.
pub(crate) fn visit_speech_regions(
    audio_path: &Path,
    model_path: &Path,
    preset: VadPreset,
    mut visit: impl FnMut(SpeechRegion) -> Result<(), String>,
) -> Result<u64, String> {
    let detector = create_detector(model_path, preset.parameters())?;
    let mut resampler: Option<StreamingAreaResampler> = None;
    let duration_ms = decode_mono(audio_path, |sample_rate, samples| {
        let resampler =
            resampler.get_or_insert_with(|| StreamingAreaResampler::new(sample_rate, SAMPLE_RATE));
        if resampler.source_rate != sample_rate {
            return Err("途中でサンプルレートが変わる音声には対応していません。".into());
        }
        let output = resampler.process(samples);
        detector.accept_waveform(&output);
        drain(&detector, &mut visit)
    })?;
    detector.flush();
    drain(&detector, &mut visit)?;
    Ok(duration_ms)
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

fn drain(
    detector: &VoiceActivityDetector,
    visit: &mut impl FnMut(SpeechRegion) -> Result<(), String>,
) -> Result<(), String> {
    while let Some(segment) = detector.front() {
        let start_sample = segment.start().max(0) as u64;
        visit(SpeechRegion {
            start_ms: start_sample.saturating_mul(1_000) / SAMPLE_RATE as u64,
            samples: segment.samples().to_vec(),
        })?;
        detector.pop();
    }
    Ok(())
}

/// Streaming box-filter resampler. It bounds memory to one decoded packet and
/// avoids aliasing when common 44.1/48 kHz meeting audio is reduced to 16 kHz.
struct StreamingAreaResampler {
    source_rate: u32,
    target_rate: u32,
    output_remaining: u64,
    weighted_sum: f64,
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
    use super::StreamingAreaResampler;

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
}
