use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{stream, StreamExt};
use reqwest::{redirect::Policy, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::{
    audio_decode, context::TranscriptionContext, segments_from_tokens, TokenTimeSource, Transcript,
    TranscriptSegment, TranscriptToken, TranscriptionOutcome,
};
use crate::commands::transcribe::{
    publish_transcription_progress, TranscriptionProgress, TranscriptionStage,
};

pub(crate) const MODEL_ID: &str = "@cf/openai/whisper-large-v3-turbo";
pub(crate) const PRICE_USD_PER_AUDIO_MINUTE: f64 = 0.0005;
pub(crate) const NEURONS_PER_AUDIO_MINUTE: f64 = 46.63;
pub(crate) const FREE_DAILY_NEURONS: f64 = 10_000.0;
const API_BASE_URL: &str = "https://api.cloudflare.com/client/v4/accounts";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TEXT_GENERATION_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const CHUNK_CORE_MS: u64 = 5 * 60 * 1_000;
const CHUNK_OVERLAP_MS: u64 = 2_000;
const CHUNK_SAMPLE_RATE: u32 = 16_000;
#[cfg(target_os = "android")]
const MAX_PARALLEL_CHUNKS: usize = 2;
#[cfg(not(target_os = "android"))]
const MAX_PARALLEL_CHUNKS: usize = 4;
const MAX_CHUNK_ATTEMPTS: u32 = 3;
const MAX_TEXT_GENERATION_ATTEMPTS: u32 = 3;
const TEXT_GENERATION_MAX_TOKENS: u32 = 32_768;

#[derive(Serialize)]
struct TranscriptionRequest {
    audio: String,
    task: &'static str,
    language: &'static str,
    vad_filter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_prompt: Option<String>,
}

#[derive(Debug, Clone)]
struct ChunkSpec {
    index: usize,
    start_ms: u64,
    duration_ms: u64,
    path: PathBuf,
}

struct ChunkWorkspace(PathBuf);

impl ChunkWorkspace {
    fn create(parent: &Path) -> Result<Self, String> {
        let path = parent.join(format!("cloudflare-stt-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&path)
            .map_err(|error| format!("音声分割用の一時領域を作成できませんでした: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for ChunkWorkspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove Cloudflare transcription chunks: {error}");
        }
    }
}

#[derive(Debug)]
struct ChunkRequestError {
    message: String,
    retryable: bool,
}

#[derive(Serialize)]
struct TextGenerationRequest<'a> {
    messages: [TextGenerationMessage<'a>; 1],
    max_tokens: u32,
    temperature: f32,
    stream: bool,
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

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct WorkersAiTextGeneration {
    #[serde(default)]
    response: String,
    #[serde(default)]
    choices: Vec<WorkersAiTextChoice>,
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct WorkersAiTextChoice {
    message: WorkersAiTextMessage,
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct WorkersAiTextMessage {
    #[serde(default)]
    content: String,
}

#[cfg(test)]
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
        .connect_timeout(Duration::from_secs(10))
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
        StatusCode::UNAUTHORIZED => "Cloudflareの認証情報が無効または期限切れです。".into(),
        StatusCode::FORBIDDEN => {
            "Cloudflareの認証情報にWorkers AIの読み取り・実行権限がありません。".into()
        }
        StatusCode::TOO_MANY_REQUESTS => {
            "Cloudflare Workers AIの利用上限に達したか、リクエストが集中しています。".into()
        }
        StatusCode::NOT_FOUND => {
            "選択したCloudflare Workers AIモデルは利用できないか、対応していません。".into()
        }
        status if status.is_server_error() => {
            format!("Cloudflare Workers AIで一時的な障害が発生しています（HTTP {status}）。")
        }
        _ => detail.map_or_else(
            || format!("Cloudflare Workers AIのリクエストに失敗しました（HTTP {status}）。"),
            |detail| format!("Cloudflare Workers AI: {detail}（HTTP {status}）"),
        ),
    }
}

pub(crate) fn is_authentication_error(error: &str) -> bool {
    const MESSAGE: &str = "Cloudflareの認証情報が無効または期限切れです。";
    error == MESSAGE
        || error
            .strip_suffix(MESSAGE)
            .is_some_and(|prefix| prefix.ends_with(": "))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextGenerationProgress {
    AttemptStarted {
        attempt: u32,
        max_attempts: u32,
    },
    StreamStarted {
        attempt: u32,
        max_attempts: u32,
    },
    RetryScheduled {
        next_attempt: u32,
        max_attempts: u32,
        delay_seconds: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextGenerationOutput {
    pub(crate) text: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) usage_estimated: bool,
}

#[derive(Debug, Default)]
struct TextGenerationUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

pub(crate) async fn generate_text<F>(
    account: &SecretString,
    api_token: &SecretString,
    model_id: &str,
    prompt: &str,
    mut on_progress: F,
) -> Result<TextGenerationOutput, String>
where
    F: FnMut(TextGenerationProgress),
{
    let account = account_id(account)?;
    let http_client = client()?;
    let mut last_error = None;
    let mut attempts_used = 0;
    for attempt in 1..=MAX_TEXT_GENERATION_ATTEMPTS {
        attempts_used = attempt;
        on_progress(TextGenerationProgress::AttemptStarted {
            attempt,
            max_attempts: MAX_TEXT_GENERATION_ATTEMPTS,
        });
        match request_text_generation(&http_client, account, api_token, model_id, prompt, || {
            on_progress(TextGenerationProgress::StreamStarted {
                attempt,
                max_attempts: MAX_TEXT_GENERATION_ATTEMPTS,
            });
        })
        .await
        {
            Ok(text) => return Ok(text),
            Err(error) => {
                let retryable = error.retryable;
                last_error = Some(error.message);
                if !retryable || attempt == MAX_TEXT_GENERATION_ATTEMPTS {
                    break;
                }
                let delay_seconds = 1 << attempt;
                on_progress(TextGenerationProgress::RetryScheduled {
                    next_attempt: attempt + 1,
                    max_attempts: MAX_TEXT_GENERATION_ATTEMPTS,
                    delay_seconds,
                });
                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
            }
        }
    }
    Err(format!(
        "会議ノート生成を{}回試行しましたが完了できませんでした: {}",
        attempts_used,
        last_error.unwrap_or_else(|| "詳細不明".into())
    ))
}

#[derive(Debug)]
struct TextGenerationError {
    message: String,
    retryable: bool,
}

async fn request_text_generation<F>(
    http_client: &reqwest::Client,
    account: &str,
    api_token: &SecretString,
    model_id: &str,
    prompt: &str,
    on_stream_started: F,
) -> Result<TextGenerationOutput, TextGenerationError>
where
    F: FnOnce(),
{
    let request = TextGenerationRequest {
        messages: [TextGenerationMessage {
            role: "user",
            content: prompt,
        }],
        max_tokens: TEXT_GENERATION_MAX_TOKENS,
        temperature: 0.1,
        stream: true,
    };
    let response = tokio::time::timeout(
        TEXT_GENERATION_IDLE_TIMEOUT,
        http_client
            .post(endpoint(account, &format!("run/{model_id}")))
            .bearer_auth(api_token.expose_secret())
            .json(&request)
            .send(),
    )
    .await
    .map_err(|_| TextGenerationError {
        message: format!(
            "Cloudflare Workers AIから{}秒間応答がありませんでした。",
            TEXT_GENERATION_IDLE_TIMEOUT.as_secs()
        ),
        retryable: true,
    })?
    .map_err(|error| TextGenerationError {
        message: format!("Cloudflareへ会議ノートを送信できませんでした: {error}"),
        retryable: error.is_timeout() || error.is_connect() || error.is_request(),
    })?;
    let status = response.status();
    if !status.is_success() {
        let bytes = response
            .bytes()
            .await
            .map_err(|error| TextGenerationError {
                message: format!("Cloudflareのエラー応答を読み取れませんでした: {error}"),
                retryable: true,
            })?;
        let body = serde_json::from_slice::<ApiEnvelope<serde_json::Value>>(&bytes).ok();
        return Err(TextGenerationError {
            message: api_error(status, body.as_ref()),
            retryable: retryable_text_generation_status(status),
        });
    }
    on_stream_started();
    read_text_generation_stream(response, prompt).await
}

async fn read_text_generation_stream(
    response: reqwest::Response,
    prompt: &str,
) -> Result<TextGenerationOutput, TextGenerationError> {
    let mut stream = response.bytes_stream();
    let mut pending = Vec::<u8>::new();
    let mut event_data = Vec::<String>::new();
    let mut output = String::new();
    let mut usage = TextGenerationUsage::default();
    loop {
        let next = tokio::time::timeout(TEXT_GENERATION_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| TextGenerationError {
                message: format!(
                    "Cloudflare Workers AIから{}秒間データを受信できませんでした。",
                    TEXT_GENERATION_IDLE_TIMEOUT.as_secs()
                ),
                retryable: true,
            })?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|error| TextGenerationError {
            message: format!("Cloudflareのストリームを読み取れませんでした: {error}"),
            retryable: true,
        })?;
        pending.extend_from_slice(&chunk);
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line = pending.drain(..=newline).collect::<Vec<_>>();
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            let line = std::str::from_utf8(&line).map_err(|_| TextGenerationError {
                message: "CloudflareのストリームがUTF-8ではありません。".into(),
                retryable: true,
            })?;
            if line.is_empty() {
                if consume_text_generation_event(&mut event_data, &mut output, &mut usage)? {
                    return finish_text_generation(output, usage, prompt);
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                event_data.push(data.trim_start().to_string());
            }
        }
    }
    if !pending.is_empty() {
        let line = std::str::from_utf8(&pending).map_err(|_| TextGenerationError {
            message: "CloudflareのストリームがUTF-8ではありません。".into(),
            retryable: true,
        })?;
        if let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") {
            event_data.push(data.trim_start().to_string());
        }
    }
    consume_text_generation_event(&mut event_data, &mut output, &mut usage)?;
    finish_text_generation(output, usage, prompt)
}

fn finish_text_generation(
    output: String,
    usage: TextGenerationUsage,
    prompt: &str,
) -> Result<TextGenerationOutput, TextGenerationError> {
    if output.trim().is_empty() {
        Err(TextGenerationError {
            message: "Cloudflare Workers AIから会議ノート本文を受信できませんでした。".into(),
            retryable: true,
        })
    } else {
        let usage_estimated = usage.input_tokens.is_none() || usage.output_tokens.is_none();
        Ok(TextGenerationOutput {
            input_tokens: usage
                .input_tokens
                .unwrap_or_else(|| estimate_text_tokens(prompt)),
            output_tokens: usage
                .output_tokens
                .unwrap_or_else(|| estimate_text_tokens(&output)),
            text: output,
            usage_estimated,
        })
    }
}

fn consume_text_generation_event(
    event_data: &mut Vec<String>,
    output: &mut String,
    usage: &mut TextGenerationUsage,
) -> Result<bool, TextGenerationError> {
    if event_data.is_empty() {
        return Ok(false);
    }
    let data = event_data.join("\n");
    event_data.clear();
    if data.trim() == "[DONE]" {
        return Ok(true);
    }
    let value: serde_json::Value =
        serde_json::from_str(&data).map_err(|error| TextGenerationError {
            message: format!("CloudflareのSSEデータを解析できませんでした: {error}"),
            retryable: true,
        })?;
    if let Some(message) = value
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        return Err(TextGenerationError {
            message: format!("Cloudflare Workers AI: {message}"),
            retryable: true,
        });
    }
    let text = value
        .get("response")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .pointer("/choices/0/delta/content")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/choices/0/text")
                .and_then(serde_json::Value::as_str)
        });
    if let Some(text) = text {
        output.push_str(text);
    }
    if let Some(value) = value.get("usage") {
        usage.input_tokens = value
            .get("prompt_tokens")
            .or_else(|| value.get("input_tokens"))
            .and_then(serde_json::Value::as_u64)
            .or(usage.input_tokens);
        usage.output_tokens = value
            .get("completion_tokens")
            .or_else(|| value.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
            .or(usage.output_tokens);
    }
    Ok(false)
}

pub(crate) fn estimate_text_tokens(value: &str) -> u64 {
    let mut ascii = 0_u64;
    let mut non_ascii = 0_u64;
    for character in value.chars() {
        if character.is_ascii() {
            ascii = ascii.saturating_add(1);
        } else {
            non_ascii = non_ascii.saturating_add(1);
        }
    }
    ascii.saturating_add(3) / 4 + non_ascii
}

fn retryable_text_generation_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
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

fn chunk_windows(audio_duration_ms: u64) -> Result<Vec<(u64, u64)>, String> {
    if audio_duration_ms == 0 {
        return Err("音声の長さを確認できないため、Cloudflare向けに分割できませんでした。".into());
    }
    let mut windows = Vec::new();
    let mut start_ms = 0u64;
    while start_ms < audio_duration_ms {
        let remaining = audio_duration_ms.saturating_sub(start_ms);
        windows.push((
            start_ms,
            remaining.min(CHUNK_CORE_MS.saturating_add(CHUNK_OVERLAP_MS)),
        ));
        start_ms = start_ms.saturating_add(CHUNK_CORE_MS);
    }
    Ok(windows)
}

fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    let data_size = samples
        .len()
        .checked_mul(2)
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| "分割した音声がWAVの上限を超えました。".to_string())?;
    let mut file =
        File::create(path).map_err(|error| format!("分割音声を作成できませんでした: {error}"))?;
    file.write_all(b"RIFF")
        .and_then(|_| file.write_all(&(36u32.saturating_add(data_size)).to_le_bytes()))
        .and_then(|_| file.write_all(b"WAVEfmt "))
        .and_then(|_| file.write_all(&16u32.to_le_bytes()))
        .and_then(|_| file.write_all(&1u16.to_le_bytes()))
        .and_then(|_| file.write_all(&1u16.to_le_bytes()))
        .and_then(|_| file.write_all(&sample_rate.to_le_bytes()))
        .and_then(|_| file.write_all(&(sample_rate.saturating_mul(2)).to_le_bytes()))
        .and_then(|_| file.write_all(&2u16.to_le_bytes()))
        .and_then(|_| file.write_all(&16u16.to_le_bytes()))
        .and_then(|_| file.write_all(b"data"))
        .and_then(|_| file.write_all(&data_size.to_le_bytes()))
        .map_err(|error| format!("分割音声のヘッダーを書き込めませんでした: {error}"))?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        file.write_all(&value.to_le_bytes())
            .map_err(|error| format!("分割音声を書き込めませんでした: {error}"))?;
    }
    file.flush()
        .map_err(|error| format!("分割音声を保存できませんでした: {error}"))
}

