pub(crate) mod audio_decode;
pub(crate) mod cloudflare;
pub(crate) mod context;
pub mod diarization;
pub(crate) mod diarization_models;
pub(crate) mod elevenlabs;
#[cfg(any(desktop, target_os = "android"))]
mod local;
#[cfg(any(desktop, target_os = "android"))]
pub(crate) mod local_diarization;
pub(crate) mod local_models;
pub(crate) mod local_settings;
pub(crate) mod mutsuna_cloud;
pub(crate) mod providers;
pub(crate) mod soniox;
pub mod types;
#[cfg(any(desktop, target_os = "android"))]
pub(crate) mod vad;
pub(crate) mod vad_models;
pub(crate) mod vad_settings;

use std::{cmp::Ordering, collections::BTreeSet, path::Path};

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
    meeting_id: &str,
    audio_path: &Path,
    audio_duration_ms: u64,
    provider: TranscriptionProvider,
    model_id: Option<&str>,
    context: Option<&context::TranscriptionContext>,
) -> Result<TranscriptionOutcome, String> {
    if provider == TranscriptionProvider::MutsunaCloud {
        // The first hosted MVP intentionally uploads only the selected mixed
        // track. This avoids silently charging separate microphone/system jobs.
        return transcribe_one(
            app,
            audio_path,
            audio_duration_ms,
            provider,
            model_id,
            context,
        )
        .await;
    }
    let tracks = crate::meeting_store::recording_tracks(app, meeting_id)?;
    let mut sources = Vec::new();
    if let Some(path) = tracks.microphone {
        sources.push((path, RecordingChannel::Microphone));
    }
    if let Some(path) = tracks.system {
        sources.push((path, RecordingChannel::System));
    }
    if sources.is_empty() {
        return transcribe_one(
            app,
            audio_path,
            audio_duration_ms,
            provider,
            model_id,
            context,
        )
        .await;
    }

    let mut outcomes = Vec::with_capacity(sources.len());
    for (path, channel) in sources {
        let duration_ms = crate::commands::transcribe::audio_duration_ms(&path)?;
        let mut outcome =
            transcribe_one(app, &path, duration_ms, provider, model_id, context).await?;
        label_channel(&mut outcome.transcript, channel);
        outcomes.push(outcome);
    }
    Ok(merge_channel_outcomes(outcomes))
}

#[derive(Debug, Clone, Copy)]
enum RecordingChannel {
    Microphone,
    System,
}

