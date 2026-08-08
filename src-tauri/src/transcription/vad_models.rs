use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const SILERO_VAD_MODEL_ID: &str = "silero-vad";
const MODEL_FILE: &str = "silero_vad.onnx";
const MODEL_VERSION: &str = "5.0";
const MODEL_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/refs/tags/v5.0/files/silero_vad.onnx";
const MODEL_SIZE: u64 = 2_313_101;
const MODEL_SHA256: &str = "6b99cbfd39246b6706f98ec13c7c50c6b299181f2474fa05cbc8046acc274396";
const DOWNLOAD_EVENT: &str = "local-vad-model-download-progress";
static DOWNLOAD_ACTIVE: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_CANCELLED: AtomicBool = AtomicBool::new(false);

struct DownloadGuard;

impl DownloadGuard {
    fn acquire() -> Result<Self, String> {
        if DOWNLOAD_ACTIVE.swap(true, Ordering::AcqRel) {
            return Err("VADモデルをダウンロード中です。".into());
        }
        DOWNLOAD_CANCELLED.store(false, Ordering::Release);
        Ok(Self)
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        DOWNLOAD_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VadModelStatus {
    model_id: &'static str,
    display_name: &'static str,
    version: &'static str,
    size_bytes: u64,
    installed: bool,
    downloading: bool,
    runtime_supported: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
}

fn model_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| {
            path.join("local-stt")
                .join("vad")
                .join(SILERO_VAD_MODEL_ID)
                .join(MODEL_VERSION)
        })
        .map_err(|error| format!("VADモデルの保存先を取得できませんでした: {error}"))
}

pub(crate) fn installed_model_path(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let path = model_directory(app)?.join(MODEL_FILE);
    if !path.exists() {
        return Ok(None);
    }
    verify_model(&path)?;
    Ok(Some(path))
}

#[tauri::command]
pub(crate) fn get_local_vad_model_status(app: AppHandle) -> Result<VadModelStatus, String> {
    Ok(VadModelStatus {
        model_id: SILERO_VAD_MODEL_ID,
        display_name: "Silero VAD",
        version: MODEL_VERSION,
        size_bytes: MODEL_SIZE,
        installed: installed_model_path(&app).is_ok_and(|path| path.is_some()),
        downloading: DOWNLOAD_ACTIVE.load(Ordering::Acquire),
        runtime_supported: cfg!(desktop),
    })
}

#[tauri::command]
pub(crate) async fn download_local_vad_model(app: AppHandle) -> Result<(), String> {
    if !cfg!(desktop) {
        return Err("このOS向けのVAD推論エンジンは準備中です。".into());
    }
    let _guard = DownloadGuard::acquire()?;
    if installed_model_path(&app).is_ok_and(|path| path.is_some()) {
        return Ok(());
    }

    let directory = model_directory(&app)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("VADモデルの保存先を作成できませんでした: {error}"))?;
    let final_path = directory.join(MODEL_FILE);
    if final_path.exists() {
        fs::remove_file(&final_path)
            .map_err(|error| format!("破損したVADモデルを置き換えられませんでした: {error}"))?;
    }
    let temporary = directory.join(format!(".{MODEL_FILE}.download-{}", uuid::Uuid::now_v7()));
    let result = download_to(&app, &temporary).await.and_then(|_| {
        fs::rename(&temporary, final_path)
            .map_err(|error| format!("VADモデルをインストールできませんでした: {error}"))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_local_vad_model_download() {
    DOWNLOAD_CANCELLED.store(true, Ordering::Release);
}

#[tauri::command]
pub(crate) fn delete_local_vad_model(app: AppHandle) -> Result<(), String> {
    if DOWNLOAD_ACTIVE.load(Ordering::Acquire) {
        return Err("ダウンロード中はVADモデルを削除できません。".into());
    }
    let directory = model_directory(&app)?;
    if directory.exists() {
        fs::remove_dir_all(directory)
            .map_err(|error| format!("VADモデルを削除できませんでした: {error}"))?;
    }
    Ok(())
}

async fn download_to(app: &AppHandle, temporary: &std::path::Path) -> Result<(), String> {
    emit_progress(app, 0);
    let response = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(|error| format!("VADモデルのダウンロードを準備できませんでした: {error}"))?
        .get(MODEL_URL)
        .send()
        .await
        .map_err(|error| format!("VADモデルをダウンロードできませんでした: {error}"))?
        .error_for_status()
        .map_err(|error| format!("VADモデルをダウンロードできませんでした: {error}"))?;
    if response
        .content_length()
        .is_some_and(|size| size != MODEL_SIZE)
    {
        return Err("VADモデルの配布サイズが変更されています。".into());
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| format!("VADモデルを保存できませんでした: {error}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_CANCELLED.load(Ordering::Acquire) {
            return Err("VADモデルのダウンロードをキャンセルしました。".into());
        }
        let chunk = chunk.map_err(|error| format!("VADモデルの受信に失敗しました: {error}"))?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or("VADモデルのサイズを確認できませんでした。")?;
        if downloaded > MODEL_SIZE {
            return Err("VADモデルが想定サイズを超えています。".into());
        }
        output
            .write_all(&chunk)
            .map_err(|error| format!("VADモデルを保存できませんでした: {error}"))?;
        hasher.update(&chunk);
        emit_progress(app, downloaded);
    }
    output
        .sync_all()
        .map_err(|error| format!("VADモデルを確定できませんでした: {error}"))?;
    if downloaded != MODEL_SIZE || format!("{:x}", hasher.finalize()) != MODEL_SHA256 {
        return Err("VADモデルの整合性を確認できませんでした。再試行してください。".into());
    }
    Ok(())
}

fn verify_model(path: &std::path::Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("VADモデルを確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != MODEL_SIZE {
        return Err("VADモデルが不正です。再インストールしてください。".into());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("VADモデルを検証できませんでした: {error}"))?;
    if format!("{:x}", Sha256::digest(&bytes)) != MODEL_SHA256 {
        return Err("VADモデルが破損しています。再インストールしてください。".into());
    }
    Ok(())
}

fn emit_progress(app: &AppHandle, downloaded_bytes: u64) {
    let _ = app.emit(
        DOWNLOAD_EVENT,
        DownloadProgress {
            model_id: SILERO_VAD_MODEL_ID,
            downloaded_bytes,
            total_bytes: MODEL_SIZE,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{MODEL_SHA256, MODEL_SIZE, MODEL_URL};

    #[test]
    fn official_model_is_pinned() {
        assert_eq!(MODEL_SIZE, 2_313_101);
        assert_eq!(MODEL_SHA256.len(), 64);
        assert!(MODEL_URL.starts_with("https://github.com/snakers4/silero-vad/raw/refs/tags/v5.0/"));
    }
}
