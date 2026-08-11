use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AndroidUpdateStatus {
    phase: String,
    checking: bool,
    available_version_code: Option<i32>,
    update_priority: i32,
    flexible_allowed: bool,
    immediate_allowed: bool,
    bytes_downloaded: u64,
    total_bytes: u64,
    error: Option<String>,
}

#[cfg(target_os = "android")]
fn call(method: &str) -> Result<AndroidUpdateStatus, String> {
    use jni::objects::{JString, JValue};

    let json = crate::android_context::with_bridge_env(
        "jp.mutsuna.echo.AppUpdateBridge",
        "Android更新ブリッジへ接続できませんでした",
        |env, app, bridge| {
            let value = env
                .call_static_method(
                    bridge,
                    method,
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(app)],
                )
                .and_then(|value| value.l())
                .map_err(|error| format!("Androidの更新処理に失敗しました: {error}"))?;
            env.get_string(&JString::from(value))
                .map(String::from)
                .map_err(|error| format!("Androidの更新状態を読み取れませんでした: {error}"))
        },
    )?;
    serde_json::from_str(&json).map_err(|error| format!("Androidの更新状態が不正です: {error}"))
}

#[cfg(not(target_os = "android"))]
fn call(_method: &str) -> Result<AndroidUpdateStatus, String> {
    Err("Android版でのみ利用できます。".to_string())
}

#[tauri::command]
pub(crate) fn get_android_update_status() -> Result<AndroidUpdateStatus, String> {
    call("getStatus")
}

#[tauri::command]
pub(crate) fn check_android_update() -> Result<AndroidUpdateStatus, String> {
    call("check")
}

#[tauri::command]
pub(crate) fn start_android_update() -> Result<AndroidUpdateStatus, String> {
    call("start")
}

#[tauri::command]
pub(crate) fn complete_android_update() -> Result<AndroidUpdateStatus, String> {
    call("complete")
}
