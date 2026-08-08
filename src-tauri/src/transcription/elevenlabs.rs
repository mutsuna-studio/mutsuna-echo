use std::{collections::HashMap, path::Path, time::Duration};

use reqwest::{multipart::Form, StatusCode};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::Value;

use super::{
    segments_from_tokens, TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment,
    TranscriptToken,
};

pub(crate) mod client;

use client::{api_error_kind, ApiErrorKind, ElevenLabsClient};

const SPEECH_TO_TEXT_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const MODEL_ID: &str = "scribe_v2";
const LANGUAGE_CODE: &str = "ja";

#[derive(Debug, Deserialize)]
struct ElevenLabsTranscript {
    #[serde(default)]
    language_code: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    words: Vec<ElevenLabsWord>,
}

#[derive(Debug, Deserialize)]
struct ElevenLabsWord {
    #[serde(default)]
    text: String,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    speaker_id: Option<String>,
}

fn seconds_to_ms(value: Option<f64>) -> Option<u64> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| (value * 1000.0).round() as u64)
}

fn speaker_label<'a>(speaker_id: &str, speakers: &'a mut HashMap<String, String>) -> &'a str {
    let next_number = speakers.len() + 1;
    speakers
        .entry(speaker_id.to_string())
        .or_insert_with(|| format!("Speaker {next_number}"))
}

fn normalize(response: ElevenLabsTranscript) -> Transcript {
    let mut speakers = HashMap::new();
    let mut previous_speaker_id = String::new();
    let mut tokens = Vec::with_capacity(response.words.len());
    let mut upcoming_speaker_ids = vec![None; response.words.len()];
    let mut upcoming_speaker_id = None;
    for (index, word) in response.words.iter().enumerate().rev() {
        if word.speaker_id.is_some() {
            upcoming_speaker_id.clone_from(&word.speaker_id);
        }
        upcoming_speaker_ids[index].clone_from(&upcoming_speaker_id);
    }

    for (index, word) in response.words.into_iter().enumerate() {
        let speaker_id = word
            .speaker_id
            .or_else(|| (!previous_speaker_id.is_empty()).then(|| previous_speaker_id.clone()))
            .or_else(|| upcoming_speaker_ids[index].clone())
            .unwrap_or_else(|| "speaker_0".to_string());
        previous_speaker_id.clone_from(&speaker_id);
        let start_ms = seconds_to_ms(word.start);
        let end_ms = seconds_to_ms(word.end).map(|end| end.max(start_ms.unwrap_or(0)));
        tokens.push(TranscriptToken {
            text: word.text,
            start_ms,
            end_ms,
            start_time_source: start_ms.map(|_| TokenTimeSource::Provider),
            end_time_source: end_ms.map(|_| TokenTimeSource::Provider),
            speaker: Some(speaker_label(&speaker_id, &mut speakers).to_string()),
            speaker_source: Some(TokenSpeakerSource::Provider),
            confidence: None,
        });
    }

    let mut segments = segments_from_tokens(&tokens);

    if segments.is_empty() && !response.text.trim().is_empty() {
        segments.push(TranscriptSegment {
            speaker: "Speaker 1".to_string(),
            start_ms: 0,
            end_ms: 0,
            text: response.text.trim().to_string(),
        });
    }

    Transcript {
        provider: "elevenlabs".to_string(),
        model: MODEL_ID.to_string(),
        language: if response.language_code.is_empty() {
            LANGUAGE_CODE.to_string()
        } else {
            response.language_code
        },
        tokens,
        segments,
    }
}

fn provider_error(status: StatusCode, body: &Value) -> String {
    match api_error_kind(body) {
        ApiErrorKind::InvalidApiKey => {
            "保存済みのElevenLabs APIキーが無効です。設定し直してください。".to_string()
        }
        ApiErrorKind::MissingPermissions => {
            "APIキーにSpeech to Text権限がありません。ElevenLabsで権限を追加してください。"
                .to_string()
        }
        ApiErrorKind::QuotaExceeded => {
            "ElevenLabsの利用可能枠が不足しています。利用状況と上限を確認してください。".to_string()
        }
        _ if status == StatusCode::PAYLOAD_TOO_LARGE => {
            "音声ファイルがElevenLabsの上限を超えています。".to_string()
        }
        _ if status == StatusCode::UNPROCESSABLE_ENTITY => {
            "音声ファイルを処理できませんでした。形式やファイル内容を確認してください。".to_string()
        }
        ApiErrorKind::Other => {
            format!("ElevenLabsで文字起こしに失敗しました（HTTP {status}）。")
        }
    }
}

