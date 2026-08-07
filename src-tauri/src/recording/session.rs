use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Local;
use tauri::{AppHandle, Manager};

use super::{
    manifest::{remove_session, RecordingManifest},
    types::RecoverableRecording,
};

pub(super) struct RecordingPaths {
    pub(super) session_id: String,
    pub(super) directory: PathBuf,
    pub(super) microphone: PathBuf,
    pub(super) system: PathBuf,
    pub(super) mixed: PathBuf,
    pub(super) final_file: PathBuf,
}

impl RecordingPaths {
    pub(super) fn create(app: &AppHandle) -> Result<Self, String> {
        let now = Local::now();
        let base_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let session_id = format!("{}-{}", now.format("%Y%m%d%H%M%S%3f"), std::process::id());
        let directory = recordings_root(app)?.join("in-progress").join(&session_id);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("録音用の一時フォルダーを作成できませんでした: {error}"))?;

        let output_directory = app
            .path()
            .audio_dir()
            .map_err(|error| format!("ミュージックフォルダーを取得できませんでした: {error}"))?
            .join("Mutsuna Echo");
        fs::create_dir_all(&output_directory)
            .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;

        Ok(Self {
            session_id,
            microphone: directory.join("microphone.partial.m4a"),
            system: directory.join("system.partial.m4a"),
            mixed: directory.join("meeting.partial.m4a"),
            final_file: unique_output_path(&output_directory, &base_name),
            directory,
        })
    }
}

fn unique_output_path(directory: &Path, base_name: &str) -> PathBuf {
    let initial = directory.join(format!("{base_name}.m4a"));
    if !initial.exists() {
        return initial;
    }
    (2..=999)
        .map(|suffix| directory.join(format!("{base_name}_{suffix}.m4a")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| directory.join(format!("{base_name}_{}.m4a", std::process::id())))
}

fn recordings_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("アプリデータフォルダーを取得できませんでした: {error}"))?
        .join("recordings"))
}

pub(super) fn atomic_copy_to_output(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "録音の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(
        ".{}.partial",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    fs::copy(source, &temporary)
        .map_err(|error| format!("録音ファイルを保存先へコピーできませんでした: {error}"))?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("録音ファイルを安全に書き込めませんでした: {error}"))?;
    fs::rename(&temporary, destination)
        .map_err(|error| format!("録音ファイルを保存先へ確定できませんでした: {error}"))
}

pub(super) fn recoverable_recordings(app: &AppHandle) -> Result<Vec<RecoverableRecording>, String> {
    let root = recordings_root(app)?.join("in-progress");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut recordings = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("復旧可能な録音を確認できませんでした: {error}"))?
    {
        let directory = match entry {
            Ok(entry) if entry.path().is_dir() => entry.path(),
            _ => continue,
        };
        let manifest = match RecordingManifest::load(&directory) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !manifest.finalized && manifest.mixed_file.exists() {
            recordings.push(RecoverableRecording {
                session_id: manifest.session_id,
                started_at: manifest.started_at,
                duration_ms: manifest.duration_ms,
                microphone: manifest.microphone,
                system_audio: manifest.system_audio,
            });
        }
    }
    recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(recordings)
}

pub(super) fn recover(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    validate_session_id(session_id)?;
    let directory = recordings_root(app)?.join("in-progress").join(session_id);
    let manifest = RecordingManifest::load(&directory)?;
    if !manifest.mixed_file.exists() {
        return Err("復旧できる音声フラグメントが見つかりません。".into());
    }
    crate::commands::transcribe::describe_audio_path(&manifest.mixed_file)
        .map_err(|error| format!("録音フラグメントを再生可能なM4Aとして復旧できませんでした。元データは破棄していません: {error}"))?;
    atomic_copy_to_output(&manifest.mixed_file, &manifest.final_file)?;
    crate::commands::transcribe::set_selected_audio_path(app, manifest.final_file.clone())?;
    remove_session(&directory)?;
    Ok(manifest.final_file)
}

pub(super) fn discard(app: &AppHandle, session_id: &str) -> Result<(), String> {
    validate_session_id(session_id)?;
    remove_session(&recordings_root(app)?.join("in-progress").join(session_id))
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("録音セッションIDが不正です。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{unique_output_path, validate_session_id};

    #[test]
    fn session_id_cannot_escape_recording_root() {
        assert!(validate_session_id("../../secret").is_err());
        assert!(validate_session_id("20260808-42").is_ok());
    }

    #[test]
    fn output_path_uses_m4a_extension() {
        let path = unique_output_path(&std::env::temp_dir(), "2026-08-08_12-00-00");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("m4a")
        );
    }
}
