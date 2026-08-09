mod audio_playback;
mod audio_waveform;
mod commands;
mod credentials;
#[cfg(desktop)]
mod meeting_detection;
mod meeting_store;
mod pending_action;
mod recording;
#[cfg(desktop)]
mod resident;
mod summary;
mod transcript_store;
pub mod transcription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .register_asynchronous_uri_scheme_protocol(
            "mutsuna-audio",
            |context, request, responder| {
                let app = context.app_handle().clone();
                let webview_label = context.webview_label().to_string();
                std::thread::spawn(move || {
                    responder.respond(audio_playback::response(&app, &webview_label, request));
                });
            },
        )
        .manage(commands::transcribe::AudioSelectionState::default())
        .manage(recording::RecordingService::default());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(meeting_detection::MeetingDetectionState::default())
        .manage(resident::ResidentState::default())
        .plugin(meeting_detection::init())
        .plugin(resident::init());
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::api_key::save_api_key,
            commands::api_key::has_api_key,
            commands::api_key::delete_api_key,
            commands::api_key::save_provider_api_key,
            commands::api_key::delete_provider_api_key,
            transcription::providers::get_transcription_providers,
            transcription::providers::list_installed_local_stt_models,
            transcription::local_models::list_local_stt_model_catalog,
            transcription::local_models::download_local_stt_model,
            transcription::local_models::cancel_local_stt_model_download,
            transcription::local_models::delete_local_stt_model,
            transcription::vad_models::get_local_vad_model_status,
            transcription::vad_models::download_local_vad_model,
            transcription::vad_models::cancel_local_vad_model_download,
            transcription::vad_models::delete_local_vad_model,
            transcription::vad_settings::get_vad_preset,
            transcription::vad_settings::set_vad_preset,
            pending_action::get_pending_action,
            pending_action::receive_pending_action,
            pending_action::acknowledge_pending_action,
            pending_action::discard_pending_action,
            commands::transcribe::select_audio_file,
            commands::transcribe::get_transcription_session,
            audio_waveform::get_selected_audio_waveform,
            commands::transcribe::transcribe_selected_audio,
            commands::transcribe::get_selected_transcription_history,
            commands::transcribe::get_selected_transcription_run,
            commands::transcribe::select_transcription_run,
            commands::transcribe::update_transcript_document,
            commands::transcribe::reset_transcript_document,
            summary::get_summary_providers,
            summary::list_summary_agent_install_status,
            summary::install_summary_agent,
            summary::delete_summary_agent,
            summary::get_selected_summary,
            summary::generate_selected_summary,
            commands::usage::get_transcription_usage,
            commands::recording::get_recording_capabilities,
            commands::recording::get_recording_status,
            commands::recording::get_recorded_audio,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::cancel_recording,
            commands::recording::list_recoverable_recordings,
            commands::recording::list_recorded_audio,
            commands::recording::list_recent_meetings,
            commands::recording::select_recorded_audio,
            commands::recording::select_meeting_audio,
            commands::recording::reveal_recorded_audio,
            commands::recording::reveal_meeting_audio,
            commands::recording::recover_recording,
            commands::recording::discard_recording,
            #[cfg(desktop)]
            meeting_detection::get_meeting_detection,
            #[cfg(desktop)]
            meeting_detection::dismiss_meeting_overlay,
            #[cfg(desktop)]
            meeting_detection::wait_for_overlay_pointer_release,
            #[cfg(all(desktop, debug_assertions))]
            meeting_detection::show_overlay_preview,
            #[cfg(all(desktop, debug_assertions))]
            meeting_detection::get_overlay_preview_mode,
            #[cfg(all(desktop, debug_assertions))]
            meeting_detection::close_overlay_preview,
            #[cfg(desktop)]
            resident::prepare_transcription_handoff
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
