use std::{fs::File, path::Path};

use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineRecognizerResult,
    OfflineTransducerModelConfig,
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use super::{
    local_models, segments_from_tokens, TokenTimeSource, Transcript, TranscriptSegment,
    TranscriptToken,
};

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
    let (tokens, segments) = normalize_result(&result, duration_ms);
    Ok(Transcript {
        provider: "local".into(),
        model: local_models::REAZONSPEECH_MODEL_ID.into(),
        language: "ja".into(),
        tokens,
        segments,
    })
}

fn normalize_result(
    result: &OfflineRecognizerResult,
    audio_duration_ms: u64,
) -> (Vec<TranscriptToken>, Vec<TranscriptSegment>) {
    let timestamps = result.timestamps.as_deref().unwrap_or_default();
    let durations = result.durations.as_deref().unwrap_or_default();
    let tokens: Vec<_> = result
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(index, text)| {
            let text = text.replace('▁', " ");
            if text.is_empty() {
                return None;
            }
            let start_ms = timestamps
                .get(index)
                .and_then(|value| valid_seconds_to_ms(*value))
                .map(|value| value.min(audio_duration_ms));
            let duration_ms = durations
                .get(index)
                .and_then(|value| valid_seconds_to_ms(*value));
            let next_start_ms = timestamps
                .get(index + 1)
                .and_then(|value| valid_seconds_to_ms(*value));
            let end_ms = start_ms.map(|start| {
                duration_ms
                    .map(|duration| start.saturating_add(duration))
                    .or(next_start_ms)
                    .unwrap_or(start)
                    .clamp(start, audio_duration_ms)
            });
            let end_time_source = end_ms.map(|_| {
                if duration_ms.is_some() {
                    TokenTimeSource::Provider
                } else {
                    TokenTimeSource::Inferred
                }
            });
            Some(TranscriptToken {
                text,
                start_ms,
                end_ms,
                start_time_source: start_ms.map(|_| TokenTimeSource::Provider),
                end_time_source,
                speaker: None,
                speaker_source: None,
                confidence: None,
            })
        })
        .collect();
    let mut segments = segments_from_tokens(&tokens);
    if segments.is_empty() && !result.text.trim().is_empty() {
        segments.push(TranscriptSegment {
            speaker: "Speaker 1".into(),
            start_ms: 0,
            end_ms: audio_duration_ms,
            text: result.text.trim().to_string(),
        });
    }
    (tokens, segments)
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

fn valid_seconds_to_ms(seconds: f32) -> Option<u64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1_000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::normalize_result;
    use sherpa_onnx::OfflineRecognizerResult;

    #[test]
    fn preserves_reazonspeech_token_timestamps_and_durations() {
        let result = OfflineRecognizerResult {
            text: "こんにちは。次です".into(),
            tokens: vec!["こん".into(), "にちは。".into(), "次です".into()],
            timestamps: Some(vec![0.12, 0.42, 1.8]),
            durations: Some(vec![0.3, 0.5, 0.4]),
        };
        let (tokens, segments) = normalize_result(&result, 3_000);
        assert_eq!(
            (tokens[0].start_ms, tokens[0].end_ms),
            (Some(120), Some(420))
        );
        assert_eq!(
            (tokens[2].start_ms, tokens[2].end_ms),
            (Some(1_800), Some(2_200))
        );
        assert_eq!(segments.len(), 2);
        assert_eq!((segments[1].start_ms, segments[1].end_ms), (1_800, 2_200));
    }

    #[test]
    fn infers_token_end_from_the_next_start_when_duration_is_missing() {
        let result = OfflineRecognizerResult {
            text: "前後".into(),
            tokens: vec!["前".into(), "後".into()],
            timestamps: Some(vec![0.1, 0.7]),
            durations: None,
        };
        let (tokens, _) = normalize_result(&result, 2_000);
        assert_eq!(tokens[0].end_ms, Some(700));
        assert_eq!(tokens[1].end_ms, Some(700));
    }
}
