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
    OAuthOrApiKey,
    CloudAccount,
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TimingGranularity {
    Token,
    Word,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionCapabilities {
    timing_granularity: TimingGranularity,
    speaker_labels: bool,
    confidence_scores: bool,
    external_diarization: bool,
    context_text: bool,
    context_terms: bool,
}

const ELEVENLABS_CAPABILITIES: TranscriptionCapabilities = TranscriptionCapabilities {
    timing_granularity: TimingGranularity::Word,
    speaker_labels: true,
    confidence_scores: false,
    external_diarization: true,
    context_text: false,
    context_terms: true,
};

const SONIOX_CAPABILITIES: TranscriptionCapabilities = TranscriptionCapabilities {
    timing_granularity: TimingGranularity::Token,
    speaker_labels: true,
    confidence_scores: true,
    external_diarization: true,
    context_text: true,
    context_terms: true,
};

const REAZONSPEECH_CAPABILITIES: TranscriptionCapabilities = TranscriptionCapabilities {
    timing_granularity: TimingGranularity::Token,
    speaker_labels: false,
    confidence_scores: false,
    external_diarization: true,
    context_text: false,
    context_terms: true,
};

const CLOUDFLARE_CAPABILITIES: TranscriptionCapabilities = TranscriptionCapabilities {
    timing_granularity: TimingGranularity::Word,
    speaker_labels: false,
    confidence_scores: false,
    external_diarization: true,
    context_text: true,
    context_terms: true,
};

const MUTSUNA_CLOUD_CAPABILITIES: TranscriptionCapabilities = TranscriptionCapabilities {
    timing_granularity: TimingGranularity::Word,
    speaker_labels: false,
    confidence_scores: false,
    external_diarization: true,
    context_text: false,
    context_terms: false,
};

#[derive(Debug, Clone, Copy)]
struct ProviderDefinition {
    id: &'static str,
    label: &'static str,
    kind: ProviderKind,
    setup: ProviderSetup,
    default_model_label: &'static str,
    capability_summary: &'static str,
    capabilities: TranscriptionCapabilities,
}

const ELEVENLABS_DEFINITION: ProviderDefinition = ProviderDefinition {
    id: "elevenlabs",
    label: "ElevenLabs",
    kind: ProviderKind::Cloud,
    setup: ProviderSetup::ApiKey,
    default_model_label: "Scribe v2",
    capability_summary: "日本語・話者分離・単語タイムスタンプ",
    capabilities: ELEVENLABS_CAPABILITIES,
};

const SONIOX_DEFINITION: ProviderDefinition = ProviderDefinition {
    id: "soniox",
    label: "Soniox",
    kind: ProviderKind::Cloud,
    setup: ProviderSetup::ApiKey,
    default_model_label: "Soniox v5",
    capability_summary: "多言語・話者分離・トークンタイムスタンプ",
    capabilities: SONIOX_CAPABILITIES,
};

const CLOUDFLARE_DEFINITION: ProviderDefinition = ProviderDefinition {
    id: "cloudflare",
    label: "Cloudflare Free",
    kind: ProviderKind::Cloud,
    setup: ProviderSetup::OAuthOrApiKey,
    default_model_label: "Whisper Large v3 Turbo",
    capability_summary: "多言語・単語タイムスタンプ・無料枠",
    capabilities: CLOUDFLARE_CAPABILITIES,
};

const MUTSUNA_CLOUD_DEFINITION: ProviderDefinition = ProviderDefinition {
    id: "mutsunaCloud",
    label: "Mutsuna Cloud",
    kind: ProviderKind::Cloud,
    setup: ProviderSetup::CloudAccount,
    default_model_label: "Mutsuna STT Standard",
    capability_summary: "APIキー不要・クレジット制・タイムスタンプ",
    capabilities: MUTSUNA_CLOUD_CAPABILITIES,
};

const LOCAL_DEFINITION: ProviderDefinition = ProviderDefinition {
    id: "local",
    label: "ローカルSTT",
    kind: ProviderKind::Local,
    setup: ProviderSetup::ModelDownload,
    default_model_label: "ローカルモデル",
    capability_summary: "端末内処理・音声を外部送信しない",
    capabilities: REAZONSPEECH_CAPABILITIES,
};

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
    capabilities: TranscriptionCapabilities,
    status_message: String,
    pricing_usd_per_hour: Option<f64>,
    pricing_verified_on: Option<&'static str>,
}

