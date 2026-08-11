use std::time::Duration;

use futures_util::FutureExt;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tauri::AppHandle;

use crate::transcription::elevenlabs::client::{api_error_kind, ApiErrorKind, ElevenLabsClient};

const ELEVENLABS_MODELS_URL: &str = "https://api.elevenlabs.io/v1/models";
const API_KEY_VALIDATION_TIMEOUT: Duration = Duration::from_secs(25);

trait CredentialStorage {
    fn save(
        &mut self,
        credential: crate::credentials::CredentialId,
        secret: &SecretString,
    ) -> Result<(), String>;
    fn has(&self, credential: crate::credentials::CredentialId) -> Result<bool, String>;
    fn load(&self, credential: crate::credentials::CredentialId) -> Result<SecretString, String>;
    fn delete(&mut self, credential: crate::credentials::CredentialId) -> Result<(), String>;
}

struct AppCredentialStorage<'a>(&'a AppHandle);

impl CredentialStorage for AppCredentialStorage<'_> {
    fn save(
        &mut self,
        credential: crate::credentials::CredentialId,
        secret: &SecretString,
    ) -> Result<(), String> {
        crate::credentials::save(self.0, credential, secret)
    }

    fn has(&self, credential: crate::credentials::CredentialId) -> Result<bool, String> {
        crate::credentials::has(self.0, credential)
    }

    fn load(&self, credential: crate::credentials::CredentialId) -> Result<SecretString, String> {
        crate::credentials::load(self.0, credential)
    }

    fn delete(&mut self, credential: crate::credentials::CredentialId) -> Result<(), String> {
        crate::credentials::delete(self.0, credential)
    }
}

fn normalized_secret(value: String, empty_message: &str) -> Result<SecretString, String> {
    let received = SecretString::from(value);
    let normalized = received.expose_secret().trim();
    if normalized.is_empty() {
        Err(empty_message.to_string())
    } else {
        Ok(SecretString::from(normalized.to_owned()))
    }
}

fn persist_verified<S: CredentialStorage>(
    storage: &mut S,
    values: &[(crate::credentials::CredentialId, &SecretString)],
) -> Result<(), String> {
    let mut previous = Vec::with_capacity(values.len());
    for (credential, _) in values {
        let value = if storage.has(*credential)? {
            Some(storage.load(*credential)?)
        } else {
            None
        };
        previous.push((*credential, value));
    }

    let result = (|| {
        for (credential, expected) in values {
            storage.save(*credential, expected)?;
            let actual = storage.load(*credential).map_err(|error| {
                format!(
                    "{}を保存後に確認できませんでした: {error}",
                    credential.label()
                )
            })?;
            if actual.expose_secret() != expected.expose_secret() {
                return Err(format!(
                    "{}を端末へ正しく保存できませんでした。",
                    credential.label()
                ));
            }
        }
        Ok(())
    })();

    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for (credential, old_value) in previous {
            let rollback = match old_value {
                Some(old_value) => storage.save(credential, &old_value),
                None => storage.delete(credential),
            };
            if let Err(rollback_error) = rollback {
                rollback_errors.push(format!("{}: {rollback_error}", credential.label()));
            }
        }
        return if rollback_errors.is_empty() {
            Err(error)
        } else {
            Err(format!(
                "{error} 保存前の状態へ戻せませんでした（{}）。",
                rollback_errors.join("、")
            ))
        };
    }
    Ok(())
}

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

async fn validate_provider_api_key(
    credential: crate::credentials::CredentialId,
    api_key: &SecretString,
) -> Result<bool, String> {
    match credential {
        crate::credentials::CredentialId::ElevenLabs => validate_api_key(api_key).await,
        crate::credentials::CredentialId::Soniox => {
            crate::transcription::soniox::validate_api_key(api_key).await?;
            Ok(true)
        }
        crate::credentials::CredentialId::CloudflareApiToken
        | crate::credentials::CredentialId::CloudflareAccountId => {
            Err("CloudflareではAPIトークンとAccount IDを一緒に設定してください。".into())
        }
    }
}

#[tauri::command]
pub(crate) async fn save_provider_api_key(
    app: AppHandle,
    provider_id: String,
    api_key: String,
    account_id: Option<String>,
) -> Result<bool, String> {
    if provider_id == "cloudflare" {
        let account_id = normalized_secret(
            account_id.unwrap_or_default(),
            "Cloudflare Account IDを入力してください。",
        )?;
        let api_key = normalized_secret(api_key, "Cloudflare APIトークンを入力してください。")?;
        crate::transcription::cloudflare::validate_credentials(&account_id, &api_key).await?;
        persist_verified(
            &mut AppCredentialStorage(&app),
            &[
                (
                    crate::credentials::CredentialId::CloudflareApiToken,
                    &api_key,
                ),
                (
                    crate::credentials::CredentialId::CloudflareAccountId,
                    &account_id,
                ),
            ],
        )?;
        return Ok(true);
    }
    let credential = crate::credentials::CredentialId::from_provider_id(&provider_id)?;
    let api_key = normalized_secret(api_key, "APIキーを入力してください。")?;
    eprintln!("[api-key] validation started provider={provider_id}");
    let validation = std::panic::AssertUnwindSafe(validate_provider_api_key(credential, &api_key))
        .catch_unwind();
    let fully_accessible = tokio::time::timeout(API_KEY_VALIDATION_TIMEOUT, validation)
        .await
        .map_err(|_| {
            "APIキーの確認に時間がかかっています。通信状態を確認して、もう一度お試しください。"
                .to_string()
        })?
        .map_err(|_| {
            "APIキーの確認処理を完了できませんでした。アプリを再起動して、もう一度お試しください。"
                .to_string()
        })??;
    eprintln!("[api-key] validation completed provider={provider_id}");
    persist_verified(&mut AppCredentialStorage(&app), &[(credential, &api_key)])?;
    eprintln!("[api-key] credential saved provider={provider_id}");
    Ok(fully_accessible)
}

