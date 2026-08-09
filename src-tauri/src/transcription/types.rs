use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum TranscriptionProvider {
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[serde(rename = "soniox")]
    Soniox,
    #[serde(rename = "local")]
    Local,
}

impl TranscriptionProvider {
    pub const ALL: [Self; 3] = [Self::ElevenLabs, Self::Soniox, Self::Local];

    pub const fn id(self) -> &'static str {
        match self {
            Self::ElevenLabs => "elevenlabs",
            Self::Soniox => "soniox",
            Self::Local => "local",
        }
    }
}

/// A provider-neutral transcription result.
///
/// Provider-specific responses are normalized into this format before they are
/// returned to the UI or written to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub provider: String,
    pub model: String,
    pub language: String,
    /// Provider-normalized timing data. Display segments can be regenerated from these tokens.
    #[serde(default)]
    pub tokens: Vec<TranscriptToken>,
    pub segments: Vec<TranscriptSegment>,
}

/// Indicates where a token's speaker assignment originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenSpeakerSource {
    Provider,
    Diarization,
    Channel,
    User,
}

/// Indicates whether a boundary was emitted by a model or derived by normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenTimeSource {
    Provider,
    Alignment,
    Inferred,
    User,
}

/// The smallest provider-neutral unit whose timing should be preserved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptToken {
    pub text: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub start_time_source: Option<TokenTimeSource>,
    pub end_time_source: Option<TokenTimeSource>,
    pub speaker: Option<String>,
    pub speaker_source: Option<TokenSpeakerSource>,
    pub confidence: Option<f32>,
}

/// A contiguous utterance made by one speaker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Default)]
struct SegmentBuilder {
    speaker: String,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    last_start_ms: Option<u64>,
    end_time_source: Option<TokenTimeSource>,
    text: String,
    character_count: usize,
}

const SEGMENT_GAP_MS: u64 = 800;
const INFERRED_SEGMENT_GAP_MS: u64 = 2_500;
const MAX_SEGMENT_DURATION_MS: u64 = 30_000;
const MAX_SEGMENT_CHARACTERS: usize = 160;
const MAX_INFERRED_TOKEN_DURATION_MS: u64 = 1_000;
const MAX_ORPHAN_CHARACTERS: usize = 1;
const MAX_ORPHAN_DURATION_MS: u64 = 1_500;
const MAX_ORPHAN_MERGE_GAP_MS: u64 = 3_000;

/// Repairs missing or implausibly long inferred token ends without modifying
/// provider- or alignment-authored boundaries.
pub(crate) fn repair_inferred_token_ends(
    tokens: &mut [TranscriptToken],
    timeline_end_ms: Option<u64>,
) -> bool {
    let mut changed = false;
    for index in 0..tokens.len() {
        if tokens[index].end_time_source != Some(TokenTimeSource::Inferred) {
            continue;
        }
        let Some(start_ms) = tokens[index].start_ms else {
            continue;
        };
        let next_start_ms = tokens
            .get(index + 1)
            .and_then(|token| token.start_ms)
            .filter(|next| *next > start_ms);
        let existing_end_ms = tokens[index].end_ms.filter(|end| *end > start_ms);
        let upper_bound = start_ms.saturating_add(MAX_INFERRED_TOKEN_DURATION_MS);
        let repaired_end_ms = next_start_ms
            .or(existing_end_ms)
            .or(timeline_end_ms.filter(|end| *end > start_ms))
            .map(|end| end.min(upper_bound))
            .unwrap_or(start_ms);
        if tokens[index].end_ms != Some(repaired_end_ms) {
            tokens[index].end_ms = Some(repaired_end_ms);
            changed = true;
        }
    }
    changed
}

/// Rebuilds display segments from provider-neutral tokens. This can be run
/// when loading stored transcripts, so segmentation improvements do not
/// require another STT request.
pub(crate) fn normalize_transcript_for_display(transcript: &mut Transcript) -> bool {
    if transcript.tokens.is_empty() {
        return false;
    }
    let timeline_end_ms = transcript
        .segments
        .iter()
        .map(|segment| segment.end_ms)
        .max();
    let mut changed = repair_inferred_token_ends(&mut transcript.tokens, timeline_end_ms);
    let segments = segments_from_tokens(&transcript.tokens);
    if transcript.segments != segments {
        transcript.segments = segments;
        changed = true;
    }
    changed
}

