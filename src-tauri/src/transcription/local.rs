use std::{collections::HashMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use super::{
    audio_decode, context::TranscriptionContext, local_models, local_settings,
    repair_inferred_token_ends, segments_from_tokens, vad, vad_models, vad_settings,
    TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
};
use crate::commands::transcribe::{
    publish_transcription_progress, TranscriptionProgress, TranscriptionStage,
};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineRecognizerResult, OfflineStream,
    OfflineTransducerModelConfig,
};

const ENCODER: &str = "encoder-epoch-99-avg-1.int8.onnx";
const DECODER: &str = "decoder-epoch-99-avg-1.onnx";
const JOINER: &str = "joiner-epoch-99-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";
const VAD_PROGRESS_UNIT_MS: u64 = 1_000;
const MAX_VAD_PROGRESS_UNITS: u32 = 100;
const VAD_CACHE_SCHEMA: u8 = 5;
const DISPLAY_WORD_CONTINUATION_GAP_MS: u64 = 160;
const RECOVERY_EDGE_GAP_MS: u64 = 1_200;
const RECOVERY_MIN_UTTERANCE_MS: u64 = 200;
const RECOVERY_MAX_UTTERANCE_MS: u64 = 15_000;

struct PendingRecognition {
    region_index: usize,
    stream: OfflineStream,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VadCacheDocument {
    schema_version: u8,
    duration_ms: u64,
    regions: Vec<vad::SpeechRegion>,
    utterance_starts_ms: Vec<u64>,
    utterances: Vec<vad::SpeechRegion>,
}

pub(crate) fn transcribe(
    app: &tauri::AppHandle,
    audio_path: &Path,
    audio_duration_ms: u64,
    model_id: &str,
    context: Option<&TranscriptionContext>,
) -> Result<Transcript, String> {
    let total_timer = crate::processing_metrics::StageTimer::start(
        "local_transcription",
        "total",
        Some(audio_duration_ms),
    );
    if model_id != local_models::REAZONSPEECH_MODEL_ID {
        return Err("選択したローカルSTTモデルには対応していません。".into());
    }
    let model_timer =
        crate::processing_metrics::StageTimer::start("local_transcription", "load_models", None);
    let model = local_models::verify_reazonspeech_installation(app)?;
    let path_string = |name: &str| model.join(name).to_string_lossy().into_owned();
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(path_string(ENCODER)),
        decoder: Some(path_string(DECODER)),
        joiner: Some(path_string(JOINER)),
    };
    config.model_config.tokens = Some(path_string(TOKENS));
    config.model_config.num_threads = crate::compute_tuning::profile().stt_threads;
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
    let vad_model = vad_models::installed_model_path(app)?.ok_or_else(|| {
        "文字起こしに必要なVADモデルがありません。設定から再インストールしてください。".to_string()
    })?;
    let preset = vad_settings::current_preset(app)?;
    model_timer.finish();
    let (tokens, segments) = transcribe_speech_regions(
        app,
        &recognizer,
        audio_path,
        audio_duration_ms,
        &vad_model,
        preset,
        hotwords.as_deref(),
        accurate,
    )?;
    let transcript = Transcript {
        provider: "local".into(),
        model: local_models::REAZONSPEECH_MODEL_ID.into(),
        language: "ja".into(),
        tokens,
        segments,
    };
    total_timer.finish();
    Ok(transcript)
}

