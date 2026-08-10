use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as FmtWrite,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::transcription::{
    normalize_transcript_for_display, Transcript, TranscriptionProvider,
    DISPLAY_SEGMENTATION_VERSION,
};

const SCHEMA_VERSION: u8 = 4;
const COMPATIBLE_SCHEMA_VERSIONS: [u8; 3] = [2, 3, SCHEMA_VERSION];
const HISTORY_SCHEMA_VERSION: u8 = 1;
const RUN_SCHEMA_VERSION: u8 = 1;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEGMENT_TEXT_BYTES: usize = 256 * 1024;
const MAX_SPEAKER_LABEL_BYTES: usize = 1_024;
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscript {
    schema_version: u8,
    #[serde(default)]
    meeting_id: Option<String>,
    #[serde(rename = "savedAt")]
    saved_at: String,
    transcript: Transcript,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionRunSummary {
    pub(crate) transcription_id: String,
    pub(crate) sequence: u32,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) language: String,
    pub(crate) edited: bool,
    #[serde(default)]
    pub(crate) cost_usd: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditableTranscriptSegment {
    pub(crate) segment_id: String,
    pub(crate) speaker: String,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) text: String,
    pub(crate) edited: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptDocument {
    revision: u64,
    updated_at: String,
    edited: bool,
    segmentation_version: u32,
    #[serde(default)]
    speaker_labels: BTreeMap<String, String>,
    segments: Vec<EditableTranscriptSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptSpeakerLabel {
    speaker: String,
    label: String,
    edited: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionSettingsSnapshot {
    model_version: Option<String>,
    vad_preset: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscriptionRun {
    schema_version: u8,
    transcription_id: String,
    meeting_id: String,
    sequence: u32,
    created_at: String,
    provider: String,
    model: String,
    language: String,
    #[serde(default)]
    settings: TranscriptionSettingsSnapshot,
    #[serde(default)]
    cost_usd: Option<String>,
    source: Transcript,
    document: TranscriptDocument,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditableTranscript {
    provider: String,
    model: String,
    language: String,
    tokens: Vec<crate::transcription::TranscriptToken>,
    speaker_labels: Vec<TranscriptSpeakerLabel>,
    segments: Vec<EditableTranscriptSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionRunDetail {
    pub(crate) transcription_id: String,
    pub(crate) sequence: u32,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) revision: u64,
    pub(crate) edited: bool,
    pub(crate) cost_usd: Option<String>,
    pub(crate) transcript: EditableTranscript,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryTranscriptSegment {
    pub(crate) segment_id: String,
    pub(crate) speaker: String,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryTranscriptSnapshot {
    pub(crate) meeting_id: String,
    pub(crate) transcription_id: String,
    pub(crate) revision: u64,
    pub(crate) language: String,
    pub(crate) segments: Vec<SummaryTranscriptSegment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionHistoryIndex {
    schema_version: u8,
    meeting_id: String,
    next_sequence: u32,
    selected_transcription_id: Option<String>,
    runs: Vec<TranscriptionRunSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionHistory {
    runs: Vec<TranscriptionRunSummary>,
    selected_transcription_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptSegmentChange {
    segment_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptSpeakerLabelChange {
    speaker: String,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredTranscriptRef<'a> {
    schema_version: u8,
    meeting_id: &'a str,
    saved_at: String,
    transcript: &'a Transcript,
}

#[derive(Default)]
pub(crate) struct TranscriptIndex {
    meeting_providers: HashSet<String>,
    legacy_file_names: HashSet<String>,
}

impl TranscriptIndex {
    pub(crate) fn load(app: &AppHandle) -> Result<Self, String> {
        let mut index = Self::default();
        let meetings = crate::meeting_store::meetings_directory(app)?;
        if let Ok(entries) = fs::read_dir(meetings) {
            for entry in entries.filter_map(Result::ok) {
                let Some(meeting_id) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if crate::meeting_store::validate_meeting_id(&meeting_id).is_err() {
                    continue;
                }
                let transcripts = entry.path().join("transcripts");
                if let Ok(history) =
                    read_history_index(&transcripts.join("index.json"), &meeting_id)
                {
                    for run in history.runs {
                        index
                            .meeting_providers
                            .insert(meeting_provider_key(&meeting_id, &run.provider));
                    }
                }
                let Ok(files) = fs::read_dir(transcripts) else {
                    continue;
                };
                for file in files.filter_map(Result::ok) {
                    let Some(provider) = transcript_provider_from_file_name(&file.file_name())
                    else {
                        continue;
                    };
                    index
                        .meeting_providers
                        .insert(meeting_provider_key(&meeting_id, &provider));
                }
            }
        }

        let legacy = legacy_transcripts_directory(app)?;
        if let Ok(entries) = fs::read_dir(legacy) {
            index.legacy_file_names = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
        }
        Ok(index)
    }

    pub(crate) fn providers_for_meeting(
        &self,
        meeting_id: &str,
        legacy_audio_path: Option<&Path>,
    ) -> Vec<String> {
        let legacy_key = legacy_audio_path.and_then(|path| audio_key(path).ok());
        TranscriptionProvider::ALL
            .into_iter()
            .filter(|provider| {
                self.meeting_providers
                    .contains(&meeting_provider_key(meeting_id, provider.id()))
                    || legacy_key.as_ref().is_some_and(|key| {
                        let primary = format!("{key}.{}.json", provider.id());
                        self.legacy_file_names.contains(&primary)
                            || self
                                .legacy_file_names
                                .contains(&format!("{primary}.backup"))
                            || (*provider == TranscriptionProvider::ElevenLabs
                                && (self.legacy_file_names.contains(&format!("{key}.json"))
                                    || self
                                        .legacy_file_names
                                        .contains(&format!("{key}.json.backup"))))
                    })
            })
            .map(|provider| provider.id().to_string())
            .collect()
    }
}

pub(crate) fn load(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    if let Some(transcript) = load_current_in(&directory, meeting_id, provider)? {
        return Ok(Some(transcript));
    }

    let legacy_directory = legacy_transcripts_directory(app)?;
    let Some(transcript) = load_legacy_in(&legacy_directory, audio_path, provider)? else {
        return Ok(None);
    };
    // 旧形式は残したまま、新しいMeeting配下へコピーして段階的に移行する。
    save_in(&directory, meeting_id, &transcript)?;
    Ok(Some(transcript))
}

pub(crate) fn history(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: Option<&Path>,
) -> Result<TranscriptionHistory, String> {
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    if !directory.join("index.json").exists() {
        if let Some(audio_path) = audio_path {
            migrate_global_legacy_transcripts(app, meeting_id, audio_path)?;
        }
    }
    let _guard = store_guard()?;
    let index = ensure_history_in(&directory, meeting_id)?;
    Ok(TranscriptionHistory {
        runs: index.runs,
        selected_transcription_id: index.selected_transcription_id,
    })
}

pub(crate) fn create_run(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
    transcript: &Transcript,
    cost_usd: Option<String>,
) -> Result<TranscriptionRunDetail, String> {
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    if !directory.join("index.json").exists() {
        migrate_global_legacy_transcripts(app, meeting_id, audio_path)?;
    }
    let _guard = store_guard()?;
    let mut index = ensure_history_in(&directory, meeting_id)?;
    let transcription_id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let sequence = index.next_sequence;
    let run = StoredTranscriptionRun {
        schema_version: RUN_SCHEMA_VERSION,
        transcription_id: transcription_id.clone(),
        meeting_id: meeting_id.to_string(),
        sequence,
        created_at: now.clone(),
        provider: transcript.provider.clone(),
        model: transcript.model.clone(),
        language: transcript.language.clone(),
        settings: settings_snapshot(app, transcript),
        cost_usd,
        source: transcript.clone(),
        document: document_from_transcript(transcript, now),
    };
    write_run(&directory, &run)?;
    index.next_sequence = sequence.saturating_add(1);
    index.selected_transcription_id = Some(transcription_id);
    index.runs.push(run_summary(&run));
    write_history_index(&directory, &index)?;
    Ok(run_detail(&run))
}

pub(crate) fn selected_run(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<Option<TranscriptionRunDetail>, String> {
    let _guard = store_guard()?;
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    let index = ensure_history_in(&directory, meeting_id)?;
    let Some(transcription_id) = index.selected_transcription_id else {
        return Ok(None);
    };
    read_run(&directory, meeting_id, &transcription_id).map(|run| Some(run_detail(&run)))
}

pub(crate) fn selected_summary_snapshot(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<Option<SummaryTranscriptSnapshot>, String> {
    let _guard = store_guard()?;
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    let index = ensure_history_in(&directory, meeting_id)?;
    let Some(transcription_id) = index.selected_transcription_id else {
        return Ok(None);
    };
    let run = read_run(&directory, meeting_id, &transcription_id)?;
    let segments = run
        .document
        .segments
        .iter()
        .map(|segment| SummaryTranscriptSegment {
            segment_id: segment.segment_id.clone(),
            speaker: run
                .document
                .speaker_labels
                .get(&segment.speaker)
                .filter(|label| !label.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| segment.speaker.clone()),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
        })
        .collect();
    Ok(Some(SummaryTranscriptSnapshot {
        meeting_id: meeting_id.to_string(),
        transcription_id,
        revision: run.document.revision,
        language: run.language,
        segments,
    }))
}

pub(crate) fn select_run(
    app: &AppHandle,
    meeting_id: &str,
    transcription_id: &str,
) -> Result<TranscriptionRunDetail, String> {
    validate_transcription_id(transcription_id)?;
    let _guard = store_guard()?;
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    let mut index = ensure_history_in(&directory, meeting_id)?;
    if !index
        .runs
        .iter()
        .any(|run| run.transcription_id == transcription_id)
    {
        return Err("選択した文字起こし履歴が見つかりません。".into());
    }
    let run = read_run(&directory, meeting_id, transcription_id)?;
    if index.selected_transcription_id.as_deref() != Some(transcription_id) {
        index.selected_transcription_id = Some(transcription_id.to_string());
        write_history_index(&directory, &index)?;
    }
    Ok(run_detail(&run))
}

pub(crate) fn update_run_segments(
    app: &AppHandle,
    meeting_id: &str,
    transcription_id: &str,
    expected_revision: u64,
    changes: Vec<TranscriptSegmentChange>,
    speaker_labels: Vec<TranscriptSpeakerLabelChange>,
    learn_correction_segment_ids: Vec<String>,
) -> Result<TranscriptionRunDetail, String> {
    validate_transcription_id(transcription_id)?;
    if changes.is_empty() && speaker_labels.is_empty() {
        return Err("保存する文字起こしの変更がありません。".into());
    }
    let _guard = store_guard()?;
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    let mut index = ensure_history_in(&directory, meeting_id)?;
    let mut run = read_run(&directory, meeting_id, transcription_id)?;
    let learn_correction_segment_ids = learn_correction_segment_ids
        .into_iter()
        .collect::<HashSet<_>>();
    let (text_changed, learned_corrections) = apply_segment_changes(
        &mut run,
        expected_revision,
        changes,
        &learn_correction_segment_ids,
    )?;
    let speaker_changed = apply_speaker_label_changes(&mut run, expected_revision, speaker_labels)?;
    let changed = text_changed || speaker_changed;
    if changed {
        let now = chrono::Utc::now().to_rfc3339();
        run.document.revision = run.document.revision.saturating_add(1);
        run.document.updated_at = now;
        run.document.edited = true;
        write_run(&directory, &run)?;
        if let Some(summary) = index
            .runs
            .iter_mut()
            .find(|summary| summary.transcription_id == transcription_id)
        {
            *summary = run_summary(&run);
        }
        write_history_index(&directory, &index)?;
        if let Err(error) =
            crate::transcription::context::learn_corrections(app, learned_corrections)
        {
            eprintln!("手動修正を学習辞書へ保存できませんでした: {error}");
        }
    }
    Ok(run_detail(&run))
}

pub(crate) fn reset_run_document(
    app: &AppHandle,
    meeting_id: &str,
    transcription_id: &str,
    expected_revision: u64,
) -> Result<TranscriptionRunDetail, String> {
    validate_transcription_id(transcription_id)?;
    let _guard = store_guard()?;
    let directory = crate::meeting_store::meeting_directory(app, meeting_id)?.join("transcripts");
    let mut index = ensure_history_in(&directory, meeting_id)?;
    let mut run = read_run(&directory, meeting_id, transcription_id)?;
    reset_document_from_source(&mut run, expected_revision, chrono::Utc::now().to_rfc3339())?;
    write_run(&directory, &run)?;
    if let Some(summary) = index
        .runs
        .iter_mut()
        .find(|summary| summary.transcription_id == transcription_id)
    {
        *summary = run_summary(&run);
    }
    write_history_index(&directory, &index)?;
    Ok(run_detail(&run))
}

fn reset_document_from_source(
    run: &mut StoredTranscriptionRun,
    expected_revision: u64,
    updated_at: String,
) -> Result<(), String> {
    if run.document.revision != expected_revision {
        return Err(
            "文字起こしが別の操作で更新されました。再読み込みしてからやり直してください。".into(),
        );
    }
    let next_revision = run.document.revision.saturating_add(1);
    run.document = document_from_transcript(&run.source, updated_at);
    run.document.revision = next_revision;
    Ok(())
}

fn apply_segment_changes(
    run: &mut StoredTranscriptionRun,
    expected_revision: u64,
    changes: Vec<TranscriptSegmentChange>,
    learn_correction_segment_ids: &HashSet<String>,
) -> Result<(bool, Vec<crate::transcription::context::TextCorrection>), String> {
    if run.document.revision != expected_revision {
        return Err(
            "文字起こしが別の操作で更新されました。再読み込みしてから編集してください。".into(),
        );
    }
    let mut changed = false;
    let mut learned_corrections = Vec::new();
    let mut seen = HashSet::new();
    for change in changes {
        validate_transcription_id(&change.segment_id)?;
        if !seen.insert(change.segment_id.clone()) {
            return Err("同じ発話区間の変更が重複しています。".into());
        }
        if change.text.len() > MAX_SEGMENT_TEXT_BYTES {
            return Err("1つの発話区間に保存できる文字数を超えています。".into());
        }
        let segment = run
            .document
            .segments
            .iter_mut()
            .find(|segment| segment.segment_id == change.segment_id)
            .ok_or_else(|| "編集対象の発話区間が見つかりません。".to_string())?;
        if segment.text != change.text {
            if learn_correction_segment_ids.contains(&change.segment_id) {
                if let Some(correction) = crate::transcription::context::correction_from_manual_edit(
                    &segment.text,
                    &change.text,
                ) {
                    learned_corrections.push(correction);
                }
            }
            segment.text = change.text;
            segment.edited = true;
            changed = true;
        }
    }
    Ok((changed, learned_corrections))
}

fn apply_speaker_label_changes(
    run: &mut StoredTranscriptionRun,
    expected_revision: u64,
    changes: Vec<TranscriptSpeakerLabelChange>,
) -> Result<bool, String> {
    if run.document.revision != expected_revision {
        return Err(
            "文字起こしが別の操作で更新されました。再読み込みしてから編集してください。".into(),
        );
    }
    let known_speakers: HashSet<_> = run
        .document
        .segments
        .iter()
        .map(|segment| segment.speaker.as_str())
        .collect();
    let mut changed = false;
    let mut seen = HashSet::new();
    for change in changes {
        if !known_speakers.contains(change.speaker.as_str()) {
            return Err("編集対象の話者が見つかりません。".into());
        }
        if !seen.insert(change.speaker.clone()) {
            return Err("同じ話者のラベル変更が重複しています。".into());
        }
        let label = change.label.trim();
        if label.len() > MAX_SPEAKER_LABEL_BYTES {
            return Err("話者ラベルが長すぎます。".into());
        }
        let next_label = (!label.is_empty() && label != change.speaker).then(|| label.to_string());
        let current_label = run.document.speaker_labels.get(&change.speaker).cloned();
        if current_label != next_label {
            if let Some(label) = next_label {
                run.document.speaker_labels.insert(change.speaker, label);
            } else {
                run.document.speaker_labels.remove(&change.speaker);
            }
            changed = true;
        }
    }
    Ok(changed)
}

fn migrate_global_legacy_transcripts(
    app: &AppHandle,
    meeting_id: &str,
    audio_path: &Path,
) -> Result<(), String> {
    for provider in TranscriptionProvider::ALL {
        let _ = load(app, meeting_id, audio_path, provider)?;
    }
    Ok(())
}

fn store_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    STORE_LOCK
        .lock()
        .map_err(|_| "文字起こし履歴の保存状態を取得できませんでした。".to_string())
}

fn ensure_history_in(
    directory: &Path,
    meeting_id: &str,
) -> Result<TranscriptionHistoryIndex, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let path = directory.join("index.json");
    if path.exists() {
        return read_history_index(&path, meeting_id);
    }
    let mut legacy = Vec::new();
    for provider in TranscriptionProvider::ALL {
        let provider_path = transcript_path_in(directory, meeting_id, provider.id())?;
        let Some(path) = [
            provider_path.clone(),
            provider_path.with_extension("json.backup"),
        ]
        .into_iter()
        .find(|path| path.exists()) else {
            continue;
        };
        let stored = read_stored_transcript(&path)?;
        if !COMPATIBLE_SCHEMA_VERSIONS.contains(&stored.schema_version)
            || stored.meeting_id.as_deref() != Some(meeting_id)
        {
            return Err("保存済みの文字起こし形式またはMeeting IDが一致しません。".into());
        }
        validate_stored_provider(&stored, provider)?;
        legacy.push(stored);
    }
    legacy.sort_by(|left, right| left.saved_at.cmp(&right.saved_at));

    let mut index = TranscriptionHistoryIndex {
        schema_version: HISTORY_SCHEMA_VERSION,
        meeting_id: meeting_id.to_string(),
        next_sequence: 1,
        selected_transcription_id: None,
        runs: Vec::new(),
    };
    for mut stored in legacy {
        normalize_transcript_for_display(&mut stored.transcript);
        let transcription_id = uuid::Uuid::now_v7().to_string();
        let sequence = index.next_sequence;
        let run = StoredTranscriptionRun {
            schema_version: RUN_SCHEMA_VERSION,
            transcription_id: transcription_id.clone(),
            meeting_id: meeting_id.to_string(),
            sequence,
            created_at: stored.saved_at.clone(),
            provider: stored.transcript.provider.clone(),
            model: stored.transcript.model.clone(),
            language: stored.transcript.language.clone(),
            settings: TranscriptionSettingsSnapshot::default(),
            cost_usd: None,
            document: document_from_transcript(&stored.transcript, stored.saved_at),
            source: stored.transcript,
        };
        write_run(directory, &run)?;
        index.next_sequence = sequence.saturating_add(1);
        index.selected_transcription_id = Some(transcription_id);
        index.runs.push(run_summary(&run));
    }
    write_history_index(directory, &index)?;
    Ok(index)
}

fn document_from_transcript(transcript: &Transcript, updated_at: String) -> TranscriptDocument {
    TranscriptDocument {
        revision: 0,
        updated_at,
        edited: false,
        segmentation_version: DISPLAY_SEGMENTATION_VERSION,
        speaker_labels: BTreeMap::new(),
        segments: transcript
            .segments
            .iter()
            .map(|segment| EditableTranscriptSegment {
                segment_id: uuid::Uuid::now_v7().to_string(),
                speaker: segment.speaker.clone(),
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text.clone(),
                edited: false,
            })
            .collect(),
    }
}

fn settings_snapshot(app: &AppHandle, transcript: &Transcript) -> TranscriptionSettingsSnapshot {
    if transcript.provider != TranscriptionProvider::Local.id() {
        return TranscriptionSettingsSnapshot::default();
    }
    let model_version = crate::transcription::local_models::list_installed(app)
        .ok()
        .and_then(|models| {
            models
                .into_iter()
                .find(|model| model.model_id == transcript.model)
                .map(|model| model.version)
        });
    let vad_preset = crate::transcription::vad_settings::current_preset(app)
        .ok()
        .and_then(|preset| serde_json::to_value(preset).ok())
        .and_then(|value| value.as_str().map(str::to_string));
    TranscriptionSettingsSnapshot {
        model_version,
        vad_preset,
    }
}

fn run_summary(run: &StoredTranscriptionRun) -> TranscriptionRunSummary {
    TranscriptionRunSummary {
        transcription_id: run.transcription_id.clone(),
        sequence: run.sequence,
        created_at: run.created_at.clone(),
        updated_at: run.document.updated_at.clone(),
        provider: run.provider.clone(),
        model: run.model.clone(),
        language: run.language.clone(),
        edited: run.document.edited,
        cost_usd: run.cost_usd.clone(),
    }
}

fn run_detail(run: &StoredTranscriptionRun) -> TranscriptionRunDetail {
    let mut seen_speakers = HashSet::new();
    let speakers: Vec<_> = run
        .document
        .segments
        .iter()
        .map(|segment| segment.speaker.clone())
        .filter(|speaker| seen_speakers.insert(speaker.clone()))
        .collect();
    TranscriptionRunDetail {
        transcription_id: run.transcription_id.clone(),
        sequence: run.sequence,
        created_at: run.created_at.clone(),
        updated_at: run.document.updated_at.clone(),
        revision: run.document.revision,
        edited: run.document.edited,
        cost_usd: run.cost_usd.clone(),
        transcript: EditableTranscript {
            provider: run.provider.clone(),
            model: run.model.clone(),
            language: run.language.clone(),
            tokens: run.source.tokens.clone(),
            speaker_labels: speakers
                .into_iter()
                .map(|speaker| {
                    let label = run
                        .document
                        .speaker_labels
                        .get(&speaker)
                        .cloned()
                        .unwrap_or_else(|| speaker.clone());
                    TranscriptSpeakerLabel {
                        edited: label != speaker,
                        speaker,
                        label,
                    }
                })
                .collect(),
            segments: run.document.segments.clone(),
        },
    }
}

fn runs_directory(directory: &Path) -> PathBuf {
    directory.join("runs")
}

fn run_path(directory: &Path, transcription_id: &str) -> Result<PathBuf, String> {
    validate_transcription_id(transcription_id)?;
    Ok(runs_directory(directory).join(format!("{transcription_id}.json")))
}

fn validate_transcription_id(value: &str) -> Result<(), String> {
    let id =
        uuid::Uuid::parse_str(value).map_err(|_| "文字起こし履歴IDが不正です。".to_string())?;
    if id.get_version() != Some(uuid::Version::SortRand) || id.to_string() != value {
        return Err("文字起こし履歴IDが不正です。".into());
    }
    Ok(())
}

fn write_run(directory: &Path, run: &StoredTranscriptionRun) -> Result<(), String> {
    write_json_atomically(&run_path(directory, &run.transcription_id)?, run)
}

fn read_run(
    directory: &Path,
    meeting_id: &str,
    transcription_id: &str,
) -> Result<StoredTranscriptionRun, String> {
    let path = run_path(directory, transcription_id)?;
    let mut run: StoredTranscriptionRun = read_bounded_json(&path)?;
    if run.schema_version != RUN_SCHEMA_VERSION
        || run.meeting_id != meeting_id
        || run.transcription_id != transcription_id
        || run.provider != run.source.provider
        || run.model != run.source.model
        || run.language != run.source.language
    {
        return Err("保存済みの文字起こし履歴が一致しません。".into());
    }
    let mut segment_ids = HashSet::new();
    let mut known_speakers = HashSet::new();
    for segment in &run.document.segments {
        validate_transcription_id(&segment.segment_id)?;
        if !segment_ids.insert(&segment.segment_id)
            || segment.text.len() > MAX_SEGMENT_TEXT_BYTES
            || segment.end_ms < segment.start_ms
        {
            return Err("保存済みの文字起こし編集データが不正です。".into());
        }
        known_speakers.insert(segment.speaker.as_str());
    }
    if run.document.speaker_labels.iter().any(|(speaker, label)| {
        !known_speakers.contains(speaker.as_str())
            || label.trim().is_empty()
            || label.len() > MAX_SPEAKER_LABEL_BYTES
    }) {
        return Err("保存済みの話者ラベルが不正です。".into());
    }
    if run.document.segmentation_version < DISPLAY_SEGMENTATION_VERSION
        && !run.document.edited
        && run.document.speaker_labels.is_empty()
    {
        let mut transcript = run.source.clone();
        normalize_transcript_for_display(&mut transcript);
        let revision = run.document.revision;
        let updated_at = run.document.updated_at.clone();
        run.document = document_from_transcript(&transcript, updated_at);
        run.document.revision = revision;
        write_run(directory, &run)?;
    }
    Ok(run)
}

fn write_history_index(directory: &Path, index: &TranscriptionHistoryIndex) -> Result<(), String> {
    write_json_atomically(&directory.join("index.json"), index)
}

fn read_history_index(path: &Path, meeting_id: &str) -> Result<TranscriptionHistoryIndex, String> {
    let index: TranscriptionHistoryIndex = read_bounded_json(path)?;
    if index.schema_version != HISTORY_SCHEMA_VERSION || index.meeting_id != meeting_id {
        return Err("保存済みの文字起こし履歴形式またはMeeting IDが一致しません。".into());
    }
    let mut ids = HashSet::new();
    let mut sequences = HashSet::new();
    for run in &index.runs {
        validate_transcription_id(&run.transcription_id)?;
        validate_provider_id(&run.provider)?;
        if run.sequence == 0
            || !ids.insert(&run.transcription_id)
            || !sequences.insert(run.sequence)
        {
            return Err("保存済みの文字起こし履歴一覧が不正です。".into());
        }
    }
    let maximum_sequence = index.runs.iter().map(|run| run.sequence).max().unwrap_or(0);
    if index.next_sequence <= maximum_sequence {
        return Err("保存済みの文字起こし履歴の実行順が不正です。".into());
    }
    if index
        .selected_transcription_id
        .as_ref()
        .is_some_and(|selected| {
            !index
                .runs
                .iter()
                .any(|run| &run.transcription_id == selected)
        })
    {
        return Err("選択中の文字起こし履歴が一覧と一致しません。".into());
    }
    Ok(index)
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("保存済みの文字起こし履歴を確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err("保存済みの文字起こし履歴ファイルが不正です。".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("保存済みの文字起こし履歴を読み込めませんでした: {error}"))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("保存済みの文字起こし履歴が壊れています: {error}"))
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "文字起こし履歴の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("文字起こし履歴の保存先を作成できませんでした: {error}"))?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    let backup = path.with_extension("json.backup");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("文字起こし履歴を書き込めませんでした: {error}"))?;
    if let Err(error) = serde_json::to_writer(&mut file, value) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "文字起こし履歴をJSONへ変換できませんでした: {error}"
        ));
    }
    let written_bytes = file
        .metadata()
        .map_err(|error| format!("文字起こし履歴の保存サイズを確認できませんでした: {error}"))?
        .len();
    if written_bytes > MAX_TRANSCRIPT_BYTES {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err("文字起こし履歴が大きすぎるため保存できませんでした。".into());
    }
    file.sync_all()
        .map_err(|error| format!("文字起こし履歴を安全に書き込めませんでした: {error}"))?;
    drop(file);
    replace_with_backup(path, &temporary, &backup)
}

fn legacy_transcripts_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("transcripts"))
        .map_err(|error| format!("文字起こしの保存先を取得できませんでした: {error}"))
}

fn transcript_path_in(
    directory: &Path,
    meeting_id: &str,
    provider_id: &str,
) -> Result<PathBuf, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    validate_provider_id(provider_id)?;
    Ok(directory.join(format!("{provider_id}.json")))
}

fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    if provider_id.is_empty()
        || !provider_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err("文字起こしプロバイダーIDが不正です。".to_string());
    }
    Ok(())
}

fn legacy_transcript_path(directory: &Path, audio_path: &Path) -> Result<PathBuf, String> {
    Ok(directory.join(format!("{}.json", audio_key(audio_path)?)))
}

fn legacy_provider_path(
    directory: &Path,
    audio_path: &Path,
    provider_id: &str,
) -> Result<PathBuf, String> {
    validate_provider_id(provider_id)?;
    Ok(directory.join(format!("{}.{provider_id}.json", audio_key(audio_path)?)))
}

fn audio_key(audio_path: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(audio_path)
        .map_err(|error| format!("音声ファイルの保存識別子を作成できませんでした: {error}"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("音声ファイルの情報を取得できませんでした: {error}"))?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified_nanos.to_le_bytes());
    let digest = hasher.finalize();
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut key, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(key)
}

fn save_in(directory: &Path, meeting_id: &str, transcript: &Transcript) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("文字起こしの保存先を作成できませんでした: {error}"))?;
    let path = transcript_path_in(directory, meeting_id, &transcript.provider)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    let backup = path.with_extension("json.backup");
    let stored = StoredTranscriptRef {
        schema_version: SCHEMA_VERSION,
        meeting_id,
        saved_at: chrono::Utc::now().to_rfc3339(),
        transcript,
    };

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("文字起こしを書き込めませんでした: {error}"))?;
    if let Err(error) = serde_json::to_writer(&mut file, &stored) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!("文字起こしをJSONへ変換できませんでした: {error}"));
    }
    let written_bytes = file
        .metadata()
        .map_err(|error| format!("文字起こしの保存サイズを確認できませんでした: {error}"))?
        .len();
    if written_bytes > MAX_TRANSCRIPT_BYTES {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err("文字起こしが大きすぎるため保存できませんでした。".to_string());
    }
    file.sync_all()
        .map_err(|error| format!("文字起こしを安全に書き込めませんでした: {error}"))?;
    drop(file);
    replace_with_backup(&path, &temporary, &backup)
}