pub(crate) async fn transcribe(path: &Path, api_key: &SecretString) -> Result<Transcript, String> {
    let form = Form::new()
        .text("model_id", MODEL_ID)
        .text("language_code", LANGUAGE_CODE)
        .text("diarize", "true")
        .text("timestamps_granularity", "word")
        .text("tag_audio_events", "false")
        .file("file", path)
        .await
        .map_err(|error| {
            eprintln!("Could not open selected audio file: {error:?}");
            "選択した音声ファイルを開けませんでした。".to_string()
        })?;

    let response = ElevenLabsClient::new(api_key, Duration::from_secs(60 * 30))?
        .post(SPEECH_TO_TEXT_URL)
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "文字起こしがタイムアウトしました。短いファイルで再試行してください。".to_string()
            } else {
                format!("ElevenLabsへ音声を送信できませんでした: {error}")
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        let provider_status = body.pointer("/detail/status").and_then(Value::as_str);
        eprintln!("ElevenLabs transcription failed: HTTP {status}, status={provider_status:?}");
        return Err(provider_error(status, &body));
    }

    let response = response
        .json::<ElevenLabsTranscript>()
        .await
        .map_err(|error| {
            eprintln!("Could not parse ElevenLabs transcript: {error:?}");
            "ElevenLabsの文字起こし結果を読み取れませんでした。".to_string()
        })?;

    Ok(normalize(response))
}

#[cfg(test)]
mod tests {
    use super::{normalize, provider_error, ElevenLabsTranscript, ElevenLabsWord};
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn groups_words_into_provider_neutral_speaker_segments() {
        let response = serde_json::from_value::<ElevenLabsTranscript>(json!({
            "language_code": "ja",
            "language_probability": 0.99,
            "text": "こんにちは。よろしくお願いします。",
            "words": [
                {
                    "text": "こんにちは。",
                    "start": 1.2,
                    "end": 1.8,
                    "speaker_id": "speaker_0",
                    "type": "word"
                },
                {
                    "text": "よろしくお願いします。",
                    "start": 2.0,
                    "end": 3.4,
                    "speaker_id": "speaker_1",
                    "type": "word"
                }
            ]
        }))
        .expect("deserialize ElevenLabs response");

        let transcript = normalize(response);

        assert_eq!(transcript.provider, "elevenlabs");
        assert_eq!(transcript.model, "scribe_v2");
        assert_eq!(transcript.tokens.len(), 2);
        assert_eq!(transcript.tokens[0].start_ms, Some(1200));
        assert_eq!(transcript.tokens[0].end_ms, Some(1800));
        assert_eq!(transcript.tokens[1].speaker.as_deref(), Some("Speaker 2"));
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[0].speaker, "Speaker 1");
        assert_eq!(transcript.segments[0].start_ms, 1200);
        assert_eq!(transcript.segments[0].end_ms, 1800);
        assert_eq!(transcript.segments[1].speaker, "Speaker 2");
        assert_eq!(transcript.segments[1].text, "よろしくお願いします。");
    }

    #[test]
    fn preserves_spacing_tokens_without_speaker_ids() {
        let response = ElevenLabsTranscript {
            language_code: "eng".to_string(),
            text: "Hello world".to_string(),
            words: vec![
                ElevenLabsWord {
                    text: "Hello".to_string(),
                    start: Some(0.0),
                    end: Some(0.4),
                    speaker_id: Some("speaker_0".to_string()),
                },
                ElevenLabsWord {
                    text: " ".to_string(),
                    start: None,
                    end: None,
                    speaker_id: None,
                },
                ElevenLabsWord {
                    text: "world".to_string(),
                    start: Some(0.5),
                    end: Some(0.9),
                    speaker_id: Some("speaker_0".to_string()),
                },
            ],
        };

        let transcript = normalize(response);
        assert_eq!(transcript.segments[0].text, "Hello world");
        assert_eq!(transcript.tokens[1].text, " ");
        assert_eq!(transcript.tokens[1].start_ms, None);
    }

    #[test]
    fn leading_spacing_inherits_the_next_provider_speaker() {
        let response = ElevenLabsTranscript {
            language_code: "ja".into(),
            text: " こんにちは".into(),
            words: vec![
                ElevenLabsWord {
                    text: " ".into(),
                    start: None,
                    end: None,
                    speaker_id: None,
                },
                ElevenLabsWord {
                    text: "こんにちは".into(),
                    start: Some(0.1),
                    end: Some(0.6),
                    speaker_id: Some("speaker_7".into()),
                },
            ],
        };
        let transcript = normalize(response);
        assert_eq!(transcript.tokens[0].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(transcript.tokens[1].speaker.as_deref(), Some("Speaker 1"));
    }

    #[test]
    fn explains_missing_speech_to_text_permission() {
        let message = provider_error(
            StatusCode::FORBIDDEN,
            &json!({ "detail": { "status": "missing_permissions" } }),
        );

        assert!(message.contains("Speech to Text権限"));
    }
}