fn transcribe_speech_regions(
    app: &tauri::AppHandle,
    recognizer: &OfflineRecognizer,
    audio_path: &Path,
    audio_duration_ms: u64,
    vad_model: &Path,
    preset: vad_settings::VadPreset,
    hotwords: Option<&str>,
    accurate: bool,
) -> Result<(Vec<TranscriptToken>, Vec<TranscriptSegment>), String> {
    let total_vad_units = vad_total_units(audio_duration_ms);
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(
            TranscriptionStage::DetectingSpeech,
            0,
            Some(total_vad_units),
        ),
    );
    let mut published_vad_units = 0u32;
    let fingerprint_timer = crate::processing_metrics::StageTimer::start(
        "local_transcription",
        "fingerprint_audio",
        Some(audio_duration_ms),
    );
    let cache_key = match crate::inference_cache::audio_fingerprint(audio_path) {
        Ok(fingerprint) => Some(crate::inference_cache::cache_key(
            &fingerprint,
            &format!(
                "vad-schema={VAD_CACHE_SCHEMA};model={};preset={preset:?};recovery-gap-ms={}",
                vad_models::MODEL_VERSION,
                vad::MISSED_SPEECH_RECOVERY_GAP_MS
            ),
        )),
        Err(error) => {
            eprintln!("Could not fingerprint audio for VAD cache: {error}");
            None
        }
    };
    fingerprint_timer.finish();
    let vad_timer = crate::processing_metrics::StageTimer::start(
        "local_transcription",
        "vad",
        Some(audio_duration_ms),
    );
    let cached = cache_key.as_deref().and_then(|key| {
        match crate::inference_cache::load_json::<VadCacheDocument>(app, "vad", key) {
            Ok(Some(document))
                if document.schema_version == VAD_CACHE_SCHEMA
                    && document.duration_ms > 0
                    && valid_cached_regions(&document.regions, document.duration_ms)
                    && valid_utterance_starts(
                        &document.utterance_starts_ms,
                        document.duration_ms,
                    )
                    && valid_cached_regions(&document.utterances, document.duration_ms) =>
            {
                Some((
                    document.duration_ms,
                    document.regions,
                    document.utterance_starts_ms,
                    document.utterances,
                ))
            }
            Ok(_) => None,
            Err(error) => {
                eprintln!("Could not load VAD cache: {error}");
                None
            }
        }
    });
    let mut pcm_cache = None;
    let (decoded_duration_ms, regions, utterance_starts_ms, utterances) = if let Some(cached) =
        cached
    {
        eprintln!("processing_cache pipeline=local_transcription stage=vad hit=true");
        cached
    } else {
        eprintln!("processing_cache pipeline=local_transcription stage=vad hit=false");
        let mut pcm_writer = match crate::pcm_cache::PcmCacheWriter::create(app, vad::SAMPLE_RATE) {
            Ok(writer) => Some(writer),
            Err(error) => {
                eprintln!("Could not create temporary PCM cache: {error}");
                None
            }
        };
        let detected = vad::visit_speech_regions(
            audio_path,
            vad_model,
            preset,
            |processed_ms| {
                let completed =
                    vad_completed_units(processed_ms, audio_duration_ms, total_vad_units);
                if completed > published_vad_units {
                    published_vad_units = completed;
                    publish_transcription_progress(
                        app,
                        TranscriptionProgress::new(
                            TranscriptionStage::DetectingSpeech,
                            completed,
                            Some(total_vad_units),
                        ),
                    );
                }
                Ok(())
            },
            |samples| {
                let write_error = pcm_writer
                    .as_mut()
                    .and_then(|writer| writer.write(samples).err());
                if let Some(error) = write_error {
                    eprintln!(
                        "temporary PCM cache disabled after write failure; falling back to decode: {error:#}"
                    );
                    pcm_writer = None;
                }
                Ok(())
            },
        )?;
        pcm_cache = pcm_writer.and_then(|writer| match writer.finish() {
            Ok(cache) => Some(cache),
            Err(error) => {
                eprintln!("Could not finalize temporary PCM cache: {error}");
                None
            }
        });
        if let Some(key) = cache_key.as_deref() {
            let document = VadCacheDocument {
                schema_version: VAD_CACHE_SCHEMA,
                duration_ms: detected.0,
                regions: detected.1.clone(),
                utterance_starts_ms: detected.2.clone(),
                utterances: detected.3.clone(),
            };
            if let Err(error) = crate::inference_cache::store_json(app, "vad", key, &document) {
                eprintln!("Could not store VAD cache: {error}");
            }
        }
        detected
    };
    vad_timer.finish();
    eprintln!(
        "processing_vad recognition_regions={} display_utterances={}",
        regions.len(),
        utterance_starts_ms.len().saturating_add(1)
    );
    let total_chunks = u32::try_from(regions.len()).unwrap_or(u32::MAX);
    if published_vad_units < total_vad_units {
        publish_transcription_progress(
            app,
            TranscriptionProgress::new(
                TranscriptionStage::DetectingSpeech,
                total_vad_units,
                Some(total_vad_units),
            ),
        );
    }
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::Transcribing, 0, Some(total_chunks)),
    );
    if total_chunks == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut tokens = Vec::new();
    let mut completed_chunks = 0u32;
    let batch_size = crate::compute_tuning::stt_batch_size(
        regions
            .iter()
            .map(vad::SpeechRegion::duration_ms)
            .max()
            .unwrap_or(1),
    );
    let mut pending = Vec::with_capacity(batch_size);
    let windows = regions
        .iter()
        .map(|region| (region.start_ms, region.duration_ms()))
        .collect::<Vec<_>>();
    let recognition_timer = crate::processing_metrics::StageTimer::start(
        "local_transcription",
        "recognition",
        Some(audio_duration_ms),
    );
    let mut accept_region = |region_index, sample_rate, region_samples: &[f32]| {
        let stream = create_stream(recognizer, hotwords);
        stream.accept_waveform(sample_rate as i32, region_samples);
        pending.push(PendingRecognition {
            region_index,
            stream,
        });
        if pending.len() >= batch_size {
            flush_recognition_batch(
                app,
                recognizer,
                &regions,
                &mut pending,
                &mut tokens,
                &mut completed_chunks,
                total_chunks,
            )?;
        }
        Ok(())
    };
    if let Some(cache) = pcm_cache.as_ref() {
        eprintln!("processing_cache pipeline=local_transcription stage=decoded_pcm hit=true");
        cache.read_regions(&windows, &mut accept_region)?;
    } else {
        eprintln!("processing_cache pipeline=local_transcription stage=decoded_pcm hit=false");
        audio_decode::decode_mono_regions_resampled(
            audio_path,
            vad::SAMPLE_RATE,
            &windows,
            &mut accept_region,
        )?;
    }
    flush_recognition_batch(
        app,
        recognizer,
        &regions,
        &mut pending,
        &mut tokens,
        &mut completed_chunks,
        total_chunks,
    )?;
    if accurate {
        recover_sparse_utterances(
            app,
            recognizer,
            audio_path,
            pcm_cache.as_ref(),
            &utterances,
            hotwords,
            &mut tokens,
            &mut completed_chunks,
            total_chunks,
        )?;
    }
    recognition_timer.finish();
    repair_inferred_token_ends(&mut tokens, Some(decoded_duration_ms));
    assign_vad_utterances(&mut tokens, &utterance_starts_ms);
    let segments = segments_from_tokens(&tokens);
    Ok((tokens, segments))
}

