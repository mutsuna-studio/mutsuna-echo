use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

fn main() {
    let model = std::env::var("MUTSUNA_SILERO_VAD_MODEL")
        .expect("MUTSUNA_SILERO_VAD_MODEL must point to silero_vad.onnx");
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model),
            threshold: 0.25,
            min_silence_duration: 0.5,
            min_speech_duration: 0.25,
            window_size: 512,
            max_speech_duration: 30.0,
        },
        sample_rate: 16_000,
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    };
    let detector = VoiceActivityDetector::create(&config, 60.0)
        .expect("Silero VAD model must initialize with the bundled runtime");
    detector.accept_waveform(&vec![0.0; 16_000]);
    detector.flush();
    println!("Silero VAD model and bundled runtime are compatible");
}
