use super::{client, json_response, State};
use crate::credentials::{self, CredentialId};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(not(target_os = "android"))]
pub(super) const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
#[cfg(not(target_os = "android"))]
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Deserialize, Serialize)]
pub(super) struct Session {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    pub account: String,
    pub email: String,
    pub expires_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Connection {
    connected: bool,
    configured: bool,
    email: Option<String>,
}

fn client_id() -> Result<&'static str, String> {
    option_env!("MUTSUNA_GOOGLE_DESKTOP_CLIENT_ID")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "Google連携の設定がありません。OAuthクライアントIDを設定してビルドしてください。".into()
        })
}

pub(super) fn load(app: &AppHandle) -> Result<Session, String> {
    let value = credentials::load(app, CredentialId::GoogleDriveSession)?;
    serde_json::from_str(value.expose_secret())
        .map_err(|_| "Googleとの連携情報を読み取れません。再連携してください。".into())
}

// A single native store entry makes the account/token tuple indivisible.
fn persist_with(
    session: &Session,
    save: impl FnOnce(&SecretString) -> Result<(), String>,
    load: impl FnOnce() -> Result<SecretString, String>,
    delete: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let value = SecretString::from(
        serde_json::to_string(session).map_err(|_| "Google連携情報を準備できません。")?,
    );
    let result = save(&value).and_then(|()| {
        let actual = load()?;
        if actual.expose_secret() == value.expose_secret() {
            Ok(())
        } else {
            Err("Google連携情報の保存確認に失敗しました。".into())
        }
    });
    if result.is_err() {
        delete()
            .map_err(|_| "Google連携情報の保存と削除に失敗しました。連携を解除してください。")?;
    }
    result
}

fn persist(app: &AppHandle, session: &Session) -> Result<(), String> {
    persist_with(
        session,
        |v| credentials::save(app, CredentialId::GoogleDriveSession, v),
        || credentials::load(app, CredentialId::GoogleDriveSession),
        || credentials::delete(app, CredentialId::GoogleDriveSession),
    )
}

async fn validated(access: String, refresh: String, expires: i64) -> Result<Session, String> {
    validated_at(
        access,
        refresh,
        expires,
        "https://www.googleapis.com/drive/v3",
    )
    .await
}

