use std::{
    collections::HashMap,
    fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

const MAX_CACHE_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FINGERPRINT_MEMO_ENTRIES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioFingerprint {
    pub(crate) sha256: String,
    pub(crate) size_bytes: u64,
}

#[derive(Clone)]
struct MemoEntry {
    size_bytes: u64,
    modified_ns: u128,
    fingerprint: AudioFingerprint,
}

static FINGERPRINT_MEMO: OnceLock<Mutex<HashMap<PathBuf, MemoEntry>>> = OnceLock::new();

pub(crate) fn audio_fingerprint(path: &Path) -> Result<AudioFingerprint, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        format!("音声キャッシュ用のファイル情報を取得できませんでした: {error}")
    })?;
    if !metadata.is_file() {
        return Err("音声キャッシュの対象がファイルではありません。".into());
    }
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let memo = FINGERPRINT_MEMO.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(entries) = memo.lock() {
        if let Some(entry) = entries
            .get(&key)
            .filter(|entry| entry.size_bytes == metadata.len() && entry.modified_ns == modified_ns)
        {
            return Ok(entry.fingerprint.clone());
        }
    }

    let file = fs::File::open(path)
        .map_err(|error| format!("音声キャッシュ用のハッシュを開始できませんでした: {error}"))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            format!("音声キャッシュ用のハッシュを計算できませんでした: {error}")
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let fingerprint = AudioFingerprint {
        sha256: format!("{:x}", hasher.finalize()),
        size_bytes: metadata.len(),
    };
    if let Ok(mut entries) = memo.lock() {
        if entries.len() >= MAX_FINGERPRINT_MEMO_ENTRIES {
            entries.clear();
        }
        entries.insert(
            key,
            MemoEntry {
                size_bytes: metadata.len(),
                modified_ns,
                fingerprint: fingerprint.clone(),
            },
        );
    }
    Ok(fingerprint)
}

pub(crate) fn cache_key(fingerprint: &AudioFingerprint, discriminator: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(fingerprint.sha256.as_bytes());
    hasher.update(fingerprint.size_bytes.to_le_bytes());
    hasher.update(discriminator.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn load_json<T: DeserializeOwned>(
    app: &AppHandle,
    namespace: &str,
    key: &str,
) -> Result<Option<T>, String> {
    let path = cache_path(app, namespace, key)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("推論キャッシュを確認できませんでした: {error}")),
    };
    if metadata.len() > MAX_CACHE_DOCUMENT_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("推論キャッシュを読み込めませんでした: {error}"))?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            eprintln!(
                "Ignoring invalid inference cache at {}: {error}",
                path.display()
            );
            Ok(None)
        }
    }
}

pub(crate) fn store_json<T: Serialize>(
    app: &AppHandle,
    namespace: &str,
    key: &str,
    value: &T,
) -> Result<(), String> {
    let path = cache_path(app, namespace, key)?;
    let directory = path
        .parent()
        .ok_or_else(|| "推論キャッシュの保存先が不正です。".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("推論キャッシュの保存先を作成できませんでした: {error}"))?;
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("推論キャッシュを作成できませんでした: {error}"))?;
    if bytes.len() as u64 > MAX_CACHE_DOCUMENT_BYTES {
        return Err("推論キャッシュが大きすぎます。".into());
    }
    let temporary = directory.join(format!(".{key}.{}.tmp", uuid::Uuid::now_v7()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("推論キャッシュを準備できませんでした: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("推論キャッシュを書き込めませんでした: {error}"))?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("古い推論キャッシュを更新できませんでした: {error}"))?;
        }
        fs::rename(&temporary, &path)
            .map_err(|error| format!("推論キャッシュを確定できませんでした: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn cache_path(app: &AppHandle, namespace: &str, key: &str) -> Result<PathBuf, String> {
    if namespace.is_empty()
        || key.is_empty()
        || !namespace
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
        || !key.bytes().all(|value| value.is_ascii_hexdigit())
    {
        return Err("推論キャッシュキーが不正です。".into());
    }
    app.path()
        .app_local_data_dir()
        .map(|root| {
            root.join("local-inference-cache")
                .join(namespace)
                .join(format!("{key}.json"))
        })
        .map_err(|error| format!("推論キャッシュの保存先を取得できませんでした: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{audio_fingerprint, cache_key};

    #[test]
    fn fingerprints_content_and_separates_configurations() {
        let path = std::env::temp_dir().join(format!("cache-audio-{}", uuid::Uuid::now_v7()));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(b"audio")
            .unwrap();
        let fingerprint = audio_fingerprint(&path).unwrap();
        assert_eq!(fingerprint.size_bytes, 5);
        assert_ne!(
            cache_key(&fingerprint, "standard"),
            cache_key(&fingerprint, "soft")
        );
        std::fs::remove_file(path).ok();
    }
}
