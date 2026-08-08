use std::{
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::commands::transcribe::SelectedAudioFile;

const SCHEMA_VERSION: u8 = 1;
const MAX_PENDING_ACTION_BYTES: u64 = 16 * 1024;
const PENDING_ACTION_FILE: &str = "pending-action.json";
pub(crate) const AVAILABLE_EVENT: &str = "pending-action-available";
pub(crate) const ACKNOWLEDGED_EVENT: &str = "pending-action-acknowledged";
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PendingActionKind {
    TranscribeMeeting,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingAction {
    schema_version: u8,
    pub(crate) id: String,
    pub(crate) kind: PendingActionKind,
    pub(crate) meeting_id: String,
    created_at: String,
}

pub(crate) fn prepare_transcription(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<PendingAction, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "画面引き渡し情報の保存処理を開始できませんでした。".to_string())?;
    let path = pending_action_path(app)?;
    if let Some(current) = load_in(&path)? {
        if current.kind == PendingActionKind::TranscribeMeeting && current.meeting_id == meeting_id
        {
            return Ok(current);
        }
    }
    let action = PendingAction {
        schema_version: SCHEMA_VERSION,
        id: Uuid::now_v7().to_string(),
        kind: PendingActionKind::TranscribeMeeting,
        meeting_id: meeting_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    save_in(&path, &action)?;
    Ok(action)
}

#[tauri::command]
pub(crate) fn get_pending_action(app: AppHandle) -> Result<Option<PendingAction>, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "画面引き渡し情報を確認できませんでした。".to_string())?;
    load_in(&pending_action_path(&app)?)
}

#[tauri::command]
pub(crate) fn receive_pending_action(
    app: AppHandle,
    action_id: String,
) -> Result<SelectedAudioFile, String> {
    let action = get_action_by_id(&app, &action_id)?;
    match action.kind {
        PendingActionKind::TranscribeMeeting => {
            crate::commands::transcribe::restore_selected_meeting(&app, &action.meeting_id)
        }
    }
}

#[tauri::command]
pub(crate) fn acknowledge_pending_action(app: AppHandle, action_id: String) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "画面引き渡しの完了処理を開始できませんでした。".to_string())?;
    let path = pending_action_path(&app)?;
    let action = load_in(&path)?
        .ok_or_else(|| "確認対象の画面引き渡し情報が見つかりません。".to_string())?;
    if action.id != action_id {
        return Err("画面引き渡し情報が更新されたため、完了できませんでした。".into());
    }
    remove_in(&path)?;
    app.emit(ACKNOWLEDGED_EVENT, action_id)
        .map_err(|error| format!("画面引き渡しの完了を通知できませんでした: {error}"))
}

fn get_action_by_id(app: &AppHandle, action_id: &str) -> Result<PendingAction, String> {
    Uuid::parse_str(action_id).map_err(|_| "画面引き渡しIDが不正です。".to_string())?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "画面引き渡し情報を確認できませんでした。".to_string())?;
    let action = load_in(&pending_action_path(app)?)?
        .ok_or_else(|| "文字起こし待ちの録音が見つかりません。".to_string())?;
    if action.id != action_id {
        return Err("文字起こし待ちの録音が更新されています。".into());
    }
    Ok(action)
}

fn pending_action_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("local").join(PENDING_ACTION_FILE))
        .map_err(|error| format!("画面引き渡し情報の保存先を取得できませんでした: {error}"))
}

fn load_in(path: &Path) -> Result<Option<PendingAction>, String> {
    let backup = path.with_extension("json.backup");
    let source = if path.exists() {
        path
    } else if backup.exists() {
        &backup
    } else {
        return Ok(None);
    };
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("画面引き渡し情報を確認できませんでした: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PENDING_ACTION_BYTES
    {
        return Err("保存済みの画面引き渡し情報が不正です。".into());
    }
    let file = fs::File::open(source)
        .map_err(|error| format!("画面引き渡し情報を読み込めませんでした: {error}"))?;
    let action: PendingAction = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("画面引き渡し情報が壊れています: {error}"))?;
    validate_action(&action)?;
    Ok(Some(action))
}

fn save_in(path: &Path, action: &PendingAction) -> Result<(), String> {
    validate_action(action)?;
    let parent = path
        .parent()
        .ok_or_else(|| "画面引き渡し情報の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("画面引き渡し情報の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(".{PENDING_ACTION_FILE}.{}.tmp", Uuid::now_v7()));
    let backup = path.with_extension("json.backup");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("画面引き渡し情報を書き込めませんでした: {error}"))?;
    if let Err(error) = serde_json::to_writer(&mut file, action) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "画面引き渡し情報をJSONへ変換できませんでした: {error}"
        ));
    }
    file.sync_all()
        .map_err(|error| format!("画面引き渡し情報を安全に書き込めませんでした: {error}"))?;
    drop(file);
    replace_with_backup(path, &temporary, &backup)
}

fn remove_in(path: &Path) -> Result<(), String> {
    for candidate in [path.to_path_buf(), path.with_extension("json.backup")] {
        if candidate.exists() {
            fs::remove_file(&candidate).map_err(|error| {
                format!("完了した画面引き渡し情報を削除できませんでした: {error}")
            })?;
        }
    }
    Ok(())
}

fn replace_with_backup(path: &Path, temporary: &Path, backup: &Path) -> Result<(), String> {
    if path.exists() {
        if backup.exists() {
            fs::remove_file(backup)
                .map_err(|error| format!("古い画面引き渡し情報を削除できませんでした: {error}"))?;
        }
        fs::rename(path, backup)
            .map_err(|error| format!("画面引き渡し情報を更新用に退避できませんでした: {error}"))?;
    }
    if let Err(error) = fs::rename(temporary, path) {
        if backup.exists() {
            let _ = fs::rename(backup, path);
        }
        return Err(format!(
            "画面引き渡し情報の保存を確定できませんでした: {error}"
        ));
    }
    if backup.exists() {
        fs::remove_file(backup).map_err(|error| {
            format!("画面引き渡し情報のバックアップを削除できませんでした: {error}")
        })?;
    }
    Ok(())
}

fn validate_action(action: &PendingAction) -> Result<(), String> {
    if action.schema_version != SCHEMA_VERSION {
        return Err("保存済みの画面引き渡し形式に対応していません。".into());
    }
    let action_id = Uuid::parse_str(&action.id)
        .map_err(|_| "保存済みの画面引き渡しIDが不正です。".to_string())?;
    if action_id.get_version_num() != 7 {
        return Err("保存済みの画面引き渡しID形式に対応していません。".into());
    }
    crate::meeting_store::validate_meeting_id(&action.meeting_id)
}

#[cfg(test)]
mod tests {
    use super::{load_in, remove_in, save_in, PendingAction, PendingActionKind, SCHEMA_VERSION};

    #[test]
    fn pending_action_round_trips_and_is_removed_after_acknowledgement() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-pending-action-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let path = root.join("pending-action.json");
        let action = PendingAction {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::now_v7().to_string(),
            kind: PendingActionKind::TranscribeMeeting,
            meeting_id: uuid::Uuid::now_v7().to_string(),
            created_at: "2026-08-08T00:00:00Z".into(),
        };
        save_in(&path, &action).expect("save pending action");
        assert_eq!(load_in(&path).expect("load pending action"), Some(action));
        remove_in(&path).expect("remove pending action");
        assert_eq!(load_in(&path).expect("load removed action"), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