#[tauri::command]
pub(crate) fn delete_provider_api_key(app: AppHandle, provider_id: String) -> Result<(), String> {
    if provider_id == "cloudflare" {
        crate::credentials::delete(&app, crate::credentials::CredentialId::CloudflareApiToken)?;
        return crate::credentials::delete(
            &app,
            crate::credentials::CredentialId::CloudflareAccountId,
        );
    }
    let credential = crate::credentials::CredentialId::from_provider_id(&provider_id)?;
    crate::credentials::delete(&app, credential)
}

/// Validate and store the API key in the operating system's credential store.
#[tauri::command]
pub(crate) async fn save_api_key(app: AppHandle, api_key: String) -> Result<bool, String> {
    save_provider_api_key(app, "elevenlabs".into(), api_key, None).await
}

/// Report whether a key is configured without returning the secret to the UI.
#[tauri::command]
pub(crate) fn has_api_key(app: AppHandle) -> Result<bool, String> {
    crate::credentials::has_api_key(&app)
}

/// Remove the saved API key from the operating system's credential store.
#[tauri::command]
pub(crate) fn delete_api_key(app: AppHandle) -> Result<(), String> {
    delete_provider_api_key(app, "elevenlabs".into())
}

/// Load the key for Rust-side ElevenLabs requests. Never expose this via Tauri.
pub(crate) fn load_api_key(app: &AppHandle) -> Result<SecretString, String> {
    crate::credentials::load_api_key(app)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::Duration,
    };

    use super::{normalized_secret, persist_verified, CredentialStorage};
    use crate::credentials::CredentialId;
    use crate::transcription::elevenlabs::client::ElevenLabsClient;
    use secrecy::{ExposeSecret, SecretString};

    #[derive(Default)]
    struct FakeCredentialStorage {
        values: HashMap<CredentialId, String>,
        save_count: usize,
        fail_save_at: Option<usize>,
        corrupt_readback_after_save: Option<CredentialId>,
    }

    impl CredentialStorage for FakeCredentialStorage {
        fn save(&mut self, credential: CredentialId, secret: &SecretString) -> Result<(), String> {
            self.save_count += 1;
            if self.fail_save_at == Some(self.save_count) {
                return Err("synthetic save failure".into());
            }
            self.values
                .insert(credential, secret.expose_secret().to_owned());
            Ok(())
        }

        fn has(&self, credential: CredentialId) -> Result<bool, String> {
            Ok(self.values.contains_key(&credential))
        }

        fn load(&self, credential: CredentialId) -> Result<SecretString, String> {
            if self.save_count > 0 && self.corrupt_readback_after_save == Some(credential) {
                return Ok(SecretString::from("synthetic-corruption".to_string()));
            }
            self.values
                .get(&credential)
                .cloned()
                .map(SecretString::from)
                .ok_or_else(|| "not installed".into())
        }

        fn delete(&mut self, credential: CredentialId) -> Result<(), String> {
            self.values.remove(&credential);
            Ok(())
        }
    }

    #[test]
    fn trims_credentials_and_rejects_blank_values() {
        let secret =
            normalized_secret("  synthetic-key\n".into(), "empty").expect("non-empty credential");
        assert_eq!(secret.expose_secret(), "synthetic-key");
        assert_eq!(
            normalized_secret(" \t\n".into(), "empty").err().as_deref(),
            Some("empty")
        );
    }

    #[test]
    fn verifies_single_credential_by_reading_it_back() {
        let mut storage = FakeCredentialStorage::default();
        let secret = SecretString::from("synthetic-soniox".to_string());

        persist_verified(&mut storage, &[(CredentialId::Soniox, &secret)])
            .expect("verified persistence");

        assert_eq!(
            storage
                .values
                .get(&CredentialId::Soniox)
                .map(String::as_str),
            Some("synthetic-soniox")
        );
    }

    #[test]
    fn rolls_back_when_readback_does_not_match() {
        let mut storage = FakeCredentialStorage {
            corrupt_readback_after_save: Some(CredentialId::Soniox),
            ..Default::default()
        };
        let secret = SecretString::from("synthetic-soniox".to_string());

        let error = persist_verified(&mut storage, &[(CredentialId::Soniox, &secret)])
            .expect_err("mismatched readback must fail");

        assert!(error.contains("正しく保存できませんでした"));
        assert!(!storage.values.contains_key(&CredentialId::Soniox));
    }

    #[test]
    fn cloudflare_pair_failure_restores_both_previous_values() {
        let mut storage = FakeCredentialStorage {
            values: HashMap::from([
                (CredentialId::CloudflareApiToken, "old-token".into()),
                (CredentialId::CloudflareAccountId, "old-account".into()),
            ]),
            fail_save_at: Some(2),
            ..Default::default()
        };
        let token = SecretString::from("new-token".to_string());
        let account = SecretString::from("new-account".to_string());

        persist_verified(
            &mut storage,
            &[
                (CredentialId::CloudflareApiToken, &token),
                (CredentialId::CloudflareAccountId, &account),
            ],
        )
        .expect_err("second write must roll back the pair");

        assert_eq!(
            storage
                .values
                .get(&CredentialId::CloudflareApiToken)
                .map(String::as_str),
            Some("old-token")
        );
        assert_eq!(
            storage
                .values
                .get(&CredentialId::CloudflareAccountId)
                .map(String::as_str),
            Some("old-account")
        );
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
