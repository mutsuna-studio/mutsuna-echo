#[cfg(target_os = "android")]
mod android_keystore;
#[cfg(all(test, not(target_os = "android")))]
#[allow(dead_code)]
mod android_keystore;
#[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
mod keyring_store;
#[cfg(target_os = "windows")]
mod windows_dpapi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialId {
    ElevenLabs,
    Soniox,
    CloudflareApiToken,
    CloudflareAccountId,
}

impl CredentialId {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::ElevenLabs => "elevenlabs",
            Self::Soniox => "soniox",
            Self::CloudflareApiToken => "cloudflare-api-token",
            Self::CloudflareAccountId => "cloudflare-account-id",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ElevenLabs => "ElevenLabs",
            Self::Soniox => "Soniox",
            Self::CloudflareApiToken => "Cloudflare APIトークン",
            Self::CloudflareAccountId => "Cloudflare Account ID",
        }
    }

    pub(crate) fn from_provider_id(provider_id: &str) -> Result<Self, String> {
        match provider_id {
            "elevenlabs" => Ok(Self::ElevenLabs),
            "soniox" => Ok(Self::Soniox),
            "cloudflare" => Ok(Self::CloudflareApiToken),
            _ => Err("APIキーを保存できないプロバイダーです。".into()),
        }
    }
}

#[cfg(target_os = "android")]
use android_keystore as store;
#[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
use keyring_store as store;
#[cfg(target_os = "windows")]
use windows_dpapi as store;

use secrecy::SecretString;
use tauri::AppHandle;

pub(crate) fn save(
    app: &AppHandle,
    credential: CredentialId,
    api_key: &SecretString,
) -> Result<(), String> {
    store::save(app, credential, api_key)
}

pub(crate) fn has(app: &AppHandle, credential: CredentialId) -> Result<bool, String> {
    store::has(app, credential)
}

pub(crate) fn load(app: &AppHandle, credential: CredentialId) -> Result<SecretString, String> {
    store::load(app, credential)
}

pub(crate) fn delete(app: &AppHandle, credential: CredentialId) -> Result<(), String> {
    store::delete(app, credential)
}

pub(crate) fn has_api_key(app: &AppHandle) -> Result<bool, String> {
    has(app, CredentialId::ElevenLabs)
}

pub(crate) fn load_api_key(app: &AppHandle) -> Result<SecretString, String> {
    load(app, CredentialId::ElevenLabs)
}