fn valid_cached_regions(regions: &[vad::SpeechRegion], duration_ms: u64) -> bool {
    regions.len() <= 1_000_000
        && regions.iter().all(|region| {
            region.start_ms <= region.speech_start_ms
                && region.speech_start_ms < region.speech_end_ms
                && region.speech_end_ms <= region.end_ms
                && region.end_ms <= duration_ms.saturating_add(1_000)
        })
        && regions
            .windows(2)
            .all(|pair| pair[0].start_ms <= pair[1].start_ms)
}

fn valid_utterance_starts(starts: &[u64], duration_ms: u64) -> bool {
    starts.len() <= 1_000_000
        && starts.iter().all(|start| *start <= duration_ms)
        && starts.windows(2).all(|pair| pair[0] < pair[1])
}

fn assign_vad_utterances(tokens: &mut [TranscriptToken], utterance_starts_ms: &[u64]) {
    let mut previous_utterance_id = None;
    let mut previous_end_ms = None;
    for token in tokens {
        let Some(start_ms) = token.start_ms else {
            continue;
        };
        let end_ms = token.end_ms.unwrap_or(start_ms);
        let midpoint_ms = start_ms.saturating_add(end_ms.saturating_sub(start_ms) / 2);
        let detected_utterance_id =
            u32::try_from(utterance_starts_ms.partition_point(|start| *start <= midpoint_ms))
                .unwrap_or(u32::MAX);
        let continues_previous_word = previous_utterance_id.zip(previous_end_ms).is_some_and(
            |(previous_id, previous_end)| {
                previous_id != detected_utterance_id
                    && start_ms.saturating_sub(previous_end) <= DISPLAY_WORD_CONTINUATION_GAP_MS
            },
        );
        let utterance_id = if continues_previous_word {
            previous_utterance_id.unwrap_or(detected_utterance_id)
        } else {
            detected_utterance_id
        };
        token.utterance_id = Some(utterance_id);
        previous_utterance_id = Some(utterance_id);
        previous_end_ms = Some(end_ms);
    }
}

