mod commands;
mod credentials;
mod recording;
pub mod transcription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::transcribe::AudioSelectionState::default())
        .manage(recording::RecordingService::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::api_key::save_api_key,
            commands::api_key::has_api_key,
            commands::api_key::delete_api_key,
            commands::transcribe::select_audio_file,
            commands::transcribe::transcribe_selected_audio,
            commands::usage::get_transcription_usage,
            commands::recording::get_recording_capabilities,
            commands::recording::get_recording_status,
            commands::recording::get_recorded_audio,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::cancel_recording,
            commands::recording::list_recoverable_recordings,
            commands::recording::recover_recording,
            commands::recording::discard_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
