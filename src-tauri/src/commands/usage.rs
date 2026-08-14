use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    sync::Mutex,
    time::Duration,
};

use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::transcription::elevenlabs::client::{api_error_kind, ApiErrorKind, ElevenLabsClient};

const SUBSCRIPTION_URL: &str = "https://api.elevenlabs.io/v1/user/subscription";
const USAGE_URL: &str =
    "https://api.elevenlabs.io/v1/workspace/analytics/query/usage-by-product-over-time";
// ElevenLabs prices Scribe v2 at $0.22/hour. API credits represent
// $0.0001 each (10,000 credits per USD), so one hour consumes 2,200 credits.
const SCRIBE_V2_CREDITS_PER_HOUR: f64 = 2_200.0;
const MAX_CLOUDFLARE_TEXT_USAGE_BYTES: u64 = 8 * 1024 * 1024;
static CLOUDFLARE_TEXT_USAGE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
struct SubscriptionResponse {
    tier: String,
    character_count: u64,
    character_limit: u64,
    next_character_count_reset_unix: Option<i64>,
    character_refresh_period: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    #[serde(default)]
    columns: Vec<String>,
    #[serde(default)]
    rows: Vec<Vec<Value>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionUsage {
    available_duration_ms: Option<u64>,
    used_duration_ms: Option<u64>,
    tier: Option<String>,
    resets_at_unix: Option<i64>,
    warning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SonioxUsage {
    monthly_cost_usd: String,
    period_start: String,
    fetched_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudflareUsage {
    estimated_cost_usd: String,
    used_duration_ms: u64,
    estimated_neurons: f64,
    transcription_count: u64,
    text_generation_count: u64,
    period_start: String,
    daily_used_duration_ms: u64,
    daily_estimated_neurons: f64,
    daily_transcription_count: u64,
    daily_text_generation_count: u64,
    daily_free_allocation_neurons: f64,
    daily_remaining_neurons: f64,
    daily_usage_percent: f64,
    daily_period_start: String,
    daily_resets_at: String,
    fetched_at: String,
}

struct CloudflareUsageEstimate {
    cost_usd: f64,
    duration_ms: u64,
    neurons: f64,
    run_count: u64,
    text_generation_count: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudflareTextUsageRecord {
    occurred_at: String,
    operation: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    estimated: bool,
    cost_usd: f64,
    neurons: f64,
}

#[derive(Debug, Default)]
struct CloudflareTextUsageEstimate {
    cost_usd: f64,
    neurons: f64,
    run_count: u64,
}

enum ApiResponse<T> {
    Data(T),
    MissingPermission,
}

async fn parse_response<T: for<'de> Deserialize<'de>>(
    response: Response,
    operation: &str,
) -> Result<ApiResponse<T>, String> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map(ApiResponse::Data)
            .map_err(|error| {
                eprintln!("Could not parse ElevenLabs {operation} response: {error:?}");
                format!("ElevenLabsの{operation}を読み取れませんでした。")
            });
    }

    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    match api_error_kind(&body) {
        ApiErrorKind::MissingPermissions => Ok(ApiResponse::MissingPermission),
        ApiErrorKind::InvalidApiKey => {
            Err("保存済みのElevenLabs APIキーが無効です。設定し直してください。".to_string())
        }
        _ if status == StatusCode::UNAUTHORIZED => {
            Err("ElevenLabsでAPIキーを認証できませんでした。".to_string())
        }
        _ if status == StatusCode::FORBIDDEN => Ok(ApiResponse::MissingPermission),
        _ => Err(format!(
            "ElevenLabsの{operation}を取得できませんでした（HTTP {status}）。"
        )),
    }
}

async fn fetch_subscription(
    client: &ElevenLabsClient,
) -> Result<ApiResponse<SubscriptionResponse>, String> {
    let response = client
        .get(SUBSCRIPTION_URL)
        .send()
        .await
        .map_err(|error| format!("ElevenLabsの契約情報を取得できませんでした: {error}"))?;

    parse_response(response, "契約情報").await
}

fn credits_to_duration_ms(credits: f64) -> u64 {
    (credits / SCRIBE_V2_CREDITS_PER_HOUR * 3_600_000.0).round() as u64
}

fn available_duration_ms(subscription: &SubscriptionResponse) -> u64 {
    let remaining_credits = subscription
        .character_limit
        .saturating_sub(subscription.character_count);
    credits_to_duration_ms(remaining_credits as f64)
}

fn period_start_ms(subscription: &SubscriptionResponse) -> i64 {
    let subscription_start = subscription
        .next_character_count_reset_unix
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .and_then(|reset| {
            let months = match subscription.character_refresh_period.as_deref() {
                Some("3_month_period") => 3,
                Some("6_month_period") => 6,
                Some("annual_period") => 12,
                _ => 1,
            };
            reset.checked_sub_months(Months::new(months))
        });

    if let Some(start) = subscription_start {
        return start.timestamp_millis();
    }

    let now = Utc::now();
    now.date_naive()
        .with_day(1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc().timestamp_millis())
        .unwrap_or_else(|| now.timestamp_millis())
}

fn is_speech_to_text_product(name: &str) -> bool {
    let normalized = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    normalized.contains("speechtotext") || normalized.contains("scribe")
}

fn numeric_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value| value.is_finite())
}