fn replace_with_backup(path: &Path, temporary: &Path, backup: &Path) -> Result<(), String> {
    if path.exists() {
        if backup.exists() {
            fs::remove_file(backup).map_err(|error| {
                format!("古い文字起こしのバックアップを削除できませんでした: {error}")
            })?;
        }
        fs::rename(path, backup)
            .map_err(|error| format!("文字起こしを更新用に退避できませんでした: {error}"))?;
    }
    if let Err(error) = fs::rename(temporary, path) {
        if backup.exists() {
            let _ = fs::rename(backup, path);
        }
        return Err(format!("文字起こしの保存を確定できませんでした: {error}"));
    }
    if backup.exists() {
        fs::remove_file(backup).map_err(|error| {
            format!("文字起こし保存後のバックアップを削除できませんでした: {error}")
        })?;
    }
    Ok(())
}

fn load_current_in(
    directory: &Path,
    meeting_id: &str,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let primary = transcript_path_in(directory, meeting_id, provider.id())?;
    let candidates = [primary.clone(), primary.with_extension("json.backup")];
    let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
        return Ok(None);
    };
    let stored = read_stored_transcript(&path)?;
    if !COMPATIBLE_SCHEMA_VERSIONS.contains(&stored.schema_version)
        || stored.meeting_id.as_deref() != Some(meeting_id)
    {
        return Err("保存済みの文字起こし形式またはMeeting IDが一致しません。".into());
    }
    validate_stored_provider(&stored, provider)?;
    let schema_version = stored.schema_version;
    let mut transcript = stored.transcript;
    let normalized = normalize_transcript_for_display(&mut transcript);
    if schema_version != SCHEMA_VERSION || normalized {
        save_in(directory, meeting_id, &transcript)?;
    }
    Ok(Some(transcript))
}

