#[cfg(not(target_os = "android"))]
use base64::Engine;
#[cfg(not(target_os = "android"))]
use futures_util::StreamExt;
#[cfg(not(target_os = "android"))]
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const PROTOCOL_VERSION: u32 = crate::local_ai_protocol::PROTOCOL_VERSION;
pub(crate) const RUNTIME_VERSION: &str = "1.13.4-1";
const MANIFEST_FILE: &str = "manifest.json";
const MAX_PACK_BYTES: u64 = 100 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const RELEASE_BASE: &str = "https://github.com/mutsuna-studio/mutsuna-echo/releases/download";
const MINISIGN_PUBLIC_KEY: &str = "RWTKl3o6RC3pHpQe7ilIUo5clZxt6vyrf+WplGxhzK/lI/p7zrlNAok+";
const PROGRESS_EVENT: &str = "local-ai-runtime-progress";

static INSTALLING: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static ACTIVE_USERS: AtomicUsize = AtomicUsize::new(0);
static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeState {
    NotInstalled,
    Downloading,
    Installing,
    Ready,
    Incompatible,
    RemovalPending,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalAiRuntimeStatus {
    state: RuntimeState,
    source: &'static str,
    protocol_version: u32,
    required_runtime_version: &'static str,
    installed_runtime_version: Option<String>,
    progress: Option<f64>,
    error: Option<String>,
    size_bytes: u64,
    can_delete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeProgress {
    state: RuntimeState,
    stage: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    progress: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    schema_version: u32,
    protocol_version: u32,
    runtime_version: String,
    target: String,
    files: Vec<RuntimeFile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeFile {
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AndroidFeatureStatus {
    state: RuntimeState,
    installed: bool,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
}

#[cfg(target_os = "android")]
fn android_feature_call(method: &str) -> Result<AndroidFeatureStatus, String> {
    use jni::objects::{JString, JValue};
    let json = crate::android_context::with_bridge_env(
        "jp.mutsuna.echo.LocalAiFeatureBridge",
        "ローカルAI配信ブリッジへ接続できませんでした",
        |env, app, bridge| {
            let value = env
                .call_static_method(
                    bridge,
                    method,
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(app)],
                )
                .and_then(|value| value.l())
                .map_err(|error| format!("Google Playの実行環境処理に失敗しました: {error}"))?;
            env.get_string(&JString::from(value))
                .map(String::from)
                .map_err(|error| format!("実行環境の状態を読み取れませんでした: {error}"))
        },
    )?;
    serde_json::from_str(&json).map_err(|error| format!("実行環境の状態が不正です: {error}"))
}

struct InstallGuard;

impl InstallGuard {
    fn acquire() -> Result<Self, String> {
        if INSTALLING.swap(true, Ordering::AcqRel) {
            return Err("ローカルAIを導入中です。".into());
        }
        CANCELLED.store(false, Ordering::Release);
        set_last_error(None);
        Ok(Self)
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

pub(crate) struct RuntimeUseGuard;

impl Drop for RuntimeUseGuard {
    fn drop(&mut self) {
        ACTIVE_USERS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn begin_use(app: &AppHandle) -> Result<RuntimeUseGuard, String> {
    ensure_loaded(app)?;
    ACTIVE_USERS.fetch_add(1, Ordering::AcqRel);
    Ok(RuntimeUseGuard)
}

#[tauri::command]
pub(crate) fn get_local_ai_runtime_status(app: AppHandle) -> Result<LocalAiRuntimeStatus, String> {
    status(&app)
}

#[cfg(not(target_os = "android"))]
fn status(app: &AppHandle) -> Result<LocalAiRuntimeStatus, String> {
    let manifest = read_installed_manifest(app);
    let has_models = any_models_installed(app);
    let error = last_error();
    let (state, installed_runtime_version) = if INSTALLING.load(Ordering::Acquire) {
        (
            RuntimeState::Downloading,
            manifest.ok().map(|value| value.runtime_version),
        )
    } else {
        match manifest {
            Ok(value) if value.protocol_version != PROTOCOL_VERSION => {
                (RuntimeState::Incompatible, Some(value.runtime_version))
            }
            Ok(value) if value.runtime_version != RUNTIME_VERSION => {
                (RuntimeState::Incompatible, Some(value.runtime_version))
            }
            Ok(value) => (RuntimeState::Ready, Some(value.runtime_version)),
            Err(_) if removal_marker(app)?.exists() => (RuntimeState::RemovalPending, None),
            Err(_) if error.is_some() => (RuntimeState::Failed, None),
            Err(_) => (RuntimeState::NotInstalled, None),
        }
    };

    Ok(LocalAiRuntimeStatus {
        state,
        source: if cfg!(target_os = "android") {
            "googlePlay"
        } else {
            "githubRelease"
        },
        protocol_version: PROTOCOL_VERSION,
        required_runtime_version: RUNTIME_VERSION,
        installed_runtime_version,
        progress: None,
        error,
        size_bytes: 25 * 1024 * 1024,
        can_delete: !has_models && ACTIVE_USERS.load(Ordering::Acquire) == 0,
    })
}

#[cfg(target_os = "android")]
fn status(app: &AppHandle) -> Result<LocalAiRuntimeStatus, String> {
    let feature = android_feature_call("getStatus")?;
    Ok(LocalAiRuntimeStatus {
        state: feature.state,
        source: "googlePlay",
        protocol_version: PROTOCOL_VERSION,
        required_runtime_version: RUNTIME_VERSION,
        installed_runtime_version: feature.installed.then(|| RUNTIME_VERSION.to_string()),
        progress: (feature.total_bytes > 0)
            .then(|| feature.downloaded_bytes as f64 / feature.total_bytes as f64),
        error: feature.error,
        size_bytes: 25 * 1024 * 1024,
        can_delete: feature.installed
            && !any_models_installed(app)
            && ACTIVE_USERS.load(Ordering::Acquire) == 0,
    })
}

#[cfg(not(target_os = "android"))]
pub(crate) fn is_installed_compatible(app: &AppHandle) -> bool {
    read_installed_manifest(app).is_ok_and(|manifest| {
        manifest.protocol_version == PROTOCOL_VERSION && manifest.runtime_version == RUNTIME_VERSION
    })
}

#[cfg(target_os = "android")]
pub(crate) fn is_installed_compatible(_app: &AppHandle) -> bool {
    android_feature_call("getStatus")
        .is_ok_and(|status| status.installed && status.state == RuntimeState::Ready)
}

#[tauri::command]
pub(crate) async fn install_local_ai_runtime(app: AppHandle) -> Result<(), String> {
    let _guard = InstallGuard::acquire()?;
    let result = install_runtime(&app).await;
    if let Err(error) = &result {
        set_last_error(Some(error.clone()));
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_local_ai_runtime_install() {
    CANCELLED.store(true, Ordering::Release);
    #[cfg(target_os = "android")]
    let _ = android_feature_call("cancel");
}

#[tauri::command]
pub(crate) async fn install_local_transcription_bundle(app: AppHandle) -> Result<(), String> {
    let _guard = InstallGuard::acquire()?;
    let result: Result<(), String> = async {
        if read_installed_manifest(&app).is_err() {
            install_runtime(&app).await?;
        }
        emit_bundle_stage(&app, "reazonSpeech", 1, 3);
        crate::transcription::local_models::download_local_stt_model(
            app.clone(),
            crate::transcription::local_models::REAZONSPEECH_MODEL_ID.into(),
        )
        .await?;
        if CANCELLED.load(Ordering::Acquire) {
            return Err("一括セットアップをキャンセルしました。".into());
        }
        emit_bundle_stage(&app, "sileroVad", 2, 3);
        crate::transcription::vad_models::download_local_vad_model(app.clone()).await?;
        emit_bundle_stage(&app, "ready", 3, 3);
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        set_last_error(Some(error.clone()));
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_local_transcription_bundle_install() {
    CANCELLED.store(true, Ordering::Release);
    crate::transcription::local_models::cancel_local_stt_model_download();
    crate::transcription::vad_models::cancel_local_vad_model_download();
    #[cfg(target_os = "android")]
    let _ = android_feature_call("cancel");
}

#[tauri::command]
pub(crate) fn delete_local_ai_runtime(app: AppHandle) -> Result<(), String> {
    if INSTALLING.load(Ordering::Acquire) {
        return Err("導入中は実行環境を削除できません。".into());
    }
    if any_models_installed(&app) {
        return Err("先に端末内文字起こし、無音検出、話者分離のモデルを削除してください。".into());
    }
    if ACTIVE_USERS.load(Ordering::Acquire) != 0 {
        return Err("ローカル処理の完了後に削除してください。".into());
    }
    #[cfg(target_os = "android")]
    {
        android_feature_call("delete")?;
        return Ok(());
    }
    #[cfg(not(target_os = "android"))]
    {
        sherpa_onnx_sys::dynamic::unload_runtime()?;
        let directory = installation_directory(&app)?;
        if directory.exists() {
            fs::remove_dir_all(&directory)
                .map_err(|error| format!("実行環境を削除できませんでした: {error}"))?;
        }
        set_last_error(None);
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
async fn install_runtime(app: &AppHandle) -> Result<(), String> {
    if read_installed_manifest(app).is_ok() {
        ensure_loaded(app)?;
        return Ok(());
    }
    emit_progress(app, RuntimeState::Downloading, "runtime", 0, 1);
    let name = pack_name()?;
    let url = format!("{RELEASE_BASE}/local-ai-runtime-v{RUNTIME_VERSION}/{name}");
    let client = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(|error| format!("実行環境の取得を準備できませんでした: {error}"))?;
    let signature_url = format!("{url}.sig");
    let (archive, signature) = futures_util::future::try_join(
        download_limited(&client, &url, MAX_PACK_BYTES, app),
        download_limited(&client, &signature_url, 64 * 1024, app),
    )
    .await?;
    if CANCELLED.load(Ordering::Acquire) {
        return Err("実行環境のダウンロードをキャンセルしました。".into());
    }
    verify_signature(&archive, &signature)?;
    emit_progress(
        app,
        RuntimeState::Installing,
        "runtime",
        archive.len() as u64,
        archive.len() as u64,
    );
    install_archive(app, &archive)?;
    ensure_loaded(app)?;
    emit_progress(app, RuntimeState::Ready, "runtime", 1, 1);
    Ok(())
}

#[cfg(target_os = "android")]
async fn install_runtime(app: &AppHandle) -> Result<(), String> {
    let mut feature = android_feature_call("install")?;
    loop {
        emit_progress(
            app,
            feature.state,
            "runtime",
            feature.downloaded_bytes,
            feature.total_bytes.max(1),
        );
        if feature.installed && feature.state == RuntimeState::Ready {
            ensure_loaded(app)?;
            emit_progress(app, RuntimeState::Ready, "runtime", 1, 1);
            return Ok(());
        }
        if feature.state == RuntimeState::Failed {
            return Err(feature
                .error
                .unwrap_or_else(|| "Google Playから実行環境を取得できませんでした。".into()));
        }
        if CANCELLED.load(Ordering::Acquire) {
            let _ = android_feature_call("cancel");
            return Err("実行環境のダウンロードをキャンセルしました。".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        feature = android_feature_call("getStatus")?;
    }
}

#[cfg(not(target_os = "android"))]
async fn download_limited(
    client: &reqwest::Client,
    url: &str,
    limit: u64,
    app: &AppHandle,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("実行環境をダウンロードできませんでした: {error}"))?
        .error_for_status()
        .map_err(|error| format!("実行環境をダウンロードできませんでした: {error}"))?;
    if response.content_length().is_some_and(|size| size > limit) {
        return Err("実行環境の配布サイズが上限を超えています。".into());
    }
    let total = response.content_length().unwrap_or(limit);
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if CANCELLED.load(Ordering::Acquire) {
            return Err("実行環境のダウンロードをキャンセルしました。".into());
        }
        let chunk = chunk.map_err(|error| format!("実行環境の受信に失敗しました: {error}"))?;
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err("実行環境の配布サイズが上限を超えています。".into());
        }
        bytes.extend_from_slice(&chunk);
        emit_progress(
            app,
            RuntimeState::Downloading,
            "runtime",
            bytes.len() as u64,
            total,
        );
    }
    Ok(bytes)
}

#[cfg(not(target_os = "android"))]
fn verify_signature(archive: &[u8], encoded_signature: &[u8]) -> Result<(), String> {
    verify_signature_with_key(archive, encoded_signature, MINISIGN_PUBLIC_KEY)
}

#[cfg(not(target_os = "android"))]
fn verify_signature_with_key(
    archive: &[u8],
    encoded_signature: &[u8],
    public_key: &str,
) -> Result<(), String> {
    let encoded_signature =
        std::str::from_utf8(encoded_signature).map_err(|_| "実行環境の署名が不正です。")?;
    let encoded_signature = encoded_signature.trim();
    let decoded_signature;
    let signature = if encoded_signature.starts_with("untrusted comment:") {
        encoded_signature
    } else {
        decoded_signature = base64::engine::general_purpose::STANDARD
            .decode(encoded_signature)
            .map_err(|_| "実行環境の署名が不正です。")?;
        std::str::from_utf8(&decoded_signature).map_err(|_| "実行環境の署名が不正です。")?
    };
    let public_key = PublicKey::from_base64(public_key)
        .map_err(|error| format!("署名鍵を読み込めませんでした: {error}"))?;
    let signature = Signature::decode(signature)
        .map_err(|error| format!("実行環境の署名を読み込めませんでした: {error}"))?;
    public_key
        .verify(archive, &signature, false)
        .map_err(|error| format!("実行環境の署名を確認できませんでした: {error}"))
}

#[cfg(not(target_os = "android"))]
fn install_archive(app: &AppHandle, archive: &[u8]) -> Result<(), String> {
    let root = runtime_root(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("実行環境の保存先を作成できませんでした: {error}"))?;
    let temporary = root.join(format!(".install-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&temporary)
        .map_err(|error| format!("実行環境の一時保存先を作成できませんでした: {error}"))?;
    let result = (|| {
        let cursor = std::io::Cursor::new(archive);
        let mut zip = zip::ZipArchive::new(cursor)
            .map_err(|error| format!("実行環境を展開できませんでした: {error}"))?;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|error| format!("実行環境を展開できませんでした: {error}"))?;
            let enclosed = entry
                .enclosed_name()
                .ok_or("実行環境に不正なパスがあります。")?
                .to_owned();
            if enclosed
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err("実行環境に不正なパスがあります。".into());
            }
            if entry.is_dir() {
                continue;
            }
            let target = temporary.join(enclosed);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&target)
                .map_err(|error| format!("実行環境を保存できませんでした: {error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("実行環境を保存できませんでした: {error}"))?;
            output
                .sync_all()
                .map_err(|error| format!("実行環境を確定できませんでした: {error}"))?;
        }
        verify_directory(&temporary)?;
        let final_directory = installation_directory(app)?;
        if final_directory.exists() {
            fs::remove_dir_all(&final_directory).map_err(|error| error.to_string())?;
        }
        fs::rename(&temporary, &final_directory)
            .map_err(|error| format!("実行環境をインストールできませんでした: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

#[cfg(not(target_os = "android"))]
fn ensure_loaded(app: &AppHandle) -> Result<(), String> {
    if sherpa_onnx_sys::dynamic::is_loaded() {
        return Ok(());
    }
    let directory = installation_directory(app)?;
    verify_directory(&directory)?;
    let paths = runtime_library_paths(&directory)?;
    sherpa_onnx_sys::dynamic::load_runtime(&paths).map_err(runtime_load_error)?;
    validate_runtime_handshake()
}

#[cfg(all(not(target_os = "android"), target_os = "macos"))]
fn runtime_load_error(error: String) -> String {
    format!(
        "ローカルAI実行環境を起動できませんでした: {error}。アドホック署名版がmacOSに拒否された場合は、システム設定の「プライバシーとセキュリティ」で実行を許可してください。"
    )
}

#[cfg(all(not(target_os = "android"), not(target_os = "macos")))]
fn runtime_load_error(error: String) -> String {
    format!("ローカルAI実行環境を起動できませんでした: {error}")
}

#[cfg(target_os = "android")]
fn ensure_loaded(_app: &AppHandle) -> Result<(), String> {
    if sherpa_onnx_sys::dynamic::is_loaded() {
        return Ok(());
    }
    let feature = android_feature_call("load")?;
    if !feature.installed || feature.state != RuntimeState::Ready {
        return Err(feature
            .error
            .unwrap_or_else(|| "ローカルAI実行環境は未導入です。".into()));
    }
    sherpa_onnx_sys::dynamic::load_runtime(&["libonnxruntime.so", "libsherpa-onnx-c-api.so"])
        .map_err(|error| format!("ローカルAI実行環境を起動できませんでした: {error}"))?;
    validate_runtime_handshake()
}

fn validate_runtime_handshake() -> Result<(), String> {
    let version = unsafe { sherpa_onnx_sys::SherpaOnnxGetVersionStr() };
    if version.is_null() {
        return Err("実行環境にHandshake用シンボルがありません。".into());
    }
    let version = unsafe { std::ffi::CStr::from_ptr(version) }.to_string_lossy();
    if !version.starts_with("1.13.4") {
        return Err(format!(
            "実行環境のSherpaバージョンに互換性がありません: {version}"
        ));
    }
    Ok(())
}

fn read_installed_manifest(app: &AppHandle) -> Result<RuntimeManifest, String> {
    let directory = installation_directory(app)?;
    verify_directory(&directory)
}

fn verify_directory(directory: &Path) -> Result<RuntimeManifest, String> {
    let manifest_path = directory.join(MANIFEST_FILE);
    let metadata =
        fs::symlink_metadata(&manifest_path).map_err(|_| "実行環境は未導入です。".to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err("実行環境のmanifestが不正です。".into());
    }
    let manifest: RuntimeManifest =
        serde_json::from_reader(fs::File::open(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("実行環境のmanifestを読めませんでした: {error}"))?;
    if manifest.schema_version != 1 || manifest.target != runtime_target()? {
        return Err("この端末と互換性のない実行環境です。".into());
    }
    for file in &manifest.files {
        let relative = Path::new(&file.path);
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("manifestに不正なパスがあります。".into());
        }
        verify_file(&directory.join(relative), file)?;
    }
    Ok(manifest)
}

fn verify_file(path: &Path, expected: &RuntimeFile) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("実行環境のファイルがありません: {}", expected.path))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != expected.size_bytes
    {
        return Err(format!(
            "実行環境のファイルサイズが不正です: {}",
            expected.path
        ));
    }
    let mut input = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != expected.sha256 {
        return Err(format!(
            "実行環境のファイルが破損しています: {}",
            expected.path
        ));
    }
    Ok(())
}

fn runtime_library_paths(directory: &Path) -> Result<Vec<PathBuf>, String> {
    #[cfg(target_os = "windows")]
    let names = ["onnxruntime.dll", "sherpa-onnx-c-api.dll"];
    #[cfg(target_os = "macos")]
    let names = ["libonnxruntime.dylib", "libsherpa-onnx-c-api.dylib"];
    #[cfg(target_os = "android")]
    let names = ["libonnxruntime.so", "libsherpa-onnx-c-api.so"];
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
    let names = ["libonnxruntime.so", "libsherpa-onnx-c-api.so"];
    let paths = names.map(|name| directory.join(name)).to_vec();
    if paths.iter().any(|path| !path.is_file()) {
        return Err("実行環境に必要なライブラリがありません。".into());
    }
    Ok(paths)
}

fn runtime_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("local-ai-runtime"))
        .map_err(|error| format!("実行環境の保存先を取得できませんでした: {error}"))
}

fn installation_directory(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(runtime_root(app)?
        .join(RUNTIME_VERSION)
        .join(runtime_target()?))
}

fn removal_marker(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(runtime_root(app)?.join("removal-pending"))
}

fn runtime_target() -> Result<String, String> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("windows-x86_64".into())
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("macos-arm64".into())
    } else if cfg!(all(target_os = "android", target_arch = "aarch64")) {
        Ok("android-arm64".into())
    } else {
        Err("このOSまたはCPU向けのローカルAI実行環境はありません。".into())
    }
}

fn pack_name() -> Result<String, String> {
    Ok(format!(
        "mutsuna-local-ai-runtime-v{RUNTIME_VERSION}-{}.zip",
        runtime_target()?
    ))
}

fn any_models_installed(app: &AppHandle) -> bool {
    crate::transcription::local_models::list_installed(app).is_ok_and(|models| !models.is_empty())
        || crate::transcription::vad_models::installed_model_path(app)
            .is_ok_and(|path| path.is_some())
        || crate::transcription::diarization_models::installed_model_directory(app).is_ok()
}

fn emit_progress(
    app: &AppHandle,
    state: RuntimeState,
    stage: &'static str,
    downloaded: u64,
    total: u64,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        RuntimeProgress {
            state,
            stage,
            downloaded_bytes: downloaded,
            total_bytes: total,
            progress: if total == 0 {
                0.0
            } else {
                downloaded as f64 / total as f64
            },
        },
    );
}

fn emit_bundle_stage(app: &AppHandle, stage: &'static str, completed: u64, total: u64) {
    emit_progress(app, RuntimeState::Installing, stage, completed, total);
}

fn errors() -> &'static Mutex<Option<String>> {
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}
fn set_last_error(value: Option<String>) {
    if let Ok(mut error) = errors().lock() {
        *error = value;
    }
}
fn last_error() -> Option<String> {
    errors().lock().ok().and_then(|error| error.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_and_release_are_pinned() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert!(RUNTIME_VERSION.starts_with("1.13.4"));
        assert_eq!(MINISIGN_PUBLIC_KEY.len(), 56);
    }
    #[test]
    fn manifest_rejects_parent_components() {
        assert!(Path::new("../escape.dll")
            .components()
            .any(|part| !matches!(part, Component::Normal(_))));
    }
    #[cfg(not(target_os = "android"))]
    #[test]
    fn tauri_wrapped_signature_verifies_and_rejects_modified_data() {
        const SYNTHETIC_PUBLIC_KEY: &str =
            "RWS4wIwS2O/1uigdO6t/J4PrJlwlweHZmq+j0o6CytMVGK2lixMeR1Yw";
        const EMPTY_ARCHIVE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTNHdJd1MyTy8xdXJaREJxVml6cjU5azlnaXplWXFDNzBTR00zUyswa1FSNWNVNTkvRjJ2d01MMlZSUk9UOFlyVi9TcmdYZ3VxREZJTVJsSWx4UkV1QWJVZ0NjcmtkYWdZPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg2Njc3NDA0CWZpbGU6cnVudGltZS56aXAKTVNwNGw4emIzZ1pZUjNReXhtMGRRTTZab2IxVkNpcFlMbjNBK1B0Ulg1bVRhNm14YTBjTDB4Z2FDWmV1UW13VTZNMGdhd2FxNEdCUWs5ZHI0dmsxQmc9PQo=";

        assert!(verify_signature_with_key(
            b"",
            EMPTY_ARCHIVE_SIGNATURE.as_bytes(),
            SYNTHETIC_PUBLIC_KEY,
        )
        .is_ok());
        assert!(verify_signature_with_key(
            b"modified",
            EMPTY_ARCHIVE_SIGNATURE.as_bytes(),
            SYNTHETIC_PUBLIC_KEY,
        )
        .is_err());

        let raw_signature = base64::engine::general_purpose::STANDARD
            .decode(EMPTY_ARCHIVE_SIGNATURE)
            .expect("synthetic signature should decode");
        assert!(verify_signature_with_key(b"", &raw_signature, SYNTHETIC_PUBLIC_KEY).is_ok());
    }
}
