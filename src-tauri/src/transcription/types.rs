use serde::{Deserialize, Serialize};

/// A provider-neutral transcription result.
///
/// Provider-specific responses are normalized into this format before they are
/// returned to the UI or written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub provider: String,
    pub model: String,
    pub language: String,
    pub segments: Vec<TranscriptSegment>,
}

/// A contiguous utterance made by one speaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
