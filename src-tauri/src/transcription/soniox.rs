use std::{collections::HashMap, path::Path, time::Duration};

use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    multipart::Form,
    redirect::Policy,
    Client, RequestBuilder, Response, StatusCode,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    segments_from_tokens, TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment,
    TranscriptToken,
};

pub(crate) const MODEL_ID: &str = "stt-async-v5";
const API_BASE_URL: &str = "https://api.soniox.com/v1";
const LANGUAGE_HINT: &str = "ja";
const MAX_POLL_ATTEMPTS: usize = 10_800;

struct SonioxClient {
    http: Client,
    authorization: HeaderValue,
}

impl SonioxClient {
    fn new(api_key: &SecretString) -> Result<Self, String> {
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {}", api_key.expose_secret())).map_err(
                |_| "Soniox APIキーの形式が不正です。設定し直してください。".to_string(),
            )?;
        authorization.set_sensitive(true);
        let http = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60 * 30))
            .build()
            .map_err(|error| {
                eprintln!("Could not build Soniox HTTP client: {error:?}");
                "Sonioxへの接続を準備できませんでした。".to_string()
            })?;
        Ok(Self {
            http,
            authorization,
        })
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.request(self.http.get(format!("{API_BASE_URL}{path}")))
    }

    fn post(&self, path: &str) -> RequestBuilder {
        self.request(self.http.post(format!("{API_BASE_URL}{path}")))
    }

    fn delete(&self, path: &str) -> RequestBuilder {
        self.request(self.http.delete(format!("{API_BASE_URL}{path}")))
    }

    fn request(&self, request: RequestBuilder) -> RequestBuilder {
        request.header(AUTHORIZATION, self.authorization.clone())
    }
}

#[derive(Debug, Deserialize)]
struct UploadedFile {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TranscriptionJob {
    id: String,
    status: String,
    #[serde(default)]
    error_type: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateTranscription<'a> {
    model: &'a str,
    file_id: &'a str,
    language_hints: [&'a str; 1],
    enable_speaker_diarization: bool,
    enable_language_identification: bool,
}

#[derive(Debug, Deserialize)]
struct SonioxTranscript {
    #[serde(default)]
    text: String,
    #[serde(default)]
    tokens: Vec<SonioxToken>,
}

#[derive(Debug, Deserialize)]
struct SonioxToken {
    #[serde(default)]
    text: String,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    confidence: Option<f32>,
    speaker: Option<String>,
    language: Option<String>,
}

fn error_message(status: StatusCode, body: &Value, operation: &str) -> String {
    let error_type = body.get("error_type").and_then(Value::as_str);
    let request_id = body.get("request_id").and_then(Value::as_str);
    let detail = match error_type {
        Some("unauthenticated") => "保存済みのSoniox APIキーが無効です。設定し直してください。".into(),
        Some("permission_denied") | Some("forbidden") => {
            "Soniox APIキーに文字起こしに必要な権限がありません。".into()
        }
        Some("limit_exceeded") => {
            "Sonioxの利用上限または同時処理数の上限に達しています。Consoleで上限を確認してください。".into()
        }
        Some("invalid_audio_file") => {
            "Sonioxで音声ファイルを処理できませんでした。形式・内容・5時間の上限を確認してください。".into()
        }
        Some("model_not_available") => {
            "Soniox v5モデルをこのプロジェクトで利用できません。".into()
        }
        _ => format!("Sonioxで{operation}に失敗しました（HTTP {status}）。"),
    };
    if let Some(request_id) = request_id {
        format!("{detail}（request ID: {request_id}）")
    } else {
        detail
    }
}

async fn parse_response<T: for<'de> Deserialize<'de>>(
    response: Response,
    operation: &str,
) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        eprintln!(
            "Soniox {operation} failed: HTTP {status}, error_type={:?}, request_id={:?}",
            body.get("error_type").and_then(Value::as_str),
            body.get("request_id").and_then(Value::as_str)
        );
        return Err(error_message(status, &body, operation));
    }
    response.json::<T>().await.map_err(|error| {
        eprintln!("Could not parse Soniox {operation} response: {error:?}");
        format!("Sonioxの{operation}結果を読み取れませんでした。")
    })
}

