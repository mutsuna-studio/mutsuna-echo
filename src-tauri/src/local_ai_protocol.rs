use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{Read, Write};

pub(crate) const PROTOCOL_VERSION: u32 = 1;
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolMessage<T> {
    pub protocol_version: u32,
    pub runtime_version: String,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "body", rename_all = "camelCase")]
pub(crate) enum RuntimeRequest {
    Handshake,
    TranscribeFile(TranscribeFileRequest),
    DiarizeFile(DiarizeFileRequest),
    OpenVadStream(OpenVadStreamRequest),
    VadSamples(VadSamplesRequest),
    CloseVadStream { stream_id: String },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "body", rename_all = "camelCase")]
pub(crate) enum RuntimeResponse {
    Handshake { compatible: bool },
    Transcription { transcript_json: String },
    Diarization { turns_json: String },
    VadStreamOpened { stream_id: String },
    VadResult { detected: bool },
    VadStreamClosed,
    Error { code: String, message: String },
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscribeFileRequest {
    pub audio_path: String,
    pub model_directory: String,
    pub vad_model_path: String,
    pub settings_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiarizeFileRequest {
    pub audio_path: String,
    pub model_directory: String,
    pub speaker_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenVadStreamRequest {
    pub model_path: String,
    pub sample_rate: u32,
    pub preset_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VadSamplesRequest {
    pub stream_id: String,
    pub samples: Vec<f32>,
}

pub(crate) fn write_frame<T: Serialize>(
    writer: &mut impl Write,
    message: &T,
) -> Result<(), String> {
    let payload = serde_json::to_vec(message)
        .map_err(|error| format!("プロトコルを作成できませんでした: {error}"))?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err("プロトコルメッセージが大きすぎます。".into());
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| writer.write_all(&payload))
        .map_err(|error| format!("プロトコルを送信できませんでした: {error}"))
}

pub(crate) fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, String> {
    let mut length = [0u8; 4];
    reader
        .read_exact(&mut length)
        .map_err(|error| format!("プロトコル長を読めませんでした: {error}"))?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err("プロトコル長が不正です。".into());
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("プロトコルを読めませんでした: {error}"))?;
    serde_json::from_slice(&payload).map_err(|error| format!("プロトコルが不正です: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn length_prefixed_protocol_round_trips_and_versions_every_message() {
        let message = ProtocolMessage {
            protocol_version: PROTOCOL_VERSION,
            runtime_version: "1.13.4-1".into(),
            payload: RuntimeRequest::Handshake,
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &message).unwrap();
        let decoded: ProtocolMessage<RuntimeRequest> = read_frame(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn oversized_frames_are_rejected_before_allocation() {
        let bytes = ((MAX_MESSAGE_BYTES + 1) as u32).to_le_bytes();
        assert!(read_frame::<RuntimeRequest>(&mut bytes.as_slice()).is_err());
    }
}
