use std::time::Duration;

use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::transcription::elevenlabs::client::{api_error_kind, ApiErrorKind, ElevenLabsClient};

const SUBSCRIPTION_URL: &str = "https://api.elevenlabs.io/v1/user/subscription";
const USAGE_URL: &str =
    "https://api.elevenlabs.io/v1/workspace/analytics/query/usage-by-product-over-time";
// ElevenLabs prices Scribe v2 at $0.22/hour. API credits represent
// $0.0001 each (10,000 credits per USD), so one hour consumes 2,200 credits.
const SCRIBE_V2_CREDITS_PER_HOUR: f64 = 2_200.0;

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
    period_start: String,
    daily_used_duration_ms: u64,
    daily_estimated_neurons: f64,
    daily_transcription_count: u64,
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

fn used_credits(usage: &UsageResponse) -> Result<f64, String> {
    // The analytics API can omit metric columns entirely when the selected
    // period has no usage. That is a valid zero-usage response.
    if usage.rows.is_empty() {
        return Ok(0.0);
    }

    let product_index = usage
        .columns
        .iter()
        .position(|column| column == "product_type")
        .ok_or_else(|| "ElevenLabsの使用量に製品情報が含まれていません。".to_string())?;
    let credits_index = usage
        .columns
        .iter()
        .position(|column| column == "credits_used")
        .ok_or_else(|| {
            eprintln!(
                "ElevenLabs usage response did not contain credits_used; columns: {:?}",
                usage.columns
            );
            "ElevenLabsの使用量レスポンス形式を確認できませんでした。".to_string()
        })?;

    Ok(usage
        .rows
        .iter()
        .filter(|row| {
            row.get(product_index)
                .and_then(Value::as_str)
                .is_some_and(is_speech_to_text_product)
        })
        .filter_map(|row| row.get(credits_index).and_then(numeric_value))
        .sum::<f64>()
        .max(0.0))
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
        ApiResponse::Data(usage) => {
            let credits = used_credits(&usage)?;
            Ok(ApiResponse::Data(credits_to_duration_ms(credits)))
        }
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
        period_start: period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        daily_used_duration_ms: daily.duration_ms,
        daily_estimated_neurons: daily.neurons,
        daily_transcription_count: daily.run_count,
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
    Ok(CloudflareUsageEstimate {
        cost_usd: estimated_cost_usd,
        duration_ms: used_duration_ms,
        neurons: estimated_neurons,
        run_count: usage.run_count,
    })
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
        available_duration_ms, credits_to_duration_ms, is_speech_to_text_product, period_start_ms,
        used_credits, SubscriptionResponse, UsageResponse,
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
        assert_eq!(used_credits(&usage).unwrap(), 1_500.0);
        assert_eq!(credits_to_duration_ms(1_500.0), 2_454_545);
    }

    #[test]
    fn treats_an_empty_analytics_response_as_zero_usage() {
        let usage = UsageResponse {
            columns: vec!["timestamp".into(), "product_type".into()],
            rows: vec![],
        };

        assert_eq!(used_credits(&usage).unwrap(), 0.0);
    }

    #[test]
    fn rejects_non_empty_analytics_without_documented_credit_column() {
        let usage = UsageResponse {
            columns: vec!["timestamp".into(), "product_type".into(), "usage".into()],
            rows: vec![vec![json!("2026-08-01"), json!("speech-to-text"), json!(1)]],
        };

        assert_eq!(
            used_credits(&usage).unwrap_err(),
            "ElevenLabsの使用量レスポンス形式を確認できませんでした。"
        );
    }
}
