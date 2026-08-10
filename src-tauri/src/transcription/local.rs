use std::{collections::HashMap, fs, path::Path};

use super::{
    audio_decode::decode_mono, context::TranscriptionContext, local_models, local_settings,
    repair_inferred_token_ends, segments_from_tokens, vad, vad_models, vad_settings,
    TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
};
use crate::commands::transcribe::{
    publish_transcription_progress, TranscriptionProgress, TranscriptionStage,
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
    context: Option<&TranscriptionContext>,
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
    let settings = local_settings::current(app)?;
    let hotwords = encode_hotwords(&model.join(TOKENS), context)?;
    let accurate = settings.mode == local_settings::LocalRecognitionMode::Accurate;
    let use_beam = accurate || hotwords.is_some();
    config.decoding_method = Some(if use_beam {
        "modified_beam_search".into()
    } else {
        "greedy_search".into()
    });
    config.max_active_paths = if accurate { 8 } else { 4 };
    config.hotwords_score = 1.5;

    let recognizer = OfflineRecognizer::create(&config)
        .ok_or_else(|| "ReazonSpeechの推論エンジンを初期化できませんでした。モデルを再インストールしてください。".to_string())?;
    let (tokens, segments) = match vad_models::installed_model_path(app)? {
        Some(vad_model) => {
            let preset = vad_settings::current_preset(app)?;
            transcribe_speech_regions(
                app,
                &recognizer,
                audio_path,
                &vad_model,
                preset,
                hotwords.as_deref(),
            )?
        }
        None => {
            publish_transcription_progress(
                app,
                TranscriptionProgress::new(TranscriptionStage::Transcribing, 0, None),
            );
            transcribe_full_audio(&recognizer, audio_path, hotwords.as_deref())?
        }
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
    hotwords: Option<&str>,
) -> Result<(Vec<TranscriptToken>, Vec<TranscriptSegment>), String> {
    let stream = create_stream(recognizer, hotwords);
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
    app: &tauri::AppHandle,
    recognizer: &OfflineRecognizer,
    audio_path: &Path,
    vad_model: &Path,
    preset: vad_settings::VadPreset,
    hotwords: Option<&str>,
) -> Result<(Vec<TranscriptToken>, Vec<TranscriptSegment>), String> {
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::DetectingSpeech, 0, None),
    );
    let mut total_chunks = 0u32;
    let audio_duration_ms = vad::visit_speech_regions(audio_path, vad_model, preset, |_| {
        total_chunks = total_chunks.saturating_add(1);
        Ok(())
    })?;
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::Transcribing, 0, Some(total_chunks)),
    );

    let mut tokens = Vec::new();
    let mut completed_chunks = 0u32;
    vad::visit_speech_regions(audio_path, vad_model, preset, |region| {
        let stream = create_stream(recognizer, hotwords);
        stream.accept_waveform(vad::SAMPLE_RATE as i32, &region.samples);
        recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| "ReazonSpeechから文字起こし結果を取得できませんでした。".to_string())?;
        let (mut region_tokens, _) =
            normalize_result(&result, region.duration_ms(), region.start_ms);
        // Padded windows overlap by design. Keep only tokens whose midpoint
        // belongs to the original VAD region, retaining boundary context for
        // recognition without duplicating transcript text.
        region_tokens.retain(|token| {
            let start = token.start_ms.unwrap_or(region.speech_start_ms);
            let end = token.end_ms.unwrap_or(start);
            let midpoint = start.saturating_add(end.saturating_sub(start) / 2);
            midpoint >= region.speech_start_ms && midpoint < region.speech_end_ms
        });
        tokens.append(&mut region_tokens);
        completed_chunks = completed_chunks.saturating_add(1);
        publish_transcription_progress(
            app,
            TranscriptionProgress::new(
                TranscriptionStage::Transcribing,
                completed_chunks,
                Some(total_chunks),
            ),
        );
        Ok(())
    })?;
    repair_inferred_token_ends(&mut tokens, Some(audio_duration_ms));
    let segments = segments_from_tokens(&tokens);
    Ok((tokens, segments))
}

fn create_stream(
    recognizer: &OfflineRecognizer,
    hotwords: Option<&str>,
) -> sherpa_onnx::OfflineStream {
    hotwords.map_or_else(
        || recognizer.create_stream(),
        |value| recognizer.create_stream_with_hotwords(value),
    )
}

/// sherpa-onnx expects hotwords as token sequences when the model has no BPE
/// vocabulary metadata. Convert user-facing terms with the model's pinned
/// tokens.txt instead of passing arbitrary strings to the native decoder.
fn encode_hotwords(
    tokens_path: &Path,
    context: Option<&TranscriptionContext>,
) -> Result<Option<String>, String> {
    let Some(context) = context.filter(|value| !value.terms.is_empty()) else {
        return Ok(None);
    };
    let text = fs::read_to_string(tokens_path)
        .map_err(|error| format!("重要用語の辞書を読み込めませんでした: {error}"))?;
    let mut vocabulary = text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|token| !token.starts_with('<') && *token != "▁")
        .map(str::to_string)
        .collect::<Vec<_>>();
    vocabulary.sort_by_key(|token| std::cmp::Reverse(token.chars().count()));
    vocabulary.dedup();

    let encoded = context
        .terms
        .iter()
        .filter_map(|term| tokenize_hotword(term, &vocabulary))
        .map(|tokens| tokens.join(" "))
        .collect::<Vec<_>>();
    Ok((!encoded.is_empty()).then(|| encoded.join("/")))
}

fn tokenize_hotword(term: &str, vocabulary: &[String]) -> Option<Vec<String>> {
    fn visit(
        term: &str,
        offset: usize,
        vocabulary: &[String],
        memo: &mut HashMap<usize, Option<Vec<String>>>,
    ) -> Option<Vec<String>> {
        if offset == term.len() {
            return Some(Vec::new());
        }
        if let Some(cached) = memo.get(&offset) {
            return cached.clone();
        }
        let remaining = &term[offset..];
        let result = vocabulary.iter().find_map(|token| {
            let surface = token.strip_prefix('▁').unwrap_or(token);
            if surface.is_empty() || !remaining.starts_with(surface) {
                return None;
            }
            let next = offset + surface.len();
            if !term.is_char_boundary(next) {
                return None;
            }
            visit(term, next, vocabulary, memo).map(|mut suffix| {
                suffix.insert(0, token.clone());
                suffix
            })
        });
        memo.insert(offset, result.clone());
        result
    }

    let compact = term.split_whitespace().collect::<String>();
    (!compact.is_empty())
        .then(|| visit(&compact, 0, vocabulary, &mut HashMap::new()))
        .flatten()
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
    repair_inferred_token_ends(
        &mut tokens,
        Some(offset_ms.saturating_add(audio_duration_ms)),
    );
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
    let maximum = if cfg!(mobile) { 4 } else { 8 };
    std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, maximum) as i32)
        .unwrap_or(2)
}

fn valid_seconds_to_ms(seconds: f32) -> Option<u64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1_000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::{normalize_result, tokenize_hotword};
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
        assert_eq!(tokens[1].end_ms, Some(1_700));
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

    #[test]
    fn converts_user_terms_to_the_models_longest_token_sequence() {
        let vocabulary = vec![
            "Mutsuna".into(),
            "Mu".into(),
            "tsu".into(),
            "na".into(),
            "Echo".into(),
        ];
        assert_eq!(
            tokenize_hotword("Mutsuna Echo", &vocabulary),
            Some(vec!["Mutsuna".into(), "Echo".into()])
        );
        assert_eq!(tokenize_hotword("未登録", &vocabulary), None);
    }
}