async fn validated_at(
    access: String,
    refresh: String,
    expires: i64,
    base: &str,
) -> Result<Session, String> {
    let access = access.trim().to_owned();
    if access.is_empty() {
        return Err("Googleから有効な認証情報を取得できませんでした。".into());
    }
    let data = json_response(
        client()?
            .get(format!("{base}/about"))
            .query(&[("fields", "user(permissionId,emailAddress)")])
            .bearer_auth(&access)
            .send()
            .await,
    )
    .await?;
    let account = data["user"]["permissionId"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("Googleアカウントを確認できませんでした。")?
        .to_owned();
    json_response(
        client()?
            .get(format!("{base}/files"))
            .query(&[("pageSize", "1"), ("fields", "files(id)")])
            .bearer_auth(&access)
            .send()
            .await,
    )
    .await?;
    Ok(Session {
        access_token: access,
        refresh_token: refresh.trim().into(),
        account,
        email: data["user"]["emailAddress"]
            .as_str()
            .unwrap_or("Googleアカウント")
            .into(),
        expires_at: chrono::Utc::now().timestamp() + expires.clamp(0, 3600),
    })
}

pub(super) async fn access(app: &AppHandle) -> Result<Session, String> {
    let old = load(app)?;
    if old.expires_at > chrono::Utc::now().timestamp() + 120 {
        return Ok(old);
    }
    #[cfg(target_os = "android")]
    let next = validated(super::android::authorize().await?, String::new(), 3000).await?;
    #[cfg(not(target_os = "android"))]
    let next = {
        if old.refresh_token.is_empty() {
            return Err("Googleとの再連携が必要です。".into());
        }
        let mut form = vec![
            ("client_id", client_id()?),
            ("refresh_token", old.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(secret) = option_env!("MUTSUNA_GOOGLE_DESKTOP_CLIENT_SECRET") {
            form.push(("client_secret", secret));
        }
        let value = json_response(client()?.post(TOKEN_URL).form(&form).send().await).await?;
        validated(
            value["access_token"].as_str().unwrap_or_default().into(),
            value["refresh_token"]
                .as_str()
                .unwrap_or(&old.refresh_token)
                .into(),
            value["expires_in"].as_i64().unwrap_or(0),
        )
        .await?
    };
    if next.account != old.account {
        return Err("Googleアカウントが変更されています。設定画面から再連携してください。".into());
    }
    persist(app, &next)?;
    Ok(next)
}

#[tauri::command]
pub(crate) fn google_drive_connection(app: AppHandle) -> Result<Connection, String> {
    let exists = credentials::has(&app, CredentialId::GoogleDriveSession)?;
    let email = if exists {
        Some(load(&app)?.email)
    } else {
        None
    };
    Ok(Connection {
        connected: exists,
        configured: cfg!(target_os = "android") || client_id().is_ok(),
        email,
    })
}

#[tauri::command]
pub(crate) async fn google_drive_connect(
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Connection, String> {
    let _guard = state
        .lock
        .try_lock()
        .map_err(|_| "Google Driveの処理が進行中です。")?;
    #[cfg(target_os = "android")]
    let session = validated(super::android::authorize().await?, String::new(), 3000).await?;
    #[cfg(not(target_os = "android"))]
    let session = desktop_connect().await?;
    persist(&app, &session)?;
    google_drive_connection(app)
}

#[tauri::command]
pub(crate) async fn google_drive_disconnect(
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let _guard = state
        .lock
        .try_lock()
        .map_err(|_| "Google Driveの処理が進行中です。")?;
    // Local disconnect deliberately leaves previously exported files intact.
    credentials::delete(&app, CredentialId::GoogleDriveSession)
}

#[cfg(not(target_os = "android"))]
async fn desktop_connect() -> Result<Session, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    let random = || -> Result<String, String> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| "Google認証を準備できません。")?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    };
    let state = random()?;
    let verifier = SecretString::from(random()?);
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Google認証の待受を開始できません。")?;
    let redirect = format!(
        "http://127.0.0.1:{}/oauth/google/callback",
        listener
            .local_addr()
            .map_err(|_| "Google認証の待受を確認できません。")?
            .port()
    );
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id()?),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("state", &state),
        ("code_challenge_method", "S256"),
        (
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose_secret().as_bytes())),
        ),
    ]);
    tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
        .map_err(|_| "Google認証用ブラウザーを開けません。")?;
    let code = tauri::async_runtime::spawn_blocking(move || callback(listener, &state))
        .await
        .map_err(|_| "Google認証の待受に失敗しました。")??;
    let mut form = vec![
        ("client_id", client_id()?),
        ("code", code.expose_secret()),
        ("redirect_uri", &redirect),
        ("code_verifier", verifier.expose_secret()),
        ("grant_type", "authorization_code"),
    ];
    if let Some(secret) = option_env!("MUTSUNA_GOOGLE_DESKTOP_CLIENT_SECRET") {
        form.push(("client_secret", secret));
    }
    let value = json_response(client()?.post(TOKEN_URL).form(&form).send().await).await?;
    let refresh = value["refresh_token"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Googleの継続アクセスを許可して再連携してください。")?;
    validated(
        value["access_token"].as_str().unwrap_or_default().into(),
        refresh.into(),
        value["expires_in"].as_i64().unwrap_or(0),
    )
    .await
}

#[cfg(not(target_os = "android"))]
fn callback(listener: std::net::TcpListener, expected: &str) -> Result<SecretString, String> {
    use std::{
        io::{Read, Write},
        time::{Duration, Instant},
    };
    listener
        .set_nonblocking(true)
        .map_err(|_| "Google認証を開始できません。")?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(180) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut bytes = [0_u8; 8192];
                let Ok(n) = stream.read(&mut bytes) else {
                    continue;
                };
                let line = String::from_utf8_lossy(&bytes[..n]);
                let Some(target) = line
                    .strip_prefix("GET ")
                    .and_then(|s| s.split_once(' ').map(|v| v.0))
                else {
                    continue;
                };
                let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
                    continue;
                };
                if url.path() != "/oauth/google/callback" {
                    continue;
                }
                let result = callback_code(&url, expected);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nReturn to Mutsuna Echo to check the connection result.");
                return result;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("Google認証の待受に失敗しました。".into()),
        }
    }
    Err("Google認証がタイムアウトしました。再度連携してください。".into())
}