fn load_legacy_in(
    directory: &Path,
    audio_path: &Path,
    provider: TranscriptionProvider,
) -> Result<Option<Transcript>, String> {
    let primary = legacy_provider_path(directory, audio_path, provider.id())?;
    let mut candidates = vec![primary.clone(), primary.with_extension("json.backup")];
    if provider == TranscriptionProvider::ElevenLabs {
        let legacy = legacy_transcript_path(directory, audio_path)?;
        candidates.push(legacy.clone());
        candidates.push(legacy.with_extension("json.backup"));
    }
    let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
        return Ok(None);
    };
    let stored = read_stored_transcript(&path)?;
    if stored.schema_version != 1 {
        return Err("保存済みの旧文字起こし形式に対応していません。".into());
    }
    validate_stored_provider(&stored, provider)?;
    let mut transcript = stored.transcript;
    normalize_transcript_for_display(&mut transcript);
    Ok(Some(transcript))
}

fn read_stored_transcript(path: &Path) -> Result<StoredTranscript, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("保存済みの文字起こしを確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err("保存済みの文字起こしファイルが不正です。".to_string());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("保存済みの文字起こしを読み込めませんでした: {error}"))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("保存済みの文字起こしが壊れています: {error}"))
}

fn validate_stored_provider(
    stored: &StoredTranscript,
    provider: TranscriptionProvider,
) -> Result<(), String> {
    if stored.transcript.provider != provider.id() {
        return Err("保存済みの文字起こしプロバイダーが一致しません。".to_string());
    }
    Ok(())
}

