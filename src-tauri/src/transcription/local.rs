use std::path::Path;

use super::{
    audio_decode::decode_mono, local_models, segments_from_tokens, vad, vad_models,
    TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineRecognizerResult,
    OfflineTransducerModelConfig,
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
    let (tokens, segments) = match vad_models::installed_model_path(app)? {
        Some(vad_model) => transcribe_speech_regions(&recognizer, audio_path, &vad_model)?,
        None => transcribe_full_audio(&recognizer, audio_path)?,
    };
    Ok(Transcript {
        provider: "local".into(),
        model: local_models::REAZONSPEECH_MODEL_ID.into(),
        language: "ja".into(),
        tokens,
        segments,
    })
}

fn transcribe_full_audio(
    recognizer: &OfflineRecognizer,
    audio_path: &Path,
) -> Result<(Vec<TranscriptToken>, Vec<TranscriptSegment>), String> {
    let stream = recognizer.create_stream();
    let duration_ms = decode_mono(audio_path, |sample_rate, samples| {
        stream.accept_waveform(sample_rate as i32, samples);
        Ok(())
    })?;
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or_else(|| "ReazonSpeechから文字起こし結果を取得できませんでした。".to_string())?;
    Ok(normalize_result(&result, duration_ms, 0))
}

fn transcribe_speech_regions(
    recognizer: &OfflineRecognizer,
    audio_path: &Path,
    vad_model: &Path,
) -> Result<(Vec<TranscriptToken>, Vec<TranscriptSegment>), String> {
    let mut tokens = Vec::new();
    vad::visit_speech_regions(audio_path, vad_model, |region| {
        let stream = recognizer.create_stream();
        stream.accept_waveform(vad::SAMPLE_RATE as i32, &region.samples);
        recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| "ReazonSpeechから文字起こし結果を取得できませんでした。".to_string())?;
        let (mut region_tokens, _) =
            normalize_result(&result, region.duration_ms(), region.start_ms);
        tokens.append(&mut region_tokens);
        Ok(())
    })?;
    let segments = segments_from_tokens(&tokens);
    Ok((tokens, segments))
}

fn normalize_result(
    result: &OfflineRecognizerResult,
    audio_duration_ms: u64,
    offset_ms: u64,
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
                start_ms: start_ms.map(|value| offset_ms.saturating_add(value)),
                end_ms: end_ms.map(|value| offset_ms.saturating_add(value)),
                start_time_source: start_ms.map(|_| TokenTimeSource::Provider),
                end_time_source,
                speaker: None,
                speaker_source: None,
                confidence: None,
            })
        })
        .collect();
    let mut tokens = tokens;
    if tokens.is_empty() && !result.text.trim().is_empty() {
        tokens.push(TranscriptToken {
            text: result.text.trim().to_string(),
            start_ms: Some(offset_ms),
            end_ms: Some(offset_ms.saturating_add(audio_duration_ms)),
            start_time_source: Some(TokenTimeSource::Inferred),
            end_time_source: Some(TokenTimeSource::Inferred),
            speaker: None,
            speaker_source: None,
            confidence: None,
        });
    }
    let mut segments = segments_from_tokens(&tokens);
    if segments.is_empty() && !result.text.trim().is_empty() {
        segments.push(TranscriptSegment {
            speaker: "Speaker 1".into(),
            start_ms: offset_ms,
            end_ms: offset_ms.saturating_add(audio_duration_ms),
            text: result.text.trim().to_string(),
        });
    }
    (tokens, segments)
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
        let (tokens, segments) = normalize_result(&result, 3_000, 0);
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
        let (tokens, _) = normalize_result(&result, 2_000, 0);
        assert_eq!(tokens[0].end_ms, Some(700));
        assert_eq!(tokens[1].end_ms, Some(700));
    }

    #[test]
    fn offsets_region_timestamps_to_the_original_audio_timeline() {
        let result = OfflineRecognizerResult {
            text: "会議".into(),
            tokens: vec!["会議".into()],
            timestamps: Some(vec![0.2]),
            durations: Some(vec![0.4]),
        };
        let (tokens, _) = normalize_result(&result, 1_000, 12_000);
        assert_eq!(
            (tokens[0].start_ms, tokens[0].end_ms),
            (Some(12_200), Some(12_600))
        );
    }
}