fn recover_sparse_utterances(
    app: &tauri::AppHandle,
    recognizer: &OfflineRecognizer,
    audio_path: &Path,
    pcm_cache: Option<&crate::pcm_cache::PcmCacheFile>,
    utterances: &[vad::SpeechRegion],
    hotwords: Option<&str>,
    tokens: &mut Vec<TranscriptToken>,
    completed_chunks: &mut u32,
    primary_chunks: u32,
) -> Result<(), String> {
    let candidates = sparse_utterance_candidates(tokens, utterances);
    if candidates.is_empty() {
        eprintln!("processing_recovery candidates=0 augmented=0 added_tokens=0");
        return Ok(());
    }
    let recovery_chunks = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    let total_chunks = primary_chunks.saturating_add(recovery_chunks);
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(
            TranscriptionStage::Transcribing,
            *completed_chunks,
            Some(total_chunks),
        ),
    );
    let batch_size = crate::compute_tuning::stt_batch_size(
        candidates
            .iter()
            .map(vad::SpeechRegion::duration_ms)
            .max()
            .unwrap_or(1),
    );
    let windows = candidates
        .iter()
        .map(|region| (region.start_ms, region.duration_ms()))
        .collect::<Vec<_>>();
    let mut pending = Vec::with_capacity(batch_size);
    let mut augmented = 0usize;
    let mut added_tokens = 0usize;
    let mut accept = |region_index: usize, sample_rate: u32, samples: &[f32]| {
        let stream = create_stream(recognizer, hotwords);
        stream.accept_waveform(sample_rate as i32, samples);
        pending.push(PendingRecognition {
            region_index,
            stream,
        });
        if pending.len() >= batch_size {
            flush_recovery_batch(
                app,
                recognizer,
                &candidates,
                &mut pending,
                tokens,
                completed_chunks,
                total_chunks,
                &mut augmented,
                &mut added_tokens,
            )?;
        }
        Ok(())
    };
    if let Some(cache) = pcm_cache {
        cache.read_regions(&windows, &mut accept)?;
    } else {
        audio_decode::decode_mono_regions_resampled(
            audio_path,
            vad::SAMPLE_RATE,
            &windows,
            &mut accept,
        )?;
    }
    flush_recovery_batch(
        app,
        recognizer,
        &candidates,
        &mut pending,
        tokens,
        completed_chunks,
        total_chunks,
        &mut augmented,
        &mut added_tokens,
    )?;
    tokens.sort_by_key(|token| token.start_ms.unwrap_or(u64::MAX));
    eprintln!(
        "processing_recovery candidates={} augmented={augmented} added_tokens={added_tokens}",
        candidates.len(),
    );
    Ok(())
}

fn sparse_utterance_candidates(
    tokens: &[TranscriptToken],
    utterances: &[vad::SpeechRegion],
) -> Vec<vad::SpeechRegion> {
    utterances
        .iter()
        .filter(|utterance| {
            let duration_ms = utterance
                .speech_end_ms
                .saturating_sub(utterance.speech_start_ms);
            if !(RECOVERY_MIN_UTTERANCE_MS..=RECOVERY_MAX_UTTERANCE_MS).contains(&duration_ms) {
                return false;
            }
            let mut midpoints = tokens
                .iter()
                .filter_map(token_midpoint_ms)
                .filter(|midpoint| {
                    *midpoint >= utterance.speech_start_ms && *midpoint < utterance.speech_end_ms
                });
            let first = midpoints.next();
            let last = midpoints.last().or(first);
            first.is_none()
                || first.is_some_and(|value| {
                    value.saturating_sub(utterance.speech_start_ms) >= RECOVERY_EDGE_GAP_MS
                })
                || last.is_some_and(|value| {
                    utterance.speech_end_ms.saturating_sub(value) >= RECOVERY_EDGE_GAP_MS
                })
        })
        .cloned()
        .collect()
}

fn flush_recovery_batch(
    app: &tauri::AppHandle,
    recognizer: &OfflineRecognizer,
    candidates: &[vad::SpeechRegion],
    pending: &mut Vec<PendingRecognition>,
    tokens: &mut Vec<TranscriptToken>,
    completed_chunks: &mut u32,
    total_chunks: u32,
    augmented: &mut usize,
    added_tokens: &mut usize,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let streams = pending.iter().map(|item| &item.stream).collect::<Vec<_>>();
    recognizer.decode_multiple_streams(&streams);
    for item in pending.drain(..) {
        let utterance = candidates
            .get(item.region_index)
            .ok_or_else(|| "補完認識区間の対応関係が不正です。".to_string())?;
        let result = item
            .stream
            .get_result()
            .ok_or_else(|| "ReazonSpeechから補完認識結果を取得できませんでした。".to_string())?;
        let (mut recovered, _) =
            normalize_result(&result, utterance.duration_ms(), utterance.start_ms);
        recovered.retain(|token| {
            token_midpoint_ms(token).is_some_and(|midpoint| {
                midpoint >= utterance.speech_start_ms && midpoint < utterance.speech_end_ms
            })
        });
        let added = augment_uncovered_utterance_edges(tokens, &mut recovered, utterance);
        if added > 0 {
            *augmented = augmented.saturating_add(1);
            *added_tokens = added_tokens.saturating_add(added);
        }
        *completed_chunks = completed_chunks.saturating_add(1);
    }
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(
            TranscriptionStage::Transcribing,
            *completed_chunks,
            Some(total_chunks),
        ),
    );
    Ok(())
}

