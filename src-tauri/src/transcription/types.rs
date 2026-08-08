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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub provider: String,
    pub model: String,
    pub language: String,
    pub segments: Vec<TranscriptSegment>,
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
