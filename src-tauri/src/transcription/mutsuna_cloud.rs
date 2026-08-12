use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::Manager;

use super::{audio_decode, Transcript, TranscriptSegment, TranscriptionOutcome};
use crate::commands::transcribe::{
    publish_transcription_progress, TranscriptionProgress, TranscriptionStage,
};

pub(crate) const MODEL_ID: &str = "mutsuna-stt-standard-v1";
const MAX_AUDIO_DURATION_MS: u64 = 5 * 60 * 1_000;
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const MAX_JSON_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const UPLOAD_SAMPLE_RATE: u32 = 16_000;
const JOB_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateJobResponse {
    job_id: String,
    uploads: Vec<UploadTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadTarget {
    track_id: String,
    method: String,
    upload_url: String,
    max_bytes: u64,
    accepted_content_types: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobStatusResponse {
    status: String,
    progress_percent: Option<u32>,
    error: Option<JobProblem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobProblem {
    instance: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobResultResponse {
    model: String,
    result: CloudTranscript,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudTranscript {
    duration_ms: u64,
    text: String,
    tracks: Vec<CloudTrack>,
    segments: Vec<CloudSegment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudTrack {
    track_id: String,
    detected_language: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudSegment {
    track_id: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
}

struct AudioWorkspace(PathBuf);

impl AudioWorkspace {
    fn create(app: &tauri::AppHandle) -> Result<Self, String> {
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|_| "Mutsuna Cloud向け音声の一時保存先を取得できませんでした。".to_string())?;
        let path = cache.join(format!("mutsuna-cloud-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&path)
            .map_err(|_| "Mutsuna Cloud向け音声の一時領域を作成できませんでした。".to_string())?;
        Ok(Self(path))
    }
}

impl Drop for AudioWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    let data_size = samples
        .len()
        .checked_mul(2)
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| "変換後の音声がMutsuna Cloudの上限を超えています。".to_string())?;
    let mut file = File::create(path)
        .map_err(|_| "Mutsuna Cloud向け音声ファイルを作成できませんでした。".to_string())?;
    file.write_all(b"RIFF")
        .and_then(|_| file.write_all(&(36u32.saturating_add(data_size)).to_le_bytes()))
        .and_then(|_| file.write_all(b"WAVEfmt "))
        .and_then(|_| file.write_all(&16u32.to_le_bytes()))
        .and_then(|_| file.write_all(&1u16.to_le_bytes()))
        .and_then(|_| file.write_all(&1u16.to_le_bytes()))
        .and_then(|_| file.write_all(&sample_rate.to_le_bytes()))
        .and_then(|_| file.write_all(&(sample_rate.saturating_mul(2)).to_le_bytes()))
        .and_then(|_| file.write_all(&2u16.to_le_bytes()))
        .and_then(|_| file.write_all(&16u16.to_le_bytes()))
        .and_then(|_| file.write_all(b"data"))
        .and_then(|_| file.write_all(&data_size.to_le_bytes()))
        .map_err(|_| "Mutsuna Cloud向け音声ヘッダーを書き込めませんでした。".to_string())?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        file.write_all(&value.to_le_bytes())
            .map_err(|_| "Mutsuna Cloud向け音声を書き込めませんでした。".to_string())?;
    }
    file.flush()
        .map_err(|_| "Mutsuna Cloud向け音声を保存できませんでした。".to_string())
}

fn convert_audio(input: &Path, output: &Path, duration_ms: u64) -> Result<(), String> {
    let mut wrote = false;
    audio_decode::decode_mono_regions_resampled(
        input,
        UPLOAD_SAMPLE_RATE,
        &[(0, duration_ms)],
        |_, sample_rate, samples| {
            if wrote {
                return Err("Mutsuna Cloud向け音声トラックを1つに変換できませんでした。".into());
            }
            write_pcm16_wav(output, sample_rate, samples)?;
            wrote = true;
            Ok(())
        },
    )?;
    if !wrote {
        return Err("Mutsuna Cloudへ送信できる音声が見つかりませんでした。".into());
    }
    Ok(())
}

async fn bounded_json<T: DeserializeOwned>(
    response: reqwest::Response,
    action: &str,
) -> Result<T, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_JSON_RESPONSE_BYTES as u64)
    {
        return Err(format!("Mutsuna Cloudの{action}応答が大きすぎます。"));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(crate::mutsuna_cloud::map_network_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_JSON_RESPONSE_BYTES {
            return Err(format!("Mutsuna Cloudの{action}応答が大きすぎます。"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| format!("Mutsuna Cloudの{action}応答を読み取れませんでした。"))
}

async fn require_success_json<T: DeserializeOwned>(
    response: reqwest::Response,
    action: &str,
) -> Result<T, String> {
    if !response.status().is_success() {
        return Err(crate::mutsuna_cloud::api_status_error(
            response.status(),
            action,
        ));
    }
    bounded_json(response, action).await
}

async fn start_job(
    session: &crate::mutsuna_cloud::MutsunaCloudSession,
    job_id: &str,
    idempotency_key: &str,
) -> Result<bool, String> {
    let request = session
        .request(
            reqwest::Method::POST,
            session.endpoint(&format!("/v1/jobs/{job_id}/start"))?,
        )?
        .header("Idempotency-Key", idempotency_key)
        .json(&serde_json::json!({}));
    let response = match session.send_idempotent(request).await {
        Ok(response) => response,
        // The server may have committed before the response was lost. The
        // caller reconciles with GET /jobs/:id rather than starting a new job.
        Err(_) => return Ok(false),
    };
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        || response.status().is_server_error()
    {
        return Ok(false);
    }
    if !response.status().is_success() {
        return Err(crate::mutsuna_cloud::api_status_error(
            response.status(),
            "文字起こし開始",
        ));
    }
    let _: serde_json::Value = bounded_json(response, "文字起こし開始").await?;
    Ok(true)
}

fn normalize_result(response: JobResultResponse) -> Result<Transcript, String> {
    if response.model != MODEL_ID
        || response.result.duration_ms > MAX_AUDIO_DURATION_MS
        || response.result.tracks.len() != 1
        || response.result.tracks[0].track_id != "mixed"
    {
        return Err("Mutsuna Cloudの文字起こし結果がリクエストと一致しません。".into());
    }
    let language = response.result.tracks[0]
        .detected_language
        .clone()
        .unwrap_or_else(|| "ja".into());
    let mut segments = Vec::with_capacity(response.result.segments.len().max(1));
    for segment in response.result.segments {
        let text = segment.text.trim();
        if segment.track_id != "mixed"
            || segment.end_ms <= segment.start_ms
            || segment.end_ms > response.result.duration_ms
            || text.is_empty()
        {
            return Err("Mutsuna Cloudの文字起こし区間が正しくありません。".into());
        }
        segments.push(TranscriptSegment {
            speaker: "話者 1".into(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: text.to_owned(),
        });
    }
    if segments.is_empty() {
        let text = response.result.text.trim();
        if text.is_empty() {
            return Err("Mutsuna Cloudから文字起こし結果を受け取れませんでした。".into());
        }
        segments.push(TranscriptSegment {
            speaker: "話者 1".into(),
            start_ms: 0,
            end_ms: response.result.duration_ms.max(1),
            text: text.to_owned(),
        });
    }
    Ok(Transcript {
        provider: "mutsunaCloud".into(),
        model: MODEL_ID.into(),
        language,
        tokens: Vec::new(),
        segments,
    })
}

async fn transcribe_inner(
    app: &tauri::AppHandle,
    audio_path: &Path,
    audio_duration_ms: u64,
) -> Result<TranscriptionOutcome, String> {
    if audio_duration_ms == 0 || audio_duration_ms > MAX_AUDIO_DURATION_MS {
        return Err("Mutsuna CloudのMVPでは1トラック・5分以内の音声を利用してください。".into());
    }
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::Preparing, 0, Some(4)),
    );
    let workspace = AudioWorkspace::create(app)?;
    let upload_path = workspace.0.join("mixed.wav");
    let source = audio_path.to_path_buf();
    let converted = upload_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        convert_audio(&source, &converted, audio_duration_ms)
    })
    .await
    .map_err(|_| "Mutsuna Cloud向け音声変換を完了できませんでした。".to_string())??;
    let audio = tokio::fs::read(&upload_path)
        .await
        .map_err(|_| "Mutsuna Cloud向け音声を開けませんでした。".to_string())?;
    if audio.is_empty() || audio.len() > MAX_UPLOAD_BYTES {
        return Err("変換後の音声がMutsuna Cloudの25MiB上限を超えています。".into());
    }
    let content_sha256 = format!("{:x}", Sha256::digest(&audio));
    let session = crate::mutsuna_cloud::session(app)?;

    let create_request = session
        .request(reqwest::Method::POST, session.endpoint("/v1/jobs")?)?
        .header(
            "Idempotency-Key",
            crate::mutsuna_cloud::new_idempotency_key("create-job"),
        )
        .json(&serde_json::json!({
            "model": MODEL_ID,
            "language": "ja-JP",
            "tracks": [{ "id": "mixed", "kind": "mixed" }]
        }));
    let created: CreateJobResponse = require_success_json(
        session.send_idempotent(create_request).await?,
        "文字起こしジョブ作成",
    )
    .await?;
    if created.job_id.is_empty() || created.uploads.len() != 1 {
        return Err("Mutsuna Cloudからアップロード先を受け取れませんでした。".into());
    }
    let upload = &created.uploads[0];
    if upload.track_id != "mixed"
        || upload.method != "PUT"
        || upload.max_bytes < audio.len() as u64
        || !upload
            .accepted_content_types
            .iter()
            .any(|content_type| content_type == "audio/wav")
    {
        return Err("Mutsuna Cloudのアップロード条件と音声が一致しません。".into());
    }
    let upload_url = url::Url::parse(&upload.upload_url)
        .map_err(|_| "Mutsuna Cloudのアップロード先URLが正しくありません。".to_string())?;
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::Transcribing, 1, Some(4)),
    );
    let upload_request = session
        .request(reqwest::Method::PUT, upload_url)?
        .header(reqwest::header::CONTENT_TYPE, "audio/wav")
        .header(reqwest::header::CONTENT_LENGTH, audio.len())
        .header("X-Content-SHA256", content_sha256)
        .timeout(UPLOAD_TIMEOUT)
        .body(audio);
    let uploaded = session.send_idempotent(upload_request).await?;
    if !uploaded.status().is_success() {
        return Err(crate::mutsuna_cloud::api_status_error(
            uploaded.status(),
            "音声アップロード",
        ));
    }

    let start_idempotency_key = crate::mutsuna_cloud::new_idempotency_key("start-job");
    // A false result is intentionally not fatal: it means the response is
    // uncertain, so the status loop below determines whether the same frozen
    // start request must be replayed.
    let _ = start_job(&session, &created.job_id, &start_idempotency_key).await?;

    let polling_started = Instant::now();
    loop {
        if polling_started.elapsed() >= JOB_TIMEOUT {
            return Err("Mutsuna Cloudの文字起こしがタイムアウトしました。".into());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        let status_request = session.request(
            reqwest::Method::GET,
            session.endpoint(&format!("/v1/jobs/{}", created.job_id))?,
        )?;
        let status_response = match session.send(status_request).await {
            Ok(response) => response,
            Err(_) => continue,
        };
        if status_response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status_response.status().is_server_error()
        {
            continue;
        }
        let status: JobStatusResponse =
            require_success_json(status_response, "文字起こし状態確認").await?;
        match status.status.as_str() {
            "ready" => {
                let _ = start_job(&session, &created.job_id, &start_idempotency_key).await?;
            }
            "queued" | "processing" => {
                let remote = status.progress_percent.unwrap_or(0).min(99);
                let completed = 2 + u32::from(remote >= 50);
                publish_transcription_progress(
                    app,
                    TranscriptionProgress::new(
                        TranscriptionStage::Transcribing,
                        completed,
                        Some(4),
                    ),
                );
            }
            "succeeded" => break,
            "failed" => {
                let insufficient = status
                    .error
                    .and_then(|error| error.instance)
                    .is_some_and(|instance| instance.ends_with("/insufficient_credits"));
                return Err(if insufficient {
                    "Mutsuna Cloudの利用可能クレジットが不足しています。".into()
                } else {
                    "Mutsuna Cloudで文字起こしを完了できませんでした。音声を確認して再試行してください。".into()
                });
            }
            "cancelled" => return Err("Mutsuna Cloudの文字起こしがキャンセルされました。".into()),
            _ => {
                return Err("Mutsuna Cloudから不正なジョブ状態を受け取りました。".into());
            }
        }
    }

    let result_started = Instant::now();
    let result: JobResultResponse = loop {
        if result_started.elapsed() >= Duration::from_secs(60) {
            return Err("Mutsuna Cloudの文字起こし結果を取得できませんでした。".into());
        }
        let result_request = session.request(
            reqwest::Method::GET,
            session.endpoint(&format!("/v1/jobs/{}/result", created.job_id))?,
        )?;
        let result_response = match session.send(result_request).await {
            Ok(response) => response,
            Err(_) => {
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };
        if result_response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            || result_response.status().is_server_error()
        {
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        }
        break require_success_json(result_response, "文字起こし結果取得").await?;
    };
    publish_transcription_progress(
        app,
        TranscriptionProgress::new(TranscriptionStage::Transcribing, 4, Some(4)),
    );
    Ok(TranscriptionOutcome {
        transcript: normalize_result(result)?,
        cost_usd: None,
    })
}

pub(crate) async fn transcribe(
    app: &tauri::AppHandle,
    audio_path: &Path,
    audio_duration_ms: u64,
) -> Result<TranscriptionOutcome, String> {
    tokio::time::timeout(
        JOB_TIMEOUT + Duration::from_secs(60),
        transcribe_inner(app, audio_path, audio_duration_ms),
    )
    .await
    .map_err(|_| "Mutsuna Cloudの文字起こしがタイムアウトしました。".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_one_track_cloud_result() {
        let transcript = normalize_result(JobResultResponse {
            model: MODEL_ID.into(),
            result: CloudTranscript {
                duration_ms: 2_000,
                text: "こんにちは".into(),
                tracks: vec![CloudTrack {
                    track_id: "mixed".into(),
                    detected_language: Some("ja".into()),
                }],
                segments: vec![CloudSegment {
                    track_id: "mixed".into(),
                    start_ms: 100,
                    end_ms: 1_900,
                    text: " こんにちは ".into(),
                }],
            },
        })
        .expect("valid cloud transcript");
        assert_eq!(transcript.provider, "mutsunaCloud");
        assert_eq!(transcript.model, MODEL_ID);
        assert_eq!(transcript.segments[0].text, "こんにちは");
    }

    #[test]
    fn rejects_segments_outside_the_audio_duration() {
        let result = normalize_result(JobResultResponse {
            model: MODEL_ID.into(),
            result: CloudTranscript {
                duration_ms: 1_000,
                text: "synthetic".into(),
                tracks: vec![CloudTrack {
                    track_id: "mixed".into(),
                    detected_language: None,
                }],
                segments: vec![CloudSegment {
                    track_id: "mixed".into(),
                    start_ms: 0,
                    end_ms: 1_001,
                    text: "synthetic".into(),
                }],
            },
        });
        assert!(result.is_err());
    }
}
