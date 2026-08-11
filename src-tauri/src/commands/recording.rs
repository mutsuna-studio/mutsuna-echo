use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{
    commands::transcribe::{
        set_selected_audio_path, set_selected_audio_with_meeting, SelectedAudioFile,
    },
    recording::{
        self,
        types::{
            RecordedAudioSummary, RecordingCapabilities, RecordingStatus, RecoverableRecording,
            StartRecordingRequest,
        },
        RecordingService,
    },
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopRecordingResult {
    status: RecordingStatus,
    audio: Option<SelectedAudioFile>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum MeetingAudioSource {
    Recording,
    Imported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentMeetingSummary {
    meeting_id: String,
    title: String,
    file_name: String,
    size_bytes: u64,
    occurred_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    audio_available: bool,
    source: MeetingAudioSource,
    transcript_providers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MeetingDeletionMode {
    AudioOnly,
    All,
}

#[tauri::command]
pub(crate) fn get_recording_capabilities() -> Result<RecordingCapabilities, String> {
    recording::capabilities()
}

#[tauri::command]
pub(crate) fn get_recording_status(
    state: State<'_, RecordingService>,
) -> Result<RecordingStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        recording::android::status()
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(state.status())
    }
}

#[tauri::command]
pub(crate) fn start_recording_monitor(
    app: AppHandle,
    state: State<'_, RecordingService>,
    request: StartRecordingRequest,
) -> Result<RecordingStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, state);
        recording::android::start_monitor(&request)
    }
    #[cfg(not(target_os = "android"))]
    {
        state.start_monitor(app, request)
    }
}

#[tauri::command]
pub(crate) fn stop_recording_monitor(state: State<'_, RecordingService>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        recording::android::stop_monitor()
    }
    #[cfg(not(target_os = "android"))]
    {
        state.stop_monitor()
    }
}

#[tauri::command]
pub(crate) fn get_recorded_audio(
    app: AppHandle,
    state: State<'_, RecordingService>,
) -> Result<Option<SelectedAudioFile>, String> {
    #[cfg(target_os = "android")]
    let status = recording::android::status()?;
    #[cfg(not(target_os = "android"))]
    let status = state.status();
    let audio = status
        .output_path
        .as_deref()
        .map(std::path::Path::new)
        .map(|path| set_selected_audio_path(&app, path.to_path_buf()))
        .transpose()?;
    #[cfg(target_os = "android")]
    if let Some(audio) = &audio {
        cache_android_recorded_waveform(&app, &status, audio);
    }
    Ok(audio)
}

#[tauri::command]
pub(crate) fn start_recording(
    app: AppHandle,
    state: State<'_, RecordingService>,
    request: StartRecordingRequest,
) -> Result<RecordingStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        crate::processing_power::sync_display_setting(&app)?;
        recording::android::start(&request)
    }
    #[cfg(not(target_os = "android"))]
    {
        state.start(app, request)
    }
}

#[tauri::command]
pub(crate) async fn stop_recording(app: AppHandle) -> Result<StopRecordingResult, String> {
    #[cfg(target_os = "android")]
    {
        recording::android::stop(false)?;
        let status = wait_for_android_stop().await?;
        let audio = status
            .output_path
            .as_deref()
            .map(std::path::Path::new)
            .map(|path| set_selected_audio_path(&app, path.to_path_buf()))
            .transpose()?;
        if let Some(audio) = &audio {
            let microphone = status
                .microphone_track_path
                .as_deref()
                .map(std::path::Path::new);
            let system = status
                .system_track_path
                .as_deref()
                .map(std::path::Path::new);
            crate::meeting_store::store_recording_tracks(
                &app,
                audio.meeting_id(),
                microphone,
                system,
            )?;
            for path in [microphone, system].into_iter().flatten() {
                let _ = std::fs::remove_file(path);
            }
            cache_android_recorded_waveform(&app, &status, audio);
        }
        // 完了結果はこの応答に含めて返すため、次回の新規録音画面へ持ち越さない。
        recording::android::clear_completed_status()?;
        return Ok(StopRecordingResult { status, audio });
    }
    #[cfg(not(target_os = "android"))]
    {
        app.state::<RecordingService>().request_stop(false)?;
        let worker_app = app.clone();
        let status = tauri::async_runtime::spawn_blocking(move || {
            worker_app.state::<RecordingService>().wait_for_stop()
        })
        .await
        .map_err(|error| format!("録音の確定処理を待機できませんでした: {error}"))??;
        let audio = status
            .output_path
            .as_deref()
            .map(std::path::Path::new)
            .map(|path| set_selected_audio_path(&app, path.to_path_buf()))
            .transpose()?;
        Ok(StopRecordingResult { status, audio })
    }
}

