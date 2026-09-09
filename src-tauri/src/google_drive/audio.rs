use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(super) struct Fingerprint {
    pub size: u64,
    pub sha256: String,
}
pub(super) fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "音声を読み込めません。")?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut size = 0;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "音声の変更を確認できません。")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        size += n as u64;
    }
    Ok(Fingerprint {
        size,
        sha256: format!("{:x}", hash.finalize()),
    })
}
pub(super) fn unchanged(local: &Fingerprint, remote: &Value) -> bool {
    remote["trashed"] == false
        && remote["size"].as_str().and_then(|s| s.parse::<u64>().ok()) == Some(local.size)
        && remote["sha256Checksum"].as_str() == Some(local.sha256.as_str())
}
pub(super) fn validate_selection(size: u64, selected: Option<u64>) -> Result<(), String> {
    if size >= super::limits::LARGE_AUDIO_BYTES && selected != Some(size) {
        return Err("音声が500MB以上あるか容量が変更されています。保存画面を開き直し、音声のチェックを選択してください。".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compare_actual_bytes_with_remote_and_fail_closed() {
        let path = std::env::temp_dir().join(format!("echo-drive-audio-{}", uuid::Uuid::now_v7()));
        std::fs::write(&path, b"synthetic audio").unwrap();
        let local = fingerprint(&path).unwrap();
        let mut remote =
            json!({"trashed":false,"size":local.size.to_string(),"sha256Checksum":local.sha256});
        assert!(unchanged(&local, &remote));
        std::fs::write(&path, b"Synthetic audio").unwrap();
        assert!(!unchanged(&fingerprint(&path).unwrap(), &remote));
        remote["trashed"] = json!(true);
        assert!(!unchanged(&local, &remote));
        assert!(!unchanged(
            &local,
            &json!({"trashed":false,"size":local.size.to_string()})
        ));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn large_audio_requires_explicit_current_size_selection() {
        assert!(validate_selection(499_999_999, None).is_ok());
        assert!(validate_selection(500_000_000, None).is_err());
        assert!(validate_selection(500_000_000, Some(500_000_000)).is_ok());
        assert!(validate_selection(500_000_001, Some(500_000_000)).is_err());
    }
}
