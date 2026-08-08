use std::{fs::File, path::Path};

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use super::{local_models, Transcript, TranscriptSegment};

const ENCODER: &str = "encoder-epoch-99-avg-1.int8.onnx";
const DECODER: &str = "decoder-epoch-99-avg-1.onnx";
const JOINER: &str = "joiner-epoch-99-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";

pub(crate) fn transcribe(
    app: &tauri::AppHandle,
    audio_path: &Path,
    model_id: &str,
) -> Result<Transcript, String> {
    if model_id != local_models::REAZONSPEECH_MODEL_ID {
        return Err("選択したローカルSTTモデルには対応していません。".into());
    }
    let model = local_models::verify_reazonspeech_installation(app)?;
    let path_string = |name: &str| model.join(name).to_string_lossy().into_owned();
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(path_string(ENCODER)),
        decoder: Some(path_string(DECODER)),
        joiner: Some(path_string(JOINER)),
    };
    config.model_config.tokens = Some(path_string(TOKENS));
    config.model_config.num_threads = available_threads();
    config.model_config.provider = Some("cpu".into());
    config.decoding_method = Some("greedy_search".into());

    let recognizer = OfflineRecognizer::create(&config)
        .ok_or_else(|| "ReazonSpeechの推論エンジンを初期化できませんでした。モデルを再インストールしてください。".to_string())?;
    let stream = recognizer.create_stream();
    let duration_ms = feed_audio(&stream, audio_path)?;
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or_else(|| "ReazonSpeechから文字起こし結果を取得できませんでした。".to_string())?;
    let text = result.text.trim().to_string();
    let segments = if text.is_empty() {
        Vec::new()
    } else {
        vec![TranscriptSegment {
            speaker: "Speaker 1".into(),
            start_ms: result
                .timestamps
                .as_ref()
                .and_then(|values| values.first())
                .map_or(0, |value| seconds_to_ms(*value)),
            end_ms: duration_ms,
            text,
        }]
    };
    Ok(Transcript {
        provider: "local".into(),
        model: local_models::REAZONSPEECH_MODEL_ID.into(),
        language: "ja".into(),
        segments,
    })
}

fn feed_audio(stream: &sherpa_onnx::OfflineStream, path: &Path) -> Result<u64, String> {
    let file =
        File::open(path).map_err(|error| format!("音声ファイルを開けませんでした: {error}"))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("音声形式を読み取れませんでした: {error}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "音声トラックが見つかりません。".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("音声デコーダーを準備できませんでした: {error}"))?;
    let mut sample_rate = 0u32;
    let mut frames = 0u64;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(format!("音声を読み込めませんでした: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(format!("音声をデコードできませんでした: {error}")),
        };
        let spec = *decoded.spec();
        if sample_rate == 0 {
            sample_rate = spec.rate;
        }
        if spec.rate != sample_rate {
            return Err("途中でサンプルレートが変わる音声には対応していません。".into());
        }
        let channels = spec.channels.count();
        if channels == 0 {
            continue;
        }
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        let mono: Vec<f32> = samples
            .samples()
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        frames = frames.saturating_add(mono.len() as u64);
        stream.accept_waveform(sample_rate as i32, &mono);
    }
    if sample_rate == 0 || frames == 0 {
        return Err("音声データが含まれていません。".into());
    }
    Ok(frames.saturating_mul(1_000) / sample_rate as u64)
}

fn available_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8) as i32)
        .unwrap_or(2)
}

fn seconds_to_ms(seconds: f32) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1_000.0).round() as u64
    } else {
        0
    }
}
