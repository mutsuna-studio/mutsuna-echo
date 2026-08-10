use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use bzip2::read::BzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const MODEL_PACK_ID: &str = "pyannote-3.0-int8-3dspeaker-eres2net-base";
pub(crate) const MODEL_PACK_VERSION: &str = "2024-10-14";
pub(crate) const SEGMENTATION_FILE: &str = "segmentation.int8.onnx";
pub(crate) const EMBEDDING_FILE: &str = "3dspeaker-eres2net-base.onnx";
const MANIFEST_FILE: &str = "manifest.json";
const SEGMENTATION_ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const SEGMENTATION_ARCHIVE_SIZE: u64 = 6_958_444;
const SEGMENTATION_ARCHIVE_SHA256: &str =
    "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488";
const SEGMENTATION_MODEL_SIZE: u64 = 1_540_506;
const SEGMENTATION_MODEL_SHA256: &str =
    "d582f4b4c6b48205de7e0643c57df0df5615a3c176189be3fc461e9d18827b5d";
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";
const EMBEDDING_SIZE: u64 = 39_593_761;
const EMBEDDING_SHA256: &str = "1a331345f04805badbb495c775a6ddffcdd1a732567d5ec8b3d5749e3c7a5e4b";
const DOWNLOAD_EVENT: &str = "local-diarization-model-download-progress";
static DOWNLOAD_ACTIVE: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiarizationModelStatus {
    model_id: &'static str,
    display_name: &'static str,
    version: &'static str,
    size_bytes: u64,
    installed: bool,
    downloading: bool,
    runtime_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifest {
    schema_version: u8,
    model_id: String,
    version: String,
    engine: String,
    segmentation: ModelSource,
    embedding: ModelSource,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelSource {
    file: String,
    size_bytes: u64,
    sha256: String,
    source_url: String,
    source_project: String,
    license: String,
}

struct DownloadGuard;

impl DownloadGuard {
    fn acquire() -> Result<Self, String> {
        if DOWNLOAD_ACTIVE.swap(true, Ordering::AcqRel) {
            return Err("話者分離モデルをダウンロード中です。".into());
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

fn model_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| {
            path.join("local-diarization")
                .join("models")
                .join(MODEL_PACK_ID)
                .join(MODEL_PACK_VERSION)
        })
        .map_err(|error| format!("話者分離モデルの保存先を取得できませんでした: {error}"))
}

pub(crate) fn installed_model_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = model_directory(app)?;
    verify_installation(&directory)?;
    Ok(directory)
}

#[tauri::command]
pub(crate) fn get_local_diarization_model_status(
    app: AppHandle,
) -> Result<DiarizationModelStatus, String> {
    Ok(DiarizationModelStatus {
        model_id: MODEL_PACK_ID,
        display_name: "pyannote 3.0 INT8 + 3D-Speaker ERes2Net Base",
        version: MODEL_PACK_VERSION,
        size_bytes: SEGMENTATION_MODEL_SIZE + EMBEDDING_SIZE,
        installed: installed_model_directory(&app).is_ok(),
        downloading: DOWNLOAD_ACTIVE.load(Ordering::Acquire),
        runtime_supported: cfg!(any(desktop, target_os = "android")),
    })
}

#[tauri::command]
pub(crate) async fn download_local_diarization_models(app: AppHandle) -> Result<(), String> {
    if !cfg!(any(desktop, target_os = "android")) {
        return Err("このOS向けのローカル話者分離は準備中です。".into());
    }
    let _guard = DownloadGuard::acquire()?;
    if installed_model_directory(&app).is_ok() {
        return Ok(());
    }

    let final_directory = model_directory(&app)?;
    if final_directory.exists() {
        fs::remove_dir_all(&final_directory).map_err(|error| {
            format!("破損した話者分離モデルを置き換えられませんでした: {error}")
        })?;
    }
    let parent = final_directory
        .parent()
        .ok_or("話者分離モデルの保存先が不正です。")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("話者分離モデルの保存先を作成できませんでした: {error}"))?;
    remove_stale_downloads(parent);
    let temporary = parent.join(format!(".download-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&temporary)
        .map_err(|error| format!("話者分離モデルの一時保存先を作成できませんでした: {error}"))?;

    let result = download_and_install(&app, &temporary).await.and_then(|_| {
        verify_installation(&temporary)?;
        fs::rename(&temporary, &final_directory)
            .map_err(|error| format!("話者分離モデルをインストールできませんでした: {error}"))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_local_diarization_model_download() {
    DOWNLOAD_CANCELLED.store(true, Ordering::Release);
}

#[tauri::command]
pub(crate) fn delete_local_diarization_models(app: AppHandle) -> Result<(), String> {
    if DOWNLOAD_ACTIVE.load(Ordering::Acquire) {
        return Err("ダウンロード中は話者分離モデルを削除できません。".into());
    }
    let directory = model_directory(&app)?;
    if directory.exists() {
        fs::remove_dir_all(&directory)
            .map_err(|error| format!("話者分離モデルを削除できませんでした: {error}"))?;
    }
    Ok(())
}

async fn download_and_install(app: &AppHandle, directory: &Path) -> Result<(), String> {
    let archive = directory.join("segmentation.tar.bz2");
    download_file(
        app,
        SEGMENTATION_ARCHIVE_URL,
        &archive,
        SEGMENTATION_ARCHIVE_SIZE,
        SEGMENTATION_ARCHIVE_SHA256,
        0,
    )
    .await?;
    extract_segmentation(&archive, directory)?;
    fs::remove_file(&archive).map_err(|error| {
        format!("話者分離モデルの一時アーカイブを削除できませんでした: {error}")
    })?;
    download_file(
        app,
        EMBEDDING_URL,
        &directory.join(EMBEDDING_FILE),
        EMBEDDING_SIZE,
        EMBEDDING_SHA256,
        SEGMENTATION_ARCHIVE_SIZE,
    )
    .await?;
    let bytes = serde_json::to_vec_pretty(&manifest())
        .map_err(|error| format!("話者分離モデルのmanifestを作成できませんでした: {error}"))?;
    fs::write(directory.join(MANIFEST_FILE), bytes)
        .map_err(|error| format!("話者分離モデルのmanifestを保存できませんでした: {error}"))?;
    Ok(())
}

async fn download_file(
    app: &AppHandle,
    url: &str,
    path: &Path,
    expected_size: u64,
    expected_hash: &str,
    completed_before: u64,
) -> Result<(), String> {
    emit_progress(app, completed_before);
    let response = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(|error| format!("話者分離モデルのダウンロードを準備できませんでした: {error}"))?
        .get(url)
        .send()
        .await
        .map_err(|error| format!("話者分離モデルをダウンロードできませんでした: {error}"))?
        .error_for_status()
        .map_err(|error| format!("話者分離モデルをダウンロードできませんでした: {error}"))?;
    if response
        .content_length()
        .is_some_and(|value| value != expected_size)
    {
        return Err("話者分離モデルの配布サイズが変更されています。".into());
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("話者分離モデルを保存できませんでした: {error}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_CANCELLED.load(Ordering::Acquire) {
            return Err("話者分離モデルのダウンロードをキャンセルしました。".into());
        }
        let chunk =
            chunk.map_err(|error| format!("話者分離モデルの受信に失敗しました: {error}"))?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or("話者分離モデルのサイズを確認できませんでした。")?;
        if downloaded > expected_size {
            return Err("話者分離モデルが想定サイズを超えています。".into());
        }
        output
            .write_all(&chunk)
            .map_err(|error| format!("話者分離モデルを保存できませんでした: {error}"))?;
        hasher.update(&chunk);
        emit_progress(app, completed_before + downloaded);
    }
    output
        .sync_all()
        .map_err(|error| format!("話者分離モデルを確定できませんでした: {error}"))?;
    if downloaded != expected_size || format!("{:x}", hasher.finalize()) != expected_hash {
        return Err("話者分離モデルの整合性を確認できませんでした。再試行してください。".into());
    }
    Ok(())
}

fn extract_segmentation(archive_path: &Path, directory: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path)
        .map_err(|error| format!("話者分離モデルのアーカイブを開けませんでした: {error}"))?;
    let mut archive = tar::Archive::new(BzDecoder::new(file));
    let mut found = false;
    for entry in archive
        .entries()
        .map_err(|error| format!("話者分離モデルのアーカイブを読めませんでした: {error}"))?
    {
        let mut entry = entry.map_err(|error| {
            format!("話者分離モデルのアーカイブを展開できませんでした: {error}")
        })?;
        let path = entry
            .path()
            .map_err(|error| format!("話者分離モデルのパスを確認できませんでした: {error}"))?;
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("話者分離モデルのアーカイブに不正なパスがあります。".into());
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("model.int8.onnx") {
            continue;
        }
        let target = directory.join(SEGMENTATION_FILE);
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| format!("話者分離モデルを展開できませんでした: {error}"))?;
        std::io::copy(&mut entry, &mut output)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("話者分離モデルを展開できませんでした: {error}"))?;
        verify_file(&target, SEGMENTATION_MODEL_SIZE, SEGMENTATION_MODEL_SHA256)?;
        found = true;
        break;
    }
    if !found {
        return Err("話者分離モデルのアーカイブにINT8モデルがありません。".into());
    }
    Ok(())
}

fn verify_installation(directory: &Path) -> Result<(), String> {
    verify_file(
        &directory.join(SEGMENTATION_FILE),
        SEGMENTATION_MODEL_SIZE,
        SEGMENTATION_MODEL_SHA256,
    )?;
    verify_file(
        &directory.join(EMBEDDING_FILE),
        EMBEDDING_SIZE,
        EMBEDDING_SHA256,
    )?;
    let manifest_path = directory.join(MANIFEST_FILE);
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|_| "話者分離モデルのmanifestがありません。".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
        return Err("話者分離モデルのmanifestが不正です。".into());
    }
    let manifest: ModelManifest = serde_json::from_reader(
        fs::File::open(manifest_path)
            .map_err(|error| format!("話者分離モデルのmanifestを開けませんでした: {error}"))?,
    )
    .map_err(|error| format!("話者分離モデルのmanifestを読めませんでした: {error}"))?;
    if manifest.schema_version != 1
        || manifest.model_id != MODEL_PACK_ID
        || manifest.version != MODEL_PACK_VERSION
    {
        return Err("話者分離モデルのmanifestに対応していません。".into());
    }
    Ok(())
}

fn verify_file(path: &Path, expected_size: u64, expected_hash: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("話者分離モデルが見つかりません: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected_size {
        return Err("話者分離モデルのサイズが不正です。".into());
    }
    let mut file = fs::File::open(path)
        .map_err(|error| format!("話者分離モデルを開けませんでした: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("話者分離モデルを検証できませんでした: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != expected_hash {
        return Err("話者分離モデルが破損しています。再インストールしてください。".into());
    }
    Ok(())
}

fn manifest() -> ModelManifest {
    ModelManifest {
        schema_version: 1,
        model_id: MODEL_PACK_ID.into(),
        version: MODEL_PACK_VERSION.into(),
        engine: "sherpa-onnx-offline-speaker-diarization".into(),
        segmentation: ModelSource {
            file: SEGMENTATION_FILE.into(),
            size_bytes: SEGMENTATION_MODEL_SIZE,
            sha256: SEGMENTATION_MODEL_SHA256.into(),
            source_url: SEGMENTATION_ARCHIVE_URL.into(),
            source_project: "pyannote/segmentation-3.0".into(),
            license: "MIT".into(),
        },
        embedding: ModelSource {
            file: EMBEDDING_FILE.into(),
            size_bytes: EMBEDDING_SIZE,
            sha256: EMBEDDING_SHA256.into(),
            source_url: EMBEDDING_URL.into(),
            source_project: "alibaba-damo-academy/3D-Speaker".into(),
            license: "Apache-2.0".into(),
        },
    }
}

fn remove_stale_downloads(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let safe = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".download-"))
            && fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
        if safe {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn emit_progress(app: &AppHandle, downloaded_bytes: u64) {
    let _ = app.emit(
        DOWNLOAD_EVENT,
        DownloadProgress {
            model_id: MODEL_PACK_ID,
            downloaded_bytes,
            total_bytes: SEGMENTATION_ARCHIVE_SIZE + EMBEDDING_SIZE,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_models_are_pinned_and_manifested() {
        assert_eq!(SEGMENTATION_ARCHIVE_SIZE, 6_958_444);
        assert_eq!(EMBEDDING_SIZE, 39_593_761);
        assert_eq!(SEGMENTATION_ARCHIVE_SHA256.len(), 64);
        assert_eq!(SEGMENTATION_MODEL_SHA256.len(), 64);
        assert_eq!(EMBEDDING_SHA256.len(), 64);
        let manifest = manifest();
        assert_eq!(manifest.segmentation.license, "MIT");
        assert_eq!(manifest.embedding.license, "Apache-2.0");
    }
}
