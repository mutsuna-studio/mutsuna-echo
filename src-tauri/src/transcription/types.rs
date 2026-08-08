use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum TranscriptionProvider {
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[serde(rename = "local")]
    Local,
}

impl TranscriptionProvider {
    pub const ALL: [Self; 2] = [Self::ElevenLabs, Self::Local];

    pub const fn id(self) -> &'static str {
        match self {
            Self::ElevenLabs => "elevenlabs",
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
}

const SEGMENT_GAP_MS: u64 = 800;
const MAX_SEGMENT_DURATION_MS: u64 = 30_000;
const MAX_SEGMENT_CHARACTERS: usize = 160;

/// Builds readable segments without discarding token-level timing.
pub(crate) fn segments_from_tokens(tokens: &[TranscriptToken]) -> Vec<TranscriptSegment> {
    let mut segments = Vec::new();
    let mut current = SegmentBuilder::default();

    for token in tokens {
        if token.text.is_empty() {
            continue;
        }
        let speaker = token
            .speaker
            .as_deref()
            .or_else(|| (!current.speaker.is_empty()).then_some(current.speaker.as_str()))
            .unwrap_or("Speaker 1")
            .to_string();
        let speaker_changed = !current.speaker.is_empty() && current.speaker != speaker;
        let inferred_end = current.end_time_source == Some(TokenTimeSource::Inferred);
        let gap_anchor = if inferred_end {
            current.last_start_ms
        } else {
            current.end_ms
        };
        let gap_threshold = if inferred_end {
            SEGMENT_GAP_MS + 400
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
        let length_boundary = current.text.chars().count() >= MAX_SEGMENT_CHARACTERS;
        if speaker_changed || gap_boundary || duration_boundary || length_boundary {
            finish_segment(&mut current, &mut segments);
        }

        if current.speaker.is_empty() {
            current.speaker = speaker;
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

        if token
            .text
            .trim_end()
            .ends_with(['。', '！', '？', '!', '?'])
        {
            finish_segment(&mut current, &mut segments);
        }
    }
    finish_segment(&mut current, &mut segments);
    segments
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
    use super::{segments_from_tokens, TokenSpeakerSource, TokenTimeSource, TranscriptToken};

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
}
