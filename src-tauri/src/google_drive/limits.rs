use reqwest::{RequestBuilder, Response};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(super) const LARGE_AUDIO_BYTES: u64 = 500_000_000;
pub(super) static LIMITS: Limits = Limits::new();

pub(super) struct Limits {
    saved_until: AtomicU64,
    quota_until: AtomicU64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub retry_at: u64,
    pub quota_paused: bool,
}
pub(super) fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

impl Limits {
    const fn new() -> Self {
        Self {
            saved_until: AtomicU64::new(0),
            quota_until: AtomicU64::new(0),
        }
    }
    pub fn status(&self, time: u64) -> Status {
        let quota = self.quota_until.load(Ordering::Relaxed);
        Status {
            retry_at: self.saved_until.load(Ordering::Relaxed).max(quota),
            quota_paused: quota > time,
        }
    }
    pub fn check_save(&self) -> Result<(), String> {
        let time = now();
        let status = self.status(time);
        if status.retry_at > time {
            return Err(format!(
                "{}あと{}秒待ってから保存してください。",
                if status.quota_paused {
                    "Googleの利用制限により送信を一時停止しています。"
                } else {
                    "連続保存を防ぐため、"
                },
                (status.retry_at - time).div_ceil(1000)
            ));
        }
        Ok(())
    }
    pub fn check_quota(&self) -> Result<(), String> {
        if self.quota_until.load(Ordering::Relaxed) > now() {
            self.check_save()?;
        }
        Ok(())
    }
    pub fn saved(&self, time: u64) {
        self.saved_until
            .fetch_max(time.saturating_add(10_000), Ordering::Relaxed);
    }
    fn pause(&self, time: u64, seconds: u64) {
        self.quota_until.fetch_max(
            time.saturating_add(seconds.saturating_mul(1000)),
            Ordering::Relaxed,
        );
    }
}

fn quota_error(status: u16, body: &Value) -> bool {
    if status == 429 {
        return true;
    }
    status == 403
        && body["error"]["errors"].as_array().is_some_and(|errors| {
            errors.iter().any(|e| {
                matches!(
                    e["reason"].as_str(),
                    Some(
                        "rateLimitExceeded"
                            | "userRateLimitExceeded"
                            | "dailyLimitExceeded"
                            | "quotaExceeded"
                            | "downloadQuotaExceeded"
                            | "storageQuotaExceeded"
                    )
                )
            })
        })
}
fn retry_after(header: Option<&str>, time: u64) -> u64 {
    header
        .and_then(|v| {
            v.parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(v).ok().map(|date| {
                    (date.timestamp_millis().max(0) as u64)
                        .saturating_sub(time)
                        .div_ceil(1000)
                })
            })
        })
        .unwrap_or(60)
        .max(60)
}

// Never surface Google's response body: it can contain sensitive request data.
pub(super) async fn check(response: Response, limits: &Limits) -> Result<Response, String> {
    if response.status().as_u16() != 403 && response.status().as_u16() != 429 {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let time = now();
    let seconds = retry_after(
        response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok()),
        time,
    );
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if quota_error(status, &body) {
        limits.pause(time, seconds);
        Err(format!("Googleの利用制限に達したため送信を一時停止しました。{}秒以上待ってから再試行してください。", seconds))
    } else {
        Err("Google Driveへの書き込み権限がありません。APIの有効化、アクセス許可、空き容量を確認してください。".into())
    }
}

#[derive(Default)]
pub(super) struct Retries(u32);
impl Retries {
    pub fn next_delay(&mut self) -> Result<Duration, String> {
        if self.0 >= 3 {
            return Err(
                "通信エラーのため自動再試行を3回で終了しました。時間を置いて再保存してください。"
                    .into(),
            );
        }
        let delay = Duration::from_secs(1 << self.0);
        self.0 += 1;
        Ok(delay)
    }
    pub async fn wait(&mut self) -> Result<(), String> {
        let delay = self.next_delay()?;
        tokio::time::sleep(delay).await;
        Ok(())
    }
}
pub(super) fn transient(result: &Result<Response, reqwest::Error>) -> bool {
    match result {
        Ok(r) => r.status().is_server_error() || r.status().as_u16() == 408,
        Err(e) => e.is_timeout() || e.is_connect() || e.is_body() || e.is_request(),
    }
}

