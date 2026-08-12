use std::time::{Duration, Instant};

use reqwest::{redirect::Policy, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use url::Url;

use crate::credentials::CredentialId;

const DEFAULT_API_BASE_URL: &str = "https://cloud.mutsuna.jp";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const IDEMPOTENT_REQUEST_ATTEMPTS: usize = 3;
const DEVICE_FLOW_TIMEOUT: Duration = Duration::from_secs(11 * 60);
const ACCESS_TOKEN_MIN_LENGTH: usize = 16;
const ACCESS_TOKEN_MAX_LENGTH: usize = 4_096;
const PREPAID_CREDIT_OFFER_ID: &str = "offer_web_prepaid_hour_v1";

pub(crate) struct MutsunaCloudState {
    connect: tokio::sync::Mutex<()>,
    status: std::sync::RwLock<MutsunaCloudStatus>,
}

impl Default for MutsunaCloudState {
    fn default() -> Self {
        Self {
            connect: tokio::sync::Mutex::new(()),
            status: std::sync::RwLock::new(MutsunaCloudStatus::disconnected()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MutsunaCloudAccountStatus {
    Active,
    ActionRequired,
    Suspended,
    Closed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutsunaCloudStatus {
    connected: bool,
    can_use: bool,
    available_credits: Option<String>,
    account_status: Option<MutsunaCloudAccountStatus>,
}

impl MutsunaCloudStatus {
    pub(crate) fn disconnected() -> Self {
        Self {
            connected: false,
            can_use: false,
            available_credits: None,
            account_status: None,
        }
    }

    pub(crate) const fn connected(&self) -> bool {
        self.connected
    }

    pub(crate) const fn can_use(&self) -> bool {
        self.can_use
    }

    #[cfg(test)]
    pub(crate) fn for_test(can_use: bool) -> Self {
        Self {
            connected: true,
            can_use,
            available_credits: Some("100".into()),
            account_status: Some(MutsunaCloudAccountStatus::Active),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStartRequest {
    client_id: &'static str,
    scopes: [&'static str; 5],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStartResponse {
    device_code: String,
    verification_uri_complete: String,
    expires_at: String,
    poll_interval_seconds: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicePollRequest<'a> {
    device_code: &'a str,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DevicePollResponse {
    Pending {
        #[serde(rename = "retryAfterSeconds")]
        retry_after_seconds: u64,
    },
    SlowDown {
        #[serde(rename = "retryAfterSeconds")]
        retry_after_seconds: u64,
    },
    Authorized {
        #[serde(rename = "tokenType")]
        token_type: String,
        #[serde(rename = "accessToken")]
        access_token: String,
        scopes: Vec<String>,
    },
    Denied,
    Expired,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingSummaryResponse {
    status: MutsunaCloudAccountStatus,
    can_use: bool,
    available_credits: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutSessionResponse {
    checkout_url: String,
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

pub(crate) struct MutsunaCloudSession {
    client: reqwest::Client,
    base_url: Url,
    access_token: SecretString,
}

impl MutsunaCloudSession {
    pub(crate) fn endpoint(&self, path: &str) -> Result<Url, String> {
        endpoint(&self.base_url, path)
    }

    pub(crate) fn request(
        &self,
        method: reqwest::Method,
        url: Url,
    ) -> Result<reqwest::RequestBuilder, String> {
        ensure_same_origin(&self.base_url, &url)?;
        Ok(self
            .client
            .request(method, url)
            .bearer_auth(self.access_token.expose_secret())
            .header(reqwest::header::ACCEPT, "application/json"))
    }

    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        request.send().await.map_err(map_network_error)
    }

    /// Retries a request whose replay semantics are already safe (a frozen
    /// idempotency key or a conditional content upload). `try_clone` preserves
    /// the exact headers/body, so a lost response cannot become a second
    /// logical server mutation.
    pub(crate) async fn send_idempotent(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        let mut next = request;
        for attempt in 0..IDEMPOTENT_REQUEST_ATTEMPTS {
            let retry = next.try_clone();
            match next.send().await {
                Ok(response)
                    if attempt + 1 < IDEMPOTENT_REQUEST_ATTEMPTS
                        && (response.status() == StatusCode::TOO_MANY_REQUESTS
                            || response.status().is_server_error()) =>
                {
                    let Some(cloned) = retry else {
                        return Ok(response);
                    };
                    next = cloned;
                }
                Ok(response) => return Ok(response),
                Err(error) if attempt + 1 < IDEMPOTENT_REQUEST_ATTEMPTS => {
                    let Some(cloned) = retry else {
                        return Err(map_network_error(error));
                    };
                    next = cloned;
                }
                Err(error) => return Err(map_network_error(error)),
            }
            tokio::time::sleep(Duration::from_millis(250 * (attempt as u64 + 1))).await;
        }
        Err("Mutsuna Cloudとの通信を再試行できませんでした。".into())
    }
}

fn api_base_url() -> Result<Url, String> {
    let raw = option_env!("MUTSUNA_CLOUD_API_URL")
        .unwrap_or(DEFAULT_API_BASE_URL)
        .trim();
    let mut url =
        Url::parse(raw).map_err(|_| "Mutsuna Cloudの接続先URLが正しくありません。".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Mutsuna Cloudの接続先は安全なHTTPS URLで指定してください。".into());
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn endpoint(base_url: &Url, path: &str) -> Result<Url, String> {
    base_url
        .join(path)
        .map_err(|_| "Mutsuna CloudのAPI URLを構築できませんでした。".to_string())
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn ensure_same_origin(base_url: &Url, target: &Url) -> Result<(), String> {
    if same_origin(base_url, target) {
        Ok(())
    } else {
        Err("Mutsuna Cloudから安全でない接続先が返されたため処理を中止しました。".into())
    }
}

fn validate_checkout_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value.trim())
        .map_err(|_| "Mutsuna Cloudから購入画面URLを受け取れませんでした。".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("checkout.stripe.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Mutsuna Cloudから安全なStripe購入画面URLを受け取れませんでした。".into());
    }
    Ok(url)
}

pub(crate) fn new_idempotency_key(prefix: &str) -> String {
    // UUID v7 includes 74 random bits sourced from the OS CSPRNG in the uuid
    // crate, while remaining unique and sortable for server-side diagnostics.
    format!("{prefix}-{}", uuid::Uuid::now_v7())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "Mutsuna Cloudへの接続を準備できませんでした。".to_string())
}

pub(crate) fn map_network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Mutsuna Cloudへの接続がタイムアウトしました。".into()
    } else if error.is_connect() {
        "Mutsuna Cloudへ接続できませんでした。DNS・TLS・通信状態を確認してください。".into()
    } else {
        "Mutsuna Cloudとの通信に失敗しました。".into()
    }
}

pub(crate) fn api_status_error(status: StatusCode, action: &str) -> String {
    match status {
        StatusCode::UNAUTHORIZED => {
            "Mutsuna Cloudのログイン期限が切れています。もう一度接続してください。".into()
        }
        StatusCode::FORBIDDEN => {
            "Mutsuna Cloudを利用する権限がありません。アカウント状態を確認してください。".into()
        }
        StatusCode::PAYLOAD_TOO_LARGE => "音声ファイルがMutsuna Cloudの上限を超えています。".into(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "Mutsuna Cloudで利用できない音声形式です。".into(),
        StatusCode::TOO_MANY_REQUESTS => {
            "Mutsuna Cloudが混み合っています。少し待って再試行してください。".into()
        }
        status if status.is_server_error() => {
            format!(
                "Mutsuna Cloudで{action}を完了できませんでした。時間をおいて再試行してください。"
            )
        }
        _ => format!("Mutsuna Cloudで{action}を完了できませんでした（HTTP {status}）。"),
    }
}

fn normalize_access_token(value: String) -> Result<SecretString, String> {
    let received = SecretString::from(value);
    let token = received.expose_secret().trim();
    let valid_characters = token.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'_' | b'~' | b'+' | b'/' | b'-' | b'=')
    });
    if token.len() < ACCESS_TOKEN_MIN_LENGTH
        || token.len() > ACCESS_TOKEN_MAX_LENGTH
        || !valid_characters
    {
        return Err("Mutsuna Cloudから有効なアクセストークンを受け取れませんでした。".into());
    }
    Ok(SecretString::from(token.to_owned()))
}

fn canonical_decimal(value: &str) -> bool {
    if value.is_empty() || value.len() > 40 || !value.is_ascii() {
        return false;
    }
    let (whole, fraction) = value
        .split_once('.')
        .map_or((value, None), |(whole, fraction)| (whole, Some(fraction)));
    let whole_valid = whole == "0"
        || (whole.len() <= 30
            && !whole.starts_with('0')
            && whole.bytes().all(|byte| byte.is_ascii_digit()));
    let fraction_valid = fraction.is_none_or(|fraction| {
        !fraction.is_empty()
            && fraction.len() <= 9
            && fraction.bytes().all(|byte| byte.is_ascii_digit())
    });
    whole_valid && fraction_valid
}

fn status_from_summary(summary: BillingSummaryResponse) -> Result<MutsunaCloudStatus, String> {
    if summary
        .available_credits
        .as_deref()
        .is_some_and(|value| !canonical_decimal(value))
    {
        return Err("Mutsuna Cloudのクレジット残高形式が正しくありません。".into());
    }
    Ok(MutsunaCloudStatus {
        connected: true,
        can_use: summary.can_use,
        available_credits: summary.available_credits,
        account_status: Some(summary.status),
    })
}

async fn fetch_billing_summary(
    client: &reqwest::Client,
    base_url: &Url,
    access_token: &SecretString,
) -> Result<BillingSummaryResponse, String> {
    let response = client
        .get(endpoint(base_url, "/v1/billing/summary")?)
        .bearer_auth(access_token.expose_secret())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(map_network_error)?;
    if !response.status().is_success() {
        return Err(api_status_error(response.status(), "アカウント確認"));
    }
    response
        .json::<BillingSummaryResponse>()
        .await
        .map_err(|_| "Mutsuna Cloudのアカウント情報を読み取れませんでした。".to_string())
}

fn persist_verified<S: CredentialStorage>(
    storage: &mut S,
    id: CredentialId,
    expected: &SecretString,
) -> Result<(), String> {
    let previous = storage.has(id)?.then(|| storage.load(id)).transpose()?;
    let result = (|| {
        storage.save(id, expected)?;
        let actual = storage.load(id)?;
        if actual.expose_secret() != expected.expose_secret() {
            return Err(format!(
                "{}を端末へ正しく保存できませんでした。",
                id.label()
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        match previous {
            Some(value) => storage.save(id, &value)?,
            None => storage.delete(id)?,
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn is_configured(app: &AppHandle) -> Result<bool, String> {
    crate::credentials::has(app, CredentialId::MutsunaCloudAccessToken)
}

pub(crate) fn cached_status(app: &AppHandle) -> Result<MutsunaCloudStatus, String> {
    let configured = is_configured(app)?;
    let state = app.state::<MutsunaCloudState>();
    let cached = state
        .status
        .read()
        .map_err(|_| "Mutsuna Cloudの接続状態を読み取れませんでした。".to_string())?
        .clone();
    if configured {
        if cached.connected {
            Ok(cached)
        } else {
            // Startup has not refreshed the remote summary yet. Stay
            // conservative until the provider command validates the account.
            Ok(MutsunaCloudStatus {
                connected: true,
                can_use: false,
                available_credits: None,
                account_status: None,
            })
        }
    } else {
        Ok(MutsunaCloudStatus::disconnected())
    }
}

fn cache_status(app: &AppHandle, status: &MutsunaCloudStatus) -> Result<(), String> {
    *app.state::<MutsunaCloudState>()
        .status
        .write()
        .map_err(|_| "Mutsuna Cloudの接続状態を更新できませんでした。".to_string())? =
        status.clone();
    Ok(())
}

pub(crate) fn session(app: &AppHandle) -> Result<MutsunaCloudSession, String> {
    let stored = crate::credentials::load(app, CredentialId::MutsunaCloudAccessToken)?;
    let access_token = normalize_access_token(stored.expose_secret().to_owned())?;
    Ok(MutsunaCloudSession {
        client: http_client()?,
        base_url: api_base_url()?,
        access_token,
    })
}

async fn connection_status(app: &AppHandle) -> Result<MutsunaCloudStatus, String> {
    if !is_configured(app)? {
        return Ok(MutsunaCloudStatus::disconnected());
    }
    let session = session(app)?;
    let summary =
        fetch_billing_summary(&session.client, &session.base_url, &session.access_token).await?;
    status_from_summary(summary)
}

pub(crate) async fn refresh_status(app: &AppHandle) -> Result<MutsunaCloudStatus, String> {
    match connection_status(app).await {
        Ok(status) => {
            cache_status(app, &status)?;
            Ok(status)
        }
        Err(error) => {
            let connected = is_configured(app).unwrap_or(false);
            let unavailable = MutsunaCloudStatus {
                connected,
                can_use: false,
                available_credits: None,
                account_status: None,
            };
            let _ = cache_status(app, &unavailable);
            Err(error)
        }
    }
}

async fn post_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: Url,
    body: &impl Serialize,
    action: &str,
) -> Result<T, String> {
    let response = client
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(body)
        .send()
        .await
        .map_err(map_network_error)?;
    if !response.status().is_success() {
        return Err(api_status_error(response.status(), action));
    }
    response
        .json::<T>()
        .await
        .map_err(|_| format!("Mutsuna Cloudの{action}応答を読み取れませんでした。"))
}

async fn poll_for_access_token(
    client: &reqwest::Client,
    base_url: &Url,
    device_code: &SecretString,
    mut poll_after: Duration,
    deadline: Instant,
) -> Result<SecretString, String> {
    loop {
        if Instant::now() >= deadline {
            return Err("Mutsuna Cloudの端末認証がタイムアウトしました。".into());
        }
        tokio::time::sleep(poll_after.min(deadline.saturating_duration_since(Instant::now())))
            .await;
        let result: DevicePollResponse = post_json(
            client,
            endpoint(base_url, "/v1/auth/device/poll")?,
            &DevicePollRequest {
                device_code: device_code.expose_secret(),
            },
            "端末認証確認",
        )
        .await?;
        match result {
            DevicePollResponse::Pending {
                retry_after_seconds,
            }
            | DevicePollResponse::SlowDown {
                retry_after_seconds,
            } => {
                poll_after = Duration::from_secs(retry_after_seconds.clamp(1, 300));
            }
            DevicePollResponse::Denied => {
                return Err("Mutsuna Cloudへの接続がキャンセルされました。".into());
            }
            DevicePollResponse::Expired => {
                return Err("Mutsuna Cloudの端末認証期限が切れました。".into());
            }
            DevicePollResponse::Authorized {
                token_type,
                access_token,
                scopes,
            } => {
                if token_type != "Bearer"
                    || !scopes.iter().any(|scope| scope == "cloud:transcribe")
                    || !scopes.iter().any(|scope| scope == "billing:read")
                {
                    return Err("Mutsuna Cloudの認証権限が不足しています。".into());
                }
                return normalize_access_token(access_token);
            }
        }
    }
}

async fn connect_flow(app: &AppHandle) -> Result<MutsunaCloudStatus, String> {
    let client = http_client()?;
    let base_url = api_base_url()?;
    let started: DeviceStartResponse = post_json(
        &client,
        endpoint(&base_url, "/v1/auth/device/start")?,
        &DeviceStartRequest {
            client_id: "mutsuna-echo-native",
            scopes: [
                "openid",
                "profile",
                "offline_access",
                "cloud:transcribe",
                "billing:read",
            ],
        },
        "端末認証開始",
    )
    .await?;
    let verification_url = Url::parse(started.verification_uri_complete.trim())
        .map_err(|_| "Mutsuna Cloudから認証用URLを受け取れませんでした。".to_string())?;
    ensure_same_origin(&base_url, &verification_url)?;
    if verification_url.scheme() != "https" {
        return Err("Mutsuna Cloudの認証用URLが安全ではありません。".into());
    }
    let expires_at = chrono::DateTime::parse_from_rfc3339(&started.expires_at)
        .map_err(|_| "Mutsuna Cloudの端末認証期限を読み取れませんでした。".to_string())?;
    let remaining = expires_at
        .signed_duration_since(chrono::Utc::now())
        .to_std()
        .map_err(|_| "Mutsuna Cloudの端末認証期限が切れています。".to_string())?;
    if remaining.is_zero() || remaining > DEVICE_FLOW_TIMEOUT {
        return Err("Mutsuna Cloudの端末認証期限が正しくありません。".into());
    }
    let device_code = SecretString::from(started.device_code.trim().to_owned());
    if device_code.expose_secret().len() < ACCESS_TOKEN_MIN_LENGTH
        || device_code.expose_secret().len() > ACCESS_TOKEN_MAX_LENGTH
    {
        return Err("Mutsuna Cloudから有効な端末コードを受け取れませんでした。".into());
    }
    let poll_after = Duration::from_secs(started.poll_interval_seconds.clamp(1, 60));
    tauri_plugin_opener::open_url(verification_url.as_str(), None::<&str>)
        .map_err(|_| "システムブラウザでMutsuna Cloudを開けませんでした。".to_string())?;

    let deadline = Instant::now() + remaining;
    let access_token =
        poll_for_access_token(&client, &base_url, &device_code, poll_after, deadline).await?;
    let summary = fetch_billing_summary(&client, &base_url, &access_token).await?;
    let status = status_from_summary(summary)?;
    persist_verified(
        &mut AppCredentialStorage(app),
        CredentialId::MutsunaCloudAccessToken,
        &access_token,
    )?;
    // Credential persistence is the durable commit. A poisoned in-memory
    // status cache must not turn that commit into an ambiguous failure.
    let _ = cache_status(app, &status);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn get_mutsuna_cloud_status(app: AppHandle) -> Result<MutsunaCloudStatus, String> {
    refresh_status(&app).await
}

#[tauri::command]
pub(crate) async fn connect_mutsuna_cloud(
    app: AppHandle,
    state: State<'_, MutsunaCloudState>,
) -> Result<MutsunaCloudStatus, String> {
    let _guard = state
        .connect
        .try_lock()
        .map_err(|_| "Mutsuna Cloudへの接続はすでに進行中です。".to_string())?;
    tokio::time::timeout(DEVICE_FLOW_TIMEOUT, connect_flow(&app))
        .await
        .map_err(|_| "Mutsuna Cloudの端末認証がタイムアウトしました。".to_string())?
}

#[tauri::command]
pub(crate) fn disconnect_mutsuna_cloud(app: AppHandle) -> Result<MutsunaCloudStatus, String> {
    crate::credentials::delete(&app, CredentialId::MutsunaCloudAccessToken)?;
    let status = MutsunaCloudStatus::disconnected();
    cache_status(&app, &status)?;
    Ok(status)
}

#[tauri::command]
pub(crate) async fn purchase_mutsuna_cloud_credits(app: AppHandle) -> Result<(), String> {
    let session = session(&app)?;
    let request = session
        .request(
            reqwest::Method::POST,
            session.endpoint("/v1/billing/checkout-sessions")?,
        )?
        .header("Idempotency-Key", new_idempotency_key("purchase"))
        .json(&serde_json::json!({ "offerId": PREPAID_CREDIT_OFFER_ID }));
    let response = session.send_idempotent(request).await?;
    if !response.status().is_success() {
        return Err(api_status_error(response.status(), "購入画面の作成"));
    }
    let checkout = response
        .json::<CheckoutSessionResponse>()
        .await
        .map_err(|_| "Mutsuna Cloudの購入画面応答を読み取れませんでした。".to_string())?;
    let checkout_url = validate_checkout_url(&checkout.checkout_url)?;
    tauri_plugin_opener::open_url(checkout_url.as_str(), None::<&str>)
        .map_err(|_| "システムブラウザで購入画面を開けませんでした。".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    use super::*;

    #[derive(Default)]
    struct FakeStorage {
        values: HashMap<CredentialId, String>,
        corrupt_readback: bool,
        save_count: usize,
    }

    impl CredentialStorage for FakeStorage {
        fn save(&mut self, id: CredentialId, value: &SecretString) -> Result<(), String> {
            self.save_count += 1;
            self.values.insert(id, value.expose_secret().to_owned());
            Ok(())
        }

        fn has(&self, id: CredentialId) -> Result<bool, String> {
            Ok(self.values.contains_key(&id))
        }

        fn load(&self, id: CredentialId) -> Result<SecretString, String> {
            if self.corrupt_readback && self.save_count > 0 {
                return Ok(SecretString::from("synthetic-corruption".to_string()));
            }
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

    fn serve_json_once(response_body: &'static str) -> (Url, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server");
        let address = listener.local_addr().expect("server address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            let mut request = [0_u8; 4_096];
            let read = stream.read(&mut request).expect("read request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            )
            .expect("JSON response");
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        (
            Url::parse(&format!("http://{address}")).expect("base URL"),
            server,
        )
    }

    #[test]
    fn access_tokens_are_trimmed_and_blank_tokens_are_rejected() {
        let token = normalize_access_token("  synthetic-access-token-1234\n".into())
            .expect("valid synthetic token");
        assert_eq!(token.expose_secret(), "synthetic-access-token-1234");
        assert!(normalize_access_token(" \t\n".into()).is_err());
    }

    #[test]
    fn verified_token_persistence_round_trips_exactly() {
        let mut storage = FakeStorage::default();
        let token = SecretString::from("synthetic-access-token-1234".to_string());
        persist_verified(&mut storage, CredentialId::MutsunaCloudAccessToken, &token)
            .expect("verified persistence");
        assert_eq!(
            storage
                .values
                .get(&CredentialId::MutsunaCloudAccessToken)
                .map(String::as_str),
            Some("synthetic-access-token-1234")
        );
    }

    #[test]
    fn readback_failure_rolls_back_the_previous_token() {
        let mut storage = FakeStorage {
            values: HashMap::from([(
                CredentialId::MutsunaCloudAccessToken,
                "synthetic-previous-token-1234".into(),
            )]),
            corrupt_readback: false,
            save_count: 0,
        };
        let token = SecretString::from("synthetic-replacement-token-1234".to_string());
        storage.corrupt_readback = true;
        persist_verified(&mut storage, CredentialId::MutsunaCloudAccessToken, &token)
            .expect_err("corrupt readback must fail");
        assert_eq!(
            storage
                .values
                .get(&CredentialId::MutsunaCloudAccessToken)
                .map(String::as_str),
            Some("synthetic-previous-token-1234")
        );
    }

    #[test]
    fn first_save_readback_failure_removes_the_unverified_token() {
        let mut storage = FakeStorage {
            corrupt_readback: true,
            ..FakeStorage::default()
        };
        let token = SecretString::from("synthetic-first-token-1234".to_string());
        persist_verified(&mut storage, CredentialId::MutsunaCloudAccessToken, &token)
            .expect_err("corrupt readback must fail");
        assert!(!storage
            .values
            .contains_key(&CredentialId::MutsunaCloudAccessToken));
    }

    #[test]
    fn device_poll_facade_returns_a_valid_scoped_token() {
        let (base_url, server) = serve_json_once(
            r#"{"status":"authorized","tokenType":"Bearer","accessToken":"synthetic-authorized-token-1234","scopes":["cloud:transcribe","billing:read"]}"#,
        );
        let client = http_client().expect("client");
        let device_code = SecretString::from("synthetic-device-code-1234".to_string());
        let token = tauri::async_runtime::block_on(poll_for_access_token(
            &client,
            &base_url,
            &device_code,
            Duration::ZERO,
            Instant::now() + Duration::from_secs(2),
        ))
        .expect("authorized token");
        assert_eq!(token.expose_secret(), "synthetic-authorized-token-1234");
        let request = server.join().expect("server join");
        assert!(request.starts_with("POST /v1/auth/device/poll HTTP/1.1"));
        assert!(request.contains(r#"{"deviceCode":"synthetic-device-code-1234"}"#));
    }

    #[test]
    fn device_poll_rejects_a_token_without_billing_scope() {
        let (base_url, server) = serve_json_once(
            r#"{"status":"authorized","tokenType":"Bearer","accessToken":"synthetic-unscoped-token-1234","scopes":["cloud:transcribe"]}"#,
        );
        let client = http_client().expect("client");
        let device_code = SecretString::from("synthetic-device-code-1234".to_string());
        let result = tauri::async_runtime::block_on(poll_for_access_token(
            &client,
            &base_url,
            &device_code,
            Duration::ZERO,
            Instant::now() + Duration::from_secs(2),
        ));
        assert!(result.is_err());
        server.join().expect("server join");
    }

    #[test]
    fn billing_summary_validation_does_not_follow_redirects() {
        let redirect_server = TcpListener::bind("127.0.0.1:0").expect("redirect server");
        let destination_server = TcpListener::bind("127.0.0.1:0").expect("destination server");
        destination_server
            .set_nonblocking(true)
            .expect("nonblocking destination");
        let redirect_address = redirect_server.local_addr().expect("redirect address");
        let destination_address = destination_server
            .local_addr()
            .expect("destination address");
        let (ready_tx, ready_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            ready_tx.send(()).expect("ready");
            let (mut stream, _) = redirect_server.accept().expect("request");
            let mut request = [0_u8; 2_048];
            let read = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.contains("authorization: Bearer synthetic-access-token-1234"));
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://{destination_address}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("redirect response");
        });
        ready_rx.recv().expect("server ready");
        let client = http_client().expect("client");
        let base = Url::parse(&format!("http://{redirect_address}")).expect("base URL");
        let token = SecretString::from("synthetic-access-token-1234".to_string());
        let result = tauri::async_runtime::block_on(fetch_billing_summary(&client, &base, &token));
        assert!(result.is_err());
        server.join().expect("server join");
        std::thread::sleep(Duration::from_millis(50));
        assert!(destination_server.accept().is_err());
    }

    #[test]
    fn summary_status_exposes_only_canonical_safe_values() {
        let status = status_from_summary(BillingSummaryResponse {
            status: MutsunaCloudAccountStatus::Active,
            can_use: true,
            available_credits: Some("1800.25".into()),
        })
        .expect("valid summary");
        assert_eq!(
            serde_json::to_value(status).expect("serialize status"),
            serde_json::json!({
                "connected": true,
                "canUse": true,
                "availableCredits": "1800.25",
                "accountStatus": "active"
            })
        );
        assert!(status_from_summary(BillingSummaryResponse {
            status: MutsunaCloudAccountStatus::Active,
            can_use: true,
            available_credits: Some("01".into()),
        })
        .is_err());
    }

    #[test]
    fn checkout_urls_are_restricted_to_stripe_https() {
        assert!(validate_checkout_url("https://checkout.stripe.com/c/pay/synthetic").is_ok());
        assert!(validate_checkout_url("http://checkout.stripe.com/c/pay/synthetic").is_err());
        assert!(validate_checkout_url("https://checkout.stripe.com:444/c/pay/synthetic").is_err());
        assert!(validate_checkout_url("https://checkout.stripe.com.attacker.test/x").is_err());
        assert!(validate_checkout_url("https://token@checkout.stripe.com/x").is_err());
    }

    #[test]
    fn purchase_idempotency_keys_contain_a_valid_uuid() {
        let key = new_idempotency_key("purchase");
        let uuid = key.strip_prefix("purchase-").expect("key prefix");
        assert!(uuid::Uuid::parse_str(uuid).is_ok());
        assert!(key.len() >= 16 && key.len() <= 128);
    }

    #[test]
    fn idempotent_requests_replay_the_exact_key_after_a_lost_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("retry server");
        let address = listener.local_addr().expect("retry address");
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("retry request");
                let mut request = [0_u8; 4_096];
                let read = stream.read(&mut request).expect("read retry request");
                requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
                if attempt == 1 {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                    )
                    .expect("retry response");
                }
            }
            requests
        });
        let base_url = Url::parse(&format!("http://{address}")).expect("retry base URL");
        let session = MutsunaCloudSession {
            client: http_client().expect("client"),
            base_url: base_url.clone(),
            access_token: SecretString::from("synthetic-access-token-1234".to_string()),
        };
        let request = session
            .request(
                reqwest::Method::POST,
                base_url.join("/idempotent").expect("retry URL"),
            )
            .expect("same origin")
            .header("Idempotency-Key", "synthetic-retry-key-0001")
            .json(&serde_json::json!({ "operation": "synthetic" }));
        let response = tauri::async_runtime::block_on(session.send_idempotent(request))
            .expect("retry response");
        assert_eq!(response.status(), StatusCode::OK);
        let requests = server.join().expect("retry server join");
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert!(request.contains("idempotency-key: synthetic-retry-key-0001"));
            assert!(request.contains(r#"{"operation":"synthetic"}"#));
        }
    }
}