#[tauri::command]
pub(crate) async fn cancel_recording(app: AppHandle) -> Result<RecordingStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        recording::android::stop(true)?;
        return wait_for_android_stop().await;
    }
    #[cfg(not(target_os = "android"))]
    {
        app.state::<RecordingService>().request_stop(true)?;
        let worker_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            worker_app.state::<RecordingService>().wait_for_stop()
        })
        .await
        .map_err(|error| format!("録音の破棄処理を待機できませんでした: {error}"))?
    }
}

#[cfg(target_os = "android")]
async fn wait_for_android_stop() -> Result<RecordingStatus, String> {
    for _ in 0..600 {
        let status = recording::android::status()?;
        if matches!(
            status.phase,
            crate::recording::types::RecordingPhase::Idle
                | crate::recording::types::RecordingPhase::Completed
                | crate::recording::types::RecordingPhase::Failed
        ) {
            return Ok(status);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(
        "Android録音の確定処理がタイムアウトしました。録音はバックグラウンドで保護されています。"
            .into(),
    )
}

#[tauri::command]
pub(crate) fn list_recoverable_recordings(
    app: AppHandle,
) -> Result<Vec<RecoverableRecording>, String> {
    recording::recoverable_recordings(&app)
}

#[tauri::command]
pub(crate) async fn list_recorded_audio(
    app: AppHandle,
) -> Result<Vec<RecordedAudioSummary>, String> {
    #[cfg(target_os = "android")]
    {
        let mut recordings = recording::completed_recordings(&app)?;
        let transcript_index =
            crate::transcript_store::TranscriptIndex::load(&app).unwrap_or_else(|error| {
                eprintln!("Could not index stored transcripts: {error}");
                crate::transcript_store::TranscriptIndex::default()
            });
        for recording in &mut recordings {
            recording.transcript_providers =
                transcript_index.providers_for_meeting(&recording.meeting_id, None);
        }
        Ok(recordings)
    }
    #[cfg(not(target_os = "android"))]
    {
        tauri::async_runtime::spawn_blocking(move || list_recorded_audio_desktop(&app))
            .await
            .map_err(|error| format!("録音履歴のMeeting情報を準備できませんでした: {error}"))?
    }
}

#[tauri::command]
pub(crate) async fn list_recent_meetings(
    app: AppHandle,
) -> Result<Vec<RecentMeetingSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || list_recent_meetings_sync(&app))
        .await
        .map_err(|error| format!("Meeting一覧の準備を完了できませんでした: {error}"))?
}

