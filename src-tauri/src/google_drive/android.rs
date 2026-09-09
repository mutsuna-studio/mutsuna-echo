use jni::objects::JString;
use std::time::{Duration, Instant};

fn call(method: &str) -> Result<Option<String>, String> {
    crate::android_context::with_bridge_env(
        "jp.mutsuna.echo.GoogleDriveBridge",
        "Google認証へ接続できません。",
        |env, _, class| {
            let value = env
                .call_static_method(class, method, "()Ljava/lang/String;", &[])
                .and_then(|v| v.l())
                .map_err(|_| "Google認証ブリッジの処理に失敗しました。")?;
            if value.is_null() {
                return Ok(None);
            }
            let value: String = env
                .get_string(&JString::from(value))
                .map_err(|_| "Google認証の応答を読めません。")?
                .into();
            Ok(Some(value))
        },
    )
}

pub(super) async fn authorize() -> Result<String, String> {
    call("begin")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(180) {
        if let Some(result) = call("takeResult")? {
            return result
                .strip_prefix("ok:")
                .map(str::to_owned)
                .ok_or_else(|| {
                    "Googleへのアクセスが許可されませんでした。アプリを開いて再連携してください。"
                        .into()
                });
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let _ = call("cancel");
    Err("Google認証がタイムアウトしました。再試行してください。".into())
}