pub(crate) async fn validate_api_key(api_key: &SecretString) -> Result<(), String> {
    let response = SonioxClient::new(api_key)?
        .get("/models")
        .send()
        .await
        .map_err(|error| format!("Sonioxに接続できませんでした: {error}"))?;
    let body: Value = parse_response(response, "APIキーの確認").await?;
    let model_available = body
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| model.get("id").and_then(Value::as_str) == Some(MODEL_ID))
        });
    if !model_available {
        return Err(
            "APIキーは有効ですが、このSonioxプロジェクトではSoniox v5 Asyncを利用できません。"
                .into(),
        );
    }
    Ok(())
}

fn normalize(response: SonioxTranscript) -> Transcript {
    let mut speakers = HashMap::<String, String>::new();
    let mut languages = HashMap::<String, usize>::new();
    let mut previous_speaker = None::<String>;
    let mut tokens = Vec::with_capacity(response.tokens.len());
    for token in response.tokens {
        let speaker_id = token.speaker.or_else(|| previous_speaker.clone());
        if let Some(speaker_id) = &speaker_id {
            previous_speaker = Some(speaker_id.clone());
        }
        let speaker = speaker_id.map(|speaker_id| {
            let next = speakers.len() + 1;
            speakers
                .entry(speaker_id)
                .or_insert_with(|| format!("Speaker {next}"))
                .clone()
        });
        if let Some(language) = &token.language {
            *languages.entry(language.clone()).or_default() += 1;
        }
        let start_ms = token.start_ms;
        let end_ms = token.end_ms.map(|end| end.max(start_ms.unwrap_or(0)));
        tokens.push(TranscriptToken {
            text: token.text,
            start_ms,
            end_ms,
            start_time_source: start_ms.map(|_| TokenTimeSource::Provider),
            end_time_source: end_ms.map(|_| TokenTimeSource::Provider),
            speaker_source: speaker.as_ref().map(|_| TokenSpeakerSource::Provider),
            speaker,
            confidence: token.confidence,
        });
    }
    let mut segments = segments_from_tokens(&tokens);
    if segments.is_empty() && !response.text.trim().is_empty() {
        segments.push(TranscriptSegment {
            speaker: "Speaker 1".into(),
            start_ms: 0,
            end_ms: 0,
            text: response.text.trim().into(),
        });
    }
    let language = languages
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(language, _)| language)
        .unwrap_or_else(|| LANGUAGE_HINT.into());
    Transcript {
        provider: "soniox".into(),
        model: MODEL_ID.into(),
        language,
        tokens,
        segments,
    }
}

async fn cleanup(client: &SonioxClient, transcription_id: Option<&str>, file_id: &str) {
    if let Some(id) = transcription_id {
        match client.delete(&format!("/transcriptions/{id}")).send().await {
            Ok(response)
                if response.status().is_success() || response.status() == StatusCode::NOT_FOUND => {
            }
            Ok(response) => eprintln!(
                "Could not delete Soniox transcription {id}: HTTP {}",
                response.status()
            ),
            Err(error) => eprintln!("Could not delete Soniox transcription {id}: {error:?}"),
        }
    }
    match client.delete(&format!("/files/{file_id}")).send().await {
        Ok(response)
            if response.status().is_success() || response.status() == StatusCode::NOT_FOUND => {}
        Ok(response) => eprintln!(
            "Could not delete Soniox file {file_id}: HTTP {}",
            response.status()
        ),
        Err(error) => eprintln!("Could not delete Soniox file {file_id}: {error:?}"),
    }
}

