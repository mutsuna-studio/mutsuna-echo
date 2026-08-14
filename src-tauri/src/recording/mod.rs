#[cfg(any(target_os = "android", test))]
#[allow(dead_code)]
pub mod android;
pub mod application_output;
mod desktop;
mod manifest;
pub mod microphone_mute;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod mixer;
mod platform;
mod service;
mod session;
pub mod system_output;
pub mod types;

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

pub use service::RecordingService;
use types::{
    AudioDevice, RecordedAudioSummary, RecordingCapabilities, RecordingStatus,
    RecoverableRecording, CHANNELS, FINAL_BITRATE, MAX_DURATION_MS, SAMPLE_RATE,
};

pub const RECORDING_STATUS_EVENT: &str = "recording-status";

pub fn capabilities() -> Result<RecordingCapabilities, String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let devices = flexaudio::devices()
            .map_err(|error| format!("音声デバイスを取得できませんでした: {error}"))?;
        let mut microphones = Vec::new();
        let mut systems = Vec::new();
        for device in devices {
            let item = AudioDevice {
                id: device.id,
                name: device.name,
                is_default: device.is_default,
            };
            if device.is_loopback {
                systems.push(item);
            } else {
                microphones.push(item);
            }
        }
        Ok(RecordingCapabilities {
            platform: if cfg!(target_os = "windows") {
                "windows"
            } else {
                "macos"
            },
            supported: true,
            microphone_supported: true,
            system_audio_supported: true,
            system_audio_limited: false,
            limitation: None,
            microphone_devices: microphones,
            system_devices: systems,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            codec: "AAC-LC",
            bitrate: FINAL_BITRATE,
            max_duration_ms: MAX_DURATION_MS,
        })
    }
    #[cfg(all(
        not(any(target_os = "windows", target_os = "macos")),
        not(target_os = "android")
    ))]
    {
        Ok(RecordingCapabilities {
            platform: "unsupported",
            supported: false,
            microphone_supported: false,
            system_audio_supported: false,
            system_audio_limited: false,
            limitation: Some("このビルドではアプリ内録音を利用できません。"),
            microphone_devices: Vec::new(),
            system_devices: Vec::new(),
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            codec: "AAC-LC",
            bitrate: FINAL_BITRATE,
            max_duration_ms: MAX_DURATION_MS,
        })
    }
    #[cfg(target_os = "android")]
    {
        android::capabilities()
    }
}

pub fn recoverable_recordings(app: &AppHandle) -> Result<Vec<RecoverableRecording>, String> {
    session::recoverable_recordings(app)
}

#[cfg(target_os = "android")]
pub fn completed_recordings(_app: &AppHandle) -> Result<Vec<RecordedAudioSummary>, String> {
    android::completed_recordings()
}

pub fn completed_recording_path(
    app: &AppHandle,
    recording_id: &str,
) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        android::copy_completed_recording(recording_id)
    }
    #[cfg(not(target_os = "android"))]
    {
        session::completed_recording_path(app, recording_id)
    }
}

pub fn recover(app: &AppHandle, session_id: &str) -> Result<std::path::PathBuf, String> {
    session::recover(app, session_id)
}

pub fn discard(app: &AppHandle, session_id: &str) -> Result<(), String> {
    session::discard(app, session_id)
}

pub(super) fn set_status(
    status: &Arc<Mutex<RecordingStatus>>,
    update: impl FnOnce(&mut RecordingStatus),
) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut current);
}

#[cfg(not(target_os = "android"))]
pub fn completed_recordings_with_paths(
    app: &AppHandle,
) -> Result<Vec<(RecordedAudioSummary, std::path::PathBuf)>, String> {
    session::completed_recordings_with_paths(app)
}

pub(super) fn publish_status(
    app: &AppHandle,
    status: &Arc<Mutex<RecordingStatus>>,
    update: impl FnOnce(&mut RecordingStatus),
) {
    let snapshot = {
        let mut current = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut current);
        current.clone()
    };
    if let Err(error) = app.emit(RECORDING_STATUS_EVENT, snapshot) {
        eprintln!("Could not emit recording status: {error:?}");
    }
}