async fn transcribe_one(
    app: &AppHandle,
    audio_path: &Path,
    audio_duration_ms: u64,
    provider: TranscriptionProvider,
    model_id: Option<&str>,
    context: Option<&context::TranscriptionContext>,
) -> Result<TranscriptionOutcome, String> {
    match provider {
        TranscriptionProvider::ElevenLabs => {
            if model_id.is_some_and(|model| model != "scribe_v2") {
                return Err("選択したElevenLabsモデルには対応していません。".into());
            }
            let api_key = crate::credentials::load_api_key(app)?;
            let transcript = elevenlabs::transcribe(audio_path, &api_key, context).await?;
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
            soniox::transcribe(audio_path, &api_key, context).await
        }
        TranscriptionProvider::Cloudflare => {
            if model_id.is_some_and(|model| model != cloudflare::MODEL_ID) {
                return Err("選択したCloudflare Workers AIモデルには対応していません。".into());
            }
            let auth = crate::cloudflare_auth::resolve_valid_credentials(app).await?;
            let _auth_method = auth.auth_method;
            cloudflare::transcribe(
                app,
                audio_path,
                audio_duration_ms,
                &auth.account_id,
                &auth.access_token,
                context,
            )
            .await
        }
        TranscriptionProvider::MutsunaCloud => {
            if model_id.is_some_and(|model| model != mutsuna_cloud::MODEL_ID) {
                return Err("選択したMutsuna Cloudモデルには対応していません。".into());
            }
            mutsuna_cloud::transcribe(app, audio_path, audio_duration_ms).await
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
            vad_models::ensure_installed(app).await.map_err(|error| {
                format!("文字起こしに必要なVADモデルを準備できませんでした: {error}")
            })?;
            #[cfg(any(desktop, target_os = "android"))]
            {
                let app = app.clone();
                let audio_path = audio_path.to_path_buf();
                let model_id = model.model_id.clone();
                let context = context.cloned();
                let transcript = tauri::async_runtime::spawn_blocking(move || {
                    local::transcribe(
                        &app,
                        &audio_path,
                        audio_duration_ms,
                        &model_id,
                        context.as_ref(),
                    )
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
            #[cfg(not(any(desktop, target_os = "android")))]
            {
                let _ = (audio_path, model);
                Err("この端末ではReazonSpeechのローカル推論をまだ利用できません。".into())
            }
        }
    }
}

fn label_channel(transcript: &mut Transcript, channel: RecordingChannel) {
    let provider_speakers = transcript
        .tokens
        .iter()
        .filter_map(|token| token.speaker.clone())
        .chain(
            transcript
                .segments
                .iter()
                .map(|segment| segment.speaker.clone()),
        )
        .collect::<BTreeSet<_>>();
    let channel_speaker = |provider_speaker: Option<&str>| match channel {
        RecordingChannel::Microphone => "自分".to_string(),
        RecordingChannel::System if provider_speakers.len() <= 1 => "相手".to_string(),
        RecordingChannel::System => provider_speaker
            .and_then(|speaker| provider_speakers.iter().position(|value| value == speaker))
            .map(|index| format!("相手 {}", index + 1))
            .unwrap_or_else(|| "相手".into()),
    };
    for token in &mut transcript.tokens {
        token.speaker = Some(channel_speaker(token.speaker.as_deref()));
        token.speaker_source = Some(TokenSpeakerSource::Channel);
    }
    if transcript.tokens.is_empty() {
        for segment in &mut transcript.segments {
            segment.speaker = channel_speaker(Some(&segment.speaker));
        }
    } else {
        transcript.segments = segments_from_tokens(&transcript.tokens);
    }
}

fn merge_channel_outcomes(mut outcomes: Vec<TranscriptionOutcome>) -> TranscriptionOutcome {
    let mut first = outcomes.remove(0);
    let mut cost = first
        .cost_usd
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok());
    for mut outcome in outcomes {
        first
            .transcript
            .tokens
            .append(&mut outcome.transcript.tokens);
        first
            .transcript
            .segments
            .append(&mut outcome.transcript.segments);
        if let Some(value) = outcome
            .cost_usd
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
        {
            cost = Some(cost.unwrap_or(0.0) + value);
        }
    }
    if first.transcript.tokens.is_empty() {
        first
            .transcript
            .segments
            .sort_by_key(|segment| segment.start_ms);
    } else {
        first.transcript.tokens.sort_by(|left, right| {
            left.start_ms
                .cmp(&right.start_ms)
                .then_with(|| left.end_ms.cmp(&right.end_ms))
                .then_with(
                    || match (left.speaker.as_deref(), right.speaker.as_deref()) {
                        (Some("自分"), Some("相手")) => Ordering::Less,
                        (Some("相手"), Some("自分")) => Ordering::Greater,
                        _ => Ordering::Equal,
                    },
                )
        });
        first.transcript.segments = segments_from_tokens(&first.transcript.tokens);
    }
    first.cost_usd = cost.map(|value| format!("{value:.6}"));
    first
}

#[cfg(test)]
mod tests {
    use super::{label_channel, merge_channel_outcomes, RecordingChannel, TranscriptionOutcome};
    use crate::transcription::{TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptToken};

    fn outcome(text: &str, start_ms: u64, cost: Option<&str>) -> TranscriptionOutcome {
        let token = TranscriptToken {
            text: text.into(),
            start_ms: Some(start_ms),
            end_ms: Some(start_ms + 300),
            start_time_source: Some(TokenTimeSource::Provider),
            end_time_source: Some(TokenTimeSource::Provider),
            speaker: Some("Speaker 1".into()),
            speaker_source: Some(TokenSpeakerSource::Provider),
            confidence: None,
            utterance_id: None,
        };
        TranscriptionOutcome {
            transcript: Transcript {
                provider: "test".into(),
                model: "test".into(),
                language: "ja".into(),
                tokens: vec![token.clone()],
                segments: crate::transcription::segments_from_tokens(&[token]),
            },
            cost_usd: cost.map(str::to_string),
        }
    }

    #[test]
    fn channel_transcripts_are_speaker_labeled_and_merged_on_one_timeline() {
        let mut microphone = outcome("質問", 1_000, Some("0.10"));
        let mut system = outcome("回答", 900, Some("0.20"));
        label_channel(&mut microphone.transcript, RecordingChannel::Microphone);
        label_channel(&mut system.transcript, RecordingChannel::System);
        let merged = merge_channel_outcomes(vec![microphone, system]);
        assert_eq!(merged.transcript.tokens[0].speaker.as_deref(), Some("相手"));
        assert_eq!(merged.transcript.tokens[1].speaker.as_deref(), Some("自分"));
        assert!(merged
            .transcript
            .tokens
            .iter()
            .all(|token| token.speaker_source == Some(TokenSpeakerSource::Channel)));
        assert_eq!(merged.cost_usd.as_deref(), Some("0.300000"));
    }
}
