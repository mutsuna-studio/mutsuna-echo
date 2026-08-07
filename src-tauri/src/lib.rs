mod commands;
mod credentials;
pub mod transcription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::transcribe::AudioSelectionState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::api_key::save_api_key,
            commands::api_key::has_api_key,
            commands::api_key::delete_api_key,
            commands::transcribe::select_audio_file,
            commands::transcribe::transcribe_selected_audio
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