fn list_recent_meetings_sync(app: &AppHandle) -> Result<Vec<RecentMeetingSummary>, String> {
    let transcript_index =
        crate::transcript_store::TranscriptIndex::load(app).unwrap_or_else(|error| {
            eprintln!("Could not index stored transcripts: {error}");
            crate::transcript_store::TranscriptIndex::default()
        });
    let mut recordings_by_meeting = HashMap::new();

    #[cfg(target_os = "android")]
    for mut recording in recording::completed_recordings(app)? {
        // Androidの録音履歴はMediaStoreのURIごとにMeeting IDを持つ一方、録音直後に
        // 選択されたキャッシュファイルはcontent hashから別の既存Meetingへ解決される
        // ことがある。MediaStoreの録音をローカルのMeetingへ同期して、一覧に同じIDを
        // 使うことで新しい録音が「最近の会議」から漏れないようにする。
        let path = recording::completed_recording_path(app, &recording.id)?;
        recording.meeting_id = crate::meeting_store::resolve_or_create(app, &path)?;
        recordings_by_meeting.insert(recording.meeting_id.clone(), recording);
    }

    #[cfg(not(target_os = "android"))]
    for (mut recording, path) in recording::completed_recordings_with_paths(app)? {
        let meeting_id = crate::meeting_store::resolve_or_create(app, &path)?;
        recording.meeting_id = meeting_id.clone();
        recordings_by_meeting.insert(meeting_id, recording);
    }

    let mut meetings = Vec::new();
    for stored in crate::meeting_store::list_stored_meetings(app)? {
        let recording = recordings_by_meeting.remove(&stored.meeting_id);
        let legacy_audio_path = recording
            .as_ref()
            .and_then(|_| crate::meeting_store::local_audio_path(app, &stored.meeting_id).ok());
        let transcript_providers = transcript_index
            .providers_for_meeting(&stored.meeting_id, legacy_audio_path.as_deref());
        // Imported audio is not part of the recorder history and may not have a
        // transcript yet. Keep it in the meeting list whenever its linked local
        // audio is still available so selecting a file can immediately populate
        // the workspace header and meeting information.
        if recording.is_none() && transcript_providers.is_empty() && !stored.audio_available {
            continue;
        }
        let occurred_at_unix_ms = recording
            .as_ref()
            .map_or(stored.updated_at_unix_ms, |item| item.recorded_at_unix_ms);
        meetings.push(RecentMeetingSummary {
            meeting_id: stored.meeting_id,
            title: stored.title,
            file_name: stored.file_name,
            size_bytes: stored.size_bytes,
            occurred_at_unix_ms,
            updated_at_unix_ms: stored.updated_at_unix_ms.max(occurred_at_unix_ms),
            audio_available: stored.audio_available,
            source: if recording.is_some() {
                MeetingAudioSource::Recording
            } else {
                MeetingAudioSource::Imported
            },
            transcript_providers,
        });
    }
    meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.occurred_at_unix_ms));
    meetings.truncate(200);
    Ok(meetings)
}

#[tauri::command]
pub(crate) fn select_meeting_audio(
    app: AppHandle,
    meeting_id: String,
) -> Result<Option<SelectedAudioFile>, String> {
    crate::meeting_store::validate_meeting_id(&meeting_id)?;
    match crate::commands::transcribe::restore_selected_meeting(&app, &meeting_id) {
        Ok(audio) => Ok(Some(audio)),
        Err(_) => {
            crate::commands::transcribe::select_meeting_without_audio(&app, &meeting_id)?;
            Ok(None)
        }
    }
}

#[tauri::command]
pub(crate) async fn delete_meeting(
    app: AppHandle,
    meeting_id: String,
    mode: MeetingDeletionMode,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::meeting_store::validate_meeting_id(&meeting_id)?;
        delete_meeting_audio_file(&app, &meeting_id)?;
        match mode {
            MeetingDeletionMode::AudioOnly => {
                crate::meeting_store::detach_audio(&app, &meeting_id)?;
                crate::commands::transcribe::select_meeting_without_audio(&app, &meeting_id)
            }
            MeetingDeletionMode::All => {
                crate::meeting_store::delete_meeting(&app, &meeting_id)?;
                crate::commands::transcribe::clear_selected_meeting(&app, &meeting_id)
            }
        }
    })
    .await
    .map_err(|error| format!("Meetingの削除処理を完了できませんでした: {error}"))?
}

fn delete_meeting_audio_file(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        for recording in recording::completed_recordings(app)? {
            let path = recording::completed_recording_path(app, &recording.id)?;
            if crate::meeting_store::resolve_or_create(app, &path)? == meeting_id {
                recording::android::delete_completed_recording(&recording.id)?;
                remove_audio_file_if_present(&path)?;
                return Ok(());
            }
        }
    }

    let Ok(path) = crate::meeting_store::local_audio_path(app, meeting_id) else {
        return Ok(());
    };
    remove_audio_file_if_present(&path)
}

