use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, LinearResampler, OfflineSpeakerDiarization,
    OfflineSpeakerDiarizationConfig, OfflineSpeakerSegmentationModelConfig,
    OfflineSpeakerSegmentationPyannoteModelConfig, SpeakerEmbeddingExtractor,
    SpeakerEmbeddingExtractorConfig,
};
use tauri::AppHandle;

use super::{audio_decode::decode_mono, diarization::SpeakerTurn, diarization_models};

const SAMPLE_RATE: usize = 16_000;
const WINDOW_SAMPLES: usize = 20 * 60 * SAMPLE_RATE;
const OVERLAP_SAMPLES: usize = 30 * SAMPLE_RATE;
const STEP_SAMPLES: usize = WINDOW_SAMPLES - OVERLAP_SAMPLES;
const MIN_CHUNK_SAMPLES: usize = SAMPLE_RATE;
const MAX_EMBEDDING_AUDIO_SAMPLES: usize = 30 * SAMPLE_RATE;
const MIN_EXCLUSIVE_SEGMENT_SAMPLES: usize = SAMPLE_RATE / 2;
const LOCAL_CLUSTER_THRESHOLD: f32 = 0.9;
const GLOBAL_EMBEDDING_THRESHOLD: f32 = 0.5;
const MIN_REQUESTED_SPEAKER_ACTIVITY_MS: u64 = 1_000;
const MAX_AUTO_SPEAKERS: usize = 10;
const DIARIZATION_CACHE_SCHEMA: u8 = 3;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LocalDiarizationStage {
    LoadingModel,
    DecodingAudio,
    DiarizingChunks,
    StitchingSpeakers,
    Finalizing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDiarizationProgress {
    pub(crate) stage: LocalDiarizationStage,
    pub(crate) completed_chunks: u32,
    pub(crate) total_chunks: Option<u32>,
    pub(crate) processed_ms: u64,
    pub(crate) total_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalDiarizationOptions {
    pub(crate) speaker_count: Option<u8>,
    pub(crate) total_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiarizationRunMetadata {
    pub(crate) model_id: String,
    pub(crate) model_version: String,
    pub(crate) completed_at: String,
    pub(crate) requested_speaker_count: Option<u8>,
    pub(crate) estimated_speaker_count: u32,
    pub(crate) chunk_duration_ms: u64,
    pub(crate) chunk_overlap_ms: u64,
    pub(crate) local_cluster_threshold: f32,
    pub(crate) global_embedding_threshold: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDiarizationOutput {
    pub(crate) turns: Vec<SpeakerTurn>,
    pub(crate) metadata: DiarizationRunMetadata,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiarizationCacheDocument {
    schema_version: u8,
    output: LocalDiarizationOutput,
}

#[derive(Debug, Clone)]
struct LocalTurn {
    start_sample: u64,
    end_sample: u64,
    speaker: i32,
}

#[derive(Debug)]
struct ChunkResult {
    index: usize,
    start_sample: u64,
    end_sample: u64,
    turns: Vec<LocalTurn>,
    embeddings: HashMap<i32, Vec<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct NodeId {
    chunk: usize,
    speaker: i32,
}

pub(crate) fn diarize(
    app: &AppHandle,
    audio_path: &Path,
    options: LocalDiarizationOptions,
    cancelled: &AtomicBool,
    mut report: impl FnMut(LocalDiarizationProgress),
) -> Result<LocalDiarizationOutput, String> {
    let total_timer = crate::processing_metrics::StageTimer::start(
        "local_diarization",
        "total",
        options.total_ms,
    );
    if options
        .speaker_count
        .is_some_and(|count| !(1..=10).contains(&count))
    {
        return Err("話者数は1〜10人で指定してください。".into());
    }
    check_cancelled(cancelled)?;
    let fingerprint_timer = crate::processing_metrics::StageTimer::start(
        "local_diarization",
        "fingerprint_audio",
        options.total_ms,
    );
    let cache_key = match crate::inference_cache::audio_fingerprint(audio_path) {
        Ok(fingerprint) => Some(crate::inference_cache::cache_key(
            &fingerprint,
            &format!(
                "diarization-schema={DIARIZATION_CACHE_SCHEMA};model={}:{};speakers={:?};window={WINDOW_SAMPLES};overlap={OVERLAP_SAMPLES};local={LOCAL_CLUSTER_THRESHOLD};global={GLOBAL_EMBEDDING_THRESHOLD};concurrent-speech-cannot-link=true;max-auto-speakers={MAX_AUTO_SPEAKERS}",
                diarization_models::MODEL_PACK_ID,
                diarization_models::MODEL_PACK_VERSION,
                options.speaker_count
            ),
        )),
        Err(error) => {
            eprintln!("Could not fingerprint audio for diarization cache: {error}");
            None
        }
    };
    fingerprint_timer.finish();
    if let Some(key) = cache_key.as_deref() {
        match crate::inference_cache::load_json::<DiarizationCacheDocument>(app, "diarization", key)
        {
            Ok(Some(document))
                if document.schema_version == DIARIZATION_CACHE_SCHEMA
                    && valid_cached_turns(&document.output.turns, options.total_ms)
                    && valid_speaker_distribution(
                        &document.output.turns,
                        options.speaker_count,
                    ) =>
            {
                eprintln!("processing_cache pipeline=local_diarization stage=diarization hit=true");
                report(LocalDiarizationProgress {
                    stage: LocalDiarizationStage::Finalizing,
                    completed_chunks: total_chunk_count(options.total_ms.unwrap_or(0)),
                    total_chunks: options.total_ms.map(total_chunk_count),
                    processed_ms: options.total_ms.unwrap_or(0),
                    total_ms: options.total_ms,
                });
                total_timer.finish();
                return Ok(document.output);
            }
            Ok(_) => {}
            Err(error) => eprintln!("Could not load diarization cache: {error}"),
        }
    }
    eprintln!("processing_cache pipeline=local_diarization stage=diarization hit=false");
    report(progress(LocalDiarizationStage::LoadingModel, 0, options, 0));
    let model_timer =
        crate::processing_metrics::StageTimer::start("local_diarization", "load_models", None);
    let model_directory = diarization_models::installed_model_directory(app)?;
    let segmentation_path = model_directory
        .join(diarization_models::SEGMENTATION_FILE)
        .to_string_lossy()
        .into_owned();
    let embedding_path = model_directory
        .join(diarization_models::EMBEDDING_FILE)
        .to_string_lossy()
        .into_owned();
    let threads = crate::compute_tuning::profile().diarization_threads;
    let embedding_config = SpeakerEmbeddingExtractorConfig {
        model: Some(embedding_path),
        num_threads: threads,
        debug: false,
        provider: Some("cpu".into()),
    };
    let diarizer = OfflineSpeakerDiarization::create(&OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation_path),
            },
            num_threads: threads,
            debug: false,
            provider: Some("cpu".into()),
        },
        embedding: embedding_config.clone(),
        clustering: FastClusteringConfig {
            num_clusters: options.speaker_count.map(i32::from).unwrap_or(-1),
            threshold: LOCAL_CLUSTER_THRESHOLD,
        },
        min_duration_on: 0.3,
        min_duration_off: 0.5,
    })
    .ok_or_else(|| {
        "話者分離モデルを読み込めませんでした。再インストールしてください。".to_string()
    })?;
    if diarizer.sample_rate() != SAMPLE_RATE as i32 {
        return Err("話者分離モデルのサンプルレートに対応していません。".into());
    }
    let extractor = SpeakerEmbeddingExtractor::create(&embedding_config)
        .ok_or_else(|| "話者埋め込みモデルを読み込めませんでした。".to_string())?;
    model_timer.finish();
    check_cancelled(cancelled)?;
    report(progress(
        LocalDiarizationStage::DecodingAudio,
        0,
        options,
        0,
    ));

    let mut chunks = Vec::new();
    let mut pending = Vec::<f32>::new();
    let mut pending_start = 0u64;
    let mut resampler: Option<LinearResampler> = None;
    let total_chunks = options.total_ms.map(total_chunk_count);
    let inference_timer = crate::processing_metrics::StageTimer::start(
        "local_diarization",
        "decode_and_infer",
        options.total_ms,
    );
    decode_mono(audio_path, |input_rate, samples| {
        check_cancelled(cancelled)?;
        let output = if input_rate == SAMPLE_RATE as u32 {
            samples.to_vec()
        } else {
            if resampler.is_none() {
                resampler = LinearResampler::create(input_rate as i32, SAMPLE_RATE as i32);
            }
            resampler
                .as_ref()
                .ok_or_else(|| "音声を16 kHzへ変換できませんでした。".to_string())?
                .resample(samples, false)
        };
        pending.extend(output);
        while pending.len() >= WINDOW_SAMPLES {
            let samples = pending[..WINDOW_SAMPLES].to_vec();
            let index = chunks.len();
            chunks.push(process_chunk(
                index,
                pending_start,
                samples,
                &diarizer,
                &extractor,
            )?);
            pending.drain(..STEP_SAMPLES);
            pending_start = pending_start.saturating_add(STEP_SAMPLES as u64);
            report(LocalDiarizationProgress {
                stage: LocalDiarizationStage::DiarizingChunks,
                completed_chunks: chunks.len() as u32,
                total_chunks,
                processed_ms: samples_to_ms(pending_start),
                total_ms: options.total_ms,
            });
            check_cancelled(cancelled)?;
        }
        Ok(())
    })?;
    if let Some(resampler) = &resampler {
        pending.extend(resampler.resample(&[], true));
    }
    let pending_end = pending_start.saturating_add(pending.len() as u64);
    let contains_new_audio = chunks
        .last()
        .is_none_or(|chunk| pending_end > chunk.end_sample);
    if chunks.is_empty() || (pending.len() >= MIN_CHUNK_SAMPLES && contains_new_audio) {
        check_cancelled(cancelled)?;
        let index = chunks.len();
        chunks.push(process_chunk(
            index,
            pending_start,
            pending,
            &diarizer,
            &extractor,
        )?);
        report(LocalDiarizationProgress {
            stage: LocalDiarizationStage::DiarizingChunks,
            completed_chunks: chunks.len() as u32,
            total_chunks: Some(chunks.len() as u32),
            processed_ms: options.total_ms.unwrap_or_else(|| {
                chunks
                    .last()
                    .map(|chunk| samples_to_ms(chunk.end_sample))
                    .unwrap_or(0)
            }),
            total_ms: options.total_ms,
        });
    }
    inference_timer.finish();
    check_cancelled(cancelled)?;
    report(LocalDiarizationProgress {
        stage: LocalDiarizationStage::StitchingSpeakers,
        completed_chunks: chunks.len() as u32,
        total_chunks: Some(chunks.len() as u32),
        processed_ms: options.total_ms.unwrap_or(0),
        total_ms: options.total_ms,
    });
    let stitching_timer = crate::processing_metrics::StageTimer::start(
        "local_diarization",
        "stitch_speakers",
        options.total_ms,
    );
    let turns = stitch_chunks(&chunks, options.speaker_count)?;
    stitching_timer.finish();
    if !valid_speaker_distribution(&turns, options.speaker_count) {
        return Err(
            "指定した話者数に対して話者分離結果が極端に偏りました。話者数を自動にするか、別の話者数で再実行してください。"
                .into(),
        );
    }
    check_cancelled(cancelled)?;
    report(LocalDiarizationProgress {
        stage: LocalDiarizationStage::Finalizing,
        completed_chunks: chunks.len() as u32,
        total_chunks: Some(chunks.len() as u32),
        processed_ms: options.total_ms.unwrap_or(0),
        total_ms: options.total_ms,
    });
    let estimated_speaker_count = turns
        .iter()
        .map(|turn| turn.speaker.as_str())
        .collect::<BTreeSet<_>>()
        .len() as u32;
    let output = LocalDiarizationOutput {
        turns,
        metadata: DiarizationRunMetadata {
            model_id: diarization_models::MODEL_PACK_ID.into(),
            model_version: diarization_models::MODEL_PACK_VERSION.into(),
            completed_at: chrono::Utc::now().to_rfc3339(),
            requested_speaker_count: options.speaker_count,
            estimated_speaker_count,
            chunk_duration_ms: samples_to_ms(WINDOW_SAMPLES as u64),
            chunk_overlap_ms: samples_to_ms(OVERLAP_SAMPLES as u64),
            local_cluster_threshold: LOCAL_CLUSTER_THRESHOLD,
            global_embedding_threshold: GLOBAL_EMBEDDING_THRESHOLD,
        },
    };
    if let Some(key) = cache_key.as_deref() {
        let document = DiarizationCacheDocument {
            schema_version: DIARIZATION_CACHE_SCHEMA,
            output: LocalDiarizationOutput {
                turns: output.turns.clone(),
                metadata: output.metadata.clone(),
            },
        };
        if let Err(error) = crate::inference_cache::store_json(app, "diarization", key, &document) {
            eprintln!("Could not store diarization cache: {error}");
        }
    }
    total_timer.finish();
    Ok(output)
}

fn valid_cached_turns(turns: &[SpeakerTurn], total_ms: Option<u64>) -> bool {
    turns.len() <= 1_000_000
        && turns.iter().all(|turn| {
            !turn.speaker.trim().is_empty()
                && turn.end_ms > turn.start_ms
                && total_ms.is_none_or(|total| turn.end_ms <= total.saturating_add(1_000))
        })
        && turns
            .windows(2)
            .all(|pair| pair[0].start_ms <= pair[1].start_ms)
}

fn valid_speaker_distribution(turns: &[SpeakerTurn], requested_speakers: Option<u8>) -> bool {
    if turns.is_empty() {
        return true;
    }
    let mut activity = HashMap::<&str, u64>::new();
    for turn in turns {
        let duration = turn.end_ms.saturating_sub(turn.start_ms);
        let total = activity.entry(turn.speaker.as_str()).or_default();
        *total = total.saturating_add(duration);
    }
    let Some(requested_speakers) = requested_speakers.map(usize::from) else {
        return activity.len() <= MAX_AUTO_SPEAKERS;
    };
    activity.len() == requested_speakers
        && (requested_speakers <= 1
            || activity
                .values()
                .all(|duration| *duration >= MIN_REQUESTED_SPEAKER_ACTIVITY_MS))
}

fn process_chunk(
    index: usize,
    start_sample: u64,
    samples: Vec<f32>,
    diarizer: &OfflineSpeakerDiarization,
    extractor: &SpeakerEmbeddingExtractor,
) -> Result<ChunkResult, String> {
    let result = diarizer
        .process(&samples)
        .ok_or_else(|| "話者分離モデルが結果を返しませんでした。".to_string())?;
    let mut turns = Vec::new();
    for segment in result.sort_by_start_time() {
        if !segment.start.is_finite()
            || !segment.end.is_finite()
            || segment.end <= segment.start
            || segment.speaker < 0
        {
            continue;
        }
        let local_start = seconds_to_samples(segment.start).min(samples.len() as u64);
        let local_end = seconds_to_samples(segment.end).min(samples.len() as u64);
        if local_end > local_start {
            turns.push(LocalTurn {
                start_sample: start_sample + local_start,
                end_sample: start_sample + local_end,
                speaker: segment.speaker,
            });
        }
    }
    let embeddings = representative_embeddings(start_sample, &samples, &turns, extractor);
    Ok(ChunkResult {
        index,
        start_sample,
        end_sample: start_sample + samples.len() as u64,
        turns,
        embeddings,
    })
}

fn representative_embeddings(
    chunk_start: u64,
    samples: &[f32],
    turns: &[LocalTurn],
    extractor: &SpeakerEmbeddingExtractor,
) -> HashMap<i32, Vec<f32>> {
    let speakers = turns
        .iter()
        .map(|turn| turn.speaker)
        .collect::<BTreeSet<_>>();
    let mut embeddings = HashMap::new();
    for speaker in speakers {
        let mut weighted_embeddings = Vec::<(usize, Vec<f32>)>::new();
        let mut collected_samples = 0usize;
        for turn in turns.iter().filter(|turn| turn.speaker == speaker) {
            let overlaps_other = turns.iter().any(|other| {
                other.speaker != speaker
                    && other.start_sample < turn.end_sample
                    && other.end_sample > turn.start_sample
            });
            if overlaps_other {
                continue;
            }
            let start = (turn.start_sample - chunk_start) as usize;
            let end = (turn.end_sample - chunk_start) as usize;
            if end.saturating_sub(start) < MIN_EXCLUSIVE_SEGMENT_SAMPLES || end > samples.len() {
                continue;
            }
            let remaining = MAX_EMBEDDING_AUDIO_SAMPLES.saturating_sub(collected_samples);
            let end = end.min(start + remaining);
            let audio = &samples[start..end];
            let Some(stream) = extractor.create_stream() else {
                continue;
            };
            stream.accept_waveform(SAMPLE_RATE as i32, audio);
            stream.input_finished();
            if extractor.is_ready(&stream) {
                if let Some(mut embedding) = extractor.compute(&stream) {
                    normalize_embedding(&mut embedding);
                    weighted_embeddings.push((audio.len(), embedding));
                }
            }
            collected_samples += audio.len();
            if collected_samples >= MAX_EMBEDDING_AUDIO_SAMPLES {
                break;
            }
        }
        let Some((_, first)) = weighted_embeddings.first() else {
            continue;
        };
        let mut representative = vec![0.0; first.len()];
        let total_weight = weighted_embeddings
            .iter()
            .map(|(weight, _)| *weight)
            .sum::<usize>();
        for (weight, embedding) in weighted_embeddings {
            let scale = weight as f32 / total_weight as f32;
            for (target, value) in representative.iter_mut().zip(embedding) {
                *target += value * scale;
            }
        }
        normalize_embedding(&mut representative);
        embeddings.insert(speaker, representative);
    }
    embeddings
}

fn stitch_chunks(
    chunks: &[ChunkResult],
    requested_speakers: Option<u8>,
) -> Result<Vec<SpeakerTurn>, String> {
    let nodes = chunks
        .iter()
        .flat_map(|chunk| {
            chunk
                .turns
                .iter()
                .map(|turn| NodeId {
                    chunk: chunk.index,
                    speaker: turn.speaker,
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut union = UnionFind::new(nodes.len());
    let node_index = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (*node, index))
        .collect::<HashMap<_, _>>();
    for pair in chunks.windows(2) {
        merge_overlap_anchors(&pair[0], &pair[1], &node_index, &mut union);
    }
    let mut clusters = cluster_members(&nodes, &mut union);
    let initial_cluster_count = clusters.len();
    agglomerate_embeddings(chunks, &nodes, &mut clusters, requested_speakers);
    eprintln!(
        "processing_diarization_clusters initial={} final={} requested={requested_speakers:?}",
        initial_cluster_count,
        clusters.len()
    );

    let cluster_for_node = clusters
        .iter()
        .enumerate()
        .flat_map(|(cluster, members)| members.iter().map(move |member| (*member, cluster)))
        .collect::<HashMap<_, _>>();
    let mut owned = Vec::<(u64, u64, usize)>::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let left = if index == 0 {
            chunk.start_sample
        } else {
            chunk.start_sample + OVERLAP_SAMPLES as u64 / 2
        };
        let right = chunks
            .get(index + 1)
            .map(|next| next.start_sample + OVERLAP_SAMPLES as u64 / 2)
            .unwrap_or(chunk.end_sample);
        for turn in &chunk.turns {
            let start = turn.start_sample.max(left);
            let end = turn.end_sample.min(right);
            if end <= start {
                continue;
            }
            let node = node_index[&NodeId {
                chunk: chunk.index,
                speaker: turn.speaker,
            }];
            owned.push((start, end, cluster_for_node[&node]));
        }
    }
    owned.sort_by_key(|(start, end, _)| (*start, *end));
    let mut first_seen = BTreeMap::<usize, u64>::new();
    for (start, _, cluster) in &owned {
        first_seen.entry(*cluster).or_insert(*start);
    }
    let mut ordered_clusters = first_seen.into_iter().collect::<Vec<_>>();
    ordered_clusters.sort_by_key(|(_, start)| *start);
    let labels = ordered_clusters
        .into_iter()
        .enumerate()
        .map(|(index, (cluster, _))| (cluster, format!("Speaker {}", index + 1)))
        .collect::<HashMap<_, _>>();
    let mut output: Vec<SpeakerTurn> = Vec::new();
    for (start, end, cluster) in owned {
        let speaker = labels
            .get(&cluster)
            .cloned()
            .ok_or_else(|| "話者ラベルを確定できませんでした。".to_string())?;
        let start_ms = samples_to_ms(start);
        let end_ms = samples_to_ms(end);
        if let Some(previous) = output.last_mut() {
            if previous.speaker == speaker && start_ms <= previous.end_ms.saturating_add(50) {
                previous.end_ms = previous.end_ms.max(end_ms);
                continue;
            }
        }
        output.push(SpeakerTurn {
            speaker,
            start_ms,
            end_ms,
            confidence: None,
        });
    }
    Ok(output)
}

fn merge_overlap_anchors(
    left: &ChunkResult,
    right: &ChunkResult,
    node_index: &HashMap<NodeId, usize>,
    union: &mut UnionFind,
) {
    let overlap_start = right.start_sample;
    let overlap_end = left.end_sample.min(right.end_sample);
    if overlap_end <= overlap_start {
        return;
    }
    let mut scores = HashMap::<(i32, i32), u64>::new();
    for a in &left.turns {
        for b in &right.turns {
            let start = a.start_sample.max(b.start_sample).max(overlap_start);
            let end = a.end_sample.min(b.end_sample).min(overlap_end);
            if end > start {
                *scores.entry((a.speaker, b.speaker)).or_default() += end - start;
            }
        }
    }
    let best_left = mutual_bests(&scores, true);
    let best_right = mutual_bests(&scores, false);
    for (left_speaker, (right_speaker, score)) in best_left {
        if score < (SAMPLE_RATE / 2) as u64
            || best_right.get(&right_speaker).map(|(speaker, _)| *speaker) != Some(left_speaker)
        {
            continue;
        }
        if let (Some(a), Some(b)) = (
            node_index.get(&NodeId {
                chunk: left.index,
                speaker: left_speaker,
            }),
            node_index.get(&NodeId {
                chunk: right.index,
                speaker: right_speaker,
            }),
        ) {
            union.union(*a, *b);
        }
    }
}

fn mutual_bests(scores: &HashMap<(i32, i32), u64>, by_left: bool) -> HashMap<i32, (i32, u64)> {
    let mut best = HashMap::new();
    for (&(left, right), &score) in scores {
        let (key, value) = if by_left {
            (left, right)
        } else {
            (right, left)
        };
        let entry = best.entry(key).or_insert((value, score));
        if score > entry.1 {
            *entry = (value, score);
        }
    }
    best
}

fn cluster_members(nodes: &[NodeId], union: &mut UnionFind) -> Vec<Vec<usize>> {
    let mut grouped = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..nodes.len() {
        grouped.entry(union.find(index)).or_default().push(index);
    }
    grouped.into_values().collect()
}

fn agglomerate_embeddings(
    chunks: &[ChunkResult],
    nodes: &[NodeId],
    clusters: &mut Vec<Vec<usize>>,
    requested_speakers: Option<u8>,
) {
    let target = requested_speakers.map(usize::from).unwrap_or(1);
    loop {
        if requested_speakers.is_some() && clusters.len() <= target {
            break;
        }
        let mut best: Option<(usize, usize, f32)> = None;
        for left in 0..clusters.len() {
            for right in left + 1..clusters.len() {
                if clusters_have_concurrent_speech(&clusters[left], &clusters[right], nodes, chunks)
                {
                    continue;
                }
                let score = average_similarity(&clusters[left], &clusters[right], nodes, chunks);
                let auto_over_limit =
                    requested_speakers.is_none() && clusters.len() > MAX_AUTO_SPEAKERS;
                let Some(score) = score
                    .or_else(|| (requested_speakers.is_some() || auto_over_limit).then_some(-2.0))
                else {
                    continue;
                };
                if best.is_none_or(|(_, _, current)| score > current) {
                    best = Some((left, right, score));
                }
            }
        }
        let Some((left, right, score)) = best else {
            break;
        };
        if requested_speakers.is_none()
            && clusters.len() <= MAX_AUTO_SPEAKERS
            && score < GLOBAL_EMBEDDING_THRESHOLD
        {
            break;
        }
        let removed = clusters.remove(right);
        clusters[left].extend(removed);
    }
}

fn clusters_have_concurrent_speech(
    left: &[usize],
    right: &[usize],
    nodes: &[NodeId],
    chunks: &[ChunkResult],
) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            let first = nodes[*a];
            let second = nodes[*b];
            if first.chunk != second.chunk {
                return false;
            }
            let turns = &chunks[first.chunk].turns;
            turns
                .iter()
                .filter(|turn| turn.speaker == first.speaker)
                .any(|first_turn| {
                    turns
                        .iter()
                        .filter(|turn| turn.speaker == second.speaker)
                        .any(|second_turn| {
                            first_turn.start_sample < second_turn.end_sample
                                && second_turn.start_sample < first_turn.end_sample
                        })
                })
        })
    })
}

