use jni::{
    objects::{JObject, JString, JValue},
    JavaVM,
};
use secrecy::{ExposeSecret, SecretString};
use tauri::AppHandle;
use zeroize::Zeroize;

const BRIDGE: &str = "jp/mutsuna/echo/SecureCredentialBridge";

fn with_env<T>(
    call: impl FnOnce(&mut jni::JNIEnv<'_>, &JObject<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let context = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("Android Keystoreを初期化できませんでした: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("Android Keystoreへ接続できませんでした: {error}"))?;
    let app = unsafe { JObject::from_raw(context.context().cast()) };
    call(&mut env, &app)
}

fn call_bool(method: &str) -> Result<bool, String> {
    with_env(|env, app| {
        env.call_static_method(
            BRIDGE,
            method,
            "(Landroid/content/Context;)Z",
            &[JValue::Object(app)],
        )
        .and_then(|value| value.z())
        .map_err(|error| format!("Android Keystoreの処理に失敗しました: {error}"))
    })
}

pub(crate) fn save_api_key(_app: &AppHandle, api_key: &SecretString) -> Result<(), String> {
    with_env(|env, app| {
        let key = env
            .new_string(api_key.expose_secret())
            .map_err(|error| format!("APIキーを安全な領域へ渡せませんでした: {error}"))?;
        env.call_static_method(
            BRIDGE,
            "save",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[JValue::Object(app), JValue::Object(&JObject::from(key))],
        )
        .map_err(|error| format!("Android KeystoreにAPIキーを保存できませんでした: {error}"))?;
        Ok(())
    })
}

pub(crate) fn has_api_key(_app: &AppHandle) -> Result<bool, String> {
    call_bool("has")
}

pub(crate) fn load_api_key(_app: &AppHandle) -> Result<SecretString, String> {
    with_env(|env, app| {
        let value = env
            .call_static_method(
                BRIDGE,
                "load",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(app)],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                format!("Android KeystoreからAPIキーを読み込めませんでした: {error}")
            })?;
        if value.is_null() {
            return Err("ElevenLabs APIキーが未設定です。".into());
        }
        let mut plain: String = env
            .get_string(&JString::from(value))
            .map_err(|error| format!("保存済みAPIキーを復号できませんでした: {error}"))?
            .into();
        if plain.is_empty() {
            return Err("ElevenLabs APIキーが未設定です。".into());
        }
        let secret = SecretString::from(std::mem::take(&mut plain));
        plain.zeroize();
        Ok(secret)
    })
}

pub(crate) fn delete_api_key(_app: &AppHandle) -> Result<(), String> {
    with_env(|env, app| {
        env.call_static_method(
            BRIDGE,
            "delete",
            "(Landroid/content/Context;)V",
            &[JValue::Object(app)],
        )
        .map(|_| ())
        .map_err(|error| format!("Android KeystoreからAPIキーを削除できませんでした: {error}"))
    })
}
