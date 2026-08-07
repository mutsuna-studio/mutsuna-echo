use jni::{
    objects::{JObject, JString, JValue},
    JavaVM,
};

use super::types::{
    RecordingCapabilities, RecordingStatus, StartRecordingRequest, CHANNELS, FINAL_BITRATE,
    MAX_DURATION_MS, SAMPLE_RATE,
};

const BRIDGE: &str = "jp/mutsuna/echo/RecordingBridge";

fn with_env<T>(
    call: impl FnOnce(&mut jni::JNIEnv<'_>, &JObject<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let context = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("Android録音ブリッジを初期化できませんでした: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("Android録音ブリッジへ接続できませんでした: {error}"))?;
    let app = unsafe { JObject::from_raw(context.context().cast()) };
    call(&mut env, &app)
}

pub fn capabilities() -> Result<RecordingCapabilities, String> {
    Ok(RecordingCapabilities {
        platform: "android", supported: true, microphone_supported: true,
        system_audio_supported: true, system_audio_limited: true,
        limitation: Some("Androidでは、再生元アプリが録音を許可した音声だけ取得できます。通話・DRM保護音声などは取得できません。また、他アプリがマイクを占有すると録音が停止することがあります。"),
        microphone_devices: Vec::new(), system_devices: Vec::new(), sample_rate: SAMPLE_RATE,
        channels: CHANNELS, codec: "AAC-LC", bitrate: FINAL_BITRATE, max_duration_ms: MAX_DURATION_MS,
    })
}

pub fn status() -> Result<RecordingStatus, String> {
    let json = with_env(|env, app| {
        let value = env
            .call_static_method(
                BRIDGE,
                "getStatus",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(app)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Android録音状態を取得できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("Android録音状態を読み取れませんでした: {error}"))
    })?;
    serde_json::from_str(&json).map_err(|error| format!("Android録音状態の応答が不正です: {error}"))
}

pub fn start(request: &StartRecordingRequest) -> Result<RecordingStatus, String> {
    request.validate()?;
    let config = serde_json::to_string(request)
        .map_err(|error| format!("録音設定を準備できませんでした: {error}"))?;
    let json = with_env(|env, app| {
        let config = env
            .new_string(config)
            .map_err(|error| format!("録音設定をAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                BRIDGE,
                "start",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(app), JValue::Object(&JObject::from(config))],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Android録音を開始できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("Android録音状態を読み取れませんでした: {error}"))
    })?;
    serde_json::from_str(&json).map_err(|error| format!("Android録音状態の応答が不正です: {error}"))
}

pub fn stop(cancel: bool) -> Result<RecordingStatus, String> {
    let json = with_env(|env, app| {
        let value = env
            .call_static_method(
                BRIDGE,
                "stop",
                "(Landroid/content/Context;Z)Ljava/lang/String;",
                &[JValue::Object(app), JValue::Bool(cancel.into())],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Android録音を停止できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("Android録音状態を読み取れませんでした: {error}"))
    })?;
    serde_json::from_str(&json).map_err(|error| format!("Android録音状態の応答が不正です: {error}"))
}

pub fn copy_content_uri(uri: &str) -> Result<std::path::PathBuf, String> {
    let path = with_env(|env, app| {
        let uri = env
            .new_string(uri)
            .map_err(|error| format!("選択した音声URIをAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                BRIDGE,
                "copyContentUri",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(app), JValue::Object(&JObject::from(uri))],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                format!("選択した音声をアプリ領域へコピーできませんでした: {error}")
            })?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("コピーした音声の場所を読み取れませんでした: {error}"))
    })?;
    Ok(std::path::PathBuf::from(path))
}

pub fn recover(session_id: &str) -> Result<std::path::PathBuf, String> {
    let path = with_env(|env, app| {
        let session_id = env
            .new_string(session_id)
            .map_err(|error| format!("復旧する録音IDをAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                BRIDGE,
                "recover",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(app),
                    JValue::Object(&JObject::from(session_id)),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Androidの録音を復旧できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("復旧した録音の場所を読み取れませんでした: {error}"))
    })?;
    Ok(std::path::PathBuf::from(path))
}
