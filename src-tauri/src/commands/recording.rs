use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    commands::transcribe::{describe_audio_path, SelectedAudioFile},
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
pub(crate) fn get_recorded_audio(
    state: State<'_, RecordingService>,
) -> Result<Option<SelectedAudioFile>, String> {
    #[cfg(target_os = "android")]
    let status = recording::android::status()?;
    #[cfg(not(target_os = "android"))]
    let status = state.status();
    status
        .output_path
        .as_deref()
        .map(std::path::Path::new)
        .map(describe_audio_path)
        .transpose()
}

#[tauri::command]
pub(crate) fn start_recording(
    app: AppHandle,
    state: State<'_, RecordingService>,
    request: StartRecordingRequest,
) -> Result<RecordingStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, state);
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
            .map(describe_audio_path)
            .transpose()?;
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
            .map(describe_audio_path)
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
pub(crate) fn list_recorded_audio(app: AppHandle) -> Result<Vec<RecordedAudioSummary>, String> {
    #[cfg(target_os = "android")]
    {
        recording::completed_recordings(&app)
    }
    #[cfg(not(target_os = "android"))]
    {
        let entries = recording::completed_recordings_with_paths(&app)?;
        let transcript_index =
            crate::transcript_store::TranscriptIndex::load(&app).unwrap_or_else(|error| {
                eprintln!("Could not index stored transcripts: {error}");
                crate::transcript_store::TranscriptIndex::default()
            });
        Ok(entries
            .into_iter()
            .map(|(mut recording, path)| {
                recording.transcript_providers = transcript_index.providers_for_audio(&path);
                recording
            })
            .collect())
    }
}

#[tauri::command]
pub(crate) fn select_recorded_audio(
    app: AppHandle,
    recording_id: String,
) -> Result<SelectedAudioFile, String> {
    let path = recording::completed_recording_path(&app, &recording_id)?;
    crate::commands::transcribe::set_selected_audio_path(&app, path)
}

#[tauri::command]
pub(crate) fn reveal_recorded_audio(app: AppHandle, recording_id: String) -> Result<(), String> {
    let path = recording::completed_recording_path(&app, &recording_id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| {
        if matches!(error, tauri_plugin_opener::Error::UnsupportedPlatform) {
            "このOSでは録音ファイルの保存場所を開けません。".to_string()
        } else {
            format!("録音ファイルの保存場所を開けませんでした: {error}")
        }
    })
}

#[tauri::command]
pub(crate) fn recover_recording(
    app: AppHandle,
    session_id: String,
) -> Result<SelectedAudioFile, String> {
    #[cfg(target_os = "android")]
    let path = {
        let _ = app;
        recording::android::recover(&session_id)?
    };
    #[cfg(not(target_os = "android"))]
    let path = recording::recover(&app, &session_id)?;
    describe_audio_path(&path)
}

#[tauri::command]
pub(crate) fn discard_recording(app: AppHandle, session_id: String) -> Result<(), String> {
    recording::discard(&app, &session_id)
}