pub(crate) fn list(app: &AppHandle) -> Result<Vec<TranscriptionProviderDescriptor>, String> {
    let elevenlabs = match crate::credentials::has_api_key(app) {
        Ok(has_api_key) => elevenlabs(has_api_key),
        Err(error) => unavailable_provider(
            ELEVENLABS_DEFINITION,
            format!("APIキーの保存状態を確認できませんでした: {error}"),
        ),
    };
    let soniox = match crate::credentials::has(app, crate::credentials::CredentialId::Soniox) {
        Ok(has_api_key) => soniox(has_api_key),
        Err(error) => unavailable_provider(
            SONIOX_DEFINITION,
            format!("APIキーの保存状態を確認できませんでした: {error}"),
        ),
    };
    let cloudflare = match crate::cloudflare_auth::is_configured(app) {
        Ok(configured) => cloudflare(configured),
        Err(error) => unavailable_provider(
            CLOUDFLARE_DEFINITION,
            format!("資格情報の保存状態を確認できませんでした: {error}"),
        ),
    };
    let mutsuna_cloud = match crate::mutsuna_cloud::cached_status(app) {
        Ok(status) => mutsuna_cloud(status),
        Err(error) => unavailable_provider(
            MUTSUNA_CLOUD_DEFINITION,
            format!("Mutsuna Cloudの接続状態を確認できませんでした: {error}"),
        ),
    };
    let vad_installed =
        super::vad_models::installed_model_path(app).is_ok_and(|path| path.is_some());
    let runtime_ready = crate::local_ai_runtime::is_installed_compatible(app);
    let local = match super::local_models::list_installed(app) {
        Ok(models) => local_provider(
            models
                .iter()
                .find(|model| model.model_id == super::local_models::REAZONSPEECH_MODEL_ID),
            vad_installed,
            runtime_ready,
        ),
        Err(error) => unavailable_provider(LOCAL_DEFINITION, error),
    };
    Ok(vec![mutsuna_cloud, elevenlabs, soniox, cloudflare, local])
}

fn unavailable_provider(
    definition: ProviderDefinition,
    status_message: String,
) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id: definition.id,
        label: definition.label,
        kind: definition.kind,
        setup: definition.setup,
        availability: ProviderAvailability::Unavailable,
        ready: false,
        configured: false,
        model_id: None,
        model_label: definition.default_model_label.into(),
        capability_summary: definition.capability_summary,
        capabilities: definition.capabilities,
        status_message,
        pricing_usd_per_hour: None,
        pricing_verified_on: None,
    }
}

fn elevenlabs(has_api_key: bool) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id: ELEVENLABS_DEFINITION.id,
        label: ELEVENLABS_DEFINITION.label,
        kind: ELEVENLABS_DEFINITION.kind,
        setup: ELEVENLABS_DEFINITION.setup,
        availability: if has_api_key {
            ProviderAvailability::Ready
        } else {
            ProviderAvailability::ApiKeyRequired
        },
        ready: has_api_key,
        configured: has_api_key,
        model_id: Some("scribe_v2".into()),
        model_label: ELEVENLABS_DEFINITION.default_model_label.into(),
        capability_summary: ELEVENLABS_DEFINITION.capability_summary,
        capabilities: ELEVENLABS_DEFINITION.capabilities,
        status_message: if has_api_key {
            "クラウドAPIで文字起こしできます。".into()
        } else {
            "ElevenLabsのAPIキーを設定してください。".into()
        },
        pricing_usd_per_hour: Some(0.22),
        pricing_verified_on: Some("2026-08-08"),
    }
}

fn soniox(has_api_key: bool) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id: SONIOX_DEFINITION.id,
        label: SONIOX_DEFINITION.label,
        kind: SONIOX_DEFINITION.kind,
        setup: SONIOX_DEFINITION.setup,
        availability: if has_api_key {
            ProviderAvailability::Ready
        } else {
            ProviderAvailability::ApiKeyRequired
        },
        ready: has_api_key,
        configured: has_api_key,
        model_id: Some(super::soniox::MODEL_ID.into()),
        model_label: SONIOX_DEFINITION.default_model_label.into(),
        capability_summary: SONIOX_DEFINITION.capability_summary,
        capabilities: SONIOX_DEFINITION.capabilities,
        status_message: if has_api_key {
            "クラウドAPIで文字起こしできます。".into()
        } else {
            "SonioxのAPIキーを設定してください。".into()
        },
        pricing_usd_per_hour: Some(0.10),
        pricing_verified_on: Some("2026-08-09"),
    }
}

