use std::time::Duration;

use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::{redirect::Policy, Client, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

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

enum ApiResponse<T> {
    Data(T),
    MissingPermission,
}

fn usage_client() -> Result<Client, String> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| {
            eprintln!("Could not build ElevenLabs usage client: {error:?}");
            "ElevenLabsの利用状況を確認する通信を準備できませんでした。".to_string()
        })
}

fn error_status(body: &Value) -> Option<&str> {
    body.pointer("/detail/status").and_then(Value::as_str)
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
    match error_status(&body) {
        Some("missing_permissions") => Ok(ApiResponse::MissingPermission),
        Some("invalid_api_key") => {
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
    client: &Client,
    api_key: &SecretString,
) -> Result<ApiResponse<SubscriptionResponse>, String> {
    let response = client
        .get(SUBSCRIPTION_URL)
        .header("xi-api-key", api_key.expose_secret())
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

    subscription_start
        .or_else(|| Utc::now().with_day(1))
        .map(|date| date.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc())
        .unwrap_or_else(Utc::now)
        .timestamp_millis()
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
        .filter(|value| value.is_finite() && *value > 0.0)
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
        .position(|column| {
            matches!(
                column.as_str(),
                "credits_used" | "credit_usage" | "credits" | "usage"
            )
        })
        .ok_or_else(|| {
            format!(
                "ElevenLabsの使用量にクレジット情報が含まれていません（列: {}）。",
                usage.columns.join(", ")
            )
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
        .sum())
}

async fn fetch_used_duration(
    client: &Client,
    api_key: &SecretString,
    start_ms: i64,
) -> Result<ApiResponse<u64>, String> {
    let end_ms = Utc::now().timestamp_millis();
    let response = client
        .post(USAGE_URL)
        .header("xi-api-key", api_key.expose_secret())
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
    let client = usage_client()?;

    let subscription = match fetch_subscription(&client, &api_key).await? {
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

    let used_duration_ms = match fetch_used_duration(
        &client,
        &api_key,
        period_start_ms(&subscription),
    )
    .await
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

#[cfg(test)]
mod tests {
    use super::{
        available_duration_ms, credits_to_duration_ms, is_speech_to_text_product, used_credits,
        SubscriptionResponse, UsageResponse,
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
}
