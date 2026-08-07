use std::time::Duration;

use reqwest::{header::HeaderValue, redirect::Policy, Client, RequestBuilder};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiErrorKind {
    InvalidApiKey,
    MissingPermissions,
    QuotaExceeded,
    Other,
}

pub(crate) struct ElevenLabsClient {
    http: Client,
    api_key: HeaderValue,
}

impl ElevenLabsClient {
    pub(crate) fn new(api_key: &SecretString, timeout: Duration) -> Result<Self, String> {
        let mut header = HeaderValue::from_str(api_key.expose_secret()).map_err(|_| {
            "ElevenLabs APIキーの形式が不正です。設定し直してください。".to_string()
        })?;
        header.set_sensitive(true);

        let http = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(20))
            .timeout(timeout)
            .build()
            .map_err(|error| {
                eprintln!("Could not build ElevenLabs HTTP client: {error:?}");
                "ElevenLabsへの接続を準備できませんでした。".to_string()
            })?;

        Ok(Self {
            http,
            api_key: header,
        })
    }

    pub(crate) fn get(&self, url: impl reqwest::IntoUrl) -> RequestBuilder {
        self.http
            .get(url)
            .header("xi-api-key", self.api_key.clone())
    }

    pub(crate) fn post(&self, url: impl reqwest::IntoUrl) -> RequestBuilder {
        self.http
            .post(url)
            .header("xi-api-key", self.api_key.clone())
    }
}

pub(crate) fn api_error_kind(body: &Value) -> ApiErrorKind {
    match body.pointer("/detail/status").and_then(Value::as_str) {
        Some("invalid_api_key") => ApiErrorKind::InvalidApiKey,
        Some("missing_permissions") => ApiErrorKind::MissingPermissions,
        Some("quota_exceeded") => ApiErrorKind::QuotaExceeded,
        _ => ApiErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::{api_error_kind, ApiErrorKind};
    use serde_json::json;

    #[test]
    fn classifies_structured_api_errors() {
        assert_eq!(
            api_error_kind(&json!({ "detail": { "status": "missing_permissions" } })),
            ApiErrorKind::MissingPermissions
        );
        assert_eq!(api_error_kind(&json!({})), ApiErrorKind::Other);
    }
}