// Only replay reads/idempotent updates. Creation and ambiguous document writes
// retain their existing explicit retry flow rather than creating duplicates.
pub(super) async fn send(request: RequestBuilder) -> Result<Response, String> {
    send_with(request, &LIMITS, false).await
}
async fn send_with(
    request: RequestBuilder,
    limits: &Limits,
    immediate: bool,
) -> Result<Response, String> {
    let mut retries = Retries::default();
    loop {
        limits.check_quota()?;
        let result = request
            .try_clone()
            .ok_or("送信内容を再試行できません。")?
            .send()
            .await;
        if transient(&result) {
            let delay = retries.next_delay()?;
            if !immediate {
                tokio::time::sleep(delay).await;
            }
            continue;
        }
        return check(result.map_err(super::network_error)?, limits).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn cooldown_is_shared_and_only_success_starts_ten_seconds() {
        let limits = Limits::new();
        assert_eq!(limits.status(100).retry_at, 0);
        limits.saved(100);
        assert_eq!(limits.status(101).retry_at, 10100);
        assert!(!limits.status(101).quota_paused);
        limits.pause(200, 120);
        limits.saved(300);
        assert_eq!(limits.status(400).retry_at, 120200);
        assert!(limits.status(400).quota_paused);
        assert!(!limits.status(120200).quota_paused);
    }
    #[test]
    fn retries_back_off_and_stop_at_three() {
        let mut retries = Retries::default();
        for expected in [1, 2, 4] {
            assert_eq!(retries.next_delay().unwrap().as_secs(), expected);
        }
        assert!(retries.next_delay().is_err());
        assert_eq!(retry_after(Some("120"), 0), 120);
        assert_eq!(retry_after(Some("bad"), 0), 60);
        assert_eq!(retry_after(Some("Thu, 01 Jan 1970 00:02:00 GMT"), 0), 120);
    }
    fn mock(statuses: Vec<u16>, body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for status in statuses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let n = socket.read(&mut bytes).unwrap();
                assert!(n > 0, "expected an HTTP request");
                write!(socket,"HTTP/1.1 {status} Test\r\nRetry-After: 120\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (address, worker)
    }
    #[test]
    fn http_recovery_exhaustion_and_quota_stop_without_replay() {
        tauri::async_runtime::block_on(async {
            for (statuses, success) in [(vec![503, 502, 200], true), (vec![503; 4], false)] {
                let (url, server) = mock(statuses, "{}");
                let limits = Limits::new();
                assert_eq!(
                    send_with(super::super::client().unwrap().get(url), &limits, true)
                        .await
                        .is_ok(),
                    success
                );
                server.join().unwrap();
            }
            for status in [403, 429] {
                let (url, server) = mock(
                    vec![status],
                    r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}],"message":"synthetic-sensitive"}}"#,
                );
                let limits = Limits::new();
                let request = super::super::client().unwrap().get(url);
                let error = send_with(request.try_clone().unwrap(), &limits, true)
                    .await
                    .unwrap_err();
                assert!(!error.contains("synthetic-sensitive"));
                assert!(limits.status(now()).quota_paused);
                // No second HTTP request is made during the pause.
                assert!(send_with(request, &limits, true).await.is_err());
                server.join().unwrap();
            }
            let (url, server) = mock(
                vec![403],
                r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#,
            );
            let limits = Limits::new();
            assert!(
                send_with(super::super::client().unwrap().get(url), &limits, true)
                    .await
                    .is_err()
            );
            assert!(!limits.status(now()).quota_paused);
            server.join().unwrap();
        });
    }
}