fn augment_uncovered_utterance_edges(
    tokens: &mut Vec<TranscriptToken>,
    recovered: &mut Vec<TranscriptToken>,
    utterance: &vad::SpeechRegion,
) -> usize {
    let existing_coverage = tokens
        .iter()
        .filter_map(token_time_range)
        .filter(|(start, end)| *end > utterance.speech_start_ms && *start < utterance.speech_end_ms)
        .fold(None::<(u64, u64)>, |coverage, (start, end)| {
            Some(match coverage {
                Some((first, last)) => (first.min(start), last.max(end)),
                None => (start, end),
            })
        });
    recovered.retain(|token| {
        let Some((start, end)) = token_time_range(token) else {
            return false;
        };
        if end <= utterance.speech_start_ms || start >= utterance.speech_end_ms {
            return false;
        }
        match existing_coverage {
            None => true,
            Some((existing_start, existing_end)) => end <= existing_start || start >= existing_end,
        }
    });
    let added = recovered.len();
    tokens.append(recovered);
    added
}

fn token_time_range(token: &TranscriptToken) -> Option<(u64, u64)> {
    let start = token.start_ms?;
    let end = token.end_ms.unwrap_or(start).max(start);
    Some((start, end))
}

fn token_midpoint_ms(token: &TranscriptToken) -> Option<u64> {
    let start_ms = token.start_ms?;
    let end_ms = token.end_ms.unwrap_or(start_ms);
    Some(start_ms.saturating_add(end_ms.saturating_sub(start_ms) / 2))
}

fn flush_recognition_batch(
    app: &tauri::AppHandle,
    recognizer: &OfflineRecognizer,
    regions: &[vad::SpeechRegion],
    pending: &mut Vec<PendingRecognition>,
    tokens: &mut Vec<TranscriptToken>,
    completed_chunks: &mut u32,
    total_chunks: u32,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let streams = pending.iter().map(|item| &item.stream).collect::<Vec<_>>();
    recognizer.decode_multiple_streams(&streams);
    for item in pending.drain(..) {
        let region = regions
            .get(item.region_index)
            .ok_or_else(|| "VAD区間の対応関係が不正です。".to_string())?;
        let result = item
            .stream
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
        *completed_chunks = completed_chunks.saturating_add(1);
    }
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(
            TranscriptionStage::Transcribing,
            *completed_chunks,
            Some(total_chunks),
        ),
    );
    Ok(())
}

fn vad_total_units(duration_ms: u64) -> u32 {
    (duration_ms
        .saturating_add(VAD_PROGRESS_UNIT_MS - 1)
        .saturating_div(VAD_PROGRESS_UNIT_MS)
        .min(MAX_VAD_PROGRESS_UNITS as u64) as u32)
        .max(1)
}

