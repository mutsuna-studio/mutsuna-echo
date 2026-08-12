#[cfg(any(target_os = "android", test))]
mod android_context;
mod android_update;
mod audio_enhancement;
mod audio_playback;
mod audio_waveform;
mod cloudflare_auth;
mod commands;
mod compute_tuning;
mod credentials;
mod inference_cache;
#[allow(dead_code)]
mod local_ai_protocol;
mod local_ai_runtime;
#[cfg(desktop)]
mod meeting_detection;
mod meeting_schema;
mod meeting_store;
mod pcm_cache;
mod pending_action;
mod processing_metrics;
mod processing_power;
mod recording;
#[cfg(desktop)]
mod resident;
mod summary;
mod transcript_store;
pub mod transcription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_process::init());
    #[cfg(not(target_os = "android"))]
    let builder = builder.register_asynchronous_uri_scheme_protocol(
        "mutsuna-audio",
        |context, request, responder| {
            let app = context.app_handle().clone();
            let webview_label = context.webview_label().to_string();
            std::thread::spawn(move || {
                responder.respond(audio_playback::response(&app, &webview_label, request));
            });
        },
    );
    let builder = builder
        .manage(commands::transcribe::AudioSelectionState::default())
        .manage(cloudflare_auth::CloudflareAuthState::default())
        .manage(processing_power::ProcessingPowerState::default())
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
            android_update::get_android_update_status,
            android_update::check_android_update,
            android_update::start_android_update,
            android_update::complete_android_update,
            local_ai_runtime::get_local_ai_runtime_status,
            local_ai_runtime::install_local_ai_runtime,
            local_ai_runtime::cancel_local_ai_runtime_install,
            local_ai_runtime::delete_local_ai_runtime,
            local_ai_runtime::install_local_transcription_bundle,
            local_ai_runtime::cancel_local_transcription_bundle_install,
            commands::api_key::save_api_key,
            commands::api_key::has_api_key,
            commands::api_key::delete_api_key,
            commands::api_key::save_provider_api_key,
            commands::api_key::delete_provider_api_key,
            cloudflare_auth::get_cloudflare_connection_status,
            cloudflare_auth::start_cloudflare_oauth,
            cloudflare_auth::select_cloudflare_oauth_account,
            cloudflare_auth::disconnect_cloudflare_oauth,
            transcription::providers::get_transcription_providers,
            transcription::providers::list_installed_local_stt_models,
            transcription::local_models::list_local_stt_model_catalog,
            transcription::local_models::download_local_stt_model,
            transcription::local_models::cancel_local_stt_model_download,
            transcription::local_models::delete_local_stt_model,
            transcription::diarization_models::get_local_diarization_model_status,
            transcription::diarization_models::download_local_diarization_models,
            transcription::diarization_models::cancel_local_diarization_model_download,
            transcription::diarization_models::delete_local_diarization_models,
            transcription::vad_models::get_local_vad_model_status,
            transcription::vad_models::download_local_vad_model,
            transcription::vad_models::cancel_local_vad_model_download,
            transcription::vad_models::delete_local_vad_model,
            transcription::vad_settings::get_vad_preset,
            transcription::vad_settings::set_vad_preset,
            transcription::local_settings::get_local_recognition_settings,
            transcription::local_settings::set_local_recognition_settings,
            transcription::context::get_global_transcription_context,
            transcription::context::set_global_transcription_context,
            transcription::context::get_meeting_transcription_context,
            transcription::context::set_meeting_transcription_context,
            pending_action::get_pending_action,
            pending_action::receive_pending_action,
            pending_action::acknowledge_pending_action,
            pending_action::discard_pending_action,
            commands::transcribe::select_audio_file,
            commands::transcribe::get_transcription_session,
            processing_power::get_processing_power_settings,
            processing_power::set_processing_power_settings,
            audio_playback::get_audio_playback_backend,
            audio_playback::load_selected_audio_for_playback,
            audio_playback::play_selected_audio,
            audio_playback::pause_selected_audio,
            audio_playback::seek_selected_audio,
            audio_playback::get_audio_playback_state,
            audio_playback::set_audio_playback_volume,
            audio_playback::set_audio_playback_rate,
            audio_playback::release_audio_playback,
            audio_waveform::get_selected_audio_waveform,
            commands::transcribe::transcribe_selected_audio,
            commands::transcribe::get_selected_transcription_history,
            commands::transcribe::get_selected_transcription_run,
            commands::transcribe::select_transcription_run,
            commands::transcribe::update_transcript_document,
            commands::transcribe::reset_transcript_document,
            commands::transcribe::diarize_selected_transcription,
            commands::transcribe::cancel_selected_diarization,
            summary::get_summary_providers,
            summary::get_summary_models,
            summary::list_summary_agent_install_status,
            summary::install_summary_agent,
            summary::delete_summary_agent,
            summary::get_selected_meeting_document,
            summary::save_edited_meeting_document,
            summary::get_latest_generation_attempt,
            summary::generate_selected_meeting_document,
            summary::revalidate_generation_attempt,
            summary::format_selected_transcript,
            commands::usage::get_transcription_usage,
            commands::usage::get_soniox_usage,
            commands::usage::get_cloudflare_usage,
            commands::recording::get_recording_capabilities,
            commands::recording::get_recording_status,
            commands::recording::start_recording_monitor,
            commands::recording::stop_recording_monitor,
            commands::recording::get_recorded_audio,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::cancel_recording,
            commands::recording::list_recoverable_recordings,
            commands::recording::list_recorded_audio,
            commands::recording::list_recent_meetings,
            commands::recording::select_recorded_audio,
            commands::recording::select_meeting_audio,
            commands::recording::delete_meeting,
            commands::recording::reveal_recorded_audio,
            commands::recording::reveal_meeting_audio,
            commands::recording::rename_meeting_audio,
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