fn remove_audio_file_if_present(path: &std::path::Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("音声ファイルを確認できませんでした: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("音声ファイルを安全に削除できませんでした。".into());
    }
    std::fs::remove_file(path)
        .map_err(|error| format!("音声ファイルを削除できませんでした: {error}"))
}

#[tauri::command]
pub(crate) fn reveal_meeting_audio(app: AppHandle, meeting_id: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::meeting_store::validate_meeting_id(&meeting_id)?;
        let mut is_public_recording = false;
        for recording in recording::completed_recordings(&app)? {
            let path = recording::completed_recording_path(&app, &recording.id)?;
            if crate::meeting_store::resolve_or_create(&app, &path)? == meeting_id {
                is_public_recording = true;
                break;
            }
        }
        if !is_public_recording {
            return Err("Androidでは、取り込んだ音声ファイルの元の保存場所を開けません。".into());
        }
        return recording::android::reveal_recording_folder();
    }
    #[cfg(not(target_os = "android"))]
    {
        let path = crate::meeting_store::local_audio_path(&app, &meeting_id)?;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| {
            if matches!(error, tauri_plugin_opener::Error::UnsupportedPlatform) {
                "このOSでは音声ファイルの保存場所を開けません。".to_string()
            } else {
                format!("音声ファイルの保存場所を開けませんでした: {error}")
            }
        })
    }
}

#[tauri::command]
pub(crate) fn rename_meeting_audio(
    app: AppHandle,
    meeting_id: String,
    new_file_name: String,
) -> Result<(), String> {
    crate::meeting_store::validate_meeting_id(&meeting_id)?;
    #[cfg(target_os = "android")]
    {
        let new_file_name = validate_audio_file_name(&new_file_name, "m4a")?;
        for recording in recording::completed_recordings(&app)? {
            let path = recording::completed_recording_path(&app, &recording.id)?;
            if crate::meeting_store::resolve_or_create(&app, &path)? == meeting_id {
                recording::android::rename_completed_recording(&recording.id, &new_file_name)?;
                crate::meeting_store::rename_audio_metadata(&app, &meeting_id, &new_file_name)?;
                return Ok(());
            }
        }
        Err("変更できる録音ファイルが見つかりませんでした。".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let current = crate::meeting_store::local_audio_path(&app, &meeting_id)?;
        let extension = current
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let new_file_name = validate_audio_file_name(&new_file_name, extension)?;
        let destination = current.with_file_name(&new_file_name);
        if destination.exists() {
            return Err("同じ名前の音声ファイルがすでにあります。".into());
        }
        std::fs::rename(&current, &destination)
            .map_err(|error| format!("音声ファイル名を変更できませんでした: {error}"))?;
        if let Err(error) = crate::meeting_store::link_existing(&app, &meeting_id, &destination)
            .and_then(|_| {
                crate::meeting_store::rename_audio_metadata(&app, &meeting_id, &new_file_name)
            })
        {
            let _ = std::fs::rename(&destination, &current);
            return Err(error);
        }
        Ok(())
    }
}

fn validate_audio_file_name(value: &str, expected_extension: &str) -> Result<String, String> {
    let value = value.trim();
    let path = std::path::Path::new(value);
    let invalid = value.is_empty()
        || value.chars().count() > 128
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
        || value
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension));
    if invalid {
        return Err(format!("ファイル名には使用できない文字が含まれているか、拡張子が.{expected_extension}ではありません。"));
    }
    Ok(value.to_string())
}

