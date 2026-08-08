mod commands;
mod credentials;
mod recording;
#[cfg(desktop)]
mod resident;
mod transcript_store;
pub mod transcription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .manage(commands::transcribe::AudioSelectionState::default())
        .manage(recording::RecordingService::default());
    #[cfg(desktop)]
    let builder = builder
        .manage(resident::ResidentState::default())
        .plugin(resident::init());
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::api_key::save_api_key,
            commands::api_key::has_api_key,
            commands::api_key::delete_api_key,
            commands::transcribe::select_audio_file,
            commands::transcribe::get_transcription_session,
            commands::transcribe::transcribe_selected_audio,
            commands::transcribe::get_selected_transcript,
            commands::usage::get_transcription_usage,
            commands::recording::get_recording_capabilities,
            commands::recording::get_recording_status,
            commands::recording::get_recorded_audio,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::cancel_recording,
            commands::recording::list_recoverable_recordings,
            commands::recording::list_recorded_audio,
            commands::recording::select_recorded_audio,
            commands::recording::reveal_recorded_audio,
            commands::recording::recover_recording,
            commands::recording::discard_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