fn transcript_provider_from_file_name(name: &std::ffi::OsStr) -> Option<String> {
    let name = name.to_str()?;
    let provider = name
        .strip_suffix(".json")
        .or_else(|| name.strip_suffix(".json.backup"))?;
    validate_provider_id(provider).ok()?;
    Some(provider.to_string())
}

fn meeting_provider_key(meeting_id: &str, provider_id: &str) -> String {
    format!("{meeting_id}:{provider_id}")
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs};

    use super::{
        apply_segment_changes, apply_speaker_label_changes, audio_key, document_from_transcript,
        ensure_history_in, load_current_in, load_legacy_in, read_run, reset_document_from_source,
        save_in, transcript_path_in, StoredTranscriptionRun, TranscriptSegmentChange,
        TranscriptSpeakerLabelChange, TranscriptionSettingsSnapshot, RUN_SCHEMA_VERSION,
        SCHEMA_VERSION,
    };
    use crate::transcription::{
        TokenSpeakerSource, TokenTimeSource, Transcript, TranscriptSegment, TranscriptToken,
        TranscriptionProvider,
    };

    fn fixture_transcript() -> Transcript {
        Transcript {
            provider: "elevenlabs".into(),
            model: "scribe_v2".into(),
            language: "ja".into(),
            tokens: vec![TranscriptToken {
                text: "テストです。".into(),
                start_ms: Some(100),
                end_ms: Some(500),
                start_time_source: Some(TokenTimeSource::Provider),
                end_time_source: Some(TokenTimeSource::Provider),
                speaker: Some("Speaker 1".into()),
                speaker_source: Some(TokenSpeakerSource::Provider),
                confidence: None,
            }],
            segments: vec![TranscriptSegment {
                speaker: "Speaker 1".into(),
                start_ms: 100,
                end_ms: 500,
                text: "テストです。".into(),
            }],
        }
    }

    #[test]
    fn transcript_round_trips_by_meeting_id() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-store-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        let transcript = fixture_transcript();
        save_in(&directory, &meeting_id, &transcript).expect("save transcript");
        assert_eq!(
            load_current_in(&directory, &meeting_id, TranscriptionProvider::ElevenLabs)
                .expect("load transcript"),
            Some(transcript)
        );
        let path =
            transcript_path_in(&directory, &meeting_id, "elevenlabs").expect("transcript path");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("elevenlabs.json")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn providers_get_distinct_storage_paths() {
        let root = std::path::Path::new("transcripts");
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let elevenlabs = transcript_path_in(root, &meeting_id, "elevenlabs").expect("path");
        let assemblyai = transcript_path_in(root, &meeting_id, "assemblyai").expect("path");
        assert_ne!(elevenlabs, assemblyai);
    }

    #[test]
    fn provider_files_migrate_to_ordered_transcription_runs() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-history-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        save_in(&directory, &meeting_id, &fixture_transcript()).expect("save legacy provider file");

        let history = ensure_history_in(&directory, &meeting_id).expect("migrate history");
        assert_eq!(history.runs.len(), 1);
        assert_eq!(history.runs[0].sequence, 1);
        assert_eq!(history.runs[0].model, "scribe_v2");
        assert_eq!(
            history.selected_transcription_id,
            Some(history.runs[0].transcription_id.clone())
        );
        let run = read_run(&directory, &meeting_id, &history.runs[0].transcription_id)
            .expect("read migrated run");
        assert_eq!(run.source.segments[0].text, "テストです。");
        assert_eq!(run.document.segments[0].text, "テストです。");
        assert!(!run.document.edited);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn edits_preserve_source_and_reject_stale_revisions() {
        let transcript = fixture_transcript();
        let transcription_id = uuid::Uuid::now_v7().to_string();
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let mut run = StoredTranscriptionRun {
            schema_version: RUN_SCHEMA_VERSION,
            transcription_id,
            meeting_id,
            sequence: 1,
            created_at: "2026-08-09T00:00:00Z".into(),
            provider: transcript.provider.clone(),
            model: transcript.model.clone(),
            language: transcript.language.clone(),
            settings: TranscriptionSettingsSnapshot::default(),
            cost_usd: None,
            document: document_from_transcript(&transcript, "2026-08-09T00:00:00Z".into()),
            source: transcript,
        };
        let segment_id = run.document.segments[0].segment_id.clone();
        let learn_ids = HashSet::from([segment_id.clone()]);
        let (changed, learned) = apply_segment_changes(
            &mut run,
            0,
            vec![TranscriptSegmentChange {
                segment_id: segment_id.clone(),
                text: "Mutsuna Echoです。".into(),
            }],
            &learn_ids,
        )
        .expect("apply edit");
        assert!(changed);
        assert_eq!(learned.len(), 1);
        assert_eq!(learned[0].from, "テスト");
        assert_eq!(learned[0].to, "Mutsuna Echo");
        assert_eq!(run.source.segments[0].text, "テストです。");
        assert_eq!(run.document.segments[0].text, "Mutsuna Echoです。");
        assert!(apply_speaker_label_changes(
            &mut run,
            0,
            vec![TranscriptSpeakerLabelChange {
                speaker: "Speaker 1".into(),
                label: "田中".into(),
            }],
        )
        .expect("apply speaker label"));
        assert_eq!(
            run.document
                .speaker_labels
                .get("Speaker 1")
                .map(String::as_str),
            Some("田中")
        );
        run.document.revision = 1;
        assert!(apply_segment_changes(
            &mut run,
            0,
            vec![TranscriptSegmentChange {
                segment_id,
                text: "競合".into(),
            }],
            &HashSet::new(),
        )
        .is_err());
        reset_document_from_source(&mut run, 1, "2026-08-09T01:00:00Z".into())
            .expect("reset document");
        assert_eq!(run.document.revision, 2);
        assert!(!run.document.edited);
        assert_eq!(run.document.segments[0].text, "テストです。");
        assert!(run.document.speaker_labels.is_empty());
    }

    #[test]
    fn batch_edits_update_multiple_segments_without_changing_source() {
        let mut transcript = fixture_transcript();
        transcript.segments.push(TranscriptSegment {
            speaker: "Speaker 1".into(),
            start_ms: 600,
            end_ms: 900,
            text: "二つ目です。".into(),
        });
        let mut run = StoredTranscriptionRun {
            schema_version: RUN_SCHEMA_VERSION,
            transcription_id: uuid::Uuid::now_v7().to_string(),
            meeting_id: uuid::Uuid::now_v7().to_string(),
            sequence: 1,
            created_at: "2026-08-09T00:00:00Z".into(),
            provider: transcript.provider.clone(),
            model: transcript.model.clone(),
            language: transcript.language.clone(),
            settings: TranscriptionSettingsSnapshot::default(),
            cost_usd: None,
            document: document_from_transcript(&transcript, "2026-08-09T00:00:00Z".into()),
            source: transcript,
        };
        let changes = run
            .document
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| TranscriptSegmentChange {
                segment_id: segment.segment_id.clone(),
                text: format!("修正{}", index + 1),
            })
            .collect();

        assert!(
            apply_segment_changes(&mut run, 0, changes, &HashSet::new())
                .expect("apply batch edit")
                .0
        );
        assert_eq!(run.document.segments[0].text, "修正1");
        assert_eq!(run.document.segments[1].text, "修正2");
        assert_eq!(run.source.segments[0].text, "テストです。");
        assert_eq!(run.source.segments[1].text, "二つ目です。");
    }

    #[test]
    fn schema_v2_without_tokens_remains_readable() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-v2-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        fs::create_dir_all(&directory).expect("create store");
        let stored = serde_json::json!({
            "schemaVersion": 2,
            "meetingId": meeting_id,
            "savedAt": "2026-08-08T00:00:00Z",
            "transcript": {
                "provider": "elevenlabs",
                "model": "scribe_v2",
                "language": "ja",
                "segments": [{
                    "speaker": "Speaker 1",
                    "startMs": 100,
                    "endMs": 500,
                    "text": "旧データ"
                }]
            }
        });
        fs::write(
            directory.join("elevenlabs.json"),
            serde_json::to_vec(&stored).expect("serialize transcript"),
        )
        .expect("write transcript");
        let loaded = load_current_in(&directory, &meeting_id, TranscriptionProvider::ElevenLabs)
            .expect("load v2 transcript")
            .expect("stored transcript");
        assert!(loaded.tokens.is_empty());
        assert_eq!(loaded.segments[0].text, "旧データ");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_v3_local_transcript_is_resegmented_and_persisted() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-v3-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let directory = root.join("transcripts");
        fs::create_dir_all(&directory).expect("create store");
        let stored = serde_json::json!({
            "schemaVersion": 3,
            "meetingId": meeting_id,
            "savedAt": "2026-08-08T00:00:00Z",
            "transcript": {
                "provider": "local",
                "model": "reazonspeech-k2-int8-fp32",
                "language": "ja",
                "tokens": [
                    {
                        "text": "会議",
                        "startMs": 100,
                        "endMs": 100,
                        "startTimeSource": "provider",
                        "endTimeSource": "inferred",
                        "speaker": null,
                        "speakerSource": null,
                        "confidence": null
                    },
                    {
                        "text": "です",
                        "startMs": 2000,
                        "endMs": 2400,
                        "startTimeSource": "provider",
                        "endTimeSource": "inferred",
                        "speaker": null,
                        "speakerSource": null,
                        "confidence": null
                    }
                ],
                "segments": [
                    {"speaker": "Speaker 1", "startMs": 100, "endMs": 100, "text": "会議"},
                    {"speaker": "Speaker 1", "startMs": 2000, "endMs": 2400, "text": "です"}
                ]
            }
        });
        let path = directory.join("local.json");
        fs::write(
            &path,
            serde_json::to_vec(&stored).expect("serialize transcript"),
        )
        .expect("write transcript");

        let loaded = load_current_in(&directory, &meeting_id, TranscriptionProvider::Local)
            .expect("load v3 transcript")
            .expect("stored transcript");
        assert_eq!(loaded.segments.len(), 1);
        assert_eq!(loaded.segments[0].text, "会議です");
        let migrated: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("read migrated transcript"))
                .expect("parse migrated transcript");
        assert_eq!(migrated["schemaVersion"], SCHEMA_VERSION);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_v1_transcript_remains_readable_for_migration() {
        let root = std::env::temp_dir().join(format!(
            "mutsuna-transcript-legacy-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let audio = root.join("meeting.m4a");
        let legacy = root.join("transcripts");
        fs::create_dir_all(&legacy).expect("create legacy store");
        fs::write(&audio, b"legacy audio").expect("write audio");
        let transcript = fixture_transcript();
        let stored = serde_json::json!({
            "schemaVersion": 1,
            "savedAt": "2026-08-08T00:00:00Z",
            "transcript": transcript
        });
        let key = audio_key(&audio).expect("legacy audio key");
        fs::write(
            legacy.join(format!("{key}.elevenlabs.json")),
            serde_json::to_vec(&stored).expect("serialize legacy transcript"),
        )
        .expect("write legacy transcript");
        assert_eq!(
            load_legacy_in(&legacy, &audio, TranscriptionProvider::ElevenLabs)
                .expect("load legacy transcript"),
            Some(fixture_transcript())
        );
        let _ = fs::remove_dir_all(root);
    }
}
