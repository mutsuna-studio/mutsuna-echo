use reqwest::StatusCode;
use serde_json::Value;
use tauri::AppHandle;

const ELEVENLABS_MODELS_URL: &str = "https://api.elevenlabs.io/v1/models";

fn error_status(body: &Value) -> Option<&str> {
    body.pointer("/detail/status").and_then(Value::as_str)
}

async fn validate_api_key(api_key: &str) -> Result<bool, String> {
    let response = reqwest::Client::new()
        .get(ELEVENLABS_MODELS_URL)
        .header("xi-api-key", api_key)
        .send()
        .await
        .map_err(|error| format!("ElevenLabsに接続できませんでした: {error}"))?;

    let http_status = response.status();

    if http_status.is_success() {
        return Ok(true);
    }

    let body = response.json::<Value>().await.unwrap_or(Value::Null);

    match error_status(&body) {
        Some("invalid_api_key") => Err("ElevenLabs APIキーが無効です。".to_string()),
        // Restricted keys are valid even when they cannot access the models
        // endpoint. Speech-to-Text permission is checked by the transcription
        // request itself.
        Some("missing_permissions") => Ok(false),
        _ => match http_status {
            StatusCode::UNAUTHORIZED => {
                Err("ElevenLabsでAPIキーを認証できませんでした。".to_string())
            }
            StatusCode::FORBIDDEN => {
                Err("このAPIキーには必要なアクセス権がありません。".to_string())
            }
            status => Err(format!(
                "ElevenLabsでAPIキーを確認できませんでした（HTTP {status}）。"
            )),
        },
    }
}

/// Validate and store the API key in the operating system's credential store.
#[tauri::command]
pub(crate) async fn save_api_key(app: AppHandle, api_key: String) -> Result<bool, String> {
    let api_key = api_key.trim();

    if api_key.is_empty() {
        return Err("APIキーを入力してください。".to_string());
    }

    let models_accessible = validate_api_key(api_key).await?;

    crate::credentials::save_api_key(&app, api_key)?;

    Ok(models_accessible)
}

/// Report whether a key is configured without returning the secret to the UI.
#[tauri::command]
pub(crate) fn has_api_key(app: AppHandle) -> Result<bool, String> {
    crate::credentials::has_api_key(&app)
}

/// Remove the saved API key from the operating system's credential store.
#[tauri::command]
pub(crate) fn delete_api_key(app: AppHandle) -> Result<(), String> {
    crate::credentials::delete_api_key(&app)
}

/// Load the key for Rust-side ElevenLabs requests. Never expose this via Tauri.
#[allow(dead_code)]
pub(crate) fn load_api_key(app: &AppHandle) -> Result<String, String> {
    crate::credentials::load_api_key(app)
}

#[cfg(test)]
mod tests {
    use super::error_status;
    use serde_json::json;

    #[test]
    fn reads_structured_error_status() {
        let body = json!({
            "detail": {
                "status": "missing_permissions",
                "message": "The API key is missing the permission models_read"
            }
        });

        assert_eq!(error_status(&body), Some("missing_permissions"));
    }

    #[test]
    fn ignores_unstructured_error_body() {
        let body = json!({ "detail": "Unauthorized" });

        assert_eq!(error_status(&body), None);
    }
}