/// Builds readable segments without discarding token-level timing.
pub(crate) fn segments_from_tokens(tokens: &[TranscriptToken]) -> Vec<TranscriptSegment> {
    let mut segments = Vec::new();
    let mut current = SegmentBuilder::default();

    for token in tokens {
        if token.text.is_empty() {
            continue;
        }
        let speaker_changed = token
            .speaker
            .as_deref()
            .is_some_and(|speaker| !current.speaker.is_empty() && current.speaker != speaker);
        let inferred_end = current.end_time_source == Some(TokenTimeSource::Inferred);
        let gap_anchor = if inferred_end {
            current.last_start_ms
        } else {
            current.end_ms
        };
        let gap_threshold = if inferred_end {
            INFERRED_SEGMENT_GAP_MS
        } else {
            SEGMENT_GAP_MS
        };
        let gap_boundary = gap_anchor
            .zip(token.start_ms)
            .is_some_and(|(end, start)| start.saturating_sub(end) >= gap_threshold);
        let duration_boundary = current
            .start_ms
            .zip(token.end_ms.or(token.start_ms))
            .is_some_and(|(start, end)| end.saturating_sub(start) >= MAX_SEGMENT_DURATION_MS);
        let length_boundary = current.character_count >= MAX_SEGMENT_CHARACTERS;
        if speaker_changed || gap_boundary || duration_boundary || length_boundary {
            let next_speaker = token
                .speaker
                .clone()
                .or_else(|| (!current.speaker.is_empty()).then(|| current.speaker.clone()))
                .unwrap_or_else(|| "Speaker 1".into());
            finish_segment(&mut current, &mut segments);
            current.speaker = next_speaker;
        }

        if current.speaker.is_empty() {
            current.speaker = token.speaker.clone().unwrap_or_else(|| "Speaker 1".into());
        }
        if let Some(start) = token.start_ms {
            current.start_ms.get_or_insert(start);
            current.last_start_ms = Some(start);
        }
        if let Some(end) = token.end_ms.or(token.start_ms) {
            current.end_ms = Some(current.end_ms.map_or(end, |value| value.max(end)));
            current.end_time_source = token.end_time_source;
        }
        current.text.push_str(&token.text);
        current.character_count += token.text.chars().count();

        if token
            .text
            .trim_end()
            .ends_with(['。', '！', '？', '!', '?'])
        {
            finish_segment(&mut current, &mut segments);
        }
    }
    finish_segment(&mut current, &mut segments);
    merge_orphan_segments(segments)
}

fn merge_orphan_segments(mut segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    let mut index = 0;
    while index < segments.len() {
        if !is_orphan_segment(&segments[index]) {
            index += 1;
            continue;
        }
        let previous_gap = index.checked_sub(1).and_then(|previous| {
            same_speaker(&segments[previous], &segments[index]).then(|| {
                segments[index]
                    .start_ms
                    .saturating_sub(segments[previous].end_ms)
            })
        });
        let next_gap = segments.get(index + 1).and_then(|next| {
            same_speaker(&segments[index], next)
                .then(|| next.start_ms.saturating_sub(segments[index].end_ms))
        });
        let merge_previous = previous_gap
            .filter(|gap| *gap <= MAX_ORPHAN_MERGE_GAP_MS)
            .filter(|gap| next_gap.is_none_or(|next| *gap <= next));
        if merge_previous.is_some() {
            let orphan = segments.remove(index);
            let previous = &mut segments[index - 1];
            previous.end_ms = previous.end_ms.max(orphan.end_ms);
            previous.text.push_str(&orphan.text);
            index = index.saturating_sub(1);
            continue;
        }
        if next_gap.is_some_and(|gap| gap <= MAX_ORPHAN_MERGE_GAP_MS) {
            let orphan = segments.remove(index);
            let next = &mut segments[index];
            next.start_ms = next.start_ms.min(orphan.start_ms);
            next.text.insert_str(0, &orphan.text);
            continue;
        }
        index += 1;
    }
    segments
}

fn is_orphan_segment(segment: &TranscriptSegment) -> bool {
    segment.text.chars().count() <= MAX_ORPHAN_CHARACTERS
        && segment.end_ms.saturating_sub(segment.start_ms) <= MAX_ORPHAN_DURATION_MS
        && !segment
            .text
            .trim_end()
            .ends_with(['。', '！', '？', '!', '?'])
}

fn same_speaker(left: &TranscriptSegment, right: &TranscriptSegment) -> bool {
    left.speaker == right.speaker
}

