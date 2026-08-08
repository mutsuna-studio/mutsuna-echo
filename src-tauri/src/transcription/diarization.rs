use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};

use super::{segments_from_tokens, TokenSpeakerSource, Transcript, TranscriptToken};

const MAX_SPEAKER_TURNS: usize = 1_000_000;
const MAX_SPEAKER_LABEL_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerTurn {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpeakerMergePolicy {
    /// Assign only tokens that do not already have a speaker.
    FillMissing,
    /// Replace previous external-diarization assignments, preserving provider/channel/user labels.
    RefreshDiarization,
    /// Replace all automatic assignments while preserving explicit user corrections.
    ReplaceAutomatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiarizationPhase {
    LoadingModel,
    AnalyzingAudio,
    Finalizing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationProgress {
    pub phase: DiarizationPhase,
    pub processed_ms: u64,
    pub total_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerMergeSummary {
    pub assigned_tokens: usize,
    pub preserved_tokens: usize,
    pub unmatched_tokens: usize,
}

#[derive(Clone, Default)]
pub struct DiarizationCancellation {
    cancelled: Arc<AtomicBool>,
}

impl DiarizationCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("話者分離をキャンセルしました。".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
pub struct DiarizationContext {
    cancellation: DiarizationCancellation,
    progress: Arc<dyn Fn(DiarizationProgress) + Send + Sync>,
}

impl Default for DiarizationContext {
    fn default() -> Self {
        Self::new(DiarizationCancellation::default(), |_| {})
    }
}

impl DiarizationContext {
    pub fn new(
        cancellation: DiarizationCancellation,
        progress: impl Fn(DiarizationProgress) + Send + Sync + 'static,
    ) -> Self {
        Self {
            cancellation,
            progress: Arc::new(progress),
        }
    }

    pub fn cancellation(&self) -> &DiarizationCancellation {
        &self.cancellation
    }

    pub fn report(&self, progress: DiarizationProgress) -> Result<(), String> {
        self.cancellation.check()?;
        (self.progress)(progress);
        Ok(())
    }
}

/// Contract implemented by cloud or local diarization adapters.
pub trait DiarizationProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn model_id(&self) -> &str;
    fn diarize(
        &self,
        audio_path: &Path,
        context: &DiarizationContext,
    ) -> Result<Vec<SpeakerTurn>, String>;
}

pub fn merge_speaker_turns(
    transcript: &mut Transcript,
    turns: &[SpeakerTurn],
    policy: SpeakerMergePolicy,
) -> Result<SpeakerMergeSummary, String> {
    if transcript.tokens.is_empty() {
        return Err(
            "トークン単位の時刻がないため話者分離結果を結合できません。再文字起こししてください。"
                .into(),
        );
    }
    validate_turns(turns)?;
    clear_replaceable_speakers(&mut transcript.tokens, policy);

    let mut sorted_turns: Vec<_> = turns.iter().collect();
    sorted_turns.sort_by_key(|turn| (turn.start_ms, turn.end_ms));
    let mut token_indices: Vec<_> = transcript
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| token.start_ms.map(|start| (index, start)))
        .collect();
    token_indices.sort_by_key(|(_, start)| *start);

    let mut active_turns: Vec<&SpeakerTurn> = Vec::new();
    let mut turn_cursor = 0usize;
    let mut summary = SpeakerMergeSummary {
        assigned_tokens: 0,
        preserved_tokens: 0,
        unmatched_tokens: 0,
    };

    for (token_index, token_start) in token_indices {
        let token = &transcript.tokens[token_index];
        if !can_assign(token, policy) {
            summary.preserved_tokens += 1;
            continue;
        }
        let token_end = token.end_ms.unwrap_or(token_start).max(token_start);
        while turn_cursor < sorted_turns.len() && sorted_turns[turn_cursor].start_ms <= token_end {
            active_turns.push(sorted_turns[turn_cursor]);
            turn_cursor += 1;
        }
        active_turns.retain(|turn| turn.end_ms >= token_start);
        let best = active_turns
            .iter()
            .copied()
            .filter_map(|turn| {
                overlap_score(token_start, token_end, turn).map(|score| (turn, score))
            })
            .max_by(|(left_turn, left_overlap), (right_turn, right_overlap)| {
                left_overlap.cmp(right_overlap).then_with(|| {
                    left_turn
                        .confidence
                        .unwrap_or(0.0)
                        .total_cmp(&right_turn.confidence.unwrap_or(0.0))
                })
            });
        if let Some((turn, _)) = best {
            let token = &mut transcript.tokens[token_index];
            token.speaker = Some(turn.speaker.trim().to_string());
            token.speaker_source = Some(TokenSpeakerSource::Diarization);
            summary.assigned_tokens += 1;
        } else {
            summary.unmatched_tokens += 1;
        }
    }

    for token in transcript
        .tokens
        .iter()
        .filter(|token| token.start_ms.is_none())
    {
        if can_assign(token, policy) {
            summary.unmatched_tokens += 1;
        } else {
            summary.preserved_tokens += 1;
        }
    }
    transcript.segments = segments_from_tokens(&transcript.tokens);
    Ok(summary)
}

fn validate_turns(turns: &[SpeakerTurn]) -> Result<(), String> {
    if turns.len() > MAX_SPEAKER_TURNS {
        return Err("話者区間が多すぎます。".into());
    }
    for turn in turns {
        let speaker = turn.speaker.trim();
        if speaker.is_empty()
            || speaker.len() > MAX_SPEAKER_LABEL_BYTES
            || turn.end_ms <= turn.start_ms
            || turn
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err("話者分離モデルが不正な話者区間を返しました。".into());
        }
    }
    Ok(())
}

fn clear_replaceable_speakers(tokens: &mut [TranscriptToken], policy: SpeakerMergePolicy) {
    for token in tokens {
        let clear = match policy {
            SpeakerMergePolicy::FillMissing => false,
            SpeakerMergePolicy::RefreshDiarization => {
                token.speaker_source == Some(TokenSpeakerSource::Diarization)
            }
            SpeakerMergePolicy::ReplaceAutomatic => {
                token.speaker_source != Some(TokenSpeakerSource::User)
            }
        };
        if clear {
            token.speaker = None;
            token.speaker_source = None;
        }
    }
}

fn can_assign(token: &TranscriptToken, policy: SpeakerMergePolicy) -> bool {
    match policy {
        SpeakerMergePolicy::FillMissing => token.speaker.is_none(),
        SpeakerMergePolicy::RefreshDiarization => {
            token.speaker.is_none() || token.speaker_source == Some(TokenSpeakerSource::Diarization)
        }
        SpeakerMergePolicy::ReplaceAutomatic => {
            token.speaker_source != Some(TokenSpeakerSource::User)
        }
    }
}

fn overlap_score(token_start: u64, token_end: u64, turn: &SpeakerTurn) -> Option<u64> {
    if token_end == token_start {
        return (turn.start_ms <= token_start && token_start < turn.end_ms).then_some(1);
    }
    let overlap_start = token_start.max(turn.start_ms);
    let overlap_end = token_end.min(turn.end_ms);
    (overlap_end > overlap_start).then_some(overlap_end - overlap_start)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::transcription::{TokenTimeSource, TranscriptSegment};

    fn token(
        text: &str,
        start: u64,
        end: u64,
        speaker: Option<(&str, TokenSpeakerSource)>,
    ) -> TranscriptToken {
        TranscriptToken {
            text: text.into(),
            start_ms: Some(start),
            end_ms: Some(end),
            start_time_source: Some(TokenTimeSource::Provider),
            end_time_source: Some(TokenTimeSource::Provider),
            speaker: speaker.map(|(label, _)| label.into()),
            speaker_source: speaker.map(|(_, source)| source),
            confidence: None,
        }
    }

    fn transcript(tokens: Vec<TranscriptToken>) -> Transcript {
        Transcript {
            provider: "local".into(),
            model: "test".into(),
            language: "ja".into(),
            segments: vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 0,
                end_ms: 1,
                text: "stale".into(),
            }],
            tokens,
        }
    }

    #[test]
    fn assigns_the_turn_with_the_largest_time_overlap() {
        let mut transcript = transcript(vec![token("発話", 900, 1_300, None)]);
        let turns = vec![
            SpeakerTurn {
                speaker: "Speaker 1".into(),
                start_ms: 0,
                end_ms: 1_000,
                confidence: Some(0.9),
            },
            SpeakerTurn {
                speaker: "Speaker 2".into(),
                start_ms: 1_000,
                end_ms: 2_000,
                confidence: Some(0.8),
            },
        ];
        let summary = merge_speaker_turns(&mut transcript, &turns, SpeakerMergePolicy::FillMissing)
            .expect("merge turns");
        assert_eq!(summary.assigned_tokens, 1);
        assert_eq!(transcript.tokens[0].speaker.as_deref(), Some("Speaker 2"));
        assert_eq!(
            transcript.tokens[0].speaker_source,
            Some(TokenSpeakerSource::Diarization)
        );
        assert_eq!(transcript.segments[0].speaker, "Speaker 2");
    }

    #[test]
    fn default_policy_preserves_any_cloud_provider_and_user_speakers() {
        let mut transcript = transcript(vec![
            token(
                "API",
                0,
                500,
                Some(("API Speaker", TokenSpeakerSource::Provider)),
            ),
            token("修正", 500, 1_000, Some(("田中", TokenSpeakerSource::User))),
            token("未設定", 1_000, 1_500, None),
        ]);
        transcript.provider = "future-cloud-stt".into();
        transcript.model = "speaker-aware-v1".into();
        let turns = vec![SpeakerTurn {
            speaker: "External Speaker".into(),
            start_ms: 0,
            end_ms: 2_000,
            confidence: None,
        }];
        merge_speaker_turns(&mut transcript, &turns, SpeakerMergePolicy::FillMissing)
            .expect("merge turns");
        assert_eq!(transcript.tokens[0].speaker.as_deref(), Some("API Speaker"));
        assert_eq!(transcript.tokens[1].speaker.as_deref(), Some("田中"));
        assert_eq!(
            transcript.tokens[2].speaker.as_deref(),
            Some("External Speaker")
        );
    }

    #[test]
    fn refresh_removes_stale_diarization_without_overwriting_provider_speakers() {
        let mut transcript = transcript(vec![
            token(
                "保持",
                0,
                400,
                Some(("Provider", TokenSpeakerSource::Provider)),
            ),
            token(
                "更新",
                500,
                900,
                Some(("Old", TokenSpeakerSource::Diarization)),
            ),
        ]);
        let turns = vec![SpeakerTurn {
            speaker: "New".into(),
            start_ms: 450,
            end_ms: 1_000,
            confidence: None,
        }];
        merge_speaker_turns(
            &mut transcript,
            &turns,
            SpeakerMergePolicy::RefreshDiarization,
        )
        .expect("refresh turns");
        assert_eq!(transcript.tokens[0].speaker.as_deref(), Some("Provider"));
        assert_eq!(transcript.tokens[1].speaker.as_deref(), Some("New"));
    }

    #[test]
    fn rejects_invalid_turns_before_mutating_tokens() {
        let original = token("発話", 0, 500, None);
        let mut transcript = transcript(vec![original.clone()]);
        let turns = vec![SpeakerTurn {
            speaker: " ".into(),
            start_ms: 500,
            end_ms: 100,
            confidence: None,
        }];
        assert!(
            merge_speaker_turns(&mut transcript, &turns, SpeakerMergePolicy::FillMissing).is_err()
        );
        assert_eq!(transcript.tokens[0], original);
    }

    #[test]
    fn cancellation_and_progress_are_shared_with_provider_adapters() {
        let cancellation = DiarizationCancellation::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let context = DiarizationContext::new(cancellation.clone(), move |progress| {
            captured.lock().expect("progress lock").push(progress);
        });
        context
            .report(DiarizationProgress {
                phase: DiarizationPhase::AnalyzingAudio,
                processed_ms: 500,
                total_ms: Some(1_000),
            })
            .expect("report progress");
        assert_eq!(events.lock().expect("events").len(), 1);
        cancellation.cancel();
        assert!(context
            .report(DiarizationProgress {
                phase: DiarizationPhase::Finalizing,
                processed_ms: 1_000,
                total_ms: Some(1_000),
            })
            .is_err());
    }
}
