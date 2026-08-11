use std::{path::Path, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{redirect::Policy, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use super::{
    context::TranscriptionContext, segments_from_tokens, TokenTimeSource, Transcript,
    TranscriptSegment, TranscriptToken, TranscriptionOutcome,
};

pub(crate) const MODEL_ID: &str = "@cf/openai/whisper-large-v3-turbo";
pub(crate) const PRICE_USD_PER_AUDIO_MINUTE: f64 = 0.0005;
pub(crate) const NEURONS_PER_AUDIO_MINUTE: f64 = 46.63;
pub(crate) const FREE_DAILY_NEURONS: f64 = 10_000.0;
const API_BASE_URL: &str = "https://api.cloudflare.com/client/v4/accounts";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize)]
struct TranscriptionRequest {
    audio: String,
    task: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_prompt: Option<String>,
}

#[derive(Serialize)]
struct TextGenerationRequest<'a> {
    messages: [TextGenerationMessage<'a>; 1],
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct TextGenerationMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    success: bool,
    result: Option<T>,
    #[serde(default)]
    errors: Vec<ApiMessage>,
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct WorkersAiTranscript {
    #[serde(default)]
    text: String,
    #[serde(default)]
    segments: Vec<WorkersAiSegment>,
    #[serde(default)]
    words: Vec<WorkersAiWord>,
    #[serde(default)]
    transcription_info: Option<TranscriptionInfo>,
}

#[derive(Debug, Deserialize)]
struct WorkersAiTextGeneration {
    #[serde(default)]
    response: String,
    #[serde(default)]
    choices: Vec<WorkersAiTextChoice>,
}

#[derive(Debug, Deserialize)]
struct WorkersAiTextChoice {
    message: WorkersAiTextMessage,
}

#[derive(Debug, Deserialize)]
struct WorkersAiTextMessage {
    #[serde(default)]
    content: String,
}

impl WorkersAiTextGeneration {
    fn into_text(self) -> Option<String> {
        if !self.response.trim().is_empty() {
            return Some(self.response);
        }
        self.choices
            .into_iter()
            .map(|choice| choice.message.content)
            .find(|content| !content.trim().is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionInfo {
    #[serde(default)]
    language: String,
}

#[derive(Debug, Deserialize)]
struct WorkersAiSegment {
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    words: Vec<WorkersAiWord>,
}

#[derive(Debug, Deserialize)]
struct WorkersAiWord {
    #[serde(default)]
    word: String,
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("Cloudflare接続を準備できませんでした: {error}"))
}

fn account_id(value: &SecretString) -> Result<&str, String> {
    let value = value.expose_secret().trim();
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Cloudflare Account IDの形式が正しくありません。".into());
    }
    Ok(value)
}

fn endpoint(account_id: &str, suffix: &str) -> String {
    format!("{API_BASE_URL}/{account_id}/ai/{suffix}")
}

fn api_error(status: StatusCode, envelope: Option<&ApiEnvelope<serde_json::Value>>) -> String {
    let detail = envelope
        .and_then(|body| body.errors.first())
        .map(|error| error.message.trim())
        .filter(|message| !message.is_empty());
    match status {
        StatusCode::UNAUTHORIZED => "Cloudflare APIトークンが無効です。".into(),
        StatusCode::FORBIDDEN => {
            "Cloudflare APIトークンにWorkers AIの読み取り・実行権限がありません。".into()
        }
        _ => detail.map_or_else(
            || format!("Cloudflare Workers AIのリクエストに失敗しました（HTTP {status}）。"),
            |detail| format!("Cloudflare Workers AI: {detail}（HTTP {status}）"),
        ),
    }
}

pub(crate) async fn validate_credentials(
    account: &SecretString,
    api_token: &SecretString,
) -> Result<(), String> {
    let account = account_id(account)?;
    let response = client()?
        .get(endpoint(account, "models/search"))
        .bearer_auth(api_token.expose_secret())
        .query(&[("per_page", "1")])
        .send()
        .await
        .map_err(|error| format!("Cloudflareへ接続できませんでした: {error}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.json::<ApiEnvelope<serde_json::Value>>().await.ok();
    Err(api_error(status, body.as_ref()))
}

pub(crate) async fn generate_text(
    account: &SecretString,
    api_token: &SecretString,
    model_id: &str,
    prompt: &str,
) -> Result<String, String> {
    let account = account_id(account)?;
    let request = TextGenerationRequest {
        messages: [TextGenerationMessage {
            role: "user",
            content: prompt,
        }],
        max_tokens: 8_192,
        temperature: 0.1,
    };
    let response = client()?
        .post(endpoint(account, &format!("run/{model_id}")))
        .bearer_auth(api_token.expose_secret())
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("Cloudflareへ会議ノートを送信できませんでした: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Cloudflareの応答を読み取れませんでした: {error}"))?;
    if !status.is_success() {
        let body = serde_json::from_slice::<ApiEnvelope<serde_json::Value>>(&bytes).ok();
        return Err(api_error(status, body.as_ref()));
    }
    let envelope: ApiEnvelope<WorkersAiTextGeneration> =
        serde_json::from_slice(&bytes).map_err(|error| {
            eprintln!("Could not parse Cloudflare Workers AI text response: {error}");
            "Cloudflare Workers AIの応答形式を読み取れませんでした。".to_string()
        })?;
    if !envelope.success {
        let message = envelope
            .errors
            .first()
            .map(|error| error.message.as_str())
            .unwrap_or("詳細不明");
        return Err(format!("Cloudflare Workers AI: {message}"));
    }
    let response = envelope
        .result
        .and_then(WorkersAiTextGeneration::into_text)
        .ok_or_else(|| {
            "Cloudflare Workers AIから会議ノート本文を受け取れませんでした。".to_string()
        })?;
    Ok(response)
}

fn prompt(context: Option<&TranscriptionContext>) -> Option<String> {
    let context = context?;
    let mut parts = Vec::new();
    if !context.background.trim().is_empty() {
        parts.push(context.background.trim().to_owned());
    }
    if !context.terms.is_empty() {
        parts.push(format!("重要用語: {}", context.terms.join("、")));
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn milliseconds(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1_000.0).round() as u64
    } else {
        0
    }
}

fn normalize(response: WorkersAiTranscript) -> Transcript {
    let language = response
        .transcription_info
        .as_ref()
        .map(|info| info.language.trim())
        .filter(|language| !language.is_empty())
        .unwrap_or("unknown")
        .to_owned();
    let mut tokens = Vec::new();
    tokens.extend(
        response
            .words
            .iter()
            .filter(|word| !word.word.is_empty())
            .map(|word| TranscriptToken {
                text: word.word.clone(),
                start_ms: Some(milliseconds(word.start)),
                end_ms: Some(milliseconds(word.end)),
                start_time_source: Some(TokenTimeSource::Provider),
                end_time_source: Some(TokenTimeSource::Provider),
                speaker: None,
                speaker_source: None,
                confidence: None,
                utterance_id: None,
            }),
    );
    for segment in &response.segments {
        if !response.words.is_empty() {
            break;
        } else if segment.words.is_empty() {
            if !segment.text.is_empty() {
                tokens.push(TranscriptToken {
                    text: segment.text.clone(),
                    start_ms: Some(milliseconds(segment.start)),
                    end_ms: Some(milliseconds(segment.end)),
                    start_time_source: Some(TokenTimeSource::Provider),
                    end_time_source: Some(TokenTimeSource::Provider),
                    speaker: None,
                    speaker_source: None,
                    confidence: None,
                    utterance_id: None,
                });
            }
        } else {
            tokens.extend(
                segment
                    .words
                    .iter()
                    .filter(|word| !word.word.is_empty())
                    .map(|word| TranscriptToken {
                        text: word.word.clone(),
                        start_ms: Some(milliseconds(word.start)),
                        end_ms: Some(milliseconds(word.end)),
                        start_time_source: Some(TokenTimeSource::Provider),
                        end_time_source: Some(TokenTimeSource::Provider),
                        speaker: None,
                        speaker_source: None,
                        confidence: None,
                        utterance_id: None,
                    }),
            );
        }
    }
    let segments = if tokens.is_empty() {
        let end_ms = response
            .segments
            .last()
            .map(|segment| milliseconds(segment.end))
            .unwrap_or(0);
        if response.text.is_empty() {
            Vec::new()
        } else {
            vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 0,
                end_ms,
                text: response.text,
            }]
        }
    } else {
        segments_from_tokens(&tokens)
    };
    Transcript {
        provider: "cloudflare".into(),
        model: MODEL_ID.into(),
        language,
        tokens,
        segments,
    }
}

pub(crate) async fn transcribe(
    path: &Path,
    audio_duration_ms: u64,
    account: &SecretString,
    api_token: &SecretString,
    context: Option<&TranscriptionContext>,
) -> Result<TranscriptionOutcome, String> {
    let account = account_id(account)?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("選択した音声ファイルを開けませんでした: {error}"))?;
    let request = TranscriptionRequest {
        audio: STANDARD.encode(bytes),
        task: "transcribe",
        initial_prompt: prompt(context),
    };
    let response = client()?
        .post(endpoint(account, &format!("run/{MODEL_ID}")))
        .bearer_auth(api_token.expose_secret())
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("Cloudflareへ音声を送信できませんでした: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Cloudflareの応答を読み取れませんでした: {error}"))?;
    if !status.is_success() {
        let body = serde_json::from_slice::<ApiEnvelope<serde_json::Value>>(&bytes).ok();
        return Err(api_error(status, body.as_ref()));
    }
    let envelope: ApiEnvelope<WorkersAiTranscript> =
        serde_json::from_slice(&bytes).map_err(|error| {
            eprintln!("Could not parse Cloudflare Workers AI response: {error}");
            "Cloudflare Workers AIの応答形式を読み取れませんでした。".to_string()
        })?;
    if !envelope.success {
        let message = envelope
            .errors
            .first()
            .map(|error| error.message.as_str())
            .unwrap_or("詳細不明");
        return Err(format!("Cloudflare Workers AI: {message}"));
    }
    let result = envelope
        .result
        .ok_or_else(|| "Cloudflare Workers AIの応答に文字起こし結果がありません。".to_string())?;
    Ok(TranscriptionOutcome {
        transcript: normalize(result),
        cost_usd: Some(format_cost_usd(audio_duration_ms)),
    })
}