fn finish_segment(builder: &mut SegmentBuilder, segments: &mut Vec<TranscriptSegment>) {
    let text = builder.text.trim();
    if !text.is_empty() {
        let start_ms = builder.start_ms.unwrap_or(0);
        segments.push(TranscriptSegment {
            speaker: if builder.speaker.is_empty() {
                "Speaker 1".into()
            } else {
                builder.speaker.clone()
            },
            start_ms,
            end_ms: builder.end_ms.unwrap_or(start_ms).max(start_ms),
            text: text.to_string(),
        });
    }
    *builder = SegmentBuilder::default();
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_transcript_for_display, repair_inferred_token_ends, segments_from_tokens,
        TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
    };

    fn token(text: &str, start_ms: u64, end_ms: u64, speaker: Option<&str>) -> TranscriptToken {
        TranscriptToken {
            text: text.into(),
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
            start_time_source: Some(TokenTimeSource::Provider),
            end_time_source: Some(TokenTimeSource::Provider),
            speaker: speaker.map(str::to_string),
            speaker_source: speaker.map(|_| TokenSpeakerSource::Provider),
            confidence: None,
        }
    }

    #[test]
    fn segments_preserve_precise_bounds_and_provider_speakers() {
        let tokens = vec![
            token("こんにちは。", 120, 640, Some("Speaker 1")),
            token("よろしく", 900, 1_300, Some("Speaker 2")),
            token("お願いします。", 1_310, 2_000, Some("Speaker 2")),
        ];
        let segments = segments_from_tokens(&tokens);
        assert_eq!(segments.len(), 2);
        assert_eq!((segments[0].start_ms, segments[0].end_ms), (120, 640));
        assert_eq!(segments[1].speaker, "Speaker 2");
        assert_eq!(segments[1].text, "よろしくお願いします。");
    }

    #[test]
    fn segments_split_on_silence_without_speaker_information() {
        let tokens = vec![
            token("前半", 0, 300, None),
            token("後半", 1_200, 1_600, None),
        ];
        let segments = segments_from_tokens(&tokens);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].start_ms, 1_200);
    }

    #[test]
    fn inferred_timestamps_use_a_less_aggressive_silence_boundary() {
        let mut tokens = vec![
            token("短い", 0, 0, None),
            token("続きです", 2_000, 2_400, None),
        ];
        for token in &mut tokens {
            token.end_time_source = Some(TokenTimeSource::Inferred);
        }
        let segments = segments_from_tokens(&tokens);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "短い続きです");
    }

    #[test]
    fn inferred_timestamps_still_split_on_long_silence() {
        let mut tokens = vec![token("前半", 0, 0, None), token("後半", 3_000, 3_400, None)];
        for token in &mut tokens {
            token.end_time_source = Some(TokenTimeSource::Inferred);
        }
        assert_eq!(segments_from_tokens(&tokens).len(), 2);
    }

    #[test]
    fn short_orphan_is_merged_without_crossing_speakers() {
        let tokens = vec![
            token("本題", 0, 500, Some("Speaker 1")),
            token("え", 1_500, 1_500, Some("Speaker 1")),
            token("回答", 5_000, 5_500, Some("Speaker 2")),
        ];
        let segments = segments_from_tokens(&tokens);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "本題え");
        assert_eq!(segments[1].text, "回答");
    }

    #[test]
    fn repairs_only_inferred_token_ends_with_a_sane_cap() {
        let mut tokens = vec![token("前", 100, 100, None), token("後", 5_000, 5_400, None)];
        tokens[0].end_time_source = Some(TokenTimeSource::Inferred);
        assert!(repair_inferred_token_ends(&mut tokens, Some(6_000)));
        assert_eq!(tokens[0].end_ms, Some(1_100));
        assert_eq!(tokens[1].end_ms, Some(5_400));
    }

    #[test]
    fn stored_transcript_segments_can_be_regenerated_from_tokens() {
        let mut first = token("会議", 100, 500, None);
        first.end_time_source = Some(TokenTimeSource::Inferred);
        let mut second = token("です", 2_000, 2_400, None);
        second.end_time_source = Some(TokenTimeSource::Inferred);
        let mut transcript = Transcript {
            provider: "local".into(),
            model: "fixture".into(),
            language: "ja".into(),
            tokens: vec![first, second],
            segments: vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 100,
                end_ms: 500,
                text: "古い区切り".into(),
            }],
        };
        assert!(normalize_transcript_for_display(&mut transcript));
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].text, "会議です");
    }
}
