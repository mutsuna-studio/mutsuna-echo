use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::types::StopReason;

pub const MANIFEST_FILE: &str = "recording.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingManifest {
    pub version: u8,
    pub session_id: String,
    pub started_at: String,
    pub updated_at: String,
    pub duration_ms: u64,
    pub microphone: bool,
    pub system_audio: bool,
    pub microphone_file: Option<PathBuf>,
    pub system_file: Option<PathBuf>,
    pub mixed_file: PathBuf,
    pub final_file: PathBuf,
    pub finalized: bool,
    pub stop_reason: Option<StopReason>,
}

impl RecordingManifest {
    pub fn save(&self, directory: &Path) -> Result<(), String> {
        let path = directory.join(MANIFEST_FILE);
        let temporary = directory.join("recording.json.tmp");
        let backup = directory.join("recording.json.backup");
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("録音の復旧情報を作成できませんでした: {error}"))?;
        let mut file = fs::File::create(&temporary)
            .map_err(|error| format!("録音の復旧情報を書き込めませんでした: {error}"))?;
        file.write_all(&json)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("録音の復旧情報を安全に書き込めませんでした: {error}"))?;
        if path.exists() {
            if backup.exists() {
                fs::remove_file(&backup).map_err(|error| {
                    format!("古い復旧情報のバックアップを削除できませんでした: {error}")
                })?;
            }
            fs::rename(&path, &backup)
                .map_err(|error| format!("復旧情報を更新用に退避できませんでした: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &path);
            }
            return Err(format!("録音の復旧情報を確定できませんでした: {error}"));
        }
        if backup.exists() {
            fs::remove_file(backup).map_err(|error| {
                format!("復旧情報の更新後バックアップを削除できませんでした: {error}")
            })?;
        }
        Ok(())
    }

    pub fn load(directory: &Path) -> Result<Self, String> {
        let primary = directory.join(MANIFEST_FILE);
        let path = if primary.exists() {
            primary
        } else {
            directory.join("recording.json.backup")
        };
        let bytes = fs::read(path)
            .map_err(|error| format!("録音の復旧情報を読み込めませんでした: {error}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("録音の復旧情報が壊れています: {error}"))
    }
}

pub fn remove_session(directory: &Path) -> Result<(), String> {
    if directory.exists() {
        fs::remove_dir_all(directory)
            .map_err(|error| format!("録音の一時データを削除できませんでした: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RecordingManifest;

    fn manifest(directory: &std::path::Path) -> RecordingManifest {
        RecordingManifest {
            version: 1,
            session_id: "20260808-42".into(),
            started_at: "2026-08-08T00:00:00Z".into(),
            updated_at: "2026-08-08T00:00:02Z".into(),
            duration_ms: 2_000,
            microphone: true,
            system_audio: true,
            microphone_file: Some(directory.join("microphone.partial.m4a")),
            system_file: Some(directory.join("system.partial.m4a")),
            mixed_file: directory.join("meeting.partial.m4a"),
            final_file: directory.join("meeting.m4a"),
            finalized: false,
            stop_reason: None,
        }
    }

    #[test]
    fn manifest_updates_and_recovers_from_backup() {
        let directory =
            std::env::temp_dir().join(format!("mutsuna-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create test directory");
        let mut value = manifest(&directory);
        value.save(&directory).expect("save manifest");
        value.duration_ms = 4_000;
        value.save(&directory).expect("update manifest");
        assert_eq!(
            RecordingManifest::load(&directory)
                .expect("load manifest")
                .duration_ms,
            4_000
        );

        std::fs::rename(
            directory.join("recording.json"),
            directory.join("recording.json.backup"),
        )
        .expect("simulate interrupted update");
        assert_eq!(
            RecordingManifest::load(&directory)
                .expect("load backup")
                .duration_ms,
            4_000
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