fn format_cost_usd(audio_duration_ms: u64) -> String {
    let cost = audio_duration_ms as f64 / 60_000.0 * PRICE_USD_PER_AUDIO_MINUTE;
    let formatted = format!("{cost:.10}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        format_cost_usd, normalize, WorkersAiTextGeneration, WorkersAiTranscript, MODEL_ID,
    };

    #[test]
    fn estimates_cost_from_cloudflare_audio_minute_pricing() {
        assert_eq!(format_cost_usd(60_000), "0.0005");
        assert_eq!(format_cost_usd(30_000), "0.00025");
    }

    #[test]
    fn normalizes_segment_and_word_timestamps() {
        let response: WorkersAiTranscript = serde_json::from_value(serde_json::json!({
            "text": "こんにちは 世界",
            "transcription_info": { "language": "ja" },
            "segments": [{
                "start": 0.1,
                "end": 1.4,
                "text": "こんにちは 世界",
                "words": [
                    { "word": "こんにちは", "start": 0.1, "end": 0.7 },
                    { "word": " 世界", "start": 0.8, "end": 1.4 }
                ]
            }]
        }))
        .expect("fixture should deserialize");
        let transcript = normalize(response);
        assert_eq!(transcript.provider, "cloudflare");
        assert_eq!(transcript.model, MODEL_ID);
        assert_eq!(transcript.language, "ja");
        assert_eq!(transcript.tokens[0].start_ms, Some(100));
        assert_eq!(transcript.tokens[1].end_ms, Some(1_400));
    }

    #[test]
    fn reads_native_and_openai_compatible_text_responses() {
        let native: WorkersAiTextGeneration =
            serde_json::from_value(serde_json::json!({ "response": "native" }))
                .expect("native response");
        assert_eq!(native.into_text().as_deref(), Some("native"));

        let compatible: WorkersAiTextGeneration = serde_json::from_value(serde_json::json!({
            "choices": [{ "message": { "content": "compatible" } }]
        }))
        .expect("OpenAI-compatible response");
        assert_eq!(compatible.into_text().as_deref(), Some("compatible"));
    }
}
