use std::{
    collections::HashSet,
    fs,
    io::BufReader,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MANIFEST_SCHEMA_VERSION: u8 = 1;
const MANIFEST_FILE: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_MODEL_BYTES: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalModelManifest {
    schema_version: u8,
    provider: String,
    model_id: String,
    version: String,
    engine: String,
    display_name: String,
    language_codes: Vec<String>,
    files: Vec<LocalModelFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalModelFile {
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledLocalModel {
    pub(crate) model_id: String,
    pub(crate) version: String,
    pub(crate) engine: String,
    pub(crate) display_name: String,
    pub(crate) language_codes: Vec<String>,
    pub(crate) size_bytes: u64,
}

pub(crate) fn list_installed(app: &AppHandle) -> Result<Vec<InstalledLocalModel>, String> {
    let root = models_directory(app)?;
    list_installed_in(&root)
}

pub(crate) fn models_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("local-stt").join("models"))
        .map_err(|error| format!("ローカルSTTモデルの保存先を取得できませんでした: {error}"))
}

fn list_installed_in(root: &Path) -> Result<Vec<InstalledLocalModel>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for model_entry in read_directories(root)? {
        for version_entry in read_directories(&model_entry)? {
            let manifest_path = version_entry.join(MANIFEST_FILE);
            if !manifest_path.exists() {
                continue;
            }
            match read_installed_manifest(&version_entry, &manifest_path) {
                Ok(model) => installed.push(model),
                Err(error) => eprintln!(
                    "Ignoring invalid local STT model at {}: {error}",
                    version_entry.display()
                ),
            }
        }
    }
    installed.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.version.cmp(&right.version))
    });
    Ok(installed)
}

fn read_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(root)
        .map_err(|error| format!("ローカルSTTモデルを確認できませんでした: {error}"))?;
    let mut directories = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("ローカルSTTモデルを確認できませんでした: {error}"))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("ローカルSTTモデルを確認できませんでした: {error}"))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            directories.push(entry.path());
        }
    }
    Ok(directories)
}

fn read_installed_manifest(
    installation: &Path,
    manifest_path: &Path,
) -> Result<InstalledLocalModel, String> {
    let metadata = fs::symlink_metadata(manifest_path)
        .map_err(|error| format!("モデルmanifestを確認できませんでした: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err("モデルmanifestが不正です。".into());
    }
    let file = fs::File::open(manifest_path)
        .map_err(|error| format!("モデルmanifestを読み込めませんでした: {error}"))?;
    let manifest: LocalModelManifest = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("モデルmanifestが壊れています: {error}"))?;
    validate_manifest(&manifest)?;

    let model_directory_name = installation
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    let version_directory_name = installation.file_name().and_then(|value| value.to_str());
    if model_directory_name != Some(manifest.model_id.as_str())
        || version_directory_name != Some(manifest.version.as_str())
    {
        return Err("モデルmanifestと保存先が一致しません。".into());
    }

    let mut size_bytes = 0u64;
    for expected in &manifest.files {
        let path = installation.join(&expected.path);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| format!("モデルファイルが見つかりません: {}", expected.path))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != expected.size_bytes
        {
            return Err(format!("モデルファイルが不正です: {}", expected.path));
        }
        size_bytes = size_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "モデルサイズを確認できませんでした。".to_string())?;
    }
    if size_bytes > MAX_MODEL_BYTES {
        return Err("ローカルSTTモデルがサイズ上限を超えています。".into());
    }

    Ok(InstalledLocalModel {
        model_id: manifest.model_id,
        version: manifest.version,
        engine: manifest.engine,
        display_name: manifest.display_name,
        language_codes: manifest.language_codes,
        size_bytes,
    })
}

fn validate_manifest(manifest: &LocalModelManifest) -> Result<(), String> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION || manifest.provider != "local" {
        return Err("モデルmanifestの形式に対応していません。".into());
    }
    for (label, value) in [
        ("モデルID", manifest.model_id.as_str()),
        ("モデルバージョン", manifest.version.as_str()),
        ("推論エンジン", manifest.engine.as_str()),
    ] {
        if !is_safe_identifier(value) {
            return Err(format!("{label}が不正です。"));
        }
    }
    if manifest.display_name.trim().is_empty() || manifest.display_name.len() > 128 {
        return Err("モデル表示名が不正です。".into());
    }
    if manifest.language_codes.len() > 64
        || manifest.language_codes.iter().any(|code| {
            code.is_empty()
                || code.len() > 16
                || !code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err("モデルの対応言語が不正です。".into());
    }
    if manifest.files.is_empty() || manifest.files.len() > 32 {
        return Err("モデルファイル一覧が不正です。".into());
    }
    let mut paths = HashSet::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let path = Path::new(&file.path);
        if file.path.is_empty()
            || !paths.insert(file.path.as_str())
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || file.size_bytes == 0
            || file.size_bytes > MAX_MODEL_BYTES
            || file.sha256.len() != 64
            || !file
                .sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err("モデルファイル定義が不正です。".into());
        }
    }
    Ok(())
}

fn is_safe_identifier(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

#[cfg(test)]
mod tests {
    use super::{list_installed_in, LocalModelFile, LocalModelManifest, MANIFEST_SCHEMA_VERSION};

    #[test]
    fn discovers_only_complete_manifest_backed_models() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-local-models-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let installation = root.join("test-model").join("1.0.0");
        std::fs::create_dir_all(&installation).expect("create model directory");
        std::fs::write(installation.join("model.bin"), b"model").expect("write model");
        let manifest = LocalModelManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            provider: "local".into(),
            model_id: "test-model".into(),
            version: "1.0.0".into(),
            engine: "test-engine".into(),
            display_name: "Test Model".into(),
            language_codes: vec!["ja".into()],
            files: vec![LocalModelFile {
                path: "model.bin".into(),
                size_bytes: 5,
                sha256: "0".repeat(64),
            }],
        };
        std::fs::write(
            installation.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");

        let installed = list_installed_in(&root).expect("scan installed models");
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].model_id, "test-model");
        assert_eq!(installed[0].size_bytes, 5);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_model_file_path_traversal() {
        let manifest = LocalModelManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            provider: "local".into(),
            model_id: "test-model".into(),
            version: "1.0.0".into(),
            engine: "test-engine".into(),
            display_name: "Test Model".into(),
            language_codes: vec!["ja".into()],
            files: vec![LocalModelFile {
                path: "../model.bin".into(),
                size_bytes: 5,
                sha256: "0".repeat(64),
            }],
        };
        assert!(super::validate_manifest(&manifest).is_err());
    }
}
