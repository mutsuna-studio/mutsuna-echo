use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 1;
pub const FINAL_BITRATE: u32 = 64_000;
pub const SOURCE_BITRATE: u32 = 96_000;
pub const MAX_DURATION_MS: u64 = 10 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecordingPhase {
    Idle,
    Starting,
    Recording,
    Finalizing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    User,
    DurationLimit,
    SourceDisconnected,
    SourceStalled,
    CaptureError,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingRequest {
    pub microphone: bool,
    pub system_audio: bool,
    pub microphone_device_id: Option<String>,
    pub system_device_id: Option<String>,
}

impl StartRecordingRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !self.microphone && !self.system_audio {
            return Err("マイクまたはシステム音声を1つ以上選択してください。".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingCapabilities {
    pub platform: &'static str,
    pub supported: bool,
    pub microphone_supported: bool,
    pub system_audio_supported: bool,
    pub system_audio_limited: bool,
    pub limitation: Option<&'static str>,
    pub microphone_devices: Vec<AudioDevice>,
    pub system_devices: Vec<AudioDevice>,
    pub sample_rate: u32,
    pub channels: u16,
    pub codec: &'static str,
    pub bitrate: u32,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub phase: RecordingPhase,
    pub session_id: Option<String>,
    pub elapsed_ms: u64,
    pub microphone_level: f32,
    pub system_level: f32,
    pub microphone: bool,
    pub system_audio: bool,
    pub output_path: Option<String>,
    pub stop_reason: Option<StopReason>,
    pub warning: Option<String>,
    pub error: Option<String>,
}

impl Default for RecordingStatus {
    fn default() -> Self {
        Self {
            phase: RecordingPhase::Idle,
            session_id: None,
            elapsed_ms: 0,
            microphone_level: 0.0,
            system_level: 0.0,
            microphone: false,
            system_audio: false,
            output_path: None,
            stop_reason: None,
            warning: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableRecording {
    pub session_id: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub microphone: bool,
    pub system_audio: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedAudioSummary {
    pub id: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub recorded_at_unix_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::StartRecordingRequest;

    #[test]
    fn at_least_one_source_is_required() {
        let request = StartRecordingRequest {
            microphone: false,
            system_audio: false,
            microphone_device_id: None,
            system_device_id: None,
        };
        assert!(request.validate().is_err());
    }
}
