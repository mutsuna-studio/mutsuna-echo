use std::time::Duration;

use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tauri::AppHandle;

use crate::transcription::elevenlabs::client::{api_error_kind, ApiErrorKind, ElevenLabsClient};

const ELEVENLABS_MODELS_URL: &str = "https://api.elevenlabs.io/v1/models";

async fn validate_api_key(api_key: &SecretString) -> Result<bool, String> {
    let response = ElevenLabsClient::new(api_key, Duration::from_secs(20))?
        .get(ELEVENLABS_MODELS_URL)
        .send()
        .await
        .map_err(|error| format!("ElevenLabsに接続できませんでした: {error}"))?;

    let http_status = response.status();

    if http_status.is_success() {
        return Ok(true);
    }

    let body = response.json::<Value>().await.unwrap_or(Value::Null);

    match api_error_kind(&body) {
        ApiErrorKind::InvalidApiKey => Err("ElevenLabs APIキーが無効です。".to_string()),
        // Restricted keys are valid even when they cannot access the models
        // endpoint. Speech-to-Text permission is checked by the transcription
        // request itself.
        ApiErrorKind::MissingPermissions => Ok(false),
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

    let models_accessible = validate_api_key(&api_key).await?;

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

    use crate::transcription::elevenlabs::client::ElevenLabsClient;
    use secrecy::SecretString;

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
            ElevenLabsClient::new(
                &SecretString::from("test-secret".to_string()),
                Duration::from_secs(20),
            )
            .expect("build client")
            .get(format!("http://{redirect_address}/validate"))
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