fn produce_chunks(
    audio_path: &Path,
    windows: &[(u64, u64)],
    workspace: &Path,
    sender: tokio::sync::mpsc::Sender<ChunkSpec>,
    mut on_produced: impl FnMut(u32),
) -> Result<(), String> {
    let mut produced = 0usize;
    audio_decode::decode_mono_regions_resampled(
        audio_path,
        CHUNK_SAMPLE_RATE,
        windows,
        |index, sample_rate, samples| {
            let path = workspace.join(format!("chunk-{index:04}.wav"));
            write_pcm16_wav(&path, sample_rate, samples)?;
            let (start_ms, duration_ms) = windows[index];
            sender
                .blocking_send(ChunkSpec {
                    index,
                    start_ms,
                    duration_ms,
                    path,
                })
                .map_err(|_| "音声チャンクの送信処理が停止しました。".to_string())?;
            produced = produced.saturating_add(1);
            on_produced(u32::try_from(produced).unwrap_or(u32::MAX));
            Ok(())
        },
    )?;
    if produced != windows.len() {
        return Err("Cloudflare向けの音声チャンクをすべて作成できませんでした。".into());
    }
    Ok(())
}

async fn request_chunk(
    http_client: &reqwest::Client,
    account: &str,
    api_token: &SecretString,
    chunk: &ChunkSpec,
    initial_prompt: Option<String>,
) -> Result<WorkersAiTranscript, ChunkRequestError> {
    let bytes = tokio::fs::read(&chunk.path)
        .await
        .map_err(|error| ChunkRequestError {
            message: format!("分割音声を開けませんでした: {error}"),
            retryable: false,
        })?;
    let request = TranscriptionRequest {
        audio: STANDARD.encode(bytes),
        task: "transcribe",
        language: "ja",
        vad_filter: true,
        initial_prompt,
    };
    let response = http_client
        .post(endpoint(account, &format!("run/{MODEL_ID}")))
        .bearer_auth(api_token.expose_secret())
        .json(&request)
        .send()
        .await
        .map_err(|error| ChunkRequestError {
            message: format!("Cloudflareへ分割音声を送信できませんでした: {error}"),
            retryable: error.is_timeout() || error.is_connect() || error.is_request(),
        })?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| ChunkRequestError {
        message: format!("Cloudflareの応答を読み取れませんでした: {error}"),
        retryable: true,
    })?;
    if !status.is_success() {
        let body = serde_json::from_slice::<ApiEnvelope<serde_json::Value>>(&bytes).ok();
        return Err(ChunkRequestError {
            message: api_error(status, body.as_ref()),
            retryable: status == StatusCode::TOO_MANY_REQUESTS
                || status == StatusCode::REQUEST_TIMEOUT
                || status.is_server_error(),
        });
    }
    let envelope: ApiEnvelope<WorkersAiTranscript> =
        serde_json::from_slice(&bytes).map_err(|error| {
            eprintln!("Could not parse Cloudflare Workers AI chunk response: {error}");
            ChunkRequestError {
                message: "Cloudflare Workers AIの応答形式を読み取れませんでした。".into(),
                retryable: false,
            }
        })?;
    if !envelope.success {
        let message = envelope
            .errors
            .first()
            .map(|error| error.message.as_str())
            .unwrap_or("詳細不明");
        return Err(ChunkRequestError {
            message: format!("Cloudflare Workers AI: {message}"),
            retryable: true,
        });
    }
    envelope.result.ok_or_else(|| ChunkRequestError {
        message: "Cloudflare Workers AIの応答に文字起こし結果がありません。".into(),
        retryable: true,
    })
}