#[cfg(any(not(target_os = "android"), test))]
fn callback_code(url: &url::Url, expected: &str) -> Result<SecretString, String> {
    let pairs: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = pairs.iter().filter(|(k, _)| k == "state").collect();
    if states.len() != 1 || states[0].1 != expected {
        return Err("Google認証のstateが一致しません。".into());
    }
    if pairs.iter().any(|(k, _)| k == "error") {
        return Err("Googleとの連携が許可されませんでした。".into());
    }
    let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
    if codes.len() != 1 || codes[0].1.trim().is_empty() {
        return Err("Googleの認証コードを取得できませんでした。".into());
    }
    Ok(SecretString::from(codes[0].1.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn provider_validation_trims_token_checks_permissions_and_rejects_http_errors() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        for status in [200, 401, 403, 302] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                for index in 0..if status == 200 { 2 } else { 1 } {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut bytes = [0; 8192];
                    let n = socket.read(&mut bytes).unwrap();
                    let request = String::from_utf8_lossy(&bytes[..n]).to_lowercase();
                    assert!(request.contains("authorization: bearer synthetic-access\r\n"));
                    assert!(request.starts_with(if index == 0 {
                        "get /about?"
                    } else {
                        "get /files?"
                    }));
                    let body = if index == 0 {
                        r#"{"user":{"permissionId":"synthetic-account","emailAddress":"test@example.invalid"}}"#
                    } else {
                        r#"{"files":[]}"#
                    };
                    write!(socket,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nLocation: http://127.0.0.1:1/should-not-follow\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
            });
            let result = tauri::async_runtime::block_on(validated_at(
                "  synthetic-access  ".into(),
                " synthetic-refresh ".into(),
                3600,
                &base,
            ));
            if status == 200 {
                let session = result.unwrap();
                assert_eq!(session.access_token, "synthetic-access");
                assert_eq!(session.refresh_token, "synthetic-refresh");
                assert_eq!(session.account, "synthetic-account");
            } else {
                let error = result.err().unwrap();
                assert!(!error.contains("synthetic-access"));
            }
            server.join().unwrap();
        }
        assert!(tauri::async_runtime::block_on(validated_at(
            "  ".into(),
            String::new(),
            0,
            "http://127.0.0.1:1"
        ))
        .is_err());
    }
    #[test]
    fn verified_store_and_failed_round_trip_cleanup() {
        let session = Session {
            access_token: "synthetic-access".into(),
            refresh_token: "synthetic-refresh".into(),
            account: "test".into(),
            email: "test@example.invalid".into(),
            expires_at: 0,
        };
        let store = RefCell::new(String::new());
        persist_with(
            &session,
            |v| {
                *store.borrow_mut() = v.expose_secret().into();
                Ok(())
            },
            || Ok(SecretString::from(store.borrow().clone())),
            || {
                store.borrow_mut().clear();
                Ok(())
            },
        )
        .unwrap();
        assert!(!store.borrow().is_empty());
        let result = persist_with(
            &session,
            |_| Ok(()),
            || Ok(SecretString::from("synthetic-corrupt")),
            || {
                store.borrow_mut().clear();
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(store.borrow().is_empty());
        assert!(persist_with(
            &session,
            |_| Err("save failed".into()),
            || unreachable!(),
            || Ok(())
        )
        .is_err());
    }
    #[test]
    fn callback_requires_state_and_rejects_denial_and_duplicates() {
        let good = url::Url::parse(
            "http://127.0.0.1/oauth/google/callback?state=synthetic&code=synthetic-code",
        )
        .unwrap();
        assert_eq!(
            callback_code(&good, "synthetic").unwrap().expose_secret(),
            "synthetic-code"
        );
        assert!(callback_code(&good, "wrong").is_err());
        for query in [
            "state=synthetic&error=access_denied",
            "state=synthetic&state=synthetic&code=a",
            "state=synthetic&code=",
        ] {
            let mut bad = good.clone();
            bad.set_query(Some(query));
            assert!(callback_code(&bad, "synthetic").is_err());
        }
    }
}