fn used_duration_ms(usage: &UsageResponse) -> Result<u64, String> {
    // The analytics API can omit metric columns entirely when the selected
    // period has no usage. That is a valid zero-usage response.
    if usage.rows.is_empty() {
        return Ok(0);
    }

    let product_index = usage
        .columns
        .iter()
        .position(|column| column == "product_type")
        .ok_or_else(|| "ElevenLabsの使用量に製品情報が含まれていません。".to_string())?;
    let minutes_index = usage
        .columns
        .iter()
        .position(|column| column == "total_minutes");
    let credits_index = usage
        .columns
        .iter()
        .position(|column| column == "credits_used");
    if minutes_index.is_none() && credits_index.is_none() {
        eprintln!(
            "ElevenLabs usage response did not contain total_minutes or credits_used; columns: {:?}",
            usage.columns
        );
        return Err("ElevenLabsの使用量レスポンス形式を確認できませんでした。".to_string());
    }

    let total = usage
        .rows
        .iter()
        .filter(|row| {
            row.get(product_index)
                .and_then(Value::as_str)
                .is_some_and(is_speech_to_text_product)
        })
        .filter_map(|row| {
            let metric_index = minutes_index.or(credits_index)?;
            row.get(metric_index).and_then(numeric_value)
        })
        .sum::<f64>()
        .max(0.0);

    Ok(if minutes_index.is_some() {
        (total * 60_000.0).round() as u64
    } else {
        credits_to_duration_ms(total)
    })
}

async fn fetch_used_duration(
    client: &ElevenLabsClient,
    start_ms: i64,
) -> Result<ApiResponse<u64>, String> {
    let end_ms = Utc::now().timestamp_millis();
    let response = client
        .post(USAGE_URL)
        .json(&serde_json::json!({
            "start_time": start_ms,
            "end_time": end_ms,
            "interval_seconds": 86_400,
            "group_by": ["product_type"],
            "time_zone": "Asia/Tokyo"
        }))
        .send()
        .await
        .map_err(|error| format!("ElevenLabsの使用量を取得できませんでした: {error}"))?;

    match parse_response::<UsageResponse>(response, "使用量").await? {
        ApiResponse::Data(usage) => Ok(ApiResponse::Data(used_duration_ms(&usage)?)),
        ApiResponse::MissingPermission => Ok(ApiResponse::MissingPermission),
    }
}