async fn request_chunk_with_retry(
    http_client: &reqwest::Client,
    account: &str,
    api_token: &SecretString,
    chunk: &ChunkSpec,
    initial_prompt: Option<String>,
) -> Result<WorkersAiTranscript, String> {
    let mut last_error = None;
    for attempt in 1..=MAX_CHUNK_ATTEMPTS {
        match request_chunk(
            http_client,
            account,
            api_token,
            chunk,
            initial_prompt.clone(),
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(error) => {
                let retryable = error.retryable;
                last_error = Some(error.message);
                if !retryable || attempt == MAX_CHUNK_ATTEMPTS {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1 << (attempt - 1))).await;
            }
        }
    }
    let start_minutes = chunk.start_ms as f64 / 60_000.0;
    let end_minutes = chunk.start_ms.saturating_add(chunk.duration_ms) as f64 / 60_000.0;
    Err(format!(
        "音声チャンク{}（{start_minutes:.1}〜{end_minutes:.1}分）の処理に失敗しました: {}",
        chunk.index + 1,
        last_error.unwrap_or_else(|| "詳細不明".into())
    ))
}

fn token_midpoint(token: &TranscriptToken) -> Option<u64> {
    match (token.start_ms, token.end_ms) {
        (Some(start), Some(end)) => Some(start.saturating_add(end.saturating_sub(start) / 2)),
        (Some(start), None) => Some(start),
        (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

fn owned_by_chunk(position_ms: Option<u64>, chunk: &ChunkSpec, total_chunks: usize) -> bool {
    let Some(position_ms) = position_ms else {
        return true;
    };
    let left = if chunk.index == 0 {
        0
    } else {
        chunk.start_ms.saturating_add(CHUNK_OVERLAP_MS / 2)
    };
    let right = if chunk.index + 1 == total_chunks {
        u64::MAX
    } else {
        chunk
            .start_ms
            .saturating_add(CHUNK_CORE_MS)
            .saturating_add(CHUNK_OVERLAP_MS / 2)
    };
    position_ms >= left && position_ms < right
}

fn merge_chunks(chunks: &[ChunkSpec], transcripts: Vec<Transcript>) -> Transcript {
    let mut language = "unknown".to_string();
    let mut tokens = Vec::new();
    let mut fallback_segments = Vec::new();
    for (chunk, transcript) in chunks.iter().zip(transcripts) {
        if language == "unknown" && transcript.language != "unknown" {
            language = transcript.language.clone();
        }
        if transcript.tokens.is_empty() {
            fallback_segments.extend(transcript.segments.into_iter().filter_map(|mut segment| {
                if segment.start_ms == segment.end_ms {
                    segment.end_ms = chunk.duration_ms;
                }
                segment.start_ms = segment.start_ms.saturating_add(chunk.start_ms);
                segment.end_ms = segment.end_ms.saturating_add(chunk.start_ms);
                let midpoint = segment
                    .start_ms
                    .saturating_add(segment.end_ms.saturating_sub(segment.start_ms) / 2);
                owned_by_chunk(Some(midpoint), chunk, chunks.len()).then_some(segment)
            }));
        } else {
            tokens.extend(transcript.tokens.into_iter().filter_map(|mut token| {
                token.start_ms = token
                    .start_ms
                    .map(|time| time.saturating_add(chunk.start_ms));
                token.end_ms = token.end_ms.map(|time| time.saturating_add(chunk.start_ms));
                owned_by_chunk(token_midpoint(&token), chunk, chunks.len()).then_some(token)
            }));
        }
    }
    tokens.sort_by_key(|token| token.start_ms.unwrap_or(u64::MAX));
    let mut segments = segments_from_tokens(&tokens);
    segments.append(&mut fallback_segments);
    segments.sort_by_key(|segment| segment.start_ms);
    Transcript {
        provider: "cloudflare".into(),
        model: MODEL_ID.into(),
        language,
        tokens,
        segments,
    }
}

pub(crate) async fn transcribe(
    app: &tauri::AppHandle,
    path: &Path,
    audio_duration_ms: u64,
    account: &SecretString,
    api_token: &SecretString,
    context: Option<&TranscriptionContext>,
) -> Result<TranscriptionOutcome, String> {
    let account = account_id(account)?;
    let windows = chunk_windows(audio_duration_ms)?;
    let total_chunks = u32::try_from(windows.len()).unwrap_or(u32::MAX);
    let total_work = total_chunks.saturating_mul(2);
    publish_transcription_progress(
        app,
        TranscriptionProgress::scaled(TranscriptionStage::Transcribing, 0, total_work, 0.05, 0.95),
    );
    let cache_directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("音声分割用の保存先を取得できませんでした: {error}"))?;
    let workspace = ChunkWorkspace::create(&cache_directory)?;
    let audio_path = path.to_path_buf();
    let chunk_workspace = workspace.0.clone();
    let chunk_windows = windows.clone();
    let (chunk_sender, chunk_receiver) =
        tokio::sync::mpsc::channel(MAX_PARALLEL_CHUNKS.saturating_mul(2));
    let completed = Arc::new(Mutex::new(0u32));
    let producer_completed = Arc::clone(&completed);
    let producer_app = app.clone();
    let producer = tauri::async_runtime::spawn_blocking(move || {
        produce_chunks(
            &audio_path,
            &chunk_windows,
            &chunk_workspace,
            chunk_sender,
            |_| {
                let mut progress = producer_completed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *progress = progress.saturating_add(1);
                publish_transcription_progress(
                    &producer_app,
                    TranscriptionProgress::scaled(
                        TranscriptionStage::Transcribing,
                        *progress,
                        total_work,
                        0.05,
                        0.95,
                    ),
                );
            },
        )
    });
    let http_client = client()?;
    let base_prompt = prompt(context);
    let chunk_stream = stream::unfold(chunk_receiver, |mut receiver| async move {
        receiver.recv().await.map(|chunk| (chunk, receiver))
    });
    let requests = chunk_stream
        .map(|chunk| {
            let app = app.clone();
            let completed = Arc::clone(&completed);
            let initial_prompt = base_prompt.clone();
            let http_client = http_client.clone();
            async move {
                let response = request_chunk_with_retry(
                    &http_client,
                    account,
                    api_token,
                    &chunk,
                    initial_prompt,
                )
                .await?;
                if let Err(error) = tokio::fs::remove_file(&chunk.path).await {
                    eprintln!("Could not remove completed Cloudflare chunk: {error}");
                }
                let mut progress = completed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *progress = progress.saturating_add(1);
                publish_transcription_progress(
                    &app,
                    TranscriptionProgress::scaled(
                        TranscriptionStage::Transcribing,
                        *progress,
                        total_work,
                        0.05,
                        0.95,
                    ),
                );
                Ok::<_, String>((chunk, normalize(response)))
            }
        })
        .buffer_unordered(MAX_PARALLEL_CHUNKS);
    futures_util::pin_mut!(requests);
    let mut chunk_specs = vec![None; windows.len()];
    let mut chunk_transcripts = vec![None; windows.len()];
    while let Some(result) = requests.next().await {
        let (chunk, transcript) = result?;
        let index = chunk.index;
        chunk_specs[index] = Some(chunk);
        chunk_transcripts[index] = Some(transcript);
    }
    producer
        .await
        .map_err(|error| format!("音声の分割処理が停止しました: {error}"))??;
    let chunks = chunk_specs
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            chunk.ok_or_else(|| format!("音声チャンク{}を作成できませんでした。", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let transcripts = chunk_transcripts
        .into_iter()
        .enumerate()
        .map(|(index, transcript)| {
            transcript.ok_or_else(|| format!("音声チャンク{}の結果がありません。", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let transcript = merge_chunks(&chunks, transcripts);
    if transcript.tokens.is_empty() && transcript.segments.is_empty() {
        return Err("Cloudflare Workers AIから文字起こし結果を受け取れませんでした。".into());
    }
    Ok(TranscriptionOutcome {
        transcript,
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
        chunk_windows, consume_text_generation_event, format_cost_usd, is_authentication_error,
        merge_chunks, normalize, retryable_text_generation_status, ChunkSpec,
        TextGenerationMessage, TextGenerationRequest, TextGenerationUsage, WorkersAiTextGeneration,
        WorkersAiTranscript, CHUNK_CORE_MS, CHUNK_OVERLAP_MS, MODEL_ID, TEXT_GENERATION_MAX_TOKENS,
    };
    use crate::transcription::{TokenTimeSource, Transcript, TranscriptToken};
    use std::path::PathBuf;

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

    #[test]
    fn text_generation_reserves_enough_output_for_structured_meeting_json() {
        let request = TextGenerationRequest {
            messages: [TextGenerationMessage {
                role: "user",
                content: "prompt",
            }],
            max_tokens: TEXT_GENERATION_MAX_TOKENS,
            temperature: 0.1,
            stream: true,
        };
        let value = serde_json::to_value(request).expect("request");
        assert_eq!(value["max_tokens"], 32_768);
        assert_eq!(value["stream"], true);
    }

    #[test]
    fn streaming_events_preserve_native_and_openai_text_deltas() {
        let mut output = String::new();
        let mut usage = TextGenerationUsage::default();
        let mut native = vec![r#"{"response":"前半"}"#.to_string()];
        let mut compatible = vec![r#"{"choices":[{"delta":{"content":"後半"}}]}"#.to_string()];
        let mut usage_event = vec![
            r#"{"usage":{"prompt_tokens":120,"completion_tokens":34,"total_tokens":154}}"#
                .to_string(),
        ];
        let mut done = vec!["[DONE]".to_string()];

        assert!(
            !consume_text_generation_event(&mut native, &mut output, &mut usage).expect("native")
        );
        assert!(
            !consume_text_generation_event(&mut compatible, &mut output, &mut usage)
                .expect("compatible")
        );
        assert!(
            !consume_text_generation_event(&mut usage_event, &mut output, &mut usage)
                .expect("usage")
        );
        assert!(consume_text_generation_event(&mut done, &mut output, &mut usage).expect("done"));
        assert_eq!(output, "前半後半");
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(34));
    }

    #[test]
    fn text_generation_retries_only_transient_http_failures() {
        for status in [
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::BAD_GATEWAY,
        ] {
            assert!(retryable_text_generation_status(status));
        }
        for status in [
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            assert!(!retryable_text_generation_status(status));
        }
    }

    #[test]
    fn oauth_unauthorized_error_is_classified_for_token_recovery() {
        assert!(is_authentication_error(
            "Cloudflareの認証情報が無効または期限切れです。"
        ));
        assert!(is_authentication_error(
            "会議ノート生成を1回試行しましたが完了できませんでした: Cloudflareの認証情報が無効または期限切れです。"
        ));
        assert!(!is_authentication_error(
            "Cloudflareの認証情報にWorkers AIの読み取り・実行権限がありません。"
        ));
        assert!(!is_authentication_error(
            "Cloudflareの認証情報が無効または期限切れです。追加情報"
        ));
    }

    #[test]
    fn long_audio_is_split_into_overlapping_five_minute_windows() {
        let duration = 2 * 60 * 60 * 1_000;
        let windows = chunk_windows(duration).expect("valid duration");
        assert_eq!(windows.len(), 24);
        assert_eq!(windows[0], (0, CHUNK_CORE_MS + CHUNK_OVERLAP_MS));
        assert_eq!(windows[1].0, CHUNK_CORE_MS);
        assert_eq!(windows.last().expect("last").1, CHUNK_CORE_MS);
        assert!(chunk_windows(0).is_err());
    }

    #[test]
    fn merging_offsets_timestamps_and_discards_overlap_duplicates() {
        let chunks = vec![
            ChunkSpec {
                index: 0,
                start_ms: 0,
                duration_ms: CHUNK_CORE_MS + CHUNK_OVERLAP_MS,
                path: PathBuf::new(),
            },
            ChunkSpec {
                index: 1,
                start_ms: CHUNK_CORE_MS,
                duration_ms: CHUNK_CORE_MS,
                path: PathBuf::new(),
            },
        ];
        let transcript = |tokens: Vec<TranscriptToken>| Transcript {
            provider: "cloudflare".into(),
            model: MODEL_ID.into(),
            language: "ja".into(),
            tokens,
            segments: Vec::new(),
        };
        let token = |text: &str, start_ms, end_ms| TranscriptToken {
            text: text.into(),
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
            start_time_source: Some(TokenTimeSource::Provider),
            end_time_source: Some(TokenTimeSource::Provider),
            speaker: None,
            speaker_source: None,
            confidence: None,
            utterance_id: None,
        };
        let merged = merge_chunks(
            &chunks,
            vec![
                transcript(vec![
                    token("前", 299_000, 300_200),
                    token("重複", 300_200, 301_000),
                ]),
                transcript(vec![token("重複", 200, 1_000), token("後", 1_100, 2_000)]),
            ],
        );
        assert_eq!(
            merged
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            vec!["前", "重複", "後"]
        );
        assert_eq!(merged.tokens[2].start_ms, Some(301_100));
    }
}