async fn transcribe_inner(
    client: &SonioxClient,
    file_id: &str,
) -> Result<(String, Transcript), (Option<String>, String)> {
    let request = CreateTranscription {
        model: MODEL_ID,
        file_id,
        language_hints: [LANGUAGE_HINT],
        enable_speaker_diarization: true,
        enable_language_identification: true,
    };
    let response = client
        .post("/transcriptions")
        .json(&request)
        .send()
        .await
        .map_err(|error| {
            (
                None,
                format!("Sonioxへ文字起こしを依頼できませんでした: {error}"),
            )
        })?;
    let job: TranscriptionJob = parse_response(response, "文字起こしの開始")
        .await
        .map_err(|error| (None, error))?;
    let transcription_id = job.id;
    for _ in 0..MAX_POLL_ATTEMPTS {
        let response = client
            .get(&format!("/transcriptions/{transcription_id}"))
            .send()
            .await
            .map_err(|error| {
                (
                    Some(transcription_id.clone()),
                    format!("Sonioxの文字起こし状態を確認できませんでした: {error}"),
                )
            })?;
        let job: TranscriptionJob = parse_response(response, "文字起こし状態の確認")
            .await
            .map_err(|error| (Some(transcription_id.clone()), error))?;
        match job.status.as_str() {
            "completed" => {
                let response = client
                    .get(&format!("/transcriptions/{transcription_id}/transcript"))
                    .send()
                    .await
                    .map_err(|error| {
                        (
                            Some(transcription_id.clone()),
                            format!("Sonioxの文字起こし結果を取得できませんでした: {error}"),
                        )
                    })?;
                let transcript =
                    parse_response::<SonioxTranscript>(response, "文字起こし結果の取得")
                        .await
                        .map_err(|error| (Some(transcription_id.clone()), error))?;
                return Ok((transcription_id, normalize(transcript)));
            }
            "error" | "failed" => {
                let kind = job.error_type.as_deref().unwrap_or("unknown");
                let message = job.error_message.as_deref().unwrap_or("詳細不明");
                return Err((
                    Some(transcription_id),
                    format!("Sonioxの文字起こしに失敗しました（{kind}）: {message}"),
                ));
            }
            _ => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    Err((
        Some(transcription_id),
        "Sonioxの文字起こし完了待ちがタイムアウトしました。".into(),
    ))
}

pub(crate) async fn transcribe(path: &Path, api_key: &SecretString) -> Result<Transcript, String> {
    let client = SonioxClient::new(api_key)?;
    let form = Form::new().file("file", path).await.map_err(|error| {
        eprintln!("Could not open selected audio for Soniox: {error:?}");
        "選択した音声ファイルを開けませんでした。".to_string()
    })?;
    let response = client
        .post("/files")
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("Sonioxへ音声をアップロードできませんでした: {error}"))?;
    let uploaded: UploadedFile = parse_response(response, "音声のアップロード").await?;
    let result = transcribe_inner(&client, &uploaded.id).await;
    let transcription_id = match &result {
        Ok((id, _)) => Some(id.as_str()),
        Err((id, _)) => id.as_deref(),
    };
    cleanup(&client, transcription_id, &uploaded.id).await;
    result
        .map(|(_, transcript)| transcript)
        .map_err(|(_, error)| error)
}

#[cfg(test)]
mod tests {
    use super::{error_message, normalize, SonioxTranscript};
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn preserves_provider_timing_speakers_and_confidence() {
        let response = serde_json::from_value::<SonioxTranscript>(json!({
            "text": "こんにちは。よろしくお願いします。",
            "tokens": [
                {"text":"こんにちは。","start_ms":100,"end_ms":600,"confidence":0.97,"speaker":"1","language":"ja"},
                {"text":"よろしくお願いします。","start_ms":800,"end_ms":1700,"confidence":0.94,"speaker":"2","language":"ja"}
            ]
        }))
        .expect("deserialize Soniox response");
        let transcript = normalize(response);
        assert_eq!(transcript.provider, "soniox");
        assert_eq!(transcript.model, "stt-async-v5");
        assert_eq!(transcript.tokens[0].confidence, Some(0.97));
        assert_eq!(transcript.tokens[1].speaker.as_deref(), Some("Speaker 2"));
        assert_eq!(transcript.segments.len(), 2);
    }

    #[test]
    fn exposes_request_id_for_support() {
        let message = error_message(
            StatusCode::UNAUTHORIZED,
            &json!({"error_type":"unauthenticated","request_id":"request-1"}),
            "APIキーの確認",
        );
        assert!(message.contains("APIキーが無効"));
        assert!(message.contains("request-1"));
    }
}
