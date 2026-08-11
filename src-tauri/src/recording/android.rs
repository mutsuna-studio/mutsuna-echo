use jni::objects::{JClass, JObject, JString, JValue};

use super::types::{
    RecordedAudioSummary, RecordingCapabilities, RecordingStatus, StartRecordingRequest, CHANNELS,
    FINAL_BITRATE, MAX_DURATION_MS, SAMPLE_RATE,
};

const BRIDGE: &str = "jp.mutsuna.echo.RecordingBridge";

fn with_env<T>(
    call: impl FnOnce(&mut jni::JNIEnv<'_>, &JObject<'_>, &JClass<'_>) -> Result<T, String>,
) -> Result<T, String> {
    crate::android_context::with_bridge_env(
        BRIDGE,
        "Android録音ブリッジへ接続できませんでした",
        call,
    )
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
    let json = with_env(|env, app, bridge| {
        let value = env
            .call_static_method(
                bridge,
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
    let status: RecordingStatus = serde_json::from_str(&json)
        .map_err(|error| format!("Android録音状態の応答が不正です: {error}"))?;
    if status.phase == super::types::RecordingPhase::Completed
        && status
            .output_path
            .as_deref()
            .is_none_or(|path| !std::path::Path::new(path).is_file())
    {
        clear_completed_status()?;
        return Ok(RecordingStatus::default());
    }
    Ok(status)
}

pub fn clear_completed_status() -> Result<(), String> {
    with_env(|env, app, bridge| {
        env.call_static_method(
            bridge,
            "clearCompletedStatus",
            "(Landroid/content/Context;)V",
            &[JValue::Object(app)],
        )
        .map(|_| ())
        .map_err(|error| format!("Android録音の完了状態を初期化できませんでした: {error}"))
    })
}

pub fn start(request: &StartRecordingRequest) -> Result<RecordingStatus, String> {
    request.validate()?;
    let config = serde_json::to_string(request)
        .map_err(|error| format!("録音設定を準備できませんでした: {error}"))?;
    let json = with_env(|env, app, bridge| {
        let config = env
            .new_string(config)
            .map_err(|error| format!("録音設定をAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
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

pub fn start_monitor(request: &StartRecordingRequest) -> Result<RecordingStatus, String> {
    request.validate()?;
    let config = serde_json::to_string(request)
        .map_err(|error| format!("入力確認設定を準備できませんでした: {error}"))?;
    let json = with_env(|env, app, bridge| {
        let config = env
            .new_string(config)
            .map_err(|error| format!("入力確認設定をAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
                "startMonitor",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(app), JValue::Object(&JObject::from(config))],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Androidの入力確認を開始できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("Androidの入力確認状態を読み取れませんでした: {error}"))
    })?;
    serde_json::from_str(&json).map_err(|error| format!("Androidの入力確認状態が不正です: {error}"))
}

pub fn stop_monitor() -> Result<(), String> {
    with_env(|env, app, bridge| {
        env.call_static_method(
            bridge,
            "stopMonitor",
            "(Landroid/content/Context;)V",
            &[JValue::Object(app)],
        )
        .map(|_| ())
        .map_err(|error| format!("Androidの入力確認を停止できませんでした: {error}"))
    })
}

pub fn stop(cancel: bool) -> Result<RecordingStatus, String> {
    let json = with_env(|env, app, bridge| {
        let value = env
            .call_static_method(
                bridge,
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
    let path = with_env(|env, app, bridge| {
        let uri = env
            .new_string(uri)
            .map_err(|error| format!("選択した音声URIをAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
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

pub fn completed_recordings() -> Result<Vec<RecordedAudioSummary>, String> {
    let json = with_env(|env, app, bridge| {
        let value = env
            .call_static_method(
                bridge,
                "listCompletedRecordings",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(app)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Androidの録音履歴を取得できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("Androidの録音履歴を読み取れませんでした: {error}"))
    })?;
    serde_json::from_str(&json)
        .map_err(|error| format!("Androidの録音履歴の応答が不正です: {error}"))
}

pub fn reveal_recording_folder() -> Result<(), String> {
    with_env(|env, app, bridge| {
        env.call_static_method(
            bridge,
            "openRecordingFolder",
            "(Landroid/content/Context;)V",
            &[JValue::Object(app)],
        )
        .map(|_| ())
        .map_err(|error| format!("Androidの録音保存場所を開けませんでした: {error}"))
    })
}

pub fn rename_completed_recording(recording_id: &str, new_file_name: &str) -> Result<(), String> {
    with_env(|env, app, bridge| {
        let recording_id = env
            .new_string(recording_id)
            .map_err(|error| format!("録音IDをAndroidへ渡せませんでした: {error}"))?;
        let new_file_name = env
            .new_string(new_file_name)
            .map_err(|error| format!("ファイル名をAndroidへ渡せませんでした: {error}"))?;
        env.call_static_method(
            bridge,
            "renameCompletedRecording",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Object(app),
                JValue::Object(&JObject::from(recording_id)),
                JValue::Object(&JObject::from(new_file_name)),
            ],
        )
        .map(|_| ())
        .map_err(|error| format!("Androidの録音ファイル名を変更できませんでした: {error}"))
    })
}

pub fn delete_completed_recording(recording_id: &str) -> Result<(), String> {
    with_env(|env, app, bridge| {
        let recording_id = env
            .new_string(recording_id)
            .map_err(|error| format!("削除する録音IDをAndroidへ渡せませんでした: {error}"))?;
        env.call_static_method(
            bridge,
            "deleteCompletedRecording",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[
                JValue::Object(app),
                JValue::Object(&JObject::from(recording_id)),
            ],
        )
        .map(|_| ())
        .map_err(|error| format!("Androidの録音ファイルを削除できませんでした: {error}"))
    })
}

pub fn copy_completed_recording(recording_id: &str) -> Result<std::path::PathBuf, String> {
    let path = with_env(|env, app, bridge| {
        let recording_id = env
            .new_string(recording_id)
            .map_err(|error| format!("選択した録音IDをAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
                "copyCompletedRecording",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(app),
                    JValue::Object(&JObject::from(recording_id)),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Androidの録音を選択できませんでした: {error}"))?;
        env.get_string(&JString::from(value))
            .map(String::from)
            .map_err(|error| format!("選択した録音の場所を読み取れませんでした: {error}"))
    })?;
    Ok(std::path::PathBuf::from(path))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredRecording {
    pub path: std::path::PathBuf,
    pub microphone_track_path: Option<std::path::PathBuf>,
    pub system_track_path: Option<std::path::PathBuf>,
}

pub fn recover(session_id: &str) -> Result<RecoveredRecording, String> {
    let json = with_env(|env, app, bridge| {
        let session_id = env
            .new_string(session_id)
            .map_err(|error| format!("復旧する録音IDをAndroidへ渡せませんでした: {error}"))?;
        let value = env
            .call_static_method(
                bridge,
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
    serde_json::from_str(&json)
        .map_err(|error| format!("復旧した録音トラックの応答が不正です: {error}"))
}
