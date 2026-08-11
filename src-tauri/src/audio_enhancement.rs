use sonora::{
    config::{AdaptiveDigital, GainController2, NoiseSuppression},
    AudioProcessing, Config, StreamConfig,
};

const CHANNELS: u16 = 1;
const FRAME_DURATION_MS: usize = 10;

/// Backend boundary for speech-oriented PCM enhancement.
///
/// Callers only depend on this contract; a native WebRTC APM adapter can replace
/// Sonora without changing decoding, VAD, or speech recognition code.
pub(crate) trait AudioEnhancer: Send {
    fn sample_rate(&self) -> u32;
    fn process(&mut self, frame: &[f32]) -> Result<ProcessedFrame, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProcessedFrame {
    pub(crate) samples: Vec<f32>,
}

pub(crate) struct SonoraBackend {
    processing: AudioProcessing,
    sample_rate: u32,
    frame_samples: usize,
}

impl SonoraBackend {
    pub(crate) fn new(sample_rate: u32) -> Result<Self, String> {
        if sample_rate == 0 || !sample_rate.is_multiple_of(100) {
            return Err("音声強調のサンプルレートでは10msフレームを構成できません。".into());
        }
        let stream = StreamConfig::new(sample_rate, CHANNELS);
        let processing = AudioProcessing::builder()
            .config(Config {
                noise_suppression: Some(NoiseSuppression::default()),
                gain_controller2: Some(GainController2 {
                    adaptive_digital: Some(AdaptiveDigital::default()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Ok(Self {
            processing,
            sample_rate,
            frame_samples: sample_rate as usize * FRAME_DURATION_MS / 1_000,
        })
    }
}

impl AudioEnhancer for SonoraBackend {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn process(&mut self, frame: &[f32]) -> Result<ProcessedFrame, String> {
        if frame.len() != self.frame_samples {
            return Err(format!(
                "音声強調には{}サンプルの10msフレームが必要です。",
                self.frame_samples
            ));
        }
        let mut samples = vec![0.0; self.frame_samples];
        self.processing
            .process_capture_f32(&[frame], &mut [&mut samples])
            .map_err(|error| format!("Sonoraで音声を強調できませんでした: {error}"))?;
        Ok(ProcessedFrame { samples })
    }
}

pub(crate) struct StreamingAudioEnhancer {
    backend: Box<dyn AudioEnhancer>,
    pending: Vec<f32>,
    frame_samples: usize,
}

impl StreamingAudioEnhancer {
    pub(crate) fn sonora(sample_rate: u32) -> Result<Self, String> {
        Self::new(Box::new(SonoraBackend::new(sample_rate)?))
    }

    fn new(backend: Box<dyn AudioEnhancer>) -> Result<Self, String> {
        let sample_rate = backend.sample_rate();
        if sample_rate == 0 || !sample_rate.is_multiple_of(100) {
            return Err("音声強調のサンプルレートでは10msフレームを構成できません。".into());
        }
        Ok(Self {
            backend,
            pending: Vec::new(),
            frame_samples: sample_rate as usize * FRAME_DURATION_MS / 1_000,
        })
    }

    pub(crate) fn accept(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
        self.pending.extend_from_slice(samples);
        let complete_samples = self.pending.len() / self.frame_samples * self.frame_samples;
        if complete_samples == 0 {
            return Ok(Vec::new());
        }
        let mut output = Vec::with_capacity(complete_samples);
        for frame in self.pending[..complete_samples].chunks_exact(self.frame_samples) {
            output.extend(self.backend.process(frame)?.samples);
        }
        self.pending.drain(..complete_samples);
        Ok(output)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<f32>, String> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let original_len = self.pending.len();
        self.pending.resize(self.frame_samples, 0.0);
        let processed = self.backend.process(&self.pending)?.samples;
        self.pending.clear();
        Ok(processed[..original_len].to_vec())
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicI64, Ordering},
            Mutex, OnceLock,
        },
    };

    use jni::{
        objects::{JClass, JShortArray},
        sys::{jint, jlong, jshortArray},
        JNIEnv,
    };

    use super::StreamingAudioEnhancer;

    static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
    static ENHANCERS: OnceLock<Mutex<HashMap<i64, StreamingAudioEnhancer>>> = OnceLock::new();

    fn enhancers() -> &'static Mutex<HashMap<i64, StreamingAudioEnhancer>> {
        ENHANCERS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn throw(env: &mut JNIEnv<'_>, message: impl AsRef<str>) {
        let _ = env.throw_new("java/lang/IllegalStateException", message.as_ref());
    }

    #[no_mangle]
    pub extern "system" fn Java_jp_mutsuna_echo_SonoraAudioEnhancer_create(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        sample_rate: jint,
    ) -> jlong {
        let sample_rate = match u32::try_from(sample_rate) {
            Ok(value) => value,
            Err(_) => {
                throw(&mut env, "音声強調のサンプルレートが不正です。");
                return 0;
            }
        };
        let enhancer = match StreamingAudioEnhancer::sonora(sample_rate) {
            Ok(enhancer) => enhancer,
            Err(error) => {
                throw(&mut env, error);
                return 0;
            }
        };
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        match enhancers().lock() {
            Ok(mut values) => {
                values.insert(handle, enhancer);
                handle
            }
            Err(_) => {
                throw(&mut env, "音声強調の状態を初期化できませんでした。");
                0
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_jp_mutsuna_echo_SonoraAudioEnhancer_process(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        handle: jlong,
        input: JShortArray<'_>,
    ) -> jshortArray {
        let length = match env.get_array_length(&input) {
            Ok(length) => length as usize,
            Err(error) => {
                throw(
                    &mut env,
                    format!("音声サンプルを読み取れませんでした: {error}"),
                );
                return std::ptr::null_mut();
            }
        };
        let mut pcm = vec![0i16; length];
        if let Err(error) = env.get_short_array_region(&input, 0, &mut pcm) {
            throw(
                &mut env,
                format!("音声サンプルを読み取れませんでした: {error}"),
            );
            return std::ptr::null_mut();
        }
        let input: Vec<f32> = pcm
            .into_iter()
            .map(|sample| sample as f32 / 32_768.0)
            .collect();
        let output = match enhancers().lock() {
            Ok(mut values) => match values.get_mut(&handle) {
                Some(enhancer) => enhancer.accept(&input),
                None => Err("音声強調セッションが見つかりません。".into()),
            },
            Err(_) => Err("音声強調の状態を取得できませんでした。".into()),
        };
        return_short_array(&mut env, output)
    }

    #[no_mangle]
    pub extern "system" fn Java_jp_mutsuna_echo_SonoraAudioEnhancer_finish(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        handle: jlong,
    ) -> jshortArray {
        let output = match enhancers().lock() {
            Ok(mut values) => match values.remove(&handle) {
                Some(mut enhancer) => enhancer.finish(),
                None => Err("音声強調セッションが見つかりません。".into()),
            },
            Err(_) => Err("音声強調の状態を取得できませんでした。".into()),
        };
        return_short_array(&mut env, output)
    }

    #[no_mangle]
    pub extern "system" fn Java_jp_mutsuna_echo_SonoraAudioEnhancer_destroy(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
        handle: jlong,
    ) {
        if let Ok(mut values) = enhancers().lock() {
            values.remove(&handle);
        }
    }

    fn return_short_array(env: &mut JNIEnv<'_>, result: Result<Vec<f32>, String>) -> jshortArray {
        let samples = match result {
            Ok(samples) => samples,
            Err(error) => {
                throw(env, error);
                return std::ptr::null_mut();
            }
        };
        let pcm: Vec<i16> = samples
            .into_iter()
            .map(|sample| {
                (sample.clamp(-1.0, 1.0) * 32_767.0)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16
            })
            .collect();
        let output = match env.new_short_array(pcm.len() as i32) {
            Ok(output) => output,
            Err(error) => {
                throw(env, format!("強調済み音声を確保できませんでした: {error}"));
                return std::ptr::null_mut();
            }
        };
        if let Err(error) = env.set_short_array_region(&output, 0, &pcm) {
            throw(env, format!("強調済み音声を書き込めませんでした: {error}"));
            return std::ptr::null_mut();
        }
        output.into_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioEnhancer, ProcessedFrame, SonoraBackend, StreamingAudioEnhancer};

    struct OffsetBackend;

    impl AudioEnhancer for OffsetBackend {
        fn sample_rate(&self) -> u32 {
            1_000
        }

        fn process(&mut self, frame: &[f32]) -> Result<ProcessedFrame, String> {
            Ok(ProcessedFrame {
                samples: frame.iter().map(|sample| sample + 1.0).collect(),
            })
        }
    }

    #[test]
    fn streams_exact_ten_ms_frames_and_preserves_the_tail_length() {
        let mut enhancer = StreamingAudioEnhancer::new(Box::new(OffsetBackend)).expect("enhancer");
        assert!(enhancer.accept(&[0.0; 6]).expect("first chunk").is_empty());
        assert_eq!(
            enhancer.accept(&[0.0; 9]).expect("second chunk"),
            vec![1.0; 10]
        );
        assert_eq!(enhancer.finish().expect("tail"), vec![1.0; 5]);
    }

    #[test]
    fn sonora_enables_noise_suppression_and_adaptive_agc2() {
        let mut backend = SonoraBackend::new(48_000).expect("Sonora backend");
        let config = backend.processing.config();
        assert!(config.noise_suppression.is_some());
        assert!(config
            .gain_controller2
            .as_ref()
            .and_then(|agc| agc.adaptive_digital.as_ref())
            .is_some());

        let processed = backend.process(&[0.0; 480]).expect("process 10ms frame");
        assert_eq!(processed.samples.len(), 480);
        assert!(processed.samples.iter().all(|sample| sample.is_finite()));
    }
}