fn average_similarity(
    left: &[usize],
    right: &[usize],
    nodes: &[NodeId],
    chunks: &[ChunkResult],
) -> Option<f32> {
    let mut total = 0.0;
    let mut count = 0usize;
    for a in left {
        for b in right {
            let first = chunks[nodes[*a].chunk].embeddings.get(&nodes[*a].speaker);
            let second = chunks[nodes[*b].chunk].embeddings.get(&nodes[*b].speaker);
            if let (Some(first), Some(second)) = (first, second) {
                total += cosine_similarity(first, second);
                count += 1;
            }
        }
    }
    (count > 0).then_some(total / count as f32)
}

fn normalize_embedding(embedding: &mut [f32]) {
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm > f32::EPSILON {
        for value in embedding {
            *value /= norm;
        }
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn progress(
    stage: LocalDiarizationStage,
    completed_chunks: u32,
    options: LocalDiarizationOptions,
    processed_ms: u64,
) -> LocalDiarizationProgress {
    LocalDiarizationProgress {
        stage,
        completed_chunks,
        total_chunks: options.total_ms.map(total_chunk_count),
        processed_ms,
        total_ms: options.total_ms,
    }
}

fn total_chunk_count(duration_ms: u64) -> u32 {
    let samples = duration_ms.saturating_mul(SAMPLE_RATE as u64) / 1_000;
    if samples <= WINDOW_SAMPLES as u64 {
        1
    } else {
        1 + (samples - WINDOW_SAMPLES as u64).div_ceil(STEP_SAMPLES as u64) as u32
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("話者分離をキャンセルしました。".into())
    } else {
        Ok(())
    }
}

fn seconds_to_samples(seconds: f32) -> u64 {
    (seconds.max(0.0) * SAMPLE_RATE as f32).round() as u64
}

fn samples_to_ms(samples: u64) -> u64 {
    samples.saturating_mul(1_000) / SAMPLE_RATE as u64
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, value: usize) -> usize {
        if self.parent[value] != value {
            self.parent[value] = self.find(self.parent[value]);
        }
        self.parent[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parent[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(index: usize, start: u64, speakers: &[(i32, u64, u64, Vec<f32>)]) -> ChunkResult {
        let mut embeddings = HashMap::new();
        let turns = speakers
            .iter()
            .map(|(speaker, from, to, embedding)| {
                let mut embedding = embedding.clone();
                normalize_embedding(&mut embedding);
                embeddings.insert(*speaker, embedding);
                LocalTurn {
                    start_sample: start + from,
                    end_sample: start + to,
                    speaker: *speaker,
                }
            })
            .collect();
        ChunkResult {
            index,
            start_sample: start,
            end_sample: start + WINDOW_SAMPLES as u64,
            turns,
            embeddings,
        }
    }

    #[test]
    fn chunk_count_uses_bounded_overlapping_windows() {
        assert_eq!(total_chunk_count(10_000), 1);
        assert_eq!(total_chunk_count(20 * 60 * 1_000), 1);
        assert_eq!(total_chunk_count(21 * 60 * 1_000), 2);
    }

    #[test]
    fn auto_clustering_repairs_sequential_overclustering_in_the_same_chunk() {
        let chunks = vec![chunk(
            0,
            0,
            &[
                (0, 0, SAMPLE_RATE as u64, vec![1.0, 0.0]),
                (
                    1,
                    SAMPLE_RATE as u64,
                    2 * SAMPLE_RATE as u64,
                    vec![1.0, 0.0],
                ),
            ],
        )];
        let turns = stitch_chunks(&chunks, None).expect("stitch chunks");
        assert_eq!(
            turns
                .iter()
                .map(|turn| &turn.speaker)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
    }

    #[test]
    fn auto_clustering_preserves_simultaneous_speakers_in_the_same_chunk() {
        let chunks = vec![chunk(
            0,
            0,
            &[
                (0, 0, 2 * SAMPLE_RATE as u64, vec![1.0, 0.0]),
                (
                    1,
                    SAMPLE_RATE as u64,
                    3 * SAMPLE_RATE as u64,
                    vec![1.0, 0.0],
                ),
            ],
        )];
        let turns = stitch_chunks(&chunks, None).expect("stitch chunks");
        assert_eq!(
            turns
                .iter()
                .map(|turn| &turn.speaker)
                .collect::<BTreeSet<_>>()
                .len(),
            2
        );
    }

    #[test]
    fn embedding_similarity_reconnects_a_speaker_across_chunks() {
        let second_start = STEP_SAMPLES as u64;
        let chunks = vec![
            chunk(0, 0, &[(0, 0, SAMPLE_RATE as u64, vec![1.0, 0.0])]),
            chunk(
                1,
                second_start,
                &[(
                    0,
                    OVERLAP_SAMPLES as u64,
                    OVERLAP_SAMPLES as u64 + SAMPLE_RATE as u64,
                    vec![0.99, 0.01],
                )],
            ),
        ];
        let turns = stitch_chunks(&chunks, None).expect("stitch chunks");
        assert!(turns.iter().all(|turn| turn.speaker == "Speaker 1"));
    }

    #[test]
    fn requested_speaker_count_merges_to_available_target() {
        let chunks = vec![
            chunk(0, 0, &[(0, 0, SAMPLE_RATE as u64, vec![1.0, 0.0])]),
            chunk(
                1,
                STEP_SAMPLES as u64,
                &[(
                    0,
                    OVERLAP_SAMPLES as u64,
                    OVERLAP_SAMPLES as u64 + SAMPLE_RATE as u64,
                    vec![0.0, 1.0],
                )],
            ),
        ];
        let turns = stitch_chunks(&chunks, Some(1)).expect("stitch chunks");
        assert!(turns.iter().all(|turn| turn.speaker == "Speaker 1"));
    }

    #[test]
    fn requested_count_can_merge_sequential_overclustering_in_one_chunk() {
        let chunks = vec![chunk(
            0,
            0,
            &[
                (0, 0, SAMPLE_RATE as u64, vec![1.0, 0.0]),
                (
                    1,
                    SAMPLE_RATE as u64,
                    2 * SAMPLE_RATE as u64,
                    vec![1.0, 0.0],
                ),
            ],
        )];
        let turns = stitch_chunks(&chunks, Some(1)).expect("stitch chunks");
        assert_eq!(
            turns
                .iter()
                .map(|turn| &turn.speaker)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
    }

    #[test]
    fn requested_count_never_merges_simultaneous_speakers() {
        let chunks = vec![chunk(
            0,
            0,
            &[
                (0, 0, 2 * SAMPLE_RATE as u64, vec![1.0, 0.0]),
                (
                    1,
                    SAMPLE_RATE as u64,
                    3 * SAMPLE_RATE as u64,
                    vec![1.0, 0.0],
                ),
            ],
        )];
        let turns = stitch_chunks(&chunks, Some(1)).expect("stitch chunks");
        assert_eq!(
            turns
                .iter()
                .map(|turn| &turn.speaker)
                .collect::<BTreeSet<_>>()
                .len(),
            2
        );
    }

    #[test]
    fn requested_speakers_rejects_a_tiny_artifact_cluster() {
        let turns = vec![
            SpeakerTurn {
                speaker: "Speaker 1".into(),
                start_ms: 0,
                end_ms: 60_000,
                confidence: None,
            },
            SpeakerTurn {
                speaker: "Speaker 2".into(),
                start_ms: 60_000,
                end_ms: 60_338,
                confidence: None,
            },
        ];
        assert!(!valid_speaker_distribution(&turns, Some(2)));
        assert!(valid_speaker_distribution(&turns, None));
    }

    #[test]
    fn automatic_speaker_distribution_rejects_pathological_overclustering() {
        let turns = (0..=MAX_AUTO_SPEAKERS)
            .map(|index| SpeakerTurn {
                speaker: format!("Speaker {}", index + 1),
                start_ms: index as u64 * 2_000,
                end_ms: index as u64 * 2_000 + 1_000,
                confidence: None,
            })
            .collect::<Vec<_>>();
        assert!(!valid_speaker_distribution(&turns, None));
    }
}