#[cfg(not(target_os = "android"))]
fn list_recorded_audio_desktop(app: &AppHandle) -> Result<Vec<RecordedAudioSummary>, String> {
    let entries = recording::completed_recordings_with_paths(app)?;
    let transcript_index =
        crate::transcript_store::TranscriptIndex::load(app).unwrap_or_else(|error| {
            eprintln!("Could not index stored transcripts: {error}");
            crate::transcript_store::TranscriptIndex::default()
        });
    entries
        .into_iter()
        .map(|(mut recording, path)| {
            let meeting_id = crate::meeting_store::resolve_or_create(app, &path)?;
            recording.meeting_id = meeting_id.clone();
            recording.transcript_providers =
                transcript_index.providers_for_meeting(&meeting_id, Some(&path));
            Ok(recording)
        })
        .collect()
}

#[tauri::command]
pub(crate) fn select_recorded_audio(
    app: AppHandle,
    recording_id: String,
    meeting_id: String,
) -> Result<SelectedAudioFile, String> {
    let path = recording::completed_recording_path(&app, &recording_id)?;
    let selected = set_selected_audio_with_meeting(&app, path, meeting_id)?;
    schedule_selected_waveform(&app, &selected);
    Ok(selected)
}

#[tauri::command]
pub(crate) fn reveal_recorded_audio(app: AppHandle, recording_id: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, recording_id);
        return recording::android::reveal_recording_folder();
    }
    #[cfg(not(target_os = "android"))]
    {
        let path = recording::completed_recording_path(&app, &recording_id)?;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| {
            if matches!(error, tauri_plugin_opener::Error::UnsupportedPlatform) {
                "このOSでは録音ファイルの保存場所を開けません。".to_string()
            } else {
                format!("録音ファイルの保存場所を開けませんでした: {error}")
            }
        })
    }
}

#[tauri::command]
pub(crate) fn recover_recording(
    app: AppHandle,
    session_id: String,
) -> Result<SelectedAudioFile, String> {
    #[cfg(target_os = "android")]
    let recovered = {
        let _ = app;
        recording::android::recover(&session_id)?
    };
    #[cfg(target_os = "android")]
    let path = recovered.path.clone();
    #[cfg(not(target_os = "android"))]
    let path = recording::recover(&app, &session_id)?;
    let selected = set_selected_audio_path(&app, path)?;
    #[cfg(target_os = "android")]
    crate::meeting_store::store_recording_tracks(
        &app,
        selected.meeting_id(),
        recovered.microphone_track_path.as_deref(),
        recovered.system_track_path.as_deref(),
    )?;
    schedule_selected_waveform(&app, &selected);
    Ok(selected)
}

fn schedule_selected_waveform(app: &AppHandle, selected: &SelectedAudioFile) {
    if let Ok((audio_path, duration_ms)) =
        crate::commands::transcribe::selected_audio_for_waveform(app, selected.meeting_id())
    {
        crate::audio_waveform::schedule_waveform_generation(
            app,
            selected.meeting_id(),
            &audio_path,
            duration_ms,
        );
    }
}

#[cfg(target_os = "android")]
fn cache_android_recorded_waveform(
    app: &AppHandle,
    status: &RecordingStatus,
    audio: &SelectedAudioFile,
) {
    let Some(waveform_path) = status.waveform_path.as_deref() else {
        return;
    };
    let waveform_path = std::path::Path::new(waveform_path);
    let cache_result = (|| {
        let bytes = std::fs::read(waveform_path)
            .map_err(|error| format!("Android録音波形を読み取れませんでした: {error}"))?;
        let peaks: Vec<f32> = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Android録音波形の形式が不正です: {error}"))?;
        let (audio_path, duration_ms) =
            crate::commands::transcribe::selected_audio_for_waveform(app, audio.meeting_id())?;
        crate::audio_waveform::cache_external_recorded_waveform(
            app,
            audio.meeting_id(),
            &audio_path,
            duration_ms,
            peaks,
        )
    })();
    if let Err(error) = cache_result {
        eprintln!("Could not cache Android recorded waveform: {error}");
    }
    let _ = std::fs::remove_file(waveform_path);
}

#[tauri::command]
pub(crate) fn discard_recording(app: AppHandle, session_id: String) -> Result<(), String> {
    recording::discard(&app, &session_id)
}