fn vad_completed_units(processed_ms: u64, duration_ms: u64, total_units: u32) -> u32 {
    if duration_ms == 0 {
        return total_units;
    }
    ((processed_ms as u128 * total_units as u128) / duration_ms as u128).min(total_units as u128)
        as u32
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
                utterance_id: None,
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
            utterance_id: None,
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

fn valid_seconds_to_ms(seconds: f32) -> Option<u64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1_000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::{
        assign_vad_utterances, augment_uncovered_utterance_edges, normalize_result,
        sparse_utterance_candidates, tokenize_hotword, vad_completed_units, vad_total_units,
    };
    use crate::transcription::vad;
    use crate::transcription::{TokenTimeSource, TranscriptToken};
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
    fn assigns_tokens_to_original_vad_utterances_after_recognition_coalescing() {
        let result = OfflineRecognizerResult {
            text: "前中後".into(),
            tokens: vec!["前".into(), "中".into(), "後".into()],
            timestamps: Some(vec![1.0, 3.0, 5.0]),
            durations: Some(vec![0.2, 0.2, 0.2]),
        };
        let (mut tokens, _) = normalize_result(&result, 6_000, 0);
        assign_vad_utterances(&mut tokens, &[2_500, 4_500]);
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.utterance_id)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn keeps_contiguous_japanese_characters_together_across_noisy_vad_boundaries() {
        let result = OfflineRecognizerResult {
            text: "連携手続".into(),
            tokens: vec!["連".into(), "携".into(), "手".into(), "続".into()],
            timestamps: Some(vec![19.354, 19.634, 26.194, 26.514]),
            durations: Some(vec![0.280, 0.280, 0.320, 0.200]),
        };
        let (mut tokens, _) = normalize_result(&result, 30_000, 0);
        assign_vad_utterances(&mut tokens, &[19_614, 26_558]);
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.utterance_id)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(0), Some(1), Some(1)]
        );
    }

    #[test]
    fn accurate_recovery_targets_uncovered_utterance_edges_only() {
        let result = OfflineRecognizerResult {
            text: "途中まで短い".into(),
            tokens: vec!["途中まで".into(), "短い".into()],
            timestamps: Some(vec![1.0, 20.1]),
            durations: Some(vec![5.0, 1.5]),
        };
        let (tokens, _) = normalize_result(&result, 22_000, 0);
        let utterances = vec![
            vad::SpeechRegion {
                start_ms: 0,
                speech_start_ms: 0,
                speech_end_ms: 10_000,
                end_ms: 10_300,
            },
            vad::SpeechRegion {
                start_ms: 19_800,
                speech_start_ms: 20_000,
                speech_end_ms: 22_000,
                end_ms: 22_000,
            },
        ];
        let candidates = sparse_utterance_candidates(&tokens, &utterances);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].speech_start_ms, 0);
    }

    fn timed_token(text: &str, start_ms: u64, end_ms: u64) -> TranscriptToken {
        TranscriptToken {
            text: text.into(),
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
            start_time_source: Some(TokenTimeSource::Provider),
            end_time_source: Some(TokenTimeSource::Provider),
            speaker: None,
            speaker_source: None,
            confidence: None,
            utterance_id: None,
        }
    }

    #[test]
    fn recovery_preserves_a_boundary_character_that_overlaps_the_utterance() {
        let utterance = vad::SpeechRegion {
            start_ms: 19_314,
            speech_start_ms: 19_614,
            speech_end_ms: 23_518,
            end_ms: 23_818,
        };
        let mut existing = vec![
            timed_token("連", 19_354, 19_634),
            timed_token("携", 19_634, 19_914),
        ];
        let mut recovered = vec![
            timed_token("連", 19_414, 19_674),
            timed_token("携", 19_674, 19_954),
        ];
        let added = augment_uncovered_utterance_edges(&mut existing, &mut recovered, &utterance);
        assert_eq!(added, 0);
        assert_eq!(
            existing
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "連携"
        );
    }

    #[test]
    fn recovery_adds_only_a_missing_utterance_tail() {
        let utterance = vad::SpeechRegion {
            start_ms: 3_314,
            speech_start_ms: 3_614,
            speech_end_ms: 13_234,
            end_ms: 13_534,
        };
        let mut existing = vec![timed_token("上から", 9_000, 10_034)];
        let mut recovered = vec![
            timed_token("上から", 9_100, 10_000),
            timed_token("えっと", 10_400, 11_100),
            timed_token("あれですね", 11_100, 13_000),
        ];
        let added = augment_uncovered_utterance_edges(&mut existing, &mut recovered, &utterance);
        assert_eq!(added, 2);
        existing.sort_by_key(|token| token.start_ms);
        assert_eq!(
            existing
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "上からえっとあれですね"
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

    #[test]
    fn vad_progress_is_bounded_and_proportional() {
        assert_eq!(vad_total_units(0), 1);
        assert_eq!(vad_total_units(1_000), 1);
        assert_eq!(vad_total_units(5_500), 6);
        assert_eq!(vad_total_units(3_829_000), 100);
        assert_eq!(vad_completed_units(0, 3_829_000, 100), 0);
        assert_eq!(vad_completed_units(1_914_500, 3_829_000, 100), 50);
        assert_eq!(vad_completed_units(3_829_000, 3_829_000, 100), 100);
    }
}
