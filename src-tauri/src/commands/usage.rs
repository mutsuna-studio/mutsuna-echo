use std::time::Duration;

use chrono::{DateTime, Months, Utc};
use reqwest::{redirect::Policy, Client, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

const SUBSCRIPTION_URL: &str = "https://api.elevenlabs.io/v1/user/subscription";
const USAGE_URL: &str =
    "https://api.elevenlabs.io/v1/workspace/analytics/query/usage-by-product-over-time";

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

fn included_scribe_minutes(tier: &str) -> Option<f64> {
    match tier.to_ascii_lowercase().as_str() {
        "free" | "free_v2" | "pay_as_you_go" => Some(4.5 * 60.0),
        "starter" => Some(27.0 * 60.0),
        "creator" => Some(100.0 * 60.0),
        "pro" => Some(450.0 * 60.0),
        "scale" => Some(1_359.0 * 60.0),
        "business" => Some(4_500.0 * 60.0),
        _ => None,
    }
}

fn available_duration_ms(subscription: &SubscriptionResponse) -> Option<u64> {
    if subscription.character_limit == 0 {
        return None;
    }

    let included_minutes = included_scribe_minutes(&subscription.tier)?;
    let remaining_credits = subscription
        .character_limit
        .saturating_sub(subscription.character_count);
    let remaining_ratio = remaining_credits as f64 / subscription.character_limit as f64;
    Some((included_minutes * remaining_ratio * 60_000.0).round() as u64)
}

fn period_start_ms(subscription: &SubscriptionResponse) -> Option<i64> {
    let reset = DateTime::<Utc>::from_timestamp(subscription.next_character_count_reset_unix?, 0)?;
    let months = match subscription.character_refresh_period.as_deref() {
        Some("3_month_period") => 3,
        Some("6_month_period") => 6,
        Some("annual_period") => 12,
        _ => 1,
    };
    reset
        .checked_sub_months(Months::new(months))
        .map(|date| date.timestamp_millis())
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
    let product_index = usage
        .columns
        .iter()
        .position(|column| column == "product_type")
        .ok_or_else(|| "ElevenLabsの使用量に製品情報が含まれていません。".to_string())?;
    let credits_index = usage
        .columns
        .iter()
        .position(|column| column == "credits_used")
        .ok_or_else(|| "ElevenLabsの使用量にクレジット情報が含まれていません。".to_string())?;

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

fn credits_to_duration_ms(credits: f64, subscription: &SubscriptionResponse) -> Option<u64> {
    let included_minutes = included_scribe_minutes(&subscription.tier)?;
    if subscription.character_limit == 0 {
        return None;
    }
    let minutes = credits / subscription.character_limit as f64 * included_minutes;
    Some((minutes * 60_000.0).round() as u64)
}

async fn fetch_used_duration(
    client: &Client,
    api_key: &SecretString,
    start_ms: i64,
    subscription: &SubscriptionResponse,
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
            let duration_ms = credits_to_duration_ms(credits, subscription)
                .ok_or_else(|| "契約プランの文字起こし時間を換算できませんでした。".to_string())?;
            Ok(ApiResponse::Data(duration_ms))
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
    let mut warning = if available_duration_ms.is_none() {
        Some(format!(
            "{}プランの文字起こし可能時間を換算できません。ElevenLabsの契約画面で残り利用枠を確認してください。",
            subscription.tier
        ))
    } else {
        None
    };

    let used_duration_ms = if available_duration_ms.is_none() {
        None
    } else if let Some(start_ms) = period_start_ms(&subscription) {
        match fetch_used_duration(&client, &api_key, start_ms, &subscription).await? {
            ApiResponse::Data(duration_ms) => Some(duration_ms),
            ApiResponse::MissingPermission => {
                warning = Some(
                    "使用済み時間を参照する権限がありません。ElevenLabsのAPIキー設定でWorkspace Analytics Full Read権限を追加してください。Speech to Text以外の生成権限は不要です。"
                        .to_string(),
                );
                None
            }
        }
    } else {
        warning =
            Some("契約期間の開始日を取得できないため、使用済み時間を表示できません。".to_string());
        None
    };

    Ok(TranscriptionUsage {
        available_duration_ms,
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
            character_count: 25_000,
            character_limit: 100_000,
            next_character_count_reset_unix: None,
            character_refresh_period: None,
        };

        assert_eq!(
            available_duration_ms(&subscription),
            Some(75 * 60 * 60 * 1_000)
        );
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
        let subscription = SubscriptionResponse {
            tier: "creator".to_string(),
            character_count: 0,
            character_limit: 100_000,
            next_character_count_reset_unix: None,
            character_refresh_period: None,
        };

        assert!(is_speech_to_text_product("Speech_to_Text"));
        assert_eq!(used_credits(&usage).unwrap(), 1_500.0);
        assert_eq!(
            credits_to_duration_ms(1_500.0, &subscription),
            Some(90 * 60_000)
        );
    }
}
