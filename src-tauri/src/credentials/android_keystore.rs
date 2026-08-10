use jni::objects::{JClass, JObject, JString, JValue};
use secrecy::{ExposeSecret, SecretString};
use tauri::AppHandle;
use zeroize::Zeroize;

use super::CredentialId;

const BRIDGE: &str = "jp.mutsuna.echo.SecureCredentialBridge";

fn with_env<T>(
    call: impl FnOnce(&mut jni::JNIEnv<'_>, &JObject<'_>, &JClass<'_>) -> Result<T, String>,
) -> Result<T, String> {
    crate::android_context::with_bridge_env(BRIDGE, "Android Keystoreへ接続できませんでした", call)
}

fn call_bool(method: &str, credential: CredentialId) -> Result<bool, String> {
    with_env(|env, app, bridge| {
        let credential = env
            .new_string(credential.id())
            .map_err(|error| format!("認証情報IDを安全な領域へ渡せませんでした: {error}"))?;
        env.call_static_method(
            bridge,
            method,
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[
                JValue::Object(app),
                JValue::Object(&JObject::from(credential)),
            ],
        )
        .and_then(|value| value.z())
        .map_err(|error| format!("Android Keystoreの処理に失敗しました: {error}"))
    })
}

pub(crate) fn save(
    _app: &AppHandle,
    credential: CredentialId,
    api_key: &SecretString,
) -> Result<(), String> {
    with_env(|env, app, bridge| {
        let credential = env
            .new_string(credential.id())
            .map_err(|error| format!("認証情報IDを安全な領域へ渡せませんでした: {error}"))?;
        let key = env
            .new_string(api_key.expose_secret())
            .map_err(|error| format!("APIキーを安全な領域へ渡せませんでした: {error}"))?;
        env.call_static_method(
            bridge,
            "save",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Object(app),
                JValue::Object(&JObject::from(credential)),
                JValue::Object(&JObject::from(key)),
            ],
        )
        .map_err(|error| format!("Android KeystoreにAPIキーを保存できませんでした: {error}"))?;
        Ok(())
    })
}

pub(crate) fn has(_app: &AppHandle, credential: CredentialId) -> Result<bool, String> {
    call_bool("has", credential)
}

pub(crate) fn load(_app: &AppHandle, credential: CredentialId) -> Result<SecretString, String> {
    with_env(|env, app, bridge| {
        let credential_id = env
            .new_string(credential.id())
            .map_err(|error| format!("認証情報IDを安全な領域へ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
                "load",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(app),
                    JValue::Object(&JObject::from(credential_id)),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                format!("Android KeystoreからAPIキーを読み込めませんでした: {error}")
            })?;
        if value.is_null() {
            return Err(format!("{} APIキーが未設定です。", credential.label()));
        }
        let mut plain: String = env
            .get_string(&JString::from(value))
            .map_err(|error| format!("保存済みAPIキーを復号できませんでした: {error}"))?
            .into();
        if plain.is_empty() {
            return Err(format!("{} APIキーが未設定です。", credential.label()));
        }
        let secret = SecretString::from(std::mem::take(&mut plain));
        plain.zeroize();
        Ok(secret)
    })
}

pub(crate) fn delete(_app: &AppHandle, credential: CredentialId) -> Result<(), String> {
    with_env(|env, app, bridge| {
        let credential = env
            .new_string(credential.id())
            .map_err(|error| format!("認証情報IDを安全な領域へ渡せませんでした: {error}"))?;
        env.call_static_method(
            bridge,
            "delete",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[
                JValue::Object(app),
                JValue::Object(&JObject::from(credential)),
            ],
        )
        .map(|_| ())
        .map_err(|error| format!("Android KeystoreからAPIキーを削除できませんでした: {error}"))
    })
}
