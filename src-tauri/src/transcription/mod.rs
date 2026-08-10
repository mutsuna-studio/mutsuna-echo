pub(crate) mod audio_decode;
pub mod diarization;
pub(crate) mod elevenlabs;
#[cfg(desktop)]
mod local;
pub(crate) mod local_models;
pub(crate) mod providers;
pub(crate) mod soniox;
pub mod types;
#[cfg(desktop)]
pub(crate) mod vad;
pub(crate) mod vad_models;
pub(crate) mod vad_settings;

use std::path::Path;

use tauri::AppHandle;

pub(crate) use types::{
    normalize_transcript_for_display, repair_inferred_token_ends, segments_from_tokens,
    DISPLAY_SEGMENTATION_VERSION,
};
pub use types::{
    TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
    TranscriptionProvider,
};

pub(crate) struct TranscriptionOutcome {
    pub(crate) transcript: Transcript,
    pub(crate) cost_usd: Option<String>,
}

pub(crate) async fn transcribe(
    app: &AppHandle,
    audio_path: &Path,
    provider: TranscriptionProvider,
    model_id: Option<&str>,
) -> Result<TranscriptionOutcome, String> {
    match provider {
        TranscriptionProvider::ElevenLabs => {
            if model_id.is_some_and(|model| model != "scribe_v2") {
                return Err("選択したElevenLabsモデルには対応していません。".into());
            }
            let api_key = crate::credentials::load_api_key(app)?;
            let transcript = elevenlabs::transcribe(audio_path, &api_key).await?;
            Ok(TranscriptionOutcome {
                transcript,
                cost_usd: None,
            })
        }
        TranscriptionProvider::Soniox => {
            if model_id.is_some_and(|model| model != soniox::MODEL_ID) {
                return Err("選択したSonioxモデルには対応していません。".into());
            }
            let api_key = crate::credentials::load(app, crate::credentials::CredentialId::Soniox)?;
            soniox::transcribe(audio_path, &api_key).await
        }
        TranscriptionProvider::Local => {
            let installed = local_models::list_installed(app)?;
            let model = match model_id {
                Some(model_id) => installed.iter().find(|model| model.model_id == model_id),
                None => installed
                    .iter()
                    .find(|model| model.model_id == local_models::REAZONSPEECH_MODEL_ID),
            }
            .ok_or_else(|| "選択したローカルSTTモデルがインストールされていません。".to_string())?;
            #[cfg(desktop)]
            {
                let app = app.clone();
                let audio_path = audio_path.to_path_buf();
                let model_id = model.model_id.clone();
                let transcript = tauri::async_runtime::spawn_blocking(move || {
                    local::transcribe(&app, &audio_path, &model_id)
                })
                .await
                .map_err(|error| {
                    format!("ローカル文字起こし処理を完了できませんでした: {error}")
                })??;
                Ok(TranscriptionOutcome {
                    transcript,
                    cost_usd: None,
                })
            }
            #[cfg(not(desktop))]
            {
                let _ = (audio_path, model);
                Err("この端末ではReazonSpeechのローカル推論をまだ利用できません。".into())
            }
        }
    }
}