#[tauri::command]
pub(crate) async fn get_transcription_usage(app: AppHandle) -> Result<TranscriptionUsage, String> {
    let api_key = crate::commands::api_key::load_api_key(&app)?;
    let client = ElevenLabsClient::new(&api_key, Duration::from_secs(20))?;

    let subscription = match fetch_subscription(&client).await? {
        ApiResponse::Data(subscription) => subscription,
        ApiResponse::MissingPermission => {
            return Ok(TranscriptionUsage {
                available_duration_ms: None,
                used_duration_ms: None,
                tier: None,
                resets_at_unix: None,
                warning: Some(
                    "APIキーに利用状況を参照する権限がありません。ElevenLabsのAPIキー設定でUserの読み取り権限を追加してください。Speech to Text以外の生成権限は不要です。"
                        .to_string(),
                ),
            });
        }
    };

    let available_duration_ms = available_duration_ms(&subscription);
    let resets_at_unix = subscription.next_character_count_reset_unix;
    let mut warning = if subscription.character_limit == 0 {
        Some(format!(
            "ElevenLabsの契約APIは{}プランの月次契約枠を0クレジットとして返しています。APIキーの「使用制限: 無制限」はキー単位の上限で、アカウントの契約枠やPay As You Go残高とは別です。",
            subscription.tier
        ))
    } else {
        None
    };

    let used_duration_ms = match fetch_used_duration(&client, period_start_ms(&subscription)).await
    {
        Ok(ApiResponse::Data(duration_ms)) => Some(duration_ms),
        Ok(ApiResponse::MissingPermission) => {
            warning = Some(
                "使用済み時間を参照する権限がありません。ElevenLabsのAPIキー設定でWorkspace Analytics Full Read権限を追加してください。Speech to Text以外の生成権限は不要です。"
                    .to_string(),
            );
            None
        }
        Err(error) => {
            warning = Some(error);
            None
        }
    };

    Ok(TranscriptionUsage {
        available_duration_ms: Some(available_duration_ms),
        used_duration_ms,
        tier: Some(subscription.tier),
        resets_at_unix,
        warning,
    })
}

