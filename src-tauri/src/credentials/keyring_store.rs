use keyring::{Entry, Error as KeyringError};
use tauri::AppHandle;

const CREDENTIAL_SERVICE: &str = "jp.mutsuna.echo";
const CREDENTIAL_USER: &str = "elevenlabs-api-key";

fn credential_error(operation: &str, error: KeyringError) -> String {
    eprintln!("Credential store {operation} failed: {error:?}");
    format!("OSの資格情報ストアで{operation}処理に失敗しました。")
}

fn credential_entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|error| credential_error("初期化", error))
}

pub(crate) fn save_api_key(_app: &AppHandle, api_key: &str) -> Result<(), String> {
    credential_entry()?
        .set_password(api_key)
        .map_err(|error| credential_error("保存", error))
}

pub(crate) fn has_api_key(_app: &AppHandle) -> Result<bool, String> {
    match credential_entry()?.get_password() {
        Ok(api_key) => Ok(!api_key.is_empty()),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(error) => Err(credential_error("読み込み", error)),
    }
}

pub(crate) fn load_api_key(_app: &AppHandle) -> Result<String, String> {
    match credential_entry()?.get_password() {
        Ok(api_key) if !api_key.is_empty() => Ok(api_key),
        Ok(_) | Err(KeyringError::NoEntry) => Err("ElevenLabs APIキーが未設定です。".to_string()),
        Err(error) => Err(credential_error("読み込み", error)),
    }
}

pub(crate) fn delete_api_key(_app: &AppHandle) -> Result<(), String> {
    match credential_entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(credential_error("削除", error)),
    }
}
