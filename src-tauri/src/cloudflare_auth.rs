use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use reqwest::{redirect::Policy, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};
use url::Url;

use crate::credentials::CredentialId;

const AUTHORIZATION_URL: &str = "https://dash.cloudflare.com/oauth2/auth";
const TOKEN_URL: &str = "https://dash.cloudflare.com/oauth2/token";
const REVOKE_URL: &str = "https://dash.cloudflare.com/oauth2/revoke";
const ACCOUNTS_URL: &str = "https://api.cloudflare.com/client/v4/accounts";
const REDIRECT_URI: &str = "http://127.0.0.1:8976/oauth/cloudflare/callback";
const OAUTH_SCOPES: &str = "ai.read offline_access";
const FLOW_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const REFRESH_SKEW_SECONDS: i64 = 60;

pub(crate) struct CloudflareAuthState {
    oauth_flow: tokio::sync::Mutex<()>,
    refresh: tokio::sync::Mutex<()>,
}

impl Default for CloudflareAuthState {
    fn default() -> Self {
        Self {
            oauth_flow: tokio::sync::Mutex::new(()),
            refresh: tokio::sync::Mutex::new(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloudflareAuthMethod {
    OAuth,
    ApiToken,
}

pub(crate) struct CloudflareAuthContext {
    pub(crate) account_id: SecretString,
    pub(crate) access_token: SecretString,
    pub(crate) auth_method: CloudflareAuthMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudflareAccountOption {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudflareConnectionStatus {
    connected: bool,
    auth_method: Option<&'static str>,
    account_name: Option<String>,
    needs_reauthentication: bool,
    account_selection_required: bool,
    accounts: Vec<CloudflareAccountOption>,
    oauth_configured: bool,
    legacy_configured: bool,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccountsEnvelope {
    success: bool,
    #[serde(default)]
    result: Vec<CloudflareAccountOption>,
}

struct PendingFlow {
    state: String,
    verifier: SecretString,
    created_at: Instant,
}

trait CredentialStorage {
    fn save(&mut self, id: CredentialId, value: &SecretString) -> Result<(), String>;
    fn has(&self, id: CredentialId) -> Result<bool, String>;
    fn load(&self, id: CredentialId) -> Result<SecretString, String>;
    fn delete(&mut self, id: CredentialId) -> Result<(), String>;
}

struct AppCredentialStorage<'a>(&'a AppHandle);

impl CredentialStorage for AppCredentialStorage<'_> {
    fn save(&mut self, id: CredentialId, value: &SecretString) -> Result<(), String> {
        crate::credentials::save(self.0, id, value)
    }

    fn has(&self, id: CredentialId) -> Result<bool, String> {
        crate::credentials::has(self.0, id)
    }

    fn load(&self, id: CredentialId) -> Result<SecretString, String> {
        crate::credentials::load(self.0, id)
    }

    fn delete(&mut self, id: CredentialId) -> Result<(), String> {
        crate::credentials::delete(self.0, id)
    }
}

fn oauth_client_id() -> Option<&'static str> {
    option_env!("MUTSUNA_CLOUDFLARE_OAUTH_CLIENT_ID")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn oauth_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "Cloudflare OAuth接続を準備できませんでした。".to_string())
}

fn random_url_safe(byte_count: usize) -> Result<String, String> {
    let mut bytes = vec![0_u8; byte_count];
    getrandom::fill(&mut bytes)
        .map_err(|_| "安全なOAuth認証情報を生成できませんでした。".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn create_pending_flow() -> Result<(PendingFlow, String), String> {
    let verifier = random_url_safe(64)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok((
        PendingFlow {
            state: random_url_safe(32)?,
            verifier: SecretString::from(verifier),
            created_at: Instant::now(),
        },
        challenge,
    ))
}

fn authorization_url(
    client_id: &str,
    flow: &PendingFlow,
    challenge: &str,
) -> Result<String, String> {
    let mut url = Url::parse(AUTHORIZATION_URL)
        .map_err(|_| "Cloudflare OAuth URLを構築できませんでした。".to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", OAUTH_SCOPES)
        .append_pair("state", &flow.state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

fn write_callback_page(stream: &mut std::net::TcpStream, success: bool) {
    let (title, message) = if success {
        (
            "Cloudflareへの接続を確認しました",
            "Mutsuna Echoへ戻ってください。",
        )
    } else {
        (
            "Cloudflareへの接続を完了できませんでした",
            "Mutsuna Echoへ戻って、もう一度お試しください。",
        )
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><style>body{{font:16px system-ui;max-width:36rem;margin:15vh auto;padding:2rem;color:#17221b}}h1{{font-size:1.35rem}}</style><h1>{title}</h1><p>{message}</p>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn wait_for_callback(listener: TcpListener) -> Result<Url, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "OAuth callbackの待受を開始できませんでした。".to_string())?;
    let started = Instant::now();
    loop {
        if started.elapsed() >= FLOW_TIMEOUT {
            return Err(
                "Cloudflare OAuthがタイムアウトしました。もう一度接続してください。".into(),
            );
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut request = [0_u8; 8_192];
                let read = stream
                    .read(&mut request)
                    .map_err(|_| "OAuth callbackを読み取れませんでした。".to_string())?;
                let first_line = String::from_utf8_lossy(&request[..read])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let target = first_line
                    .strip_prefix("GET ")
                    .and_then(|value| value.split_once(' ').map(|(target, _)| target))
                    .ok_or_else(|| "OAuth callbackの形式が正しくありません。".to_string())?;
                let callback = Url::parse(&format!("http://127.0.0.1{target}"))
                    .map_err(|_| "OAuth callbackの形式が正しくありません。".to_string())?;
                let valid_path = callback.path() == "/oauth/cloudflare/callback";
                write_callback_page(&mut stream, valid_path);
                if valid_path {
                    return Ok(callback);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Err("OAuth callbackを受信できませんでした。".into()),
        }
    }
}

fn callback_value(callback: &Url, name: &str) -> Option<String> {
    callback
        .query_pairs()
        .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
}

fn validate_callback(callback: &Url, flow: &PendingFlow) -> Result<String, String> {
    if flow.created_at.elapsed() >= FLOW_TIMEOUT {
        return Err("Cloudflare OAuthの有効時間が切れました。もう一度接続してください。".into());
    }
    if let Some(error) = callback_value(callback, "error") {
        return match error.as_str() {
            "access_denied" => Err("Cloudflare OAuthがキャンセルされました。".into()),
            _ => Err("Cloudflareで認証を許可できませんでした。".into()),
        };
    }
    let returned_state = callback_value(callback, "state")
        .ok_or_else(|| "OAuth stateを確認できませんでした。".to_string())?;
    if returned_state.as_bytes() != flow.state.as_bytes() {
        return Err("OAuth stateが一致しないため、接続を拒否しました。".into());
    }
    callback_value(callback, "code")
        .filter(|code| !code.trim().is_empty())
        .ok_or_else(|| "Cloudflareから認可コードを受け取れませんでした。".to_string())
}

async fn exchange_authorization_code(
    client: &reqwest::Client,
    client_id: &str,
    code: &str,
    verifier: &SecretString,
) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", client_id),
            ("code_verifier", verifier.expose_secret()),
        ])
        .send()
        .await
        .map_err(map_oauth_network_error)?;
    parse_token_response(response, "Cloudflare OAuthのtoken交換に失敗しました。").await
}

async fn parse_token_response(
    response: reqwest::Response,
    default_message: &str,
) -> Result<TokenResponse, String> {
    let status = response.status();
    if status.is_success() {
        let token = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| "Cloudflare OAuthの応答形式を読み取れませんでした。".to_string())?;
        if token.access_token.trim().is_empty() || token.expires_in <= 0 {
            return Err("Cloudflare OAuthから有効なtokenを受け取れませんでした。".into());
        }
        return Ok(token);
    }
    let body = response.json::<OAuthErrorResponse>().await.ok();
    let oauth_error = body.as_ref().and_then(|body| body.error.as_deref());
    match (status, oauth_error) {
        (_, Some("invalid_grant")) => {
            Err("Cloudflare OAuthの認証期限が切れました。再接続してください。".into())
        }
        (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _) => {
            Err("Cloudflare OAuthを認証できませんでした。再接続してください。".into())
        }
        _ => Err(format!("{default_message}（HTTP {status}）")),
    }
}

fn map_oauth_network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Cloudflare OAuthへの接続がタイムアウトしました。".into()
    } else if error.is_connect() {
        "Cloudflare OAuthへ接続できませんでした。DNS・TLS・通信状態を確認してください。".into()
    } else {
        "Cloudflare OAuthとの通信に失敗しました。".into()
    }
}

async fn fetch_accounts(
    client: &reqwest::Client,
    access_token: &SecretString,
) -> Result<Vec<CloudflareAccountOption>, String> {
    let response = client
        .get(ACCOUNTS_URL)
        .bearer_auth(access_token.expose_secret())
        .query(&[("per_page", "50")])
        .send()
        .await
        .map_err(map_oauth_network_error)?;
    match response.status() {
        StatusCode::UNAUTHORIZED => return Err("Cloudflare OAuth tokenが無効です。".into()),
        StatusCode::FORBIDDEN => {
            return Err("Cloudflareアカウントを確認する権限がありません。".into())
        }
        status if !status.is_success() => {
            return Err(format!(
                "Cloudflareアカウントを取得できませんでした（HTTP {status}）。"
            ))
        }
        _ => {}
    }
    let envelope = response
        .json::<AccountsEnvelope>()
        .await
        .map_err(|_| "Cloudflareアカウント一覧を読み取れませんでした。".to_string())?;
    if !envelope.success || envelope.result.is_empty() {
        return Err("OAuthで利用可能なCloudflareアカウントがありません。".into());
    }
    Ok(envelope.result)
}

fn persist_verified<S: CredentialStorage>(
    storage: &mut S,
    values: &[(CredentialId, &SecretString)],
) -> Result<(), String> {
    let mut previous = Vec::with_capacity(values.len());
    for (id, _) in values {
        previous.push((
            *id,
            storage.has(*id)?.then(|| storage.load(*id)).transpose()?,
        ));
    }
    let result = (|| {
        for (id, expected) in values {
            storage.save(*id, expected)?;
            let actual = storage.load(*id)?;
            if actual.expose_secret() != expected.expose_secret() {
                return Err(format!(
                    "{}を端末へ正しく保存できませんでした。",
                    id.label()
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        for (id, value) in previous {
            match value {
                Some(value) => storage.save(id, &value)?,
                None => storage.delete(id)?,
            }
        }
        return Err(error);
    }
    Ok(())
}

fn serialize_accounts(accounts: &[CloudflareAccountOption]) -> Result<SecretString, String> {
    serde_json::to_string(accounts)
        .map(SecretString::from)
        .map_err(|_| "Cloudflareアカウント情報を保存できませんでした。".to_string())
}

fn stored_accounts(app: &AppHandle) -> Vec<CloudflareAccountOption> {
    crate::credentials::load(app, CredentialId::CloudflareOAuthAccounts)
        .ok()
        .and_then(|value| serde_json::from_str(value.expose_secret()).ok())
        .unwrap_or_default()
}

fn legacy_configured(app: &AppHandle) -> Result<bool, String> {
    Ok(
        crate::credentials::has(app, CredentialId::CloudflareApiToken)?
            && crate::credentials::has(app, CredentialId::CloudflareAccountId)?,
    )
}

fn oauth_has_tokens(app: &AppHandle) -> Result<bool, String> {
    Ok(
        crate::credentials::has(app, CredentialId::CloudflareOAuthAccessToken)?
            && crate::credentials::has(app, CredentialId::CloudflareOAuthRefreshToken)?
            && crate::credentials::has(app, CredentialId::CloudflareOAuthExpiresAt)?,
    )
}

fn oauth_connected(app: &AppHandle) -> Result<bool, String> {
    Ok(oauth_has_tokens(app)?
        && crate::credentials::has(app, CredentialId::CloudflareOAuthAccountId)?)
}

fn stored_auth_method<S: CredentialStorage>(
    storage: &S,
) -> Result<Option<CloudflareAuthMethod>, String> {
    let oauth = storage.has(CredentialId::CloudflareOAuthAccessToken)?
        && storage.has(CredentialId::CloudflareOAuthRefreshToken)?
        && storage.has(CredentialId::CloudflareOAuthExpiresAt)?
        && storage.has(CredentialId::CloudflareOAuthAccountId)?;
    if oauth {
        return Ok(Some(CloudflareAuthMethod::OAuth));
    }
    let legacy = storage.has(CredentialId::CloudflareApiToken)?
        && storage.has(CredentialId::CloudflareAccountId)?;
    Ok(legacy.then_some(CloudflareAuthMethod::ApiToken))
}

pub(crate) fn is_configured(app: &AppHandle) -> Result<bool, String> {
    Ok(stored_auth_method(&AppCredentialStorage(app))?.is_some())
}

pub(crate) fn connection_status(app: &AppHandle) -> Result<CloudflareConnectionStatus, String> {
    let oauth_tokens = oauth_has_tokens(app)?;
    let oauth_connected = oauth_connected(app)?;
    let legacy = legacy_configured(app)?;
    let accounts = if oauth_tokens && !oauth_connected {
        stored_accounts(app)
    } else {
        Vec::new()
    };
    let account_name = if oauth_connected {
        crate::credentials::load(app, CredentialId::CloudflareOAuthAccountName)
            .ok()
            .map(|value| value.expose_secret().to_owned())
    } else {
        None
    };
    Ok(CloudflareConnectionStatus {
        connected: oauth_connected || legacy,
        auth_method: if oauth_connected {
            Some("oauth")
        } else if legacy {
            Some("apiToken")
        } else {
            None
        },
        account_name,
        needs_reauthentication: false,
        account_selection_required: oauth_tokens && !oauth_connected && !accounts.is_empty(),
        accounts,
        oauth_configured: oauth_client_id().is_some(),
        legacy_configured: legacy,
    })
}

fn expiration_from_now(expires_in: i64) -> SecretString {
    SecretString::from((Utc::now().timestamp() + expires_in).to_string())
}

fn token_needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at <= now + REFRESH_SKEW_SECONDS
}

async fn request_refresh(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    refresh: &SecretString,
) -> Result<TokenResponse, String> {
    let response = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.expose_secret()),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(map_oauth_network_error)?;
    parse_token_response(response, "Cloudflare OAuth tokenの更新に失敗しました。").await
}

async fn refresh_oauth(app: &AppHandle, state: &CloudflareAuthState) -> Result<(), String> {
    let _guard = state.refresh.lock().await;
    let expires = crate::credentials::load(app, CredentialId::CloudflareOAuthExpiresAt)?;
    let expires_at = expires.expose_secret().parse::<i64>().unwrap_or_default();
    if !token_needs_refresh(expires_at, Utc::now().timestamp()) {
        return Ok(());
    }
    let client_id =
        oauth_client_id().ok_or_else(|| "Cloudflare OAuthが設定されていません。".to_string())?;
    let refresh = crate::credentials::load(app, CredentialId::CloudflareOAuthRefreshToken)?;
    let client = oauth_http_client()?;
    let token = request_refresh(&client, TOKEN_URL, client_id, &refresh).await?;
    let access = SecretString::from(token.access_token.trim().to_owned());
    let next_refresh = SecretString::from(
        token
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(refresh.expose_secret())
            .to_owned(),
    );
    let expires = expiration_from_now(token.expires_in);
    persist_verified(
        &mut AppCredentialStorage(app),
        &[
            (CredentialId::CloudflareOAuthAccessToken, &access),
            (CredentialId::CloudflareOAuthRefreshToken, &next_refresh),
            (CredentialId::CloudflareOAuthExpiresAt, &expires),
        ],
    )
}

pub(crate) async fn resolve_valid_credentials(
    app: &AppHandle,
) -> Result<CloudflareAuthContext, String> {
    if oauth_connected(app)? {
        let state = app.state::<CloudflareAuthState>();
        refresh_oauth(app, &state).await.map_err(|_| {
            "Cloudflare OAuthを更新できませんでした。Cloudflareへ再接続してください。".to_string()
        })?;
        return Ok(CloudflareAuthContext {
            account_id: crate::credentials::load(app, CredentialId::CloudflareOAuthAccountId)?,
            access_token: crate::credentials::load(app, CredentialId::CloudflareOAuthAccessToken)?,
            auth_method: CloudflareAuthMethod::OAuth,
        });
    }
    if legacy_configured(app)? {
        return Ok(CloudflareAuthContext {
            account_id: crate::credentials::load(app, CredentialId::CloudflareAccountId)?,
            access_token: crate::credentials::load(app, CredentialId::CloudflareApiToken)?,
            auth_method: CloudflareAuthMethod::ApiToken,
        });
    }
    Err("Cloudflareへ接続してください。".into())
}

#[tauri::command]
pub(crate) fn get_cloudflare_connection_status(
    app: AppHandle,
) -> Result<CloudflareConnectionStatus, String> {
    connection_status(&app)
}

#[tauri::command]
pub(crate) async fn start_cloudflare_oauth(
    app: AppHandle,
    state: State<'_, CloudflareAuthState>,
) -> Result<CloudflareConnectionStatus, String> {
    let _flow_guard = state
        .oauth_flow
        .try_lock()
        .map_err(|_| "Cloudflare OAuthはすでに進行中です。".to_string())?;
    let client_id =
        oauth_client_id().ok_or_else(|| "Cloudflare OAuthが設定されていません。".to_string())?;
    let listener = TcpListener::bind("127.0.0.1:8976").map_err(|_| {
        "OAuth callback用ポート8976を使用できません。ほかの接続処理を終了してください。".to_string()
    })?;
    let (flow, challenge) = create_pending_flow()?;
    let auth_url = authorization_url(client_id, &flow, &challenge)?;
    tauri_plugin_opener::open_url(&auth_url, None::<&str>)
        .map_err(|_| "システムブラウザでCloudflareを開けませんでした。".to_string())?;
    let callback = tauri::async_runtime::spawn_blocking(move || wait_for_callback(listener))
        .await
        .map_err(|_| "OAuth callback処理が停止しました。".to_string())??;
    let code = validate_callback(&callback, &flow)?;
    let client = oauth_http_client()?;
    let token = exchange_authorization_code(&client, client_id, &code, &flow.verifier).await?;
    let refresh_token = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Cloudflareからrefresh tokenを受け取れませんでした。".to_string())?;
    let access = SecretString::from(token.access_token.trim().to_owned());
    let refresh = SecretString::from(refresh_token.to_owned());
    let expires = expiration_from_now(token.expires_in);
    let accounts = fetch_accounts(&client, &access).await?;
    for account in &accounts {
        crate::transcription::cloudflare::validate_credentials(
            &SecretString::from(account.id.clone()),
            &access,
        )
        .await?;
    }
    let accounts_json = serialize_accounts(&accounts)?;
    let mut values = vec![
        (CredentialId::CloudflareOAuthAccessToken, &access),
        (CredentialId::CloudflareOAuthRefreshToken, &refresh),
        (CredentialId::CloudflareOAuthExpiresAt, &expires),
        (CredentialId::CloudflareOAuthAccounts, &accounts_json),
    ];
    let selected_id;
    let selected_name;
    if accounts.len() == 1 {
        selected_id = SecretString::from(accounts[0].id.clone());
        selected_name = SecretString::from(accounts[0].name.clone());
        values.push((CredentialId::CloudflareOAuthAccountId, &selected_id));
        values.push((CredentialId::CloudflareOAuthAccountName, &selected_name));
    }
    persist_verified(&mut AppCredentialStorage(&app), &values)?;
    if accounts.len() > 1 {
        crate::credentials::delete(&app, CredentialId::CloudflareOAuthAccountId)?;
        crate::credentials::delete(&app, CredentialId::CloudflareOAuthAccountName)?;
    }
    connection_status(&app)
}

#[tauri::command]
pub(crate) fn select_cloudflare_oauth_account(
    app: AppHandle,
    account_id: String,
) -> Result<CloudflareConnectionStatus, String> {
    let account = stored_accounts(&app)
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| "選択したCloudflareアカウントは利用できません。".to_string())?;
    let id = SecretString::from(account.id);
    let name = SecretString::from(account.name);
    persist_verified(
        &mut AppCredentialStorage(&app),
        &[
            (CredentialId::CloudflareOAuthAccountId, &id),
            (CredentialId::CloudflareOAuthAccountName, &name),
        ],
    )?;
    connection_status(&app)
}

const OAUTH_CREDENTIALS: [CredentialId; 6] = [
    CredentialId::CloudflareOAuthAccessToken,
    CredentialId::CloudflareOAuthRefreshToken,
    CredentialId::CloudflareOAuthExpiresAt,
    CredentialId::CloudflareOAuthAccountId,
    CredentialId::CloudflareOAuthAccountName,
    CredentialId::CloudflareOAuthAccounts,
];

async fn revoke_refresh_token(
    client: &reqwest::Client,
    revoke_url: &str,
    client_id: &str,
    refresh: &SecretString,
) -> bool {
    client
        .post(revoke_url)
        .form(&[
            ("client_id", client_id),
            ("token_type_hint", "refresh_token"),
            ("token", refresh.expose_secret()),
        ])
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[tauri::command]
pub(crate) async fn disconnect_cloudflare_oauth(app: AppHandle) -> Result<bool, String> {
    let refresh = crate::credentials::load(&app, CredentialId::CloudflareOAuthRefreshToken).ok();
    let revoked = if let (Some(client_id), Some(refresh)) = (oauth_client_id(), refresh) {
        match oauth_http_client() {
            Ok(client) => revoke_refresh_token(&client, REVOKE_URL, client_id, &refresh).await,
            Err(_) => false,
        }
    } else {
        false
    };
    for credential in OAUTH_CREDENTIALS {
        crate::credentials::delete(&app, credential)?;
    }
    Ok(revoked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Default)]
    struct FakeStorage {
        values: HashMap<CredentialId, String>,
        saves: usize,
        fail_at: Option<usize>,
    }

    impl CredentialStorage for FakeStorage {
        fn save(&mut self, id: CredentialId, value: &SecretString) -> Result<(), String> {
            self.saves += 1;
            if self.fail_at == Some(self.saves) {
                return Err("synthetic failure".into());
            }
            self.values.insert(id, value.expose_secret().to_owned());
            Ok(())
        }

        fn has(&self, id: CredentialId) -> Result<bool, String> {
            Ok(self.values.contains_key(&id))
        }

        fn load(&self, id: CredentialId) -> Result<SecretString, String> {
            self.values
                .get(&id)
                .cloned()
                .map(SecretString::from)
                .ok_or_else(|| "missing".into())
        }

        fn delete(&mut self, id: CredentialId) -> Result<(), String> {
            self.values.remove(&id);
            Ok(())
        }
    }

    fn mock_response(status: &str, body: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let status = status.to_owned();
        let body = body.to_owned();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(
                request.contains("grant_type=refresh_token")
                    || request.contains("token_type_hint=refresh_token")
            );
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write response");
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn pkce_is_s256_and_url_safe() {
        let (flow, challenge) = create_pending_flow().expect("create PKCE");
        assert!(flow.verifier.expose_secret().len() >= 43);
        assert!(!challenge.contains('='));
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(flow.verifier.expose_secret().as_bytes()))
        );
    }

    #[test]
    fn callback_requires_matching_state() {
        let (flow, _) = create_pending_flow().expect("create state");
        let matching = Url::parse(&format!(
            "{REDIRECT_URI}?code=synthetic&state={}",
            flow.state
        ))
        .expect("callback URL");
        assert_eq!(
            validate_callback(&matching, &flow).as_deref(),
            Ok("synthetic")
        );
        let mismatch = Url::parse(&format!("{REDIRECT_URI}?code=synthetic&state=wrong"))
            .expect("callback URL");
        assert!(validate_callback(&mismatch, &flow)
            .expect_err("mismatch rejected")
            .contains("一致しない"));
    }

    #[test]
    fn oauth_cancel_is_distinct() {
        let (flow, _) = create_pending_flow().expect("create state");
        let callback = Url::parse(&format!(
            "{REDIRECT_URI}?error=access_denied&state={}",
            flow.state
        ))
        .expect("callback URL");
        assert!(validate_callback(&callback, &flow)
            .expect_err("cancelled")
            .contains("キャンセル"));
    }

    #[test]
    fn token_response_parses_rotation_fields() {
        let token: TokenResponse = serde_json::from_str(
            r#"{"access_token":"access","expires_in":3600,"refresh_token":"refresh","scope":"ai.read offline_access"}"#,
        )
        .expect("token response");
        assert_eq!(token.expires_in, 3600);
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
    }

    #[test]
    fn oauth_credentials_are_atomic_and_rollback_on_failure() {
        let mut storage = FakeStorage {
            values: HashMap::from([
                (
                    CredentialId::CloudflareOAuthAccessToken,
                    "old-access".into(),
                ),
                (
                    CredentialId::CloudflareOAuthRefreshToken,
                    "old-refresh".into(),
                ),
            ]),
            fail_at: Some(2),
            ..Default::default()
        };
        let access = SecretString::from("new-access".to_string());
        let refresh = SecretString::from("new-refresh".to_string());
        persist_verified(
            &mut storage,
            &[
                (CredentialId::CloudflareOAuthAccessToken, &access),
                (CredentialId::CloudflareOAuthRefreshToken, &refresh),
            ],
        )
        .expect_err("second save fails");
        assert_eq!(
            storage
                .values
                .get(&CredentialId::CloudflareOAuthAccessToken)
                .map(String::as_str),
            Some("old-access")
        );
        assert_eq!(
            storage
                .values
                .get(&CredentialId::CloudflareOAuthRefreshToken)
                .map(String::as_str),
            Some("old-refresh")
        );
    }

    #[test]
    fn authorization_url_contains_only_public_flow_values() {
        let (flow, challenge) = create_pending_flow().expect("create flow");
        let url = authorization_url("public-client", &flow, &challenge).expect("auth URL");
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=ai.read+offline_access"));
        assert!(!url.contains(flow.verifier.expose_secret()));
        assert!(!url.contains("client_secret"));
    }

    #[test]
    fn connection_status_serialization_never_contains_secrets() {
        let status = CloudflareConnectionStatus {
            connected: true,
            auth_method: Some("oauth"),
            account_name: Some("Synthetic Account".into()),
            needs_reauthentication: false,
            account_selection_required: false,
            accounts: Vec::new(),
            oauth_configured: true,
            legacy_configured: false,
        };
        let json = serde_json::to_string(&status).expect("serialize status");
        for forbidden in [
            "accessToken",
            "refreshToken",
            "authorizationCode",
            "codeVerifier",
            "synthetic-secret",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn auth_resolver_prefers_oauth_then_legacy() {
        let mut storage = FakeStorage::default();
        assert_eq!(stored_auth_method(&storage).expect("empty resolver"), None);
        storage.values.extend([
            (CredentialId::CloudflareApiToken, "legacy-token".into()),
            (CredentialId::CloudflareAccountId, "legacy-account".into()),
        ]);
        assert_eq!(
            stored_auth_method(&storage).expect("legacy resolver"),
            Some(CloudflareAuthMethod::ApiToken)
        );
        storage.values.extend([
            (CredentialId::CloudflareOAuthAccessToken, "access".into()),
            (CredentialId::CloudflareOAuthRefreshToken, "refresh".into()),
            (CredentialId::CloudflareOAuthExpiresAt, "9999999999".into()),
            (
                CredentialId::CloudflareOAuthAccountId,
                "oauth-account".into(),
            ),
        ]);
        assert_eq!(
            stored_auth_method(&storage).expect("OAuth resolver"),
            Some(CloudflareAuthMethod::OAuth)
        );
    }

    #[test]
    fn expired_tokens_require_refresh() {
        assert!(token_needs_refresh(1_000, 1_000));
        assert!(token_needs_refresh(1_050, 1_000));
        assert!(!token_needs_refresh(1_061, 1_000));
    }

    #[test]
    fn refresh_success_and_failure_are_distinct() {
        let (success_url, success_server) = mock_response(
            "200 OK",
            r#"{"access_token":"rotated-access","expires_in":3600,"refresh_token":"rotated-refresh","scope":"ai.read"}"#,
        );
        let client = oauth_http_client().expect("OAuth client");
        let refresh = SecretString::from("synthetic-refresh".to_string());
        let token = tauri::async_runtime::block_on(request_refresh(
            &client,
            &success_url,
            "public-client",
            &refresh,
        ))
        .expect("refresh succeeds");
        assert_eq!(token.access_token, "rotated-access");
        success_server.join().expect("success server");

        let (failure_url, failure_server) =
            mock_response("400 Bad Request", r#"{"error":"invalid_grant"}"#);
        let error = tauri::async_runtime::block_on(request_refresh(
            &client,
            &failure_url,
            "public-client",
            &refresh,
        ))
        .expect_err("refresh fails");
        assert!(error.contains("再接続"));
        failure_server.join().expect("failure server");
    }

    #[test]
    fn refresh_mutex_is_single_flight() {
        let mutex = Arc::new(tokio::sync::Mutex::new(()));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let run = || {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            let mutex = Arc::clone(&mutex);
            async move {
                let _guard = mutex.lock().await;
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }
        };
        tauri::async_runtime::block_on(futures_util::future::join(run(), run()));
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn revoke_posts_refresh_token_and_delete_clears_all_oauth_values() {
        let (url, server) = mock_response("200 OK", "{}");
        let client = oauth_http_client().expect("OAuth client");
        let refresh = SecretString::from("synthetic-refresh".to_string());
        assert!(tauri::async_runtime::block_on(revoke_refresh_token(
            &client,
            &url,
            "public-client",
            &refresh
        )));
        server.join().expect("revoke server");

        let mut storage = FakeStorage::default();
        for id in OAUTH_CREDENTIALS {
            storage.values.insert(id, "synthetic".into());
        }
        for id in OAUTH_CREDENTIALS {
            storage.delete(id).expect("delete credential");
        }
        assert!(storage.values.is_empty());
    }
}