#[tauri::command]
pub(crate) async fn get_soniox_usage(app: AppHandle) -> Result<SonioxUsage, String> {
    let api_key = crate::credentials::load(&app, crate::credentials::CredentialId::Soniox)?;
    let now = Utc::now();
    let period_start = now
        .date_naive()
        .with_day(1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc())
        .ok_or_else(|| "Sonioxの利用期間を計算できませんでした。".to_string())?;
    let monthly_cost_usd = crate::transcription::soniox::current_month_cost_usd(&api_key).await?;
    Ok(SonioxUsage {
        monthly_cost_usd,
        period_start: period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        fetched_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

#[tauri::command]
pub(crate) fn get_cloudflare_usage(app: AppHandle) -> Result<CloudflareUsage, String> {
    let now = Utc::now();
    let period_start = now
        .date_naive()
        .with_day(1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc())
        .ok_or_else(|| "Cloudflareの利用期間を計算できませんでした。".to_string())?;
    let daily_period_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|date| date.and_utc())
        .ok_or_else(|| "Cloudflareの日次利用期間を計算できませんでした。".to_string())?;
    let monthly = estimate_cloudflare_usage_since(&app, period_start)?;
    let daily = estimate_cloudflare_usage_since(&app, daily_period_start)?;
    let free_allocation = crate::transcription::cloudflare::FREE_DAILY_NEURONS;
    let daily_remaining_neurons = (free_allocation - daily.neurons).max(0.0);
    let daily_usage_percent = if free_allocation > 0.0 {
        daily.neurons / free_allocation * 100.0
    } else {
        0.0
    };
    let daily_resets_at = daily_period_start + chrono::Duration::days(1);
    let formatted = format!("{:.10}", monthly.cost_usd);
    Ok(CloudflareUsage {
        estimated_cost_usd: formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned(),
        used_duration_ms: monthly.duration_ms,
        estimated_neurons: monthly.neurons,
        transcription_count: monthly.run_count,
        text_generation_count: monthly.text_generation_count,
        period_start: period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        daily_used_duration_ms: daily.duration_ms,
        daily_estimated_neurons: daily.neurons,
        daily_transcription_count: daily.run_count,
        daily_text_generation_count: daily.text_generation_count,
        daily_free_allocation_neurons: free_allocation,
        daily_remaining_neurons,
        daily_usage_percent,
        daily_period_start: daily_period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        daily_resets_at: daily_resets_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        fetched_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

fn estimate_cloudflare_usage_since(
    app: &AppHandle,
    since: DateTime<Utc>,
) -> Result<CloudflareUsageEstimate, String> {
    let usage = crate::transcript_store::provider_cost_usage_since(
        app,
        crate::transcription::TranscriptionProvider::Cloudflare.id(),
        since,
    )?;
    let mut used_minutes =
        usage.cost_usd / crate::transcription::cloudflare::PRICE_USD_PER_AUDIO_MINUTE;
    let mut recovered_durations = std::collections::HashMap::<String, u64>::new();
    for run in &usage.unpriced_runs {
        let duration_ms = if let Some(duration_ms) = recovered_durations.get(&run.meeting_id) {
            *duration_ms
        } else {
            let duration_ms = cloudflare_billed_duration_ms(app, &run.meeting_id)
                .unwrap_or(run.fallback_duration_ms);
            recovered_durations.insert(run.meeting_id.clone(), duration_ms);
            duration_ms
        };
        used_minutes += duration_ms as f64 / 60_000.0;
    }
    let estimated_cost_usd =
        used_minutes * crate::transcription::cloudflare::PRICE_USD_PER_AUDIO_MINUTE;
    let used_duration_ms = (used_minutes * 60_000.0).round().max(0.0) as u64;
    let estimated_neurons =
        used_minutes * crate::transcription::cloudflare::NEURONS_PER_AUDIO_MINUTE;
    let text = cloudflare_text_usage_since(app, since)?;
    Ok(CloudflareUsageEstimate {
        cost_usd: estimated_cost_usd + text.cost_usd,
        duration_ms: used_duration_ms,
        neurons: estimated_neurons + text.neurons,
        run_count: usage.run_count,
        text_generation_count: text.run_count,
    })
}

pub(crate) fn record_cloudflare_text_usage(
    app: &AppHandle,
    operation: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    estimated: bool,
) -> Result<(), String> {
    let record = cloudflare_text_usage_record(
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        operation,
        model,
        input_tokens,
        output_tokens,
        estimated,
    )?;
    let bytes = serde_json::to_vec(&record)
        .map_err(|error| format!("Cloudflareのテキスト利用状況を変換できませんでした: {error}"))?;
    let _guard = CLOUDFLARE_TEXT_USAGE_LOCK
        .lock()
        .map_err(|_| "Cloudflareのテキスト利用状況を保存できませんでした。".to_string())?;
    migrate_historical_cloudflare_text_usage(app)?;
    let path = cloudflare_text_usage_path(app)?;
    if path.metadata().is_ok_and(|metadata| {
        metadata.len().saturating_add(bytes.len() as u64 + 1) > MAX_CLOUDFLARE_TEXT_USAGE_BYTES
    }) {
        return Err("Cloudflareのテキスト利用履歴が上限に達しました。".into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("Cloudflareの利用履歴保存先を作成できませんでした: {error}")
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Cloudflareのテキスト利用履歴を開けませんでした: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_data())
        .map_err(|error| format!("Cloudflareのテキスト利用履歴を保存できませんでした: {error}"))
}

fn cloudflare_text_usage_since(
    app: &AppHandle,
    since: DateTime<Utc>,
) -> Result<CloudflareTextUsageEstimate, String> {
    let _guard = CLOUDFLARE_TEXT_USAGE_LOCK
        .lock()
        .map_err(|_| "Cloudflareのテキスト利用状況を集計できませんでした。".to_string())?;
    migrate_historical_cloudflare_text_usage(app)?;
    let path = cloudflare_text_usage_path(app)?;
    if !path.exists() {
        return Ok(CloudflareTextUsageEstimate::default());
    }
    let metadata = path
        .metadata()
        .map_err(|error| format!("Cloudflareのテキスト利用履歴を確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_CLOUDFLARE_TEXT_USAGE_BYTES {
        return Err("Cloudflareのテキスト利用履歴が不正です。".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("Cloudflareのテキスト利用履歴を開けませんでした: {error}"))?;
    let mut estimate = CloudflareTextUsageEstimate::default();
    for line in BufReader::new(file).lines() {
        let line = line
            .map_err(|error| format!("Cloudflareのテキスト利用履歴を読めませんでした: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: CloudflareTextUsageRecord = serde_json::from_str(&line)
            .map_err(|_| "Cloudflareのテキスト利用履歴が壊れています。".to_string())?;
        let occurred_at = DateTime::parse_from_rfc3339(&record.occurred_at)
            .map_err(|_| "Cloudflareのテキスト利用日時を読み取れませんでした。".to_string())?
            .with_timezone(&Utc);
        if occurred_at < since {
            continue;
        }
        estimate.cost_usd += record.cost_usd;
        estimate.neurons += record.neurons;
        estimate.run_count = estimate.run_count.saturating_add(1);
    }
    Ok(estimate)
}

fn cloudflare_text_usage_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("usage").join("cloudflare-text.jsonl"))
        .map_err(|error| format!("Cloudflareの利用履歴保存先を確認できませんでした: {error}"))
}

fn cloudflare_text_usage_record(
    occurred_at: String,
    operation: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    estimated: bool,
) -> Result<CloudflareTextUsageRecord, String> {
    let (
        input_cost_per_million,
        output_cost_per_million,
        input_neurons_per_million,
        output_neurons_per_million,
    ) = cloudflare_text_model_pricing(model)
        .ok_or_else(|| "Cloudflareテキストモデルの利用単価を確認できませんでした。".to_string())?;
    Ok(CloudflareTextUsageRecord {
        occurred_at,
        operation: operation.to_owned(),
        model: model.to_owned(),
        input_tokens,
        output_tokens,
        estimated,
        cost_usd: input_tokens as f64 / 1_000_000.0 * input_cost_per_million
            + output_tokens as f64 / 1_000_000.0 * output_cost_per_million,
        neurons: input_tokens as f64 / 1_000_000.0 * input_neurons_per_million
            + output_tokens as f64 / 1_000_000.0 * output_neurons_per_million,
    })
}

fn migrate_historical_cloudflare_text_usage(app: &AppHandle) -> Result<(), String> {
    let path = cloudflare_text_usage_path(app)?;
    let marker = path.with_extension("v1-migrated");
    if marker.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("Cloudflareの利用履歴保存先を作成できませんでした: {error}")
        })?;
    }
    let historical = crate::meeting_schema::historical_cloudflare_text_usage(app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Cloudflareのテキスト利用履歴を開けませんでした: {error}"))?;
    for historical in historical {
        let record = cloudflare_text_usage_record(
            historical.occurred_at,
            &historical.operation,
            &historical.model,
            historical.input_tokens,
            historical.output_tokens,
            true,
        )?;
        let bytes = serde_json::to_vec(&record).map_err(|error| {
            format!("Cloudflareの過去のテキスト利用状況を変換できませんでした: {error}")
        })?;
        file.write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|error| {
                format!("Cloudflareの過去のテキスト利用履歴を保存できませんでした: {error}")
            })?;
    }
    file.sync_data().map_err(|error| {
        format!("Cloudflareの過去のテキスト利用履歴を保存できませんでした: {error}")
    })?;
    fs::write(marker, b"1\n")
        .map_err(|error| format!("Cloudflareの利用履歴移行を確定できませんでした: {error}"))
}

fn cloudflare_text_model_pricing(model: &str) -> Option<(f64, f64, f64, f64)> {
    match model {
        "@cf/zai-org/glm-4.7-flash" => Some((0.060, 0.400, 5_500.0, 36_400.0)),
        "@cf/ibm-granite/granite-4.0-h-micro" => Some((0.017, 0.112, 1_542.0, 10_158.0)),
        "@cf/google/gemma-4-26b-a4b-it" => Some((0.100, 0.300, 9_091.0, 27_273.0)),
        _ => None,
    }
}

fn cloudflare_billed_duration_ms(app: &AppHandle, meeting_id: &str) -> Result<u64, String> {
    let tracks = crate::meeting_store::recording_tracks(app, meeting_id)?;
    let sources = [tracks.microphone, tracks.system]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if !sources.is_empty() {
        return sources.into_iter().try_fold(0_u64, |total, path| {
            crate::commands::transcribe::audio_duration_ms(&path)
                .map(|duration| total.saturating_add(duration))
        });
    }
    let path = crate::meeting_store::local_audio_path(app, meeting_id)?;
    crate::commands::transcribe::audio_duration_ms(&path)
}

#[cfg(test)]
mod tests {
    use super::{
        available_duration_ms, cloudflare_text_model_pricing, credits_to_duration_ms,
        is_speech_to_text_product, period_start_ms, used_duration_ms, SubscriptionResponse,
        UsageResponse,
    };
    use serde_json::json;

    #[test]
    fn converts_remaining_creator_credits_to_scribe_time() {
        let subscription = SubscriptionResponse {
            tier: "creator".to_string(),
            character_count: 55_000,
            character_limit: 220_000,
            next_character_count_reset_unix: None,
            character_refresh_period: None,
        };

        assert_eq!(available_duration_ms(&subscription), 75 * 60 * 60 * 1_000);
    }

    #[test]
    fn uses_model_specific_cloudflare_text_pricing() {
        assert_eq!(
            cloudflare_text_model_pricing("@cf/zai-org/glm-4.7-flash"),
            Some((0.060, 0.400, 5_500.0, 36_400.0))
        );
        assert!(cloudflare_text_model_pricing("@cf/unknown/model").is_none());
    }

    #[test]
    fn converts_credits_without_depending_on_plan_tier() {
        assert_eq!(credits_to_duration_ms(2_200.0), 60 * 60 * 1_000);
    }

    #[test]
    fn preserves_the_billing_cycle_reset_time() {
        let subscription = SubscriptionResponse {
            tier: "creator".to_string(),
            character_count: 0,
            character_limit: 10_000,
            next_character_count_reset_unix: Some(1_787_054_400),
            character_refresh_period: Some("monthly_period".to_string()),
        };

        assert_eq!(period_start_ms(&subscription), 1_784_376_000_000);
    }

    #[test]
    fn sums_only_speech_to_text_credits_and_converts_them() {
        let usage = UsageResponse {
            columns: vec![
                "timestamp".into(),
                "product_type".into(),
                "credits_used".into(),
            ],
            rows: vec![
                vec![json!("2026-08-01"), json!("speech-to-text"), json!(1_000)],
                vec![json!("2026-08-02"), json!("speech_to_text"), json!("500")],
                vec![json!("2026-08-02"), json!("text-to-speech"), json!(99_000)],
            ],
        };
        assert!(is_speech_to_text_product("Speech_to_Text"));
        assert_eq!(used_duration_ms(&usage).unwrap(), 2_454_545);
    }

    #[test]
    fn reads_total_minutes_from_live_analytics_response() {
        let usage = UsageResponse {
            columns: vec![
                "product_type".into(),
                "timestamp".into(),
                "total_usage".into(),
                "total_minutes".into(),
                "total_cost".into(),
                "usage_count".into(),
                "total_charge_count".into(),
            ],
            rows: vec![
                vec![
                    json!("speech-to-text"),
                    json!("2026-08-01"),
                    json!(1),
                    json!("1.25"),
                    json!(0),
                    json!(1),
                    json!(1),
                ],
                vec![
                    json!("scribe"),
                    json!("2026-08-02"),
                    json!(1),
                    json!(0.5),
                    json!(0),
                    json!(1),
                    json!(1),
                ],
                vec![
                    json!("text-to-speech"),
                    json!("2026-08-02"),
                    json!(1),
                    json!(99),
                    json!(0),
                    json!(1),
                    json!(1),
                ],
            ],
        };

        assert_eq!(used_duration_ms(&usage).unwrap(), 105_000);
    }

    #[test]
    fn treats_an_empty_analytics_response_as_zero_usage() {
        let usage = UsageResponse {
            columns: vec!["timestamp".into(), "product_type".into()],
            rows: vec![],
        };

        assert_eq!(used_duration_ms(&usage).unwrap(), 0);
    }

    #[test]
    fn rejects_non_empty_analytics_without_documented_credit_column() {
        let usage = UsageResponse {
            columns: vec!["timestamp".into(), "product_type".into(), "usage".into()],
            rows: vec![vec![json!("2026-08-01"), json!("speech-to-text"), json!(1)]],
        };

        assert_eq!(
            used_duration_ms(&usage).unwrap_err(),
            "ElevenLabsの使用量レスポンス形式を確認できませんでした。"
        );
    }
}
