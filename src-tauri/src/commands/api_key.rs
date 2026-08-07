use std::time::Duration;

use reqwest::{redirect::Policy, Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tauri::AppHandle;

const ELEVENLABS_MODELS_URL: &str = "https://api.elevenlabs.io/v1/models";

fn elevenlabs_client() -> Result<Client, String> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| {
            eprintln!("Could not build ElevenLabs HTTP client: {error:?}");
            "ElevenLabsへの接続を準備できませんでした。".to_string()
        })
}

fn error_status(body: &Value) -> Option<&str> {
    body.pointer("/detail/status").and_then(Value::as_str)
}

async fn validate_api_key(api_key: &str) -> Result<bool, String> {
    let response = elevenlabs_client()?
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
    let received_api_key = SecretString::from(api_key);
    let api_key = SecretString::from(received_api_key.expose_secret().trim().to_owned());

    if api_key.expose_secret().is_empty() {
        return Err("APIキーを入力してください。".to_string());
    }

    let models_accessible = validate_api_key(api_key.expose_secret()).await?;

    crate::credentials::save_api_key(&app, &api_key)?;

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
pub(crate) fn load_api_key(app: &AppHandle) -> Result<SecretString, String> {
    crate::credentials::load_api_key(app)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::Duration,
    };

    use super::{elevenlabs_client, error_status};
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

    #[test]
    fn credential_client_does_not_follow_redirects() {
        let redirect_server = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        let destination_server = TcpListener::bind("127.0.0.1:0").expect("bind destination");
        destination_server
            .set_nonblocking(true)
            .expect("set destination nonblocking");

        let redirect_address = redirect_server.local_addr().expect("redirect address");
        let destination_address = destination_server
            .local_addr()
            .expect("destination address");
        let (ready_tx, ready_rx) = mpsc::channel();

        let server = std::thread::spawn(move || {
            ready_tx.send(()).expect("signal server readiness");
            let (mut stream, _) = redirect_server.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).expect("read request");
            assert!(String::from_utf8_lossy(&request[..read]).contains("xi-api-key: test-secret"));
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://{destination_address}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect");
        });

        ready_rx.recv().expect("wait for server readiness");
        let response = tauri::async_runtime::block_on(async {
            elevenlabs_client()
                .expect("build client")
                .get(format!("http://{redirect_address}/validate"))
                .header("xi-api-key", "test-secret")
                .send()
                .await
                .expect("send request")
        });

        assert!(response.status().is_redirection());
        server.join().expect("join redirect server");
        std::thread::sleep(Duration::from_millis(50));
        assert!(destination_server.accept().is_err());
    }
}
