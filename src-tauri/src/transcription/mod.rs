pub(crate) mod elevenlabs;
pub(crate) mod local_models;
pub(crate) mod providers;
pub mod types;

use std::path::Path;

use tauri::AppHandle;

pub use types::{Transcript, TranscriptSegment, TranscriptionProvider};

pub(crate) async fn transcribe(
    app: &AppHandle,
    audio_path: &Path,
    provider: TranscriptionProvider,
    model_id: Option<&str>,
) -> Result<Transcript, String> {
    match provider {
        TranscriptionProvider::ElevenLabs => {
            if model_id.is_some_and(|model| model != "scribe_v2") {
                return Err("選択したElevenLabsモデルには対応していません。".into());
            }
            let api_key = crate::credentials::load_api_key(app)?;
            elevenlabs::transcribe(audio_path, &api_key).await
        }
        TranscriptionProvider::Local => {
            let installed = local_models::list_installed(app)?;
            let model = match model_id {
                Some(model_id) => installed.iter().find(|model| model.model_id == model_id),
                None => installed.first(),
            }
            .ok_or_else(|| "選択したローカルSTTモデルがインストールされていません。".to_string())?;
            Err(format!(
                "ローカルSTTモデル「{}」は検出されていますが、推論エンジンはまだ利用できません。",
                model.display_name
            ))
        }
    }
}