fn cloudflare(configured: bool) -> TranscriptionProviderDescriptor {
    TranscriptionProviderDescriptor {
        id: CLOUDFLARE_DEFINITION.id,
        label: CLOUDFLARE_DEFINITION.label,
        kind: CLOUDFLARE_DEFINITION.kind,
        setup: CLOUDFLARE_DEFINITION.setup,
        availability: if configured {
            ProviderAvailability::Ready
        } else {
            ProviderAvailability::ApiKeyRequired
        },
        ready: configured,
        configured,
        model_id: Some(super::cloudflare::MODEL_ID.into()),
        model_label: CLOUDFLARE_DEFINITION.default_model_label.into(),
        capability_summary: CLOUDFLARE_DEFINITION.capability_summary,
        capabilities: CLOUDFLARE_DEFINITION.capabilities,
        status_message: if configured {
            "Cloudflare Workers AIの無料枠を利用できます。".into()
        } else {
            "Cloudflareへ接続してください。APIトークンも詳細設定から利用できます。".into()
        },
        pricing_usd_per_hour: Some(super::cloudflare::PRICE_USD_PER_AUDIO_MINUTE * 60.0),
        pricing_verified_on: Some("2026-08-11"),
    }
}

fn mutsuna_cloud(
    status: crate::mutsuna_cloud::MutsunaCloudStatus,
) -> TranscriptionProviderDescriptor {
    let configured = status.connected();
    let ready = configured && status.can_use();
    TranscriptionProviderDescriptor {
        id: MUTSUNA_CLOUD_DEFINITION.id,
        label: MUTSUNA_CLOUD_DEFINITION.label,
        kind: MUTSUNA_CLOUD_DEFINITION.kind,
        setup: MUTSUNA_CLOUD_DEFINITION.setup,
        availability: if ready {
            ProviderAvailability::Ready
        } else {
            ProviderAvailability::Unavailable
        },
        ready,
        configured,
        model_id: Some(super::mutsuna_cloud::MODEL_ID.into()),
        model_label: MUTSUNA_CLOUD_DEFINITION.default_model_label.into(),
        capability_summary: MUTSUNA_CLOUD_DEFINITION.capability_summary,
        capabilities: MUTSUNA_CLOUD_DEFINITION.capabilities,
        status_message: if ready {
            "Mutsuna Cloudのクレジットで文字起こしできます。".into()
        } else if configured {
            "Mutsuna Cloudのアカウント状態またはクレジット残高を確認してください。".into()
        } else {
            "Mutsuna Cloudへ接続するとAPIキーなしで利用できます。".into()
        },
        pricing_usd_per_hour: None,
        pricing_verified_on: None,
    }
}

fn local_provider(
    installed: Option<&InstalledLocalModel>,
    vad_installed: bool,
    runtime_ready: bool,
) -> TranscriptionProviderDescriptor {
    let engine_available = cfg!(any(desktop, target_os = "android"));
    let (availability, configured, model_id, model_label, status_message) = match installed {
        Some(model) => (
            if engine_available && vad_installed && runtime_ready {
                ProviderAvailability::Ready
            } else if engine_available && runtime_ready {
                ProviderAvailability::ModelRequired
            } else {
                ProviderAvailability::EngineUnavailable
            },
            true,
            Some(model.model_id.clone()),
            model.display_name.clone(),
            if engine_available && vad_installed && runtime_ready {
                "端末内で日本語を文字起こしできます。話者分離には未対応です。".into()
            } else if !runtime_ready {
                "利用を続けるには設定からローカルAI実行環境を追加してください。".into()
            } else if engine_available {
                "音声区間検出モデルの導入が完了すると会議ページで選択できます。".into()
            } else {
                "このOSではReazonSpeechの推論エンジンをまだ利用できません。".into()
            },
        ),
        None => (
            ProviderAvailability::ModelRequired,
            false,
            None,
            "ダウンロードしたモデルを使用".into(),
            "ReazonSpeech K2をダウンロードすると端末内で文字起こしできます。".into(),
        ),
    };
    TranscriptionProviderDescriptor {
        id: LOCAL_DEFINITION.id,
        label: LOCAL_DEFINITION.label,
        kind: LOCAL_DEFINITION.kind,
        setup: LOCAL_DEFINITION.setup,
        availability,
        ready: installed.is_some() && vad_installed && engine_available && runtime_ready,
        configured,
        model_id,
        model_label,
        capability_summary: LOCAL_DEFINITION.capability_summary,
        capabilities: LOCAL_DEFINITION.capabilities,
        status_message,
        pricing_usd_per_hour: None,
        pricing_verified_on: None,
    }
}

