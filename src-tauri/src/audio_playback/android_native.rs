use jni::objects::{JClass, JObject, JString, JValue};
use serde::Deserialize;

const BRIDGE: &str = "jp.mutsuna.echo.AudioPlaybackBridge";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePlaybackState {
    pub(super) loaded: bool,
    pub(super) playing: bool,
    pub(super) position_ms: u64,
    pub(super) duration_ms: u64,
    pub(super) buffered_position_ms: u64,
    pub(super) buffering: bool,
    pub(super) ended: bool,
    pub(super) error: Option<String>,
}

enum Argument<'a> {
    None,
    String(&'a str),
    Long(i64),
    Float(f32),
}

fn call(
    method: &str,
    signature: &str,
    argument: Argument<'_>,
) -> Result<NativePlaybackState, String> {
    let json = crate::android_context::with_bridge_env(
        BRIDGE,
        "Android音声再生ブリッジへ接続できませんでした",
        |env, app, bridge: &JClass<'_>| {
            let value = match argument {
                Argument::None => {
                    env.call_static_method(bridge, method, signature, &[JValue::Object(app)])
                }
                Argument::Long(value) => env.call_static_method(
                    bridge,
                    method,
                    signature,
                    &[JValue::Object(app), JValue::Long(value)],
                ),
                Argument::Float(value) => env.call_static_method(
                    bridge,
                    method,
                    signature,
                    &[JValue::Object(app), JValue::Float(value)],
                ),
                Argument::String(value) => {
                    let value = env.new_string(value).map_err(|error| {
                        format!("音声ファイルの場所をAndroidへ渡せませんでした: {error}")
                    })?;
                    let value = JObject::from(value);
                    env.call_static_method(
                        bridge,
                        method,
                        signature,
                        &[JValue::Object(app), JValue::Object(&value)],
                    )
                }
            }
            .and_then(|value| value.l())
            .map_err(|error| format!("Android音声再生処理を実行できませんでした: {error}"))?;
            env.get_string(&JString::from(value))
                .map(String::from)
                .map_err(|error| format!("Android音声再生状態を読み取れませんでした: {error}"))
        },
    )?;
    serde_json::from_str(&json)
        .map_err(|error| format!("Android音声再生状態の応答が不正です: {error}"))
}

pub(super) fn load(path: &str) -> Result<NativePlaybackState, String> {
    call(
        "load",
        "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
        Argument::String(path),
    )
}

pub(super) fn simple(method: &str) -> Result<NativePlaybackState, String> {
    call(
        method,
        "(Landroid/content/Context;)Ljava/lang/String;",
        Argument::None,
    )
}

pub(super) fn seek(position_ms: u64) -> Result<NativePlaybackState, String> {
    let position_ms = i64::try_from(position_ms).unwrap_or(i64::MAX);
    call(
        "seekTo",
        "(Landroid/content/Context;J)Ljava/lang/String;",
        Argument::Long(position_ms),
    )
}

pub(super) fn set_float(method: &str, value: f32) -> Result<NativePlaybackState, String> {
    call(
        method,
        "(Landroid/content/Context;F)Ljava/lang/String;",
        Argument::Float(value),
    )
}
