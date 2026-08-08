use serde::Serialize;
use tauri::AppHandle;

use super::local_models::InstalledLocalModel;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum ProviderKind {
    Cloud,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum ProviderSetup {
    ApiKey,
    ModelDownload,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum ProviderAvailability {
    Ready,
    ApiKeyRequired,
    ModelRequired,
    EngineUnavailable,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionProviderDescriptor {
    id: &'static str,
    label: &'static str,
    kind: ProviderKind,
    setup: ProviderSetup,
    availability: ProviderAvailability,
    ready: bool,
    configured: bool,
    model_id: Option<String>,
    model_label: String,
    capability_summary: &'static str,
    status_message: String,
    pricing_usd_per_hour: Option<f64>,
    pricing_verified_on: Option<&'static str>,
}

pub(crate) fn list(app: &AppHandle) -> Result<Vec<TranscriptionProviderDescriptor>, String> {
    let cloud = match crate::credentials::has_api_key(app) {
        Ok(has_api_key) => elevenlabs(has_api_key),
        Err(error) => unavailable_provider(
            "elevenlabs",
            "ElevenLabs",
            ProviderKind::Cloud,
            ProviderSetup::ApiKey,
            "Scribe v2",
            "日本語・話者分離・単語タイムスタンプ",
            format!("APIキーの保存状態を確認できませんでした: {error}"),
        ),
    };
    let local = match super::local_models::list_installed(app) {
        Ok(models) => local_provider(models.first()),
        Err(error) => unavailable_provider(
            "local",
            "ローカルSTT",
            ProviderKind::Local,
            ProviderSetup::ModelDownload,
            "ローカルモデル",
            "端末内処理・音声を外部送信しない",
            error,
        ),
    };
    Ok(vec![cloud, local])
}

fn unavailable_provider(
    id: &'static str,
    label: &'static str,
    kind: ProviderKind,
    setup: ProviderSetup,
    model_label: &str,
    capability_summary: &'static str,
    status_message: String,
) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id,
        label,
        kind,
        setup,
        availability: ProviderAvailability::Unavailable,
        ready: false,
        configured: false,
        model_id: None,
        model_label: model_label.into(),
        capability_summary,
        status_message,
        pricing_usd_per_hour: None,
        pricing_verified_on: None,
    }
}

fn elevenlabs(has_api_key: bool) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id: "elevenlabs",
        label: "ElevenLabs",
        kind: ProviderKind::Cloud,
        setup: ProviderSetup::ApiKey,
        availability: if has_api_key {
            ProviderAvailability::Ready
        } else {
            ProviderAvailability::ApiKeyRequired
        },
        ready: has_api_key,
        configured: has_api_key,
        model_id: Some("scribe_v2".into()),
        model_label: "Scribe v2".into(),
        capability_summary: "日本語・話者分離・単語タイムスタンプ",
        status_message: if has_api_key {
            "クラウドAPIで文字起こしできます。".into()
        } else {
            "ElevenLabsのAPIキーを設定してください。".into()
        },
        pricing_usd_per_hour: Some(0.22),
        pricing_verified_on: Some("2026-08-08"),
    }
}

fn local_provider(installed: Option<&InstalledLocalModel>) -> TranscriptionProviderDescriptor {
    let (availability, configured, model_id, model_label, status_message) = match installed {
        Some(model) => (
            ProviderAvailability::EngineUnavailable,
            true,
            Some(model.model_id.clone()),
            model.display_name.clone(),
            "モデルを検出しました。ローカル推論エンジンは次の実装で有効になります。".into(),
        ),
        None => (
            ProviderAvailability::ModelRequired,
            false,
            None,
            "ダウンロードしたモデルを使用".into(),
            "任意ダウンロード型モデルの管理基盤を準備済みです。ダウンロードUIはまだ利用できません。"
                .into(),
        ),
    };
    TranscriptionProviderDescriptor {
        id: "local",
        label: "ローカルSTT",
        kind: ProviderKind::Local,
        setup: ProviderSetup::ModelDownload,
        availability,
        ready: false,
        configured,
        model_id,
        model_label,
        capability_summary: "端末内処理・音声を外部送信しない",
        status_message,
        pricing_usd_per_hour: None,
        pricing_verified_on: None,
    }
}

#[tauri::command]
pub(crate) fn get_transcription_providers(
    app: AppHandle,
) -> Result<Vec<TranscriptionProviderDescriptor>, String> {
    list(&app)
}

#[tauri::command]
pub(crate) fn list_installed_local_stt_models(
    app: AppHandle,
) -> Result<Vec<InstalledLocalModel>, String> {
    super::local_models::list_installed(&app)
}

#[cfg(test)]
mod tests {
    use super::{local_provider, ProviderAvailability, ProviderKind, ProviderSetup};

    #[test]
    fn local_provider_requires_an_external_model() {
        let provider = local_provider(None);
        assert!(!provider.ready);
        assert!(!provider.configured);
        assert!(matches!(provider.kind, ProviderKind::Local));
        assert!(matches!(provider.setup, ProviderSetup::ModelDownload));
        assert!(matches!(
            provider.availability,
            ProviderAvailability::ModelRequired
        ));
        assert!(provider.pricing_usd_per_hour.is_none());
    }
}