#[tauri::command]
pub(crate) async fn get_transcription_providers(
    app: AppHandle,
) -> Result<Vec<TranscriptionProviderDescriptor>, String> {
    // Keep `ready` tied to the server-authoritative account/credit state even
    // when this command races the dedicated status request during startup.
    let _ = crate::mutsuna_cloud::refresh_status(&app).await;
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
    use super::{
        local_provider, mutsuna_cloud, soniox, InstalledLocalModel, ProviderAvailability,
        ProviderKind, ProviderSetup, TimingGranularity,
    };

    #[test]
    fn local_provider_requires_an_external_model() {
        let provider = local_provider(None, false, false);
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

    #[test]
    fn installed_local_model_is_ready_when_the_native_engine_is_bundled() {
        let model = InstalledLocalModel {
            model_id: super::super::local_models::REAZONSPEECH_MODEL_ID.into(),
            version: "2024-08-01".into(),
            engine: "sherpa-onnx-transducer".into(),
            display_name: "ReazonSpeech K2 int8-fp32".into(),
            language_codes: vec!["ja".into()],
            size_bytes: 169_180_699,
        };
        let provider = local_provider(Some(&model), true, true);
        assert_eq!(provider.ready, cfg!(any(desktop, target_os = "android")));
        assert!(provider.configured);
        assert_eq!(
            provider.capabilities.timing_granularity,
            TimingGranularity::Token
        );
        assert!(!provider.capabilities.speaker_labels);
        assert!(provider.capabilities.external_diarization);
        assert!(!provider.capabilities.context_text);
        assert!(provider.capabilities.context_terms);
    }

    #[test]
    fn installed_local_model_is_hidden_from_meetings_until_vad_is_installed() {
        let model = InstalledLocalModel {
            model_id: super::super::local_models::REAZONSPEECH_MODEL_ID.into(),
            version: "2024-08-01".into(),
            engine: "sherpa-onnx-transducer".into(),
            display_name: "ReazonSpeech K2 int8-fp32".into(),
            language_codes: vec!["ja".into()],
            size_bytes: 169_180_699,
        };

        let provider = local_provider(Some(&model), false, true);

        assert!(!provider.ready);
        assert!(provider.configured);
        assert!(matches!(
            provider.availability,
            ProviderAvailability::ModelRequired
        ));
    }

    #[test]
    fn soniox_exposes_v5_provider_capabilities() {
        let provider = soniox(true);
        assert!(provider.ready);
        assert_eq!(provider.model_id.as_deref(), Some("stt-async-v5"));
        assert_eq!(
            provider.capabilities.timing_granularity,
            TimingGranularity::Token
        );
        assert!(provider.capabilities.speaker_labels);
        assert!(provider.capabilities.confidence_scores);
        assert!(provider.capabilities.context_text);
        assert!(provider.capabilities.context_terms);
        assert_eq!(provider.pricing_usd_per_hour, Some(0.10));
    }

    #[test]
    fn mutsuna_cloud_requires_a_native_account_connection() {
        let disconnected = mutsuna_cloud(crate::mutsuna_cloud::MutsunaCloudStatus::disconnected());
        assert!(!disconnected.ready);
        assert!(!disconnected.configured);
        assert!(matches!(disconnected.setup, ProviderSetup::CloudAccount));
        assert!(matches!(
            disconnected.availability,
            ProviderAvailability::Unavailable
        ));

        let connected = mutsuna_cloud(crate::mutsuna_cloud::MutsunaCloudStatus::for_test(true));
        assert!(connected.ready);
        assert!(connected.configured);
        assert_eq!(
            connected.model_id.as_deref(),
            Some("mutsuna-stt-standard-v1")
        );
        assert!(matches!(
            connected.availability,
            ProviderAvailability::Ready
        ));

        let unavailable = mutsuna_cloud(crate::mutsuna_cloud::MutsunaCloudStatus::for_test(false));
        assert!(unavailable.configured);
        assert!(!unavailable.ready);
        assert!(matches!(
            unavailable.availability,
            ProviderAvailability::Unavailable
        ));
    }
}
