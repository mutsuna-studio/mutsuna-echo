use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::{summary::GeneratedCandidate, transcript_store::SummaryTranscriptSnapshot};

pub(crate) type MeetingDocument = Value;
const SCHEMA_VERSION: &str = "1.0.0";
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
const ATTEMPT_SCHEMA_VERSION: &str = "1.0.0";
const MAX_ATTEMPT_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub(crate) struct GenerationAttempt {
    attempt_id: String,
    directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationAttemptManifest {
    schema_version: String,
    attempt_id: String,
    meeting_id: String,
    transcription_id: String,
    source_revision: u64,
    provider: String,
    requested_model: String,
    resolved_model: Option<String>,
    started_at: String,
    updated_at: String,
    completed_at: Option<String>,
    status: String,
    stage: String,
    error: Option<String>,
    artifacts: Vec<GenerationAttemptArtifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationAttemptArtifact {
    kind: String,
    stage: String,
    path: String,
    sha256: String,
    byte_length: u64,
    created_at: String,
    final_output: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerationAttemptSummary {
    pub(crate) attempt_id: String,
    pub(crate) transcription_id: String,
    pub(crate) source_revision: u64,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) started_at: String,
    pub(crate) status: String,
    pub(crate) stage: String,
    pub(crate) error: Option<String>,
    pub(crate) can_revalidate: bool,
}

pub(crate) fn begin_generation_attempt(
    app: &AppHandle,
    transcript: &SummaryTranscriptSnapshot,
    provider: &str,
    model: &str,
) -> Result<GenerationAttempt, String> {
    let _guard = lock_store()?;
    let attempts = attempts_directory(app, &transcript.meeting_id)?;
    fs::create_dir_all(&attempts)
        .map_err(|error| format!("生成試行の保存先を作成できませんでした: {error}"))?;
    let attempt_id = uuid::Uuid::now_v7().to_string();
    let directory = attempts.join(&attempt_id);
    fs::create_dir(&directory)
        .map_err(|error| format!("生成試行の保存先を作成できませんでした: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let manifest = GenerationAttemptManifest {
        schema_version: ATTEMPT_SCHEMA_VERSION.into(),
        attempt_id: attempt_id.clone(),
        meeting_id: transcript.meeting_id.clone(),
        transcription_id: transcript.transcription_id.clone(),
        source_revision: transcript.revision,
        provider: provider.into(),
        requested_model: model.into(),
        resolved_model: None,
        started_at: now.clone(),
        updated_at: now,
        completed_at: None,
        status: "generating".into(),
        stage: "starting".into(),
        error: None,
        artifacts: Vec::new(),
    };
    write_manifest(&directory, &manifest)?;
    Ok(GenerationAttempt {
        attempt_id,
        directory,
    })
}

impl GenerationAttempt {
    pub(crate) fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    pub(crate) fn record_response(
        &self,
        stage: &str,
        output: &str,
        final_output: bool,
    ) -> Result<(), String> {
        validate_attempt_stage(stage)?;
        let bytes = output.as_bytes();
        if bytes.len() as u64 > MAX_ATTEMPT_ARTIFACT_BYTES {
            return Err("AIの生成結果が大きすぎるため、診断用に保存できませんでした。".into());
        }
        let _guard = lock_store()?;
        let relative = format!("responses/{stage}.txt");
        let path = self.directory.join(&relative);
        create_parent_and_write_new(&path, bytes)?;
        self.add_artifact_locked(
            "rawResponse",
            stage,
            &relative,
            bytes,
            final_output,
            "received",
        )
    }

    pub(crate) fn record_candidate(
        &self,
        stage: &str,
        candidate: &Value,
        final_output: bool,
    ) -> Result<(), String> {
        validate_attempt_stage(stage)?;
        let bytes = serde_json::to_vec_pretty(candidate)
            .map_err(|error| format!("AIの解析結果を診断用に変換できませんでした: {error}"))?;
        if bytes.len() as u64 > MAX_ATTEMPT_ARTIFACT_BYTES {
            return Err("AIの解析結果が大きすぎるため、診断用に保存できませんでした。".into());
        }
        let _guard = lock_store()?;
        let relative = format!("candidates/{stage}.json");
        let path = self.directory.join(&relative);
        create_parent_and_write_new(&path, &bytes)?;
        self.add_artifact_locked(
            "candidate",
            stage,
            &relative,
            &bytes,
            final_output,
            "parsed",
        )
    }

    pub(crate) fn set_resolved_model(&self, model: &str) -> Result<(), String> {
        let _guard = lock_store()?;
        let mut manifest = read_manifest(&self.directory)?;
        manifest.resolved_model = Some(model.into());
        manifest.updated_at = chrono::Utc::now().to_rfc3339();
        write_manifest(&self.directory, &manifest)
    }

    pub(crate) fn fail(&self, stage: &str, error: &str) -> Result<(), String> {
        let _guard = lock_store()?;
        let mut manifest = read_manifest(&self.directory)?;
        if manifest.status == "completed" {
            return Ok(());
        }
        manifest.status = "failed".into();
        manifest.stage = stage.into();
        manifest.error = Some(error.chars().take(4_096).collect());
        manifest.updated_at = chrono::Utc::now().to_rfc3339();
        write_manifest(&self.directory, &manifest)
    }

    pub(crate) fn fail_if_active(&self, stage: &str, error: &str) -> Result<(), String> {
        let _guard = lock_store()?;
        let mut manifest = read_manifest(&self.directory)?;
        if manifest.status != "generating" {
            return Ok(());
        }
        manifest.status = "failed".into();
        manifest.stage = stage.into();
        manifest.error = Some(error.chars().take(4_096).collect());
        manifest.updated_at = chrono::Utc::now().to_rfc3339();
        write_manifest(&self.directory, &manifest)
    }

    pub(crate) fn complete(&self, model: &str) -> Result<(), String> {
        let _guard = lock_store()?;
        let mut manifest = read_manifest(&self.directory)?;
        let now = chrono::Utc::now().to_rfc3339();
        manifest.resolved_model = Some(model.into());
        manifest.status = "completed".into();
        manifest.stage = "persisted".into();
        manifest.error = None;
        manifest.updated_at = now.clone();
        manifest.completed_at = Some(now);
        write_manifest(&self.directory, &manifest)
    }

    fn add_artifact_locked(
        &self,
        kind: &str,
        stage: &str,
        relative: &str,
        bytes: &[u8],
        final_output: bool,
        manifest_stage: &str,
    ) -> Result<(), String> {
        let mut manifest = read_manifest(&self.directory)?;
        let now = chrono::Utc::now().to_rfc3339();
        manifest.stage = manifest_stage.into();
        manifest.updated_at = now.clone();
        manifest.artifacts.push(GenerationAttemptArtifact {
            kind: kind.into(),
            stage: stage.into(),
            path: relative.replace('\\', "/"),
            sha256: sha256(bytes),
            byte_length: bytes.len() as u64,
            created_at: now,
            final_output,
        });
        write_manifest(&self.directory, &manifest)
    }
}

pub(crate) fn latest_generation_attempt(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<Option<GenerationAttemptSummary>, String> {
    let attempts = attempts_directory(app, meeting_id)?;
    if !attempts.exists() {
        return Ok(None);
    }
    let mut directories = fs::read_dir(&attempts)
        .map_err(|error| format!("生成試行を確認できませんでした: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| entry.file_name());
    let Some(latest) = directories.last() else {
        return Ok(None);
    };
    let manifest = read_manifest(&latest.path())?;
    Ok(Some(manifest_summary(&manifest)))
}

pub(crate) fn load_attempt_final_candidate(
    app: &AppHandle,
    meeting_id: &str,
    attempt_id: &str,
) -> Result<(GenerationAttemptSummary, Value), String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    uuid::Uuid::parse_str(attempt_id).map_err(|_| "生成試行IDが不正です。".to_string())?;
    let directory = attempts_directory(app, meeting_id)?.join(attempt_id);
    let manifest = read_manifest(&directory)?;
    if manifest.meeting_id != meeting_id || manifest.attempt_id != attempt_id {
        return Err("生成試行の識別情報が一致しません。".into());
    }
    let artifact = select_revalidation_artifact(&manifest)
        .ok_or_else(|| "再検証できる生成結果が保存されていません。".to_string())?;
    let path = safe_artifact_path(&directory, &artifact.path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("保存済みAI応答を確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_ATTEMPT_ARTIFACT_BYTES {
        return Err("保存済みAI応答が不正です。".into());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("保存済みAI応答を読めませんでした: {error}"))?;
    if sha256(raw.as_bytes()) != artifact.sha256 {
        return Err("保存済みAI応答の整合性を確認できませんでした。".into());
    }
    let candidate = parse_candidate_json(&raw)?;
    Ok((manifest_summary(&manifest), candidate))
}

fn select_revalidation_artifact(
    manifest: &GenerationAttemptManifest,
) -> Option<&GenerationAttemptArtifact> {
    manifest
        .artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "candidate" && artifact.final_output)
        .or_else(|| {
            manifest
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.kind == "rawResponse" && artifact.final_output)
        })
}

pub(crate) fn generation_attempt_for(
    app: &AppHandle,
    meeting_id: &str,
    attempt_id: &str,
) -> Result<GenerationAttempt, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    uuid::Uuid::parse_str(attempt_id).map_err(|_| "生成試行IDが不正です。".to_string())?;
    let directory = attempts_directory(app, meeting_id)?.join(attempt_id);
    let manifest = read_manifest(&directory)?;
    if manifest.meeting_id != meeting_id || manifest.attempt_id != attempt_id {
        return Err("生成試行の識別情報が一致しません。".into());
    }
    Ok(GenerationAttempt {
        attempt_id: attempt_id.into(),
        directory,
    })
}

fn manifest_summary(manifest: &GenerationAttemptManifest) -> GenerationAttemptSummary {
    GenerationAttemptSummary {
        attempt_id: manifest.attempt_id.clone(),
        transcription_id: manifest.transcription_id.clone(),
        source_revision: manifest.source_revision,
        provider: manifest.provider.clone(),
        model: manifest
            .resolved_model
            .clone()
            .unwrap_or_else(|| manifest.requested_model.clone()),
        started_at: manifest.started_at.clone(),
        status: manifest.status.clone(),
        stage: manifest.stage.clone(),
        error: manifest.error.clone(),
        can_revalidate: manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == "rawResponse" && artifact.final_output),
    }
}

fn parse_candidate_json(output: &str) -> Result<Value, String> {
    let trimmed = output.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let json = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    match serde_json::from_str(json) {
        Ok(value) => Ok(value),
        Err(direct_error) => {
            for (index, character) in json.char_indices() {
                if character != '{' {
                    continue;
                }
                let mut values =
                    serde_json::Deserializer::from_str(&json[index..]).into_iter::<Value>();
                let Some(Ok(value)) = values.next() else {
                    continue;
                };
                let Some(object) = value.as_object() else {
                    continue;
                };
                if [
                    "meeting",
                    "summary",
                    "participants",
                    "topics",
                    "decisions",
                    "actionItems",
                ]
                .iter()
                .any(|key| object.contains_key(*key))
                {
                    return Ok(value);
                }
            }
            Err(
                if direct_error.classify() == serde_json::error::Category::Eof {
                    format!(
                    "AIの生成結果がJSONの途中で終了しました。出力上限に達した可能性があります: {direct_error}"
                )
                } else {
                    format!("AIの要約結果をJSONとして解析できませんでした: {direct_error}")
                },
            )
        }
    }
}

#[cfg(test)]
pub(crate) fn normalize_candidate_enums(candidate: &mut Value) {
    normalize_candidate(candidate, None);
}

pub(crate) fn normalize_candidate(candidate: &mut Value, default_language: Option<&str>) {
    normalize_candidate_shape(candidate, default_language);
    normalize_candidate_field_basis(candidate);
    if let Some(meeting_type) = candidate
        .get_mut("meeting")
        .and_then(|meeting| meeting.get_mut("meetingType"))
    {
        let normalized = match meeting_type.as_str().map(str::trim) {
            Some("internal" | "社内" | "社内会議" | "内部会議") => "internal",
            Some("client" | "顧客" | "顧客会議" | "クライアント") => "client",
            Some("sales" | "営業" | "商談") => "sales",
            Some("interview" | "面接" | "インタビュー") => "interview",
            Some("standup" | "スタンドアップ" | "朝会" | "デイリー") => "standup",
            Some("retrospective" | "振り返り" | "レトロスペクティブ") => {
                "retrospective"
            }
            Some("workshop" | "ワークショップ") => "workshop",
            Some("other" | "その他") => "other",
            Some("unknown" | "不明") | None => "unknown",
            Some(_) => "unknown",
        };
        *meeting_type = Value::String(normalized.to_string());
    }

    normalize_collection_enum(
        candidate,
        "participants",
        "kind",
        "unknown",
        |value| match value {
            "person" | "人物" | "個人" => Some("person"),
            "group" | "グループ" | "組織" => Some("group"),
            "unknown" | "不明" => Some("unknown"),
            _ => None,
        },
    );
    normalize_collection_enum(
        candidate,
        "participants",
        "attendance",
        "unknown",
        |value| match value {
            "present" | "出席" | "参加" => Some("present"),
            "remote" | "リモート" | "オンライン" => Some("remote"),
            "absent" | "欠席" => Some("absent"),
            "unknown" | "不明" => Some("unknown"),
            _ => None,
        },
    );
    normalize_collection_enum(candidate, "topics", "status", "open", |value| match value {
        "discussed" | "議論済み" | "完了" => Some("discussed"),
        "open" | "未解決" | "継続" => Some("open"),
        "deferred" | "保留" | "延期" => Some("deferred"),
        _ => None,
    });
    normalize_collection_enum(
        candidate,
        "decisions",
        "status",
        "tentative",
        |value| match value {
            "active" | "決定" | "有効" => Some("active"),
            "tentative" | "暫定" => Some("tentative"),
            "superseded" | "置換済み" => Some("superseded"),
            "revoked" | "撤回" => Some("revoked"),
            _ => None,
        },
    );
    normalize_collection_enum(
        candidate,
        "actionItems",
        "status",
        "open",
        |value| match value {
            "open" | "未着手" => Some("open"),
            "in_progress" | "進行中" => Some("in_progress"),
            "blocked" | "ブロック" => Some("blocked"),
            "done" | "完了" => Some("done"),
            "cancelled" | "キャンセル" => Some("cancelled"),
            _ => None,
        },
    );
    normalize_collection_enum(
        candidate,
        "openIssues",
        "status",
        "open",
        |value| match value {
            "open" | "未解決" => Some("open"),
            "resolved" | "解決済み" => Some("resolved"),
            "deferred" | "保留" => Some("deferred"),
            "cancelled" | "キャンセル" => Some("cancelled"),
            _ => None,
        },
    );
    normalize_collection_enum(
        candidate,
        "questions",
        "status",
        "open",
        |value| match value {
            "open" | "未回答" => Some("open"),
            "answered" | "回答済み" => Some("answered"),
            "deferred" | "保留" => Some("deferred"),
            _ => None,
        },
    );
}

fn normalize_candidate_shape(candidate: &mut Value, default_language: Option<&str>) {
    let Some(root) = candidate.as_object_mut() else {
        return;
    };
    if let Some(meeting) = root.get_mut("meeting").and_then(Value::as_object_mut) {
        // The first generation stage must not decide the meeting title. Even if a
        // model ignores the prompt and returns one, the note-only finishing stage
        // remains the single authoritative source for generated titles.
        meeting.remove("title");
        if let Some(field_basis) = meeting.get_mut("fieldBasis").and_then(Value::as_object_mut) {
            field_basis.remove("title");
            field_basis.remove("/title");
        }
        meeting
            .entry("meetingType")
            .or_insert_with(|| json!("unknown"));
        normalize_non_empty_string(meeting, "timeZone", "unknown");
        normalize_language_codes(meeting, default_language);
        meeting.entry("fieldBasis").or_insert_with(|| json!({}));
    }
    if !root.contains_key("summary") {
        let nested = root
            .get_mut("meeting")
            .and_then(Value::as_object_mut)
            .and_then(|meeting| meeting.remove("summary"));
        root.insert(
            "summary".into(),
            nested.unwrap_or_else(|| json!({"keyPoints": [], "fieldBasis": {}})),
        );
    }
    for collection in [
        "participants",
        "topics",
        "decisions",
        "actionItems",
        "openIssues",
        "questions",
        "notes",
    ] {
        root.entry(collection).or_insert_with(|| json!([]));
    }
    for (collection, prefix) in [
        ("participants", "p"),
        ("topics", "t"),
        ("decisions", "d"),
        ("actionItems", "a"),
        ("openIssues", "i"),
        ("questions", "q"),
        ("notes", "n"),
    ] {
        assign_missing_temporary_keys(root, collection, prefix);
    }
    if let Some(participants) = root.get_mut("participants").and_then(Value::as_array_mut) {
        for participant in participants {
            let Some(participant) = participant.as_object_mut() else {
                continue;
            };
            rename_field(participant, "name", "displayName");
            participant
                .entry("kind")
                .or_insert_with(|| json!("unknown"));
            participant
                .entry("attendance")
                .or_insert_with(|| json!("unknown"));
            participant.entry("speakerIds").or_insert_with(|| json!([]));
            participant
                .entry("identityStatus")
                .or_insert_with(|| json!("unknown"));
            participant.entry("evidence").or_insert_with(|| json!([]));
            participant.entry("fieldBasis").or_insert_with(|| json!({}));
        }
    }
    if let Some(topics) = root.get_mut("topics").and_then(Value::as_array_mut) {
        for (index, topic) in topics.iter_mut().enumerate() {
            let Some(topic) = topic.as_object_mut() else {
                continue;
            };
            topic.entry("order").or_insert_with(|| json!(index));
            topic.entry("status").or_insert_with(|| json!("discussed"));
            topic.entry("participantKeys").or_insert_with(|| json!([]));
            normalize_reference_array(topic, "participantKeys");
            topic.entry("evidence").or_insert_with(|| json!([]));
            topic.entry("fieldBasis").or_insert_with(|| json!({}));
        }
    }
    for (collection, default_status, reference_fields) in [
        (
            "decisions",
            "tentative",
            &[
                "topicKeys",
                "ownerParticipantKeys",
                "supersedesDecisionKeys",
            ][..],
        ),
        (
            "actionItems",
            "open",
            &[
                "assigneeParticipantKeys",
                "topicKeys",
                "relatedDecisionKeys",
                "blockerIssueKeys",
            ][..],
        ),
        (
            "openIssues",
            "open",
            &[
                "ownerParticipantKeys",
                "topicKeys",
                "relatedDecisionKeys",
                "relatedActionItemKeys",
            ][..],
        ),
        (
            "questions",
            "open",
            &["directedToParticipantKeys", "topicKeys", "relatedIssueKeys"][..],
        ),
    ] {
        if let Some(items) = root.get_mut(collection).and_then(Value::as_array_mut) {
            for item in items {
                let Some(item) = item.as_object_mut() else {
                    continue;
                };
                item.entry("status")
                    .or_insert_with(|| json!(default_status));
                for field in reference_fields {
                    item.entry(*field).or_insert_with(|| json!([]));
                    normalize_reference_array(item, field);
                }
                if collection == "questions" {
                    if let Some(answer) = item.get_mut("answer").and_then(Value::as_object_mut) {
                        answer
                            .entry("answeredByParticipantKeys")
                            .or_insert_with(|| json!([]));
                        normalize_reference_array(answer, "answeredByParticipantKeys");
                    }
                }
                item.entry("evidence").or_insert_with(|| json!([]));
                item.entry("fieldBasis").or_insert_with(|| json!({}));
            }
        }
    }
    if let Some(notes) = root.get_mut("notes").and_then(Value::as_array_mut) {
        for note in notes {
            let Some(note) = note.as_object_mut() else {
                continue;
            };
            note.entry("topicKeys").or_insert_with(|| json!([]));
            normalize_reference_array(note, "topicKeys");
            note.entry("evidence").or_insert_with(|| json!([]));
            note.entry("fieldBasis").or_insert_with(|| json!({}));
        }
    }
    if let Some(summary) = root.get_mut("summary").and_then(Value::as_object_mut) {
        summary.entry("keyPoints").or_insert_with(|| json!([]));
        summary.entry("fieldBasis").or_insert_with(|| json!({}));
        if let Some(key_points) = summary.get_mut("keyPoints").and_then(Value::as_array_mut) {
            for (index, key_point) in key_points.iter_mut().enumerate() {
                if let Some(text) = key_point.as_str() {
                    *key_point = json!({
                        "key": format!("k{}", index + 1),
                        "text": text,
                        "evidence": [],
                        "fieldBasis": {"/text": "inferred"}
                    });
                }
            }
            assign_missing_keys_in_array(key_points, "k");
        }
    }
    normalize_evidence_arrays(candidate);
}

fn normalize_non_empty_string(object: &mut Map<String, Value>, field: &str, fallback: &str) {
    let should_replace = object
        .get(field)
        .is_none_or(|value| value.as_str().is_none_or(|value| value.trim().is_empty()));
    if should_replace {
        object.insert(field.into(), Value::String(fallback.into()));
    }
}

fn normalize_language_codes(object: &mut Map<String, Value>, default_language: Option<&str>) {
    let fallback = default_language
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .unwrap_or("und");
    match object.get_mut("languageCodes") {
        Some(Value::String(language)) if !language.trim().is_empty() => {
            *object.get_mut("languageCodes").expect("languageCodes") = json!([language.trim()]);
        }
        Some(Value::Array(languages)) if !languages.is_empty() => {}
        Some(value) => *value = json!([fallback]),
        None => {
            object.insert("languageCodes".into(), json!([fallback]));
        }
    }
}

fn normalize_evidence_arrays(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(evidence) = object.get_mut("evidence") {
                match evidence {
                    Value::Object(_) => {
                        let single = std::mem::take(evidence);
                        *evidence = Value::Array(vec![single]);
                    }
                    Value::Null => *evidence = json!([]),
                    _ => {}
                }
            }
            for child in object.values_mut() {
                normalize_evidence_arrays(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_evidence_arrays(item);
            }
        }
        _ => {}
    }
}

fn assign_missing_temporary_keys(root: &mut Map<String, Value>, collection: &str, prefix: &str) {
    if let Some(items) = root.get_mut(collection).and_then(Value::as_array_mut) {
        assign_missing_keys_in_array(items, prefix);
    }
}

fn assign_missing_keys_in_array(items: &mut [Value], prefix: &str) {
    let mut used = HashSet::new();
    let mut sequence = 1_u64;
    for item in items {
        let Some(item) = item.as_object_mut() else {
            continue;
        };
        let existing = item
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|key| temporary_key_is_valid(key, prefix));
        if existing.is_some_and(|key| used.insert(key.to_string())) {
            continue;
        }
        loop {
            let key = format!("{prefix}{sequence}");
            sequence += 1;
            if used.insert(key.clone()) {
                item.insert("key".into(), Value::String(key));
                break;
            }
        }
    }
}

fn temporary_key_is_valid(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn normalize_reference_array(object: &mut Map<String, Value>, field: &str) {
    let Some(value) = object.get_mut(field) else {
        return;
    };
    match value {
        Value::String(reference) if reference.trim().is_empty() => *value = json!([]),
        Value::String(reference) => *value = json!([reference.trim()]),
        Value::Null => *value = json!([]),
        _ => {}
    }
}

fn rename_field(object: &mut Map<String, Value>, old: &str, new: &str) {
    if !object.contains_key(new) {
        if let Some(value) = object.remove(old) {
            object.insert(new.into(), value);
        }
    }
}

fn normalize_candidate_field_basis(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let scalar_basis = object
                .get("fieldBasis")
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(normalize_basis_value);
            if let Some(basis) = scalar_basis {
                let fields = object
                    .iter()
                    .filter(|(field, value)| {
                        !matches!(field.as_str(), "key" | "evidence" | "fieldBasis")
                            && !value.is_null()
                    })
                    .map(|(field, _)| (json_pointer_field(field), Value::String(basis.into())))
                    .collect();
                object.insert("fieldBasis".into(), Value::Object(fields));
            } else if let Some(Value::Object(basis_map)) = object.get_mut("fieldBasis") {
                let original = std::mem::take(basis_map);
                for (field, basis) in original {
                    let normalized = basis
                        .as_str()
                        .map(str::trim)
                        .and_then(normalize_basis_value)
                        .map_or(basis, |basis| Value::String(basis.into()));
                    basis_map.insert(json_pointer_field(&field), normalized);
                }
            }
            for child in object.values_mut() {
                normalize_candidate_field_basis(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                normalize_candidate_field_basis(child);
            }
        }
        _ => {}
    }
}

fn normalize_basis_value(value: &str) -> Option<&'static str> {
    match value {
        "explicit" | "明示" => Some("explicit"),
        "normalized" | "正規化" => Some("normalized"),
        "inferred" | "推論" => Some("inferred"),
        _ => None,
    }
}

fn json_pointer_field(field: &str) -> String {
    if field.starts_with('/') {
        field.into()
    } else {
        format!("/{}", field.replace('~', "~0").replace('/', "~1"))
    }
}

fn normalize_collection_enum(
    candidate: &mut Value,
    collection: &str,
    field: &str,
    fallback: &'static str,
    normalize: impl Fn(&str) -> Option<&'static str>,
) {
    let Some(items) = candidate.get_mut(collection).and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(value) = item.get_mut(field) else {
            continue;
        };
        let normalized = value
            .as_str()
            .map(str::trim)
            .and_then(&normalize)
            .unwrap_or(fallback);
        *value = Value::String(normalized.to_string());
    }
}

pub(crate) fn validate_candidate(
    candidate: &Value,
    transcript: &SummaryTranscriptSnapshot,
) -> Result<(), String> {
    let root = candidate
        .as_object()
        .ok_or_else(|| "抽出結果はJSON objectである必要があります。".to_string())?;
    for field in [
        "meeting",
        "participants",
        "summary",
        "topics",
        "decisions",
        "actionItems",
        "openIssues",
        "questions",
        "notes",
    ] {
        if !root.contains_key(field) {
            return Err(format!("抽出結果に必須フィールド {field} がありません。"));
        }
    }
    let meeting = object(root.get("meeting"), "meeting")?;
    for field in ["meetingType", "timeZone", "languageCodes", "fieldBasis"] {
        if !meeting.contains_key(field) {
            return Err(format!("meeting.{field} がありません。"));
        }
    }
    enum_field(
        meeting,
        "meetingType",
        &[
            "internal",
            "client",
            "sales",
            "interview",
            "standup",
            "retrospective",
            "workshop",
            "other",
            "unknown",
        ],
    )?;
    validate_basis_map(meeting.get("fieldBasis"), "meeting.fieldBasis")?;
    let summary = object(root.get("summary"), "summary")?;
    for field in ["overview", "keyPoints", "fieldBasis"] {
        if !summary.contains_key(field) {
            return Err(format!("summary.{field} がありません。"));
        }
    }
    required_string(summary, "overview", "summary")?;
    validate_basis_map(summary.get("fieldBasis"), "summary.fieldBasis")?;
    let known_segments: HashSet<_> = transcript
        .segments
        .iter()
        .map(|segment| segment.segment_id.as_str())
        .collect();
    let specs = [
        ("participants", "p", "key"),
        ("topics", "t", "key"),
        ("decisions", "d", "key"),
        ("actionItems", "a", "key"),
        ("openIssues", "i", "key"),
        ("questions", "q", "key"),
        ("notes", "n", "key"),
    ];
    let mut keys = HashMap::<&str, HashSet<String>>::new();
    for (collection, prefix, key_field) in specs {
        let mut seen = HashSet::new();
        for (index, item) in array(root.get(collection), collection)?.iter().enumerate() {
            let item = item
                .as_object()
                .ok_or_else(|| format!("{collection}の項目がobjectではありません。"))?;
            let key = required_string(item, key_field, collection)?;
            if !valid_temporary_key(key, prefix) || !seen.insert(key.to_string()) {
                return Err(format!("{collection}の一時keyが不正または重複しています。"));
            }
            validate_candidate_evidence(item.get("evidence"), &known_segments)?;
            validate_basis_map(
                item.get("fieldBasis"),
                &format!("{collection}[{index}].fieldBasis"),
            )?;
            for required in required_candidate_fields(collection) {
                if !item.contains_key(*required) {
                    return Err(format!("{collection}.{required} がありません。"));
                }
            }
            validate_candidate_enums(collection, item)?;
        }
        keys.insert(collection, seen);
    }
    validate_references(root, &keys)?;
    for (index, item) in array(summary.get("keyPoints"), "summary.keyPoints")?
        .iter()
        .enumerate()
    {
        let item = item
            .as_object()
            .ok_or_else(|| "keyPointがobjectではありません。".to_string())?;
        validate_candidate_evidence(item.get("evidence"), &known_segments)?;
        for field in ["key", "text", "fieldBasis"] {
            if !item.contains_key(field) {
                return Err(format!("keyPoint.{field} がありません。"));
            }
        }
        validate_basis_map(
            item.get("fieldBasis"),
            &format!("summary.keyPoints[{index}].fieldBasis"),
        )?;
    }
    Ok(())
}

fn validate_basis_map(value: Option<&Value>, path: &str) -> Result<(), String> {
    let map = object(value, path)?;
    if map
        .values()
        .any(|basis| !matches!(basis.as_str(), Some("explicit" | "normalized" | "inferred")))
    {
        return Err(format!("{path}に未対応の値があります。"));
    }
    Ok(())
}

fn validate_candidate_enums(collection: &str, item: &Map<String, Value>) -> Result<(), String> {
    match collection {
        "participants" => {
            enum_field(item, "kind", &["person", "group", "unknown"])?;
            enum_field(
                item,
                "attendance",
                &["present", "remote", "absent", "unknown"],
            )?;
        }
        "topics" => enum_field(item, "status", &["discussed", "open", "deferred"])?,
        "decisions" => enum_field(
            item,
            "status",
            &["active", "tentative", "superseded", "revoked"],
        )?,
        "actionItems" => enum_field(
            item,
            "status",
            &["open", "in_progress", "blocked", "done", "cancelled"],
        )?,
        "openIssues" => enum_field(
            item,
            "status",
            &["open", "resolved", "deferred", "cancelled"],
        )?,
        "questions" => enum_field(item, "status", &["open", "answered", "deferred"])?,
        _ => {}
    }
    Ok(())
}

fn enum_field(object: &Map<String, Value>, field: &str, allowed: &[&str]) -> Result<(), String> {
    let value = required_string(object, field, "candidate")?;
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("{field}の値に対応していません。"))
    }
}

fn required_candidate_fields(collection: &str) -> &'static [&'static str] {
    match collection {
        "participants" => &[
            "displayName",
            "kind",
            "attendance",
            "speakerIds",
            "identityStatus",
        ],
        "topics" => &["title", "order", "status", "participantKeys"],
        "decisions" => &[
            "statement",
            "status",
            "topicKeys",
            "ownerParticipantKeys",
            "supersedesDecisionKeys",
        ],
        "actionItems" => &[
            "title",
            "status",
            "assigneeParticipantKeys",
            "topicKeys",
            "relatedDecisionKeys",
            "blockerIssueKeys",
        ],
        "openIssues" => &[
            "title",
            "status",
            "ownerParticipantKeys",
            "topicKeys",
            "relatedDecisionKeys",
            "relatedActionItemKeys",
        ],
        "questions" => &[
            "text",
            "status",
            "directedToParticipantKeys",
            "topicKeys",
            "relatedIssueKeys",
        ],
        "notes" => &["body", "topicKeys"],
        _ => &[],
    }
}

fn validate_candidate_evidence(value: Option<&Value>, known: &HashSet<&str>) -> Result<(), String> {
    for evidence in array(value, "evidence")? {
        let evidence = evidence
            .as_object()
            .ok_or_else(|| "evidenceがobjectではありません。".to_string())?;
        let relation = required_string(evidence, "relation", "evidence")?;
        if !matches!(relation, "direct" | "contextual") {
            return Err("Evidence relationが不正です。".into());
        }
        let spans = array(evidence.get("spans"), "evidence.spans")?;
        if spans.is_empty() {
            return Err("Evidence spansは空にできません。".into());
        }
        for span in spans {
            let span = span
                .as_object()
                .ok_or_else(|| "Evidence spanがobjectではありません。".to_string())?;
            let segment_id = required_string(span, "segmentId", "evidence span")?;
            if !known.contains(segment_id) {
                return Err("Evidenceが存在しないTranscript segmentを参照しています。".into());
            }
        }
    }
    Ok(())
}

fn validate_references(
    root: &Map<String, Value>,
    keys: &HashMap<&str, HashSet<String>>,
) -> Result<(), String> {
    let rules = [
        ("topics", "participantKeys", "participants"),
        ("decisions", "topicKeys", "topics"),
        ("decisions", "ownerParticipantKeys", "participants"),
        ("decisions", "supersedesDecisionKeys", "decisions"),
        ("actionItems", "assigneeParticipantKeys", "participants"),
        ("actionItems", "topicKeys", "topics"),
        ("actionItems", "relatedDecisionKeys", "decisions"),
        ("actionItems", "blockerIssueKeys", "openIssues"),
        ("openIssues", "ownerParticipantKeys", "participants"),
        ("openIssues", "topicKeys", "topics"),
        ("openIssues", "relatedDecisionKeys", "decisions"),
        ("openIssues", "relatedActionItemKeys", "actionItems"),
        ("questions", "directedToParticipantKeys", "participants"),
        ("questions", "topicKeys", "topics"),
        ("questions", "relatedIssueKeys", "openIssues"),
        ("notes", "topicKeys", "topics"),
    ];
    for (collection, field, target) in rules {
        for item in array(root.get(collection), collection)? {
            let item = item.as_object().expect("validated candidate item");
            for reference in string_array(item.get(field), field)? {
                if !keys
                    .get(target)
                    .is_some_and(|known| known.contains(reference))
                {
                    return Err(format!("{collection}.{field}に不正な参照があります。"));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn persist_candidate(
    app: &AppHandle,
    generated: &GeneratedCandidate,
    transcript: &SummaryTranscriptSnapshot,
) -> Result<MeetingDocument, String> {
    validate_candidate(&generated.candidate, transcript)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "会議ドキュメントを保存できませんでした。".to_string())?;
    let directory = documents_directory(app, &generated.meeting_id)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("会議ドキュメントの保存先を作成できませんでした: {error}"))?;
    let previous = read_latest_in(&directory)?;
    let revision = previous
        .as_ref()
        .and_then(|value| value["revision"].as_u64())
        .unwrap_or(0)
        + 1;
    let fallback_title = crate::meeting_store::meeting_title(app, &generated.meeting_id)?;
    let document = normalize(
        &fallback_title,
        generated,
        transcript,
        revision,
        previous.as_ref(),
    )?;
    validate_document(&document, transcript)?;
    write_document(&directory, &document)?;
    Ok(document)
}

fn normalize(
    fallback_title: &str,
    generated: &GeneratedCandidate,
    transcript: &SummaryTranscriptSnapshot,
    revision: u64,
    previous: Option<&Value>,
) -> Result<Value, String> {
    let candidate = generated
        .candidate
        .as_object()
        .expect("validated candidate");
    let now = &generated.generated_at;
    let run_id = new_id("gen");
    let source_hash = transcript_hash(transcript);
    let mut id_maps = HashMap::<&str, HashMap<String, String>>::new();
    for (collection, prefix) in [
        ("participants", "par"),
        ("topics", "top"),
        ("decisions", "dec"),
        ("actionItems", "act"),
        ("openIssues", "iss"),
        ("questions", "que"),
        ("notes", "not"),
    ] {
        id_maps.insert(
            collection,
            array(candidate.get(collection), collection)?
                .iter()
                .filter_map(|item| {
                    item["key"]
                        .as_str()
                        .map(|key| (key.to_string(), new_id(prefix)))
                })
                .collect(),
        );
    }
    let segment_by_id: HashMap<_, _> = transcript
        .segments
        .iter()
        .map(|item| (item.segment_id.as_str(), item))
        .collect();
    let mut evidence = Vec::new();
    let mut evidence_ids = HashMap::<String, String>::new();
    let mut normalize_evidence = |value: Option<&Value>| -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        for item in array(value, "evidence")? {
            let signature = serde_json::to_string(item).unwrap_or_default();
            if let Some(id) = evidence_ids.get(&signature) {
                ids.push(id.clone());
                continue;
            }
            let id = new_id("ev");
            let spans = array(item.get("spans"), "spans")?.iter().filter_map(|span| {
                let raw_id = span["segmentId"].as_str()?;
                let source = segment_by_id.get(raw_id)?;
                Some(json!({"segmentId": prefixed_id("seg", raw_id), "startMs": span["startMs"].as_u64().unwrap_or(source.start_ms), "endMs": span["endMs"].as_u64().unwrap_or(source.end_ms)}))
            }).collect::<Vec<_>>();
            evidence.push(json!({"evidenceId": id, "relation": item["relation"], "spans": spans, "quote": item.get("quote").and_then(Value::as_str).unwrap_or("")}));
            evidence_ids.insert(signature, id.clone());
            ids.push(id);
        }
        Ok(ids)
    };
    let participants = normalize_collection(
        candidate,
        "participants",
        "participantId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let mut speaker_mappings = Vec::new();
    for participant in array(candidate.get("participants"), "participants")? {
        let key = participant["key"]
            .as_str()
            .expect("validated participant key");
        let participant_id = &id_maps["participants"][key];
        let participant_evidence = normalize_evidence(participant.get("evidence"))?;
        for speaker_id in string_array(participant.get("speakerIds"), "speakerIds")? {
            speaker_mappings.push(json!({"mappingId":new_id("map"),"speakerId":stable_speaker_id(speaker_id),"participantId":participant_id,"status":"inferred","evidenceIds":participant_evidence,"recordMeta":record_meta(&run_id,now,participant,&participant_evidence)}));
        }
    }
    let topics = normalize_collection(
        candidate,
        "topics",
        "topicId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let decisions = normalize_collection(
        candidate,
        "decisions",
        "decisionId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let actions = normalize_collection(
        candidate,
        "actionItems",
        "actionItemId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let issues = normalize_collection(
        candidate,
        "openIssues",
        "issueId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let questions = normalize_collection(
        candidate,
        "questions",
        "questionId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let notes = normalize_collection(
        candidate,
        "notes",
        "noteId",
        &id_maps,
        &run_id,
        now,
        &mut normalize_evidence,
    )?;
    let summary_candidate = candidate["summary"].as_object().expect("validated summary");
    let key_points = array(summary_candidate.get("keyPoints"), "keyPoints")?.iter().map(|item| {
        let ids = normalize_evidence(item.get("evidence"))?;
        Ok(json!({"keyPointId": new_id("key"), "text": item["text"], "evidenceIds": ids, "recordMeta": record_meta(&run_id, now, item, &ids)}))
    }).collect::<Result<Vec<_>, String>>()?;
    let meeting_candidate = candidate["meeting"].as_object().expect("validated meeting");
    let organizer_participant_id = meeting_candidate
        .get("organizerParticipantKey")
        .and_then(Value::as_str)
        .and_then(|key| id_maps["participants"].get(key))
        .cloned();
    let created_at = previous
        .and_then(|value| value["createdAt"].as_str())
        .unwrap_or(now);
    let mut document = json!({
        "schemaVersion": SCHEMA_VERSION, "documentType": "meeting", "documentId": prefixed_id("mtg", &generated.meeting_id),
        "revision": revision, "createdAt": created_at, "updatedAt": now,
        "sourceTranscript": {"documentId": prefixed_id("trn", &generated.transcription_id), "revision": generated.source_revision + 1, "contentHash": source_hash},
        "meeting": {"title": meeting_candidate.get("title"), "meetingType": meeting_candidate["meetingType"], "purpose": meeting_candidate.get("purpose"), "startedAt": meeting_candidate.get("startedAt"), "endedAt": meeting_candidate.get("endedAt"), "timeZone": meeting_candidate["timeZone"], "languageCodes": meeting_candidate["languageCodes"], "organizerParticipantId": organizer_participant_id, "externalRefs": []},
        "participants": participants, "speakerMappings": speaker_mappings,
        "summary": {"oneLine": summary_candidate.get("oneLine"), "overview": summary_candidate["overview"], "keyPoints": key_points},
        "topics": topics, "decisions": decisions, "actionItems": actions, "openIssues": issues, "questions": questions, "notes": notes, "evidence": evidence,
        "generationRuns": [{"runId": run_id, "mode": if revision == 1 {"initial"} else {"regenerate"}, "createdAt": now, "provider": generated.provider, "model": generated.model, "promptId": "meeting-extraction", "promptVersion": "1.1.0", "sourceTranscriptRevision": generated.source_revision + 1, "sourceTranscriptHash": source_hash, "outputSchemaVersion": SCHEMA_VERSION, "warnings": []}],
        "qualityChecks": [],
        "latestGenerationRunId": run_id, "editorial": {"fieldStates": {}}
    });
    clean_nulls(&mut document);
    if let Some(previous) = previous {
        merge_previous(&mut document, previous);
    }
    if document["meeting"]["title"]
        .as_str()
        .is_none_or(str::is_empty)
    {
        document["meeting"]["title"] = Value::String(fallback_title.to_string());
    }
    Ok(document)
}

fn normalize_collection<F>(
    candidate: &Map<String, Value>,
    collection: &'static str,
    id_field: &str,
    maps: &HashMap<&str, HashMap<String, String>>,
    run_id: &str,
    now: &str,
    evidence: &mut F,
) -> Result<Vec<Value>, String>
where
    F: FnMut(Option<&Value>) -> Result<Vec<String>, String>,
{
    array(candidate.get(collection), collection)?
        .iter()
        .map(|item| {
            let source = item.as_object().expect("validated record");
            let key = source["key"].as_str().expect("validated key");
            let ids = evidence(source.get("evidence"))?;
            let mut result = Map::new();
            result.insert(
                id_field.into(),
                Value::String(maps[collection][key].clone()),
            );
            for (field, value) in source {
                if matches!(
                    field.as_str(),
                    "key" | "evidence" | "fieldBasis" | "speakerIds" | "identityStatus"
                ) {
                    continue;
                }
                let persistent_field = field.replace("Keys", "Ids").replace("Key", "Id");
                result.insert(persistent_field, map_references(field, value, maps));
            }
            if collection != "participants" {
                result.insert("evidenceIds".into(), json!(ids));
            }
            if collection == "participants" {
                result.entry("aliases").or_insert_with(|| json!([]));
                result.entry("externalRefs").or_insert_with(|| json!([]));
            }
            if collection == "openIssues" {
                if let Some(resolution) = source.get("resolution").and_then(Value::as_object) {
                    let resolution_evidence = evidence(resolution.get("evidence"))?;
                    result.insert("resolution".into(), json!({"text":resolution["text"],"evidenceIds":resolution_evidence}));
                }
            }
            if collection == "questions" {
                if let Some(answer) = source.get("answer").and_then(Value::as_object) {
                    let answer_evidence = evidence(answer.get("evidence"))?;
                    let answered_by = map_references("answeredByParticipantKeys", &answer["answeredByParticipantKeys"], maps);
                    result.insert("answer".into(), json!({"text":answer["text"],"answeredByParticipantIds":answered_by,"evidenceIds":answer_evidence}));
                }
            }
            result.insert("recordMeta".into(), record_meta(run_id, now, item, &ids));
            Ok(Value::Object(result))
        })
        .collect()
}

fn map_references(
    field: &str,
    value: &Value,
    maps: &HashMap<&str, HashMap<String, String>>,
) -> Value {
    let target = if field.contains("Participant") {
        Some("participants")
    } else if field.contains("Topic") {
        Some("topics")
    } else if field.contains("Decision") {
        Some("decisions")
    } else if field.contains("ActionItem") {
        Some("actionItems")
    } else if field.contains("Issue") {
        Some("openIssues")
    } else {
        None
    };
    let Some(target) = target else {
        return value.clone();
    };
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .filter_map(|value| {
                    maps[target]
                        .get(value.as_str()?)
                        .cloned()
                        .map(Value::String)
                })
                .collect(),
        ),
        Value::String(key) => maps[target]
            .get(key)
            .cloned()
            .map(Value::String)
            .unwrap_or(Value::Null),
        _ => value.clone(),
    }
}

fn record_meta(run_id: &str, now: &str, candidate: &Value, evidence_ids: &[String]) -> Value {
    let basis = candidate
        .get("fieldBasis")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let states = basis.into_iter().map(|(field, basis)| (json_pointer_field(&field), json!({"source":"ai","basis":basis,"locked":false,"updatedAt":now,"evidenceIds":evidence_ids,"generationRunId":run_id}))).collect::<Map<_,_>>();
    json!({"origin":"ai","lifecycle":"active","lifecycleSource":"ai","createdAt":now,"updatedAt":now,"generationRunId":run_id,"fingerprint":sha256(canonical_text(candidate).as_bytes()),"fieldStates":states})
}

fn merge_previous(current: &mut Value, previous: &Value) {
    let mut id_replacements = HashMap::<String, String>::new();
    for collection in [
        "participants",
        "topics",
        "decisions",
        "actionItems",
        "openIssues",
        "questions",
        "notes",
    ] {
        let Some(current_items) = current[collection].as_array_mut() else {
            continue;
        };
        let Some(previous_items) = previous[collection].as_array() else {
            continue;
        };
        let id_field = match collection {
            "participants" => "participantId",
            "topics" => "topicId",
            "decisions" => "decisionId",
            "actionItems" => "actionItemId",
            "openIssues" => "issueId",
            "questions" => "questionId",
            _ => "noteId",
        };
        for item in current_items.iter_mut() {
            let fingerprint = item["recordMeta"]["fingerprint"].as_str();
            if let Some(old) = previous_items
                .iter()
                .find(|old| old["recordMeta"]["fingerprint"].as_str() == fingerprint)
            {
                if let (Some(new_id), Some(old_id)) =
                    (item[id_field].as_str(), old[id_field].as_str())
                {
                    id_replacements.insert(new_id.to_string(), old_id.to_string());
                }
                item[id_field] = old[id_field].clone();
                if let Some(states) = old["recordMeta"]["fieldStates"].as_object() {
                    for (pointer, state) in states {
                        if state["locked"].as_bool() == Some(true) {
                            if let Some(field) = pointer.strip_prefix('/') {
                                item[field] = old[field].clone();
                                item["recordMeta"]["fieldStates"][pointer] = state.clone();
                            }
                        }
                    }
                }
            }
        }
        let fingerprints: HashSet<String> = current_items
            .iter()
            .filter_map(|item| {
                item["recordMeta"]["fingerprint"]
                    .as_str()
                    .map(str::to_string)
            })
            .collect();
        current_items.extend(
            previous_items
                .iter()
                .filter(|old| {
                    old["recordMeta"]["lifecycle"] == "deleted"
                        || (old["recordMeta"]["origin"] == "user"
                            && !fingerprints
                                .contains(old["recordMeta"]["fingerprint"].as_str().unwrap_or("")))
                })
                .cloned()
                .map(|mut old| {
                    if old.get("evidenceIds").is_some() {
                        old["evidenceIds"] = json!([]);
                    }
                    old
                }),
        );
    }
    let mut runs = previous["generationRuns"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    runs.extend(
        current["generationRuns"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    current["generationRuns"] = Value::Array(runs);
    current["qualityChecks"] = previous
        .get("qualityChecks")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Some(latest) = previous.get("latestQualityCheckId") {
        current["latestQualityCheckId"] = latest.clone();
    }
    current["editorial"] = previous["editorial"].clone();
    if let Some(states) = previous["editorial"]["fieldStates"].as_object() {
        for (pointer, state) in states {
            if state["locked"].as_bool() == Some(true) {
                if let (Some(previous_value), Some(current_value)) =
                    (previous.pointer(pointer), current.pointer_mut(pointer))
                {
                    *current_value = previous_value.clone();
                }
            }
        }
    }
    merge_previous_key_points(current, previous);
    replace_values(current, &id_replacements);
}

fn merge_previous_key_points(current: &mut Value, previous: &Value) {
    let Some(current_items) = current["summary"]["keyPoints"].as_array_mut() else {
        return;
    };
    let Some(previous_items) = previous["summary"]["keyPoints"].as_array() else {
        return;
    };
    for item in current_items {
        let fingerprint = item["recordMeta"]["fingerprint"].as_str();
        let Some(old) = previous_items
            .iter()
            .find(|old| old["recordMeta"]["fingerprint"].as_str() == fingerprint)
        else {
            continue;
        };
        item["keyPointId"] = old["keyPointId"].clone();
        if let Some(states) = old["recordMeta"]["fieldStates"].as_object() {
            for (pointer, state) in states {
                if state["locked"].as_bool() == Some(true) {
                    if let Some(field) = pointer.strip_prefix('/') {
                        item[field] = old[field].clone();
                        item["recordMeta"]["fieldStates"][pointer] = state.clone();
                    }
                }
            }
        }
    }
}

fn replace_values(value: &mut Value, replacements: &HashMap<String, String>) {
    match value {
        Value::String(current) => {
            if let Some(replacement) = replacements.get(current) {
                *current = replacement.clone();
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| replace_values(value, replacements)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|value| replace_values(value, replacements)),
        _ => {}
    }
}

pub(crate) fn selected(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<Option<MeetingDocument>, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "会議ドキュメントを読み込めませんでした。".to_string())?;
    read_latest_in(&documents_directory(app, meeting_id)?)
}

pub(crate) fn apply_quality_check(
    app: &AppHandle,
    meeting_id: &str,
    expected_revision: u64,
    provider: &str,
    model: &str,
    result: &Value,
) -> Result<MeetingDocument, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "会議ノートの仕上げ結果を保存できませんでした。".to_string())?;
    let directory = documents_directory(app, meeting_id)?;
    let mut current = read_latest_in(&directory)?
        .ok_or_else(|| "仕上げ対象の会議ノートがありません。".to_string())?;
    if current["revision"].as_u64() != Some(expected_revision) {
        return Err("仕上げ中に会議ノートが更新されました。".into());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let check_id = new_id("qck");
    let title_locked =
        current["editorial"]["fieldStates"]["/meeting/title"]["locked"].as_bool() == Some(true);
    let title = result.get("title").and_then(Value::as_str).map(str::trim);
    let mut title_applied = false;
    if !title_locked {
        if let Some(title) = title.filter(|title| !title.is_empty()) {
            if current["meeting"]["title"] != title {
                current["meeting"]["title"] = json!(title);
                current["editorial"]["fieldStates"]["/meeting/title"] = json!({
                    "source":"ai",
                    "basis":"derived",
                    "locked":false,
                    "updatedAt":now,
                    "generationRunId":current["latestGenerationRunId"]
                });
                title_applied = true;
            }
        }
    }

    let quality_check = json!({
        "checkId": check_id,
        "createdAt": now,
        "provider": provider,
        "model": model,
        "generationRunId": current.get("latestGenerationRunId"),
        "status": result["consistency"]["status"],
        "findings": result["consistency"]["findings"],
        "title": title,
        "titleApplied": title_applied,
        "error": result.get("error")
    });
    current
        .as_object_mut()
        .expect("meeting document")
        .entry("qualityChecks")
        .or_insert_with(|| json!([]));
    current["qualityChecks"]
        .as_array_mut()
        .ok_or_else(|| "qualityChecksはarrayである必要があります。".to_string())?
        .push(quality_check);
    current["latestQualityCheckId"] = json!(check_id);
    current["revision"] = json!(expected_revision + 1);
    current["updatedAt"] = json!(now);
    clean_nulls(&mut current);
    validate_document_integrity(&current)?;
    write_document(&directory, &current)?;
    Ok(current)
}

pub(crate) fn save_user_edit(
    app: &AppHandle,
    meeting_id: &str,
    expected_revision: u64,
    edited: &Value,
) -> Result<MeetingDocument, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "会議ノートの編集を保存できませんでした。".to_string())?;
    let directory = documents_directory(app, meeting_id)?;
    let mut current = read_latest_in(&directory)?
        .ok_or_else(|| "編集する会議ノートがありません。".to_string())?;
    if current["revision"].as_u64() != Some(expected_revision) {
        return Err(
            "会議ノートが更新されています。最新の内容を読み込んでから編集してください。".into(),
        );
    }
    if edited["documentId"] != current["documentId"]
        || edited["sourceTranscript"] != current["sourceTranscript"]
    {
        return Err("編集対象の会議ノートが一致しません。".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    copy_document_text(&mut current, edited, &now)?;
    current["revision"] = json!(expected_revision + 1);
    current["updatedAt"] = json!(now);
    // sourceTranscript is immutable for user edits (checked above). Validate the
    // document's own structure and references without requiring it to match the
    // currently selected transcript, which may legitimately be a newer revision.
    validate_document_integrity(&current)?;
    write_document(&directory, &current)?;
    Ok(current)
}

fn copy_document_text(current: &mut Value, edited: &Value, now: &str) -> Result<(), String> {
    copy_root_text(current, edited, "meeting", "title", true, now)?;
    copy_root_text(current, edited, "meeting", "purpose", false, now)?;
    copy_root_text(current, edited, "summary", "overview", true, now)?;
    copy_root_text(current, edited, "summary", "oneLine", false, now)?;
    copy_nested_collection_text(
        current,
        edited,
        "summary",
        "keyPoints",
        "keyPointId",
        &["text"],
        now,
    )?;
    copy_collection_text(
        current,
        edited,
        "participants",
        "participantId",
        &["displayName"],
        now,
    )?;
    copy_collection_text(
        current,
        edited,
        "topics",
        "topicId",
        &["title", "summary"],
        now,
    )?;
    copy_collection_text(
        current,
        edited,
        "decisions",
        "decisionId",
        &["statement", "rationale"],
        now,
    )?;
    copy_collection_text(
        current,
        edited,
        "actionItems",
        "actionItemId",
        &["title", "description"],
        now,
    )?;
    copy_collection_text(
        current,
        edited,
        "openIssues",
        "issueId",
        &["title", "description"],
        now,
    )?;
    copy_collection_text(current, edited, "questions", "questionId", &["text"], now)?;
    copy_collection_text(current, edited, "notes", "noteId", &["title", "body"], now)?;
    copy_question_answers(current, edited, now)?;

    let edited_actions = array(edited.get("actionItems"), "actionItems")?;
    for action in current["actionItems"]
        .as_array_mut()
        .expect("validated document actions")
    {
        let Some(id) = action["actionItemId"].as_str().map(str::to_string) else {
            continue;
        };
        let Some(source) = edited_actions
            .iter()
            .find(|item| item["actionItemId"] == id)
        else {
            continue;
        };
        let status = source["status"]
            .as_str()
            .filter(|status| {
                matches!(
                    *status,
                    "open" | "in_progress" | "blocked" | "done" | "cancelled"
                )
            })
            .ok_or_else(|| "アクション項目の状態が不正です。".to_string())?;
        if action["status"] != status {
            action["status"] = json!(status);
            mark_record_field(action, "/status", now);
        }
    }
    Ok(())
}

fn copy_root_text(
    current: &mut Value,
    edited: &Value,
    section: &str,
    field: &str,
    required: bool,
    now: &str,
) -> Result<(), String> {
    let value = edited[section][field].as_str().unwrap_or("").trim();
    let pointer = format!("/{section}/{field}");
    let next = if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    };
    if current[section][field] == next {
        return Ok(());
    }
    validate_edited_text(value, required, field)?;
    if value.is_empty() {
        current[section]
            .as_object_mut()
            .expect("document section")
            .remove(field);
    } else {
        current[section][field] = next;
    }
    current["editorial"]["fieldStates"][&pointer] = user_field_state(now);
    Ok(())
}

fn copy_collection_text(
    current: &mut Value,
    edited: &Value,
    collection: &str,
    id_field: &str,
    fields: &[&str],
    now: &str,
) -> Result<(), String> {
    let edited_items = array(edited.get(collection), collection)?;
    for item in current[collection]
        .as_array_mut()
        .expect("document collection")
    {
        let Some(id) = item[id_field].as_str().map(str::to_string) else {
            continue;
        };
        let Some(source) = edited_items.iter().find(|source| source[id_field] == id) else {
            continue;
        };
        for field in fields {
            let value = source[*field].as_str().unwrap_or("").trim();
            let next = if value.is_empty() {
                Value::Null
            } else {
                json!(value)
            };
            if item[*field] == next {
                continue;
            }
            let required = match collection {
                "participants" => *field == "displayName",
                "topics" | "actionItems" | "openIssues" => *field == "title",
                "decisions" => *field == "statement",
                "questions" => *field == "text",
                "notes" => *field == "body",
                _ => false,
            };
            validate_edited_text(value, required, field)?;
            if value.is_empty() {
                item.as_object_mut().expect("document item").remove(*field);
            } else {
                item[*field] = next;
            }
            mark_record_field(item, &format!("/{field}"), now);
        }
    }
    Ok(())
}

fn copy_nested_collection_text(
    current: &mut Value,
    edited: &Value,
    section: &str,
    collection: &str,
    id_field: &str,
    fields: &[&str],
    now: &str,
) -> Result<(), String> {
    let edited_items = array(edited[section].get(collection), collection)?;
    let current_items = current[section][collection]
        .as_array_mut()
        .ok_or_else(|| format!("{section}.{collection}はarrayである必要があります。"))?;
    for item in current_items {
        let Some(id) = item[id_field].as_str().map(str::to_string) else {
            continue;
        };
        let Some(source) = edited_items.iter().find(|source| source[id_field] == id) else {
            continue;
        };
        for field in fields {
            let value = source[*field].as_str().unwrap_or("").trim();
            if item[*field] == value {
                continue;
            }
            validate_edited_text(value, true, field)?;
            item[*field] = json!(value);
            mark_record_field(item, &format!("/{field}"), now);
        }
    }
    Ok(())
}

fn copy_question_answers(current: &mut Value, edited: &Value, now: &str) -> Result<(), String> {
    let edited_items = array(edited.get("questions"), "questions")?;
    for item in current["questions"]
        .as_array_mut()
        .expect("document questions")
    {
        let Some(id) = item["questionId"].as_str().map(str::to_string) else {
            continue;
        };
        let Some(source) = edited_items
            .iter()
            .find(|source| source["questionId"] == id)
        else {
            continue;
        };
        let Some(answer) = item.get_mut("answer") else {
            continue;
        };
        let value = source["answer"]["text"].as_str().unwrap_or("").trim();
        if answer["text"] == value {
            continue;
        }
        validate_edited_text(value, true, "answer.text")?;
        answer["text"] = json!(value);
        mark_record_field(item, "/answer/text", now);
    }
    Ok(())
}

fn validate_edited_text(value: &str, required: bool, field: &str) -> Result<(), String> {
    if required && value.is_empty() {
        return Err(format!("{field}は空にできません。"));
    }
    if value.chars().count() > 20_000 {
        return Err(format!("{field}が長すぎます。"));
    }
    Ok(())
}

fn user_field_state(now: &str) -> Value {
    json!({"source":"user","basis":"user_supplied","locked":true,"updatedAt":now})
}

fn mark_record_field(item: &mut Value, pointer: &str, now: &str) {
    item["recordMeta"]["updatedAt"] = json!(now);
    item["recordMeta"]["fieldStates"][pointer] = user_field_state(now);
}

fn validate_document(
    document: &Value,
    transcript: &SummaryTranscriptSnapshot,
) -> Result<(), String> {
    validate_document_integrity(document)?;
    if document["sourceTranscript"]["contentHash"] != transcript_hash(transcript) {
        return Err("MeetingDocumentの形式またはTranscript参照が不正です。".into());
    }
    Ok(())
}

fn validate_document_integrity(document: &Value) -> Result<(), String> {
    if document["schemaVersion"] != SCHEMA_VERSION
        || document["documentType"] != "meeting"
        || document["revision"]
            .as_u64()
            .is_none_or(|revision| revision == 0)
    {
        return Err("MeetingDocumentの形式またはTranscript参照が不正です。".into());
    }
    let evidence: HashSet<_> = array(document.get("evidence"), "evidence")?
        .iter()
        .filter_map(|item| item["evidenceId"].as_str())
        .collect();
    for collection in [
        "speakerMappings",
        "topics",
        "decisions",
        "actionItems",
        "openIssues",
        "questions",
        "notes",
    ] {
        for item in array(document.get(collection), collection)? {
            if string_array(item.get("evidenceIds"), "evidenceIds")?
                .iter()
                .any(|id| !evidence.contains(*id))
            {
                return Err("MeetingDocumentのEvidence参照が不正です。".into());
            }
        }
    }
    for item in array(
        document
            .get("summary")
            .and_then(|summary| summary.get("keyPoints")),
        "summary.keyPoints",
    )? {
        if string_array(item.get("evidenceIds"), "evidenceIds")?
            .iter()
            .any(|id| !evidence.contains(*id))
        {
            return Err("KeyPointのEvidence参照が不正です。".into());
        }
    }
    let participants: HashSet<_> = array(document.get("participants"), "participants")?
        .iter()
        .filter_map(|item| item["participantId"].as_str())
        .collect();
    for item in array(document.get("actionItems"), "actionItems")? {
        if string_array(item.get("assigneeParticipantIds"), "assigneeParticipantIds")?
            .iter()
            .any(|id| !participants.contains(*id))
        {
            return Err("ActionItemの参加者参照が不正です。".into());
        }
    }
    for item in array(document.get("speakerMappings"), "speakerMappings")? {
        if !item["participantId"]
            .as_str()
            .is_some_and(|id| participants.contains(id))
        {
            return Err("SpeakerMappingの参加者参照が不正です。".into());
        }
    }
    Ok(())
}

fn documents_directory(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    Ok(crate::meeting_store::meeting_directory(app, meeting_id)?.join("meeting-documents"))
}
fn attempts_directory(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    Ok(documents_directory(app, meeting_id)?.join("attempts"))
}
fn lock_store() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    STORE_LOCK
        .lock()
        .map_err(|_| "会議ドキュメントの保存処理を開始できませんでした。".to_string())
}
fn manifest_path(directory: &Path) -> PathBuf {
    directory.join("manifest.json")
}
fn read_manifest(directory: &Path) -> Result<GenerationAttemptManifest, String> {
    let path = manifest_path(directory);
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("生成試行の記録を確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_ATTEMPT_ARTIFACT_BYTES {
        return Err("生成試行の記録が不正です。".into());
    }
    serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("生成試行の記録を読めませんでした: {error}"))?,
    )
    .map_err(|error| format!("生成試行の記録が壊れています: {error}"))
}
fn write_manifest(directory: &Path, manifest: &GenerationAttemptManifest) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("生成試行の記録を変換できませんでした: {error}"))?;
    let target = manifest_path(directory);
    let temporary = directory.join(format!(".manifest.{}.tmp", uuid::Uuid::now_v7()));
    write_new(&temporary, &bytes)?;
    if target.exists() {
        fs::remove_file(&target)
            .map_err(|error| format!("生成試行の記録を更新できませんでした: {error}"))?;
    }
    fs::rename(&temporary, &target)
        .map_err(|error| format!("生成試行の記録を確定できませんでした: {error}"))
}
fn create_parent_and_write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "生成試行の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("生成試行の保存先を作成できませんでした: {error}"))?;
    write_new(path, bytes)
}
fn validate_attempt_stage(stage: &str) -> Result<(), String> {
    if stage.is_empty()
        || stage.len() > 64
        || !stage
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("生成試行の段階名が不正です。".into());
    }
    Ok(())
}
fn safe_artifact_path(directory: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.split(['/', '\\']).any(|part| part == "..")
    {
        return Err("生成試行の保存パスが不正です。".into());
    }
    Ok(relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(directory.to_path_buf(), |path, part| path.join(part)))
}
fn latest_path(directory: &Path) -> PathBuf {
    directory.join("latest.json")
}
fn read_latest_in(directory: &Path) -> Result<Option<Value>, String> {
    let path = latest_path(directory);
    if !path.exists() {
        return Ok(None);
    }
    let metadata =
        fs::metadata(&path).map_err(|e| format!("会議ドキュメントを確認できませんでした: {e}"))?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return Err("保存済み会議ドキュメントが不正です。".into());
    }
    serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("会議ドキュメントを読めませんでした: {e}"))?,
    )
    .map(Some)
    .map_err(|e| format!("会議ドキュメントが壊れています: {e}"))
}
fn write_document(directory: &Path, document: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|e| format!("会議ドキュメントを変換できませんでした: {e}"))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("会議ドキュメントが大きすぎます。".into());
    }
    let revision = directory.join(format!("revision-{}.json", document["revision"]));
    write_new(&revision, &bytes)?;
    let latest = latest_path(directory);
    let temporary = directory.join(format!(".latest.{}.tmp", uuid::Uuid::now_v7()));
    write_new(&temporary, &bytes)?;
    if latest.exists() {
        fs::remove_file(&latest)
            .map_err(|e| format!("会議ドキュメントを更新できませんでした: {e}"))?
    }
    fs::rename(temporary, latest)
        .map_err(|e| format!("会議ドキュメントを確定できませんでした: {e}"))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("会議ドキュメントを書き込めませんでした: {e}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("会議ドキュメントを安全に保存できませんでした: {e}"))
}
fn object<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{name}はobjectである必要があります。"))
}
fn array<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a Vec<Value>, String> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name}はarrayである必要があります。"))
}
fn string_array<'a>(value: Option<&'a Value>, name: &str) -> Result<Vec<&'a str>, String> {
    array(value, name)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| format!("{name}には文字列だけを指定できます。"))
        })
        .collect()
}
fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    name: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{name}.{field}がありません。"))
}
fn valid_temporary_key(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}
fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7())
}
fn prefixed_id(prefix: &str, value: &str) -> String {
    if value.starts_with(&format!("{prefix}_")) {
        value.into()
    } else {
        format!("{prefix}_{value}")
    }
}
fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn transcript_hash(snapshot: &SummaryTranscriptSnapshot) -> String {
    sha256(&serde_json::to_vec(&snapshot.segments).unwrap_or_default())
}
fn canonical_text(value: &Value) -> String {
    for field in ["statement", "title", "text", "body", "displayName"] {
        if let Some(text) = value.get(field).and_then(Value::as_str) {
            let evidence = value.get("evidence").cloned().unwrap_or(Value::Null);
            return format!(
                "{}:{}",
                text.trim().to_lowercase(),
                serde_json::to_string(&evidence).unwrap_or_default()
            );
        }
    }
    serde_json::to_string(value).unwrap_or_default()
}
fn stable_speaker_id(value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    format!("spk_{}", &digest[..16])
}
fn clean_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, value| !value.is_null());
            for value in map.values_mut() {
                clean_nulls(value)
            }
        }
        Value::Array(values) => {
            for value in values {
                clean_nulls(value)
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_store::SummaryTranscriptSegment;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mutsuna-echo-meeting-schema-test-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn attempt_in(directory: &Path) -> GenerationAttempt {
        let attempt_id = uuid::Uuid::now_v7().to_string();
        let attempt_directory = directory.join(&attempt_id);
        fs::create_dir(&attempt_directory).expect("attempt directory");
        let now = chrono::Utc::now().to_rfc3339();
        write_manifest(
            &attempt_directory,
            &GenerationAttemptManifest {
                schema_version: ATTEMPT_SCHEMA_VERSION.into(),
                attempt_id: attempt_id.clone(),
                meeting_id: uuid::Uuid::now_v7().to_string(),
                transcription_id: uuid::Uuid::now_v7().to_string(),
                source_revision: 2,
                provider: "cloudflare".into(),
                requested_model: "test-model".into(),
                resolved_model: None,
                started_at: now.clone(),
                updated_at: now,
                completed_at: None,
                status: "generating".into(),
                stage: "starting".into(),
                error: None,
                artifacts: Vec::new(),
            },
        )
        .expect("manifest");
        GenerationAttempt {
            attempt_id,
            directory: attempt_directory,
        }
    }

    fn transcript() -> SummaryTranscriptSnapshot {
        SummaryTranscriptSnapshot {
            meeting_id: uuid::Uuid::now_v7().to_string(),
            transcription_id: uuid::Uuid::now_v7().to_string(),
            revision: 0,
            language: "ja".into(),
            segments: vec![SummaryTranscriptSegment {
                segment_id: "segment-1".into(),
                speaker: "Speaker 1".into(),
                start_ms: 10,
                end_ms: 20,
                text: "田中さんが対応します。".into(),
            }],
        }
    }

    fn candidate(segment_id: &str) -> Value {
        json!({
            "meeting":{"title":"定例","meetingType":"internal","timeZone":"Asia/Tokyo","languageCodes":["ja"],"fieldBasis":{"title":"explicit"}},
            "participants":[{"key":"p1","displayName":"田中","kind":"person","attendance":"present","speakerIds":["Speaker 1"],"identityStatus":"identified","evidence":[{"relation":"direct","spans":[{"segmentId":segment_id}]}],"fieldBasis":{"displayName":"explicit"}}],
            "summary":{"overview":"概要","keyPoints":[],"fieldBasis":{"overview":"explicit"}},
            "topics":[],"decisions":[],
            "actionItems":[{"key":"a1","title":"対応","status":"open","assigneeParticipantKeys":["p1"],"topicKeys":[],"relatedDecisionKeys":[],"blockerIssueKeys":[],"evidence":[{"relation":"direct","spans":[{"segmentId":segment_id}]}],"fieldBasis":{"title":"explicit"}}],
            "openIssues":[],"questions":[],"notes":[]
        })
    }

    #[test]
    fn candidate_accepts_all_required_structures_and_references() {
        validate_candidate(&candidate("segment-1"), &transcript()).expect("valid candidate");
    }

    #[test]
    fn candidate_does_not_require_title_before_quality_check() {
        let mut value = candidate("segment-1");
        value["meeting"]
            .as_object_mut()
            .expect("meeting")
            .remove("title");
        validate_candidate(&value, &transcript()).expect("candidate without title");
    }

    #[test]
    fn first_generation_title_is_discarded_before_validation() {
        let mut value = candidate("segment-1");
        value["meeting"]["title"] = json!("first-stage title");
        value["meeting"]["fieldBasis"]["/title"] = json!("inferred");

        normalize_candidate_enums(&mut value);

        assert!(value["meeting"].get("title").is_none());
        assert!(value["meeting"]["fieldBasis"].get("/title").is_none());
        validate_candidate(&value, &transcript()).expect("candidate without first-stage title");
    }

    #[test]
    fn candidate_requires_non_empty_summary_overview() {
        let mut missing = candidate("segment-1");
        missing["summary"]
            .as_object_mut()
            .expect("summary")
            .remove("overview");
        assert!(validate_candidate(&missing, &transcript()).is_err());

        let mut empty = candidate("segment-1");
        empty["summary"]["overview"] = json!("   ");
        assert!(validate_candidate(&empty, &transcript()).is_err());
    }

    #[test]
    fn generation_attempt_keeps_raw_candidate_and_failure() {
        let root = TestDirectory::new();
        let attempt = attempt_in(&root.0);
        let raw = r#"{"meeting":{"meetingType":"unsupported"}}"#;

        attempt
            .record_response("single", raw, true)
            .expect("raw response");
        attempt
            .record_candidate(
                "single",
                &json!({"meeting":{"meetingType":"unknown"}}),
                true,
            )
            .expect("candidate");
        attempt
            .fail("validation_failed", "meetingTypeの値に対応していません。")
            .expect("failure");

        assert_eq!(
            fs::read_to_string(attempt.directory.join("responses/single.txt"))
                .expect("saved response"),
            raw
        );
        assert!(attempt.directory.join("candidates/single.json").is_file());
        let manifest = read_manifest(&attempt.directory).expect("saved manifest");
        assert_eq!(manifest.status, "failed");
        assert_eq!(manifest.stage, "validation_failed");
        assert_eq!(manifest.artifacts.len(), 2);
        assert!(manifest
            .artifacts
            .iter()
            .all(|artifact| artifact.final_output));
    }

    #[test]
    fn generation_attempt_completion_preserves_artifacts() {
        let root = TestDirectory::new();
        let attempt = attempt_in(&root.0);
        attempt
            .record_response("merge", "{}", true)
            .expect("raw response");
        attempt.complete("resolved-model").expect("completion");

        let manifest = read_manifest(&attempt.directory).expect("saved manifest");
        assert_eq!(manifest.status, "completed");
        assert_eq!(manifest.stage, "persisted");
        assert_eq!(manifest.resolved_model.as_deref(), Some("resolved-model"));
        assert!(attempt.directory.join("responses/merge.txt").is_file());
    }

    #[test]
    fn saved_response_parser_accepts_json_fences_and_rejects_traversal() {
        assert_eq!(
            parse_candidate_json("```json\n{\"meeting\":{}}\n```").expect("candidate")["meeting"],
            json!({})
        );
        assert!(safe_artifact_path(Path::new("attempt"), "../meeting.json").is_err());
        assert!(safe_artifact_path(Path::new("attempt"), "/absolute.json").is_err());
    }

    #[test]
    fn saved_response_parser_extracts_candidate_after_acp_notice() {
        let parsed = parse_candidate_json(concat!(
            "Warning: Skill descriptions were shortened.\n",
            r#"{"meeting":{"title":"定例"},"summary":{"overview":"概要"}}"#,
        ))
        .expect("candidate after notice");

        assert_eq!(parsed["meeting"]["title"], "定例");
    }

    #[test]
    fn revalidation_prefers_normalized_candidate_over_raw_response() {
        let root = TestDirectory::new();
        let attempt = attempt_in(&root.0);
        attempt
            .record_response("single", "Warning before JSON", true)
            .expect("raw response");
        attempt
            .record_candidate("single", &json!({"meeting":{"title":"定例"}}), true)
            .expect("candidate");
        let manifest = read_manifest(&attempt.directory).expect("manifest");

        let selected = select_revalidation_artifact(&manifest).expect("artifact");

        assert_eq!(selected.kind, "candidate");
        assert_eq!(selected.path, "candidates/single.json");
    }

    #[test]
    fn candidate_rejects_unknown_evidence_and_reference() {
        assert!(validate_candidate(&candidate("missing"), &transcript()).is_err());
        let mut value = candidate("segment-1");
        value["actionItems"][0]["assigneeParticipantKeys"] = json!(["p9"]);
        assert!(validate_candidate(&value, &transcript()).is_err());
    }

    #[test]
    fn meeting_type_is_normalized_before_candidate_validation() {
        let mut localized = candidate("segment-1");
        localized["meeting"]["meetingType"] = json!("社内会議");
        normalize_candidate_enums(&mut localized);
        assert_eq!(localized["meeting"]["meetingType"], "internal");
        validate_candidate(&localized, &transcript()).expect("normalized candidate");

        localized["meeting"]["meetingType"] = json!("モデル独自の分類");
        normalize_candidate_enums(&mut localized);
        assert_eq!(localized["meeting"]["meetingType"], "unknown");
    }

    #[test]
    fn scalar_field_basis_is_expanded_to_json_pointer_maps() {
        let mut value = candidate("segment-1");
        value["meeting"]["fieldBasis"] = json!("inferred");
        value["summary"]["fieldBasis"] = json!("normalized");
        value["participants"][0]["fieldBasis"] = json!("explicit");
        value["actionItems"][0]["fieldBasis"] = json!("推論");

        normalize_candidate_enums(&mut value);

        assert_eq!(value["meeting"]["fieldBasis"]["/meetingType"], "inferred");
        assert_eq!(value["summary"]["fieldBasis"]["/overview"], "normalized");
        assert_eq!(
            value["participants"][0]["fieldBasis"]["/displayName"],
            "explicit"
        );
        assert_eq!(value["actionItems"][0]["fieldBasis"]["/title"], "inferred");
        validate_candidate(&value, &transcript()).expect("normalized scalar basis");
    }

    #[test]
    fn shorthand_candidate_shape_is_normalized_without_inventing_content() {
        let mut value = candidate("segment-1");
        let summary = value
            .as_object_mut()
            .expect("root")
            .remove("summary")
            .expect("summary");
        value["meeting"]["summary"] = summary;
        value["meeting"]["summary"]["keyPoints"] = json!(["重要な要点"]);
        let participant = value["participants"][0]
            .as_object_mut()
            .expect("participant");
        let display_name = participant.remove("displayName").expect("display name");
        participant.insert("name".into(), display_name);
        for field in ["kind", "attendance", "speakerIds", "identityStatus"] {
            participant.remove(field);
        }
        value["topics"] = json!([{
            "key": "t1",
            "title": "議題",
            "evidence": [],
            "fieldBasis": "inferred"
        }]);

        normalize_candidate_enums(&mut value);

        assert!(value.get("summary").is_some());
        assert!(value["meeting"].get("summary").is_none());
        assert_eq!(value["participants"][0]["displayName"], "田中");
        assert_eq!(value["participants"][0]["kind"], "unknown");
        assert_eq!(value["topics"][0]["order"], 0);
        assert_eq!(value["summary"]["keyPoints"][0]["text"], "重要な要点");
        validate_candidate(&value, &transcript()).expect("normalized shorthand candidate");
    }

    #[test]
    fn missing_temporary_keys_and_scalar_references_are_normalized() {
        let mut value = candidate("segment-1");
        value["topics"] = json!([{
            "key": "t1", "title": "議題", "participantKeys": "p1",
            "evidence": [], "fieldBasis": {"/title": "explicit"}
        }]);
        value["decisions"] = json!([{
            "statement": "実施する", "status": "active", "topicKeys": "t1",
            "ownerParticipantKeys": "p1", "supersedesDecisionKeys": null,
            "evidence": [], "fieldBasis": {"/statement": "explicit"}
        }]);
        value["actionItems"] = json!([{
            "title": "対応", "status": "open", "assigneeParticipantKeys": "p1",
            "topicKeys": "t1", "relatedDecisionKeys": "d1", "blockerIssueKeys": null,
            "evidence": [], "fieldBasis": {"/title": "explicit"}
        }]);
        value["openIssues"] = json!([{
            "title": "課題", "status": "open", "ownerParticipantKeys": "p1",
            "topicKeys": "t1", "relatedDecisionKeys": "d1", "relatedActionItemKeys": "a1",
            "evidence": [], "fieldBasis": {"/title": "explicit"}
        }]);
        value["questions"] = json!([{
            "text": "確認事項", "status": "open", "directedToParticipantKeys": "p1",
            "topicKeys": "t1", "relatedIssueKeys": "i1",
            "evidence": [], "fieldBasis": {"/text": "explicit"}
        }]);
        value["notes"] = json!([{
            "body": "補足", "topicKeys": "t1",
            "evidence": [], "fieldBasis": {"/body": "explicit"}
        }]);

        normalize_candidate_enums(&mut value);

        assert_eq!(value["decisions"][0]["key"], "d1");
        assert_eq!(value["actionItems"][0]["key"], "a1");
        assert_eq!(value["openIssues"][0]["key"], "i1");
        assert_eq!(value["questions"][0]["key"], "q1");
        assert_eq!(value["notes"][0]["key"], "n1");
        assert_eq!(value["decisions"][0]["ownerParticipantKeys"], json!(["p1"]));
        assert_eq!(
            value["actionItems"][0]["relatedDecisionKeys"],
            json!(["d1"])
        );
        assert_eq!(value["decisions"][0]["supersedesDecisionKeys"], json!([]));
        validate_candidate(&value, &transcript()).expect("normalized generated shorthand");
    }

    #[test]
    fn single_evidence_objects_are_preserved_as_arrays() {
        let mut value = candidate("segment-1");
        let participant_evidence = value["participants"][0]["evidence"][0].take();
        value["participants"][0]["evidence"] = participant_evidence;
        value["actionItems"][0]["evidence"] = Value::Null;
        value["summary"]["keyPoints"] = json!([{
            "key": "k1",
            "text": "重要事項",
            "evidence": {
                "relation": "direct",
                "spans": [{"segmentId": "segment-1"}],
                "quote": "田中さんが対応します。"
            },
            "fieldBasis": {"/text": "explicit"}
        }]);

        normalize_candidate_enums(&mut value);

        assert!(value["participants"][0]["evidence"].is_array());
        assert_eq!(
            value["participants"][0]["evidence"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(value["actionItems"][0]["evidence"], json!([]));
        assert!(value["summary"]["keyPoints"][0]["evidence"].is_array());
        validate_candidate(&value, &transcript()).expect("normalized evidence arrays");
    }

    #[test]
    fn missing_meeting_locale_metadata_uses_safe_defaults() {
        let mut value = candidate("segment-1");
        value["meeting"]
            .as_object_mut()
            .expect("meeting")
            .remove("timeZone");
        value["meeting"]
            .as_object_mut()
            .expect("meeting")
            .remove("languageCodes");

        normalize_candidate(&mut value, Some("ja"));

        assert_eq!(value["meeting"]["timeZone"], "unknown");
        assert_eq!(value["meeting"]["languageCodes"], json!(["ja"]));
        validate_candidate(&value, &transcript()).expect("normalized meeting locale metadata");
    }

    #[test]
    fn normalizer_assigns_ids_and_maps_speakers_and_participants() {
        let transcript = transcript();
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: transcript.revision,
            provider: "test".into(),
            model: "test-model".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: candidate("segment-1"),
        };
        let document = normalize("fallback", &generated, &transcript, 1, None).expect("normalize");
        validate_document(&document, &transcript).expect("document");
        assert!(document["participants"][0]["participantId"]
            .as_str()
            .is_some_and(|id| id.starts_with("par_")));
        assert!(document["participants"][0].get("evidenceIds").is_none());
        assert_eq!(
            document["speakerMappings"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            document["actionItems"][0]["assigneeParticipantIds"][0],
            document["participants"][0]["participantId"]
        );
        assert!(document["evidence"][0]["spans"][0]["segmentId"]
            .as_str()
            .is_some_and(|id| id.starts_with("seg_")));
    }

    #[test]
    fn user_edit_updates_only_editable_content_and_locks_fields() {
        let transcript = transcript();
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: transcript.revision,
            provider: "test".into(),
            model: "test-model".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: candidate("segment-1"),
        };
        let mut current =
            normalize("fallback", &generated, &transcript, 1, None).expect("normalize");
        let original_document_id = current["documentId"].clone();
        let mut edited = current.clone();
        edited["summary"]["overview"] = json!("編集した概要");
        edited["actionItems"][0]["title"] = json!("編集した対応");
        edited["actionItems"][0]["status"] = json!("done");
        edited["documentId"] = json!("改ざんされたID");

        copy_document_text(&mut current, &edited, "2026-08-12T01:00:00Z").expect("edit");

        assert_eq!(current["summary"]["overview"], "編集した概要");
        assert_eq!(current["actionItems"][0]["title"], "編集した対応");
        assert_eq!(current["actionItems"][0]["status"], "done");
        assert_eq!(current["documentId"], original_document_id);
        assert_eq!(
            current["editorial"]["fieldStates"]["/summary/overview"]["locked"],
            true
        );
        assert_eq!(
            current["actionItems"][0]["recordMeta"]["fieldStates"]["/title"]["source"],
            "user"
        );
    }

    #[test]
    fn user_edit_rejects_empty_required_text() {
        let transcript = transcript();
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: transcript.revision,
            provider: "test".into(),
            model: "test-model".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: candidate("segment-1"),
        };
        let mut current =
            normalize("fallback", &generated, &transcript, 1, None).expect("normalize");
        let mut edited = current.clone();
        edited["summary"]["overview"] = json!("   ");

        assert!(copy_document_text(&mut current, &edited, "2026-08-12T01:00:00Z").is_err());
    }

    #[test]
    fn action_status_edit_ignores_unchanged_optional_empty_title() {
        let transcript = transcript();
        let mut source = candidate("segment-1");
        source["notes"] = json!([{
            "key":"n1",
            "body":"補足",
            "topicKeys":[],
            "evidence":[],
            "fieldBasis":{"body":"explicit"}
        }]);
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: transcript.revision,
            provider: "test".into(),
            model: "test-model".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: source,
        };
        let mut current =
            normalize("fallback", &generated, &transcript, 1, None).expect("normalize");
        let mut edited = current.clone();
        edited["actionItems"][0]["status"] = json!("done");

        copy_document_text(&mut current, &edited, "2026-08-12T01:00:00Z")
            .expect("status-only edit");

        assert_eq!(current["actionItems"][0]["status"], "done");
        assert!(current["notes"][0].get("title").is_none());
    }

    #[test]
    fn user_edit_integrity_validation_accepts_an_older_source_transcript() {
        let transcript = transcript();
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: transcript.revision,
            provider: "test".into(),
            model: "test-model".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: candidate("segment-1"),
        };
        let mut document =
            normalize("fallback", &generated, &transcript, 1, None).expect("normalize");
        let mut edited = document.clone();
        edited["actionItems"][0]["status"] = json!("done");
        copy_document_text(&mut document, &edited, "2026-08-12T01:00:00Z").expect("status edit");
        document["revision"] = json!(2);

        let mut newer_transcript = transcript.clone();
        newer_transcript.revision += 1;
        newer_transcript.segments[0].text = "編集後の文字起こしです。".into();

        assert!(validate_document(&document, &newer_transcript).is_err());
        validate_document_integrity(&document).expect("user-edited document integrity");
        assert_eq!(document["actionItems"][0]["status"], "done");
    }

    #[test]
    fn regeneration_preserves_ids_and_updates_cross_references() {
        let transcript = transcript();
        let generated = GeneratedCandidate {
            meeting_id: transcript.meeting_id.clone(),
            transcription_id: transcript.transcription_id.clone(),
            source_revision: 0,
            provider: "test".into(),
            model: "test".into(),
            generated_at: "2026-08-12T00:00:00Z".into(),
            candidate: candidate("segment-1"),
        };
        let previous = normalize("fallback", &generated, &transcript, 1, None).expect("first");
        let participant_id = previous["participants"][0]["participantId"].clone();
        let regenerated =
            normalize("fallback", &generated, &transcript, 2, Some(&previous)).expect("regenerate");
        assert_eq!(
            regenerated["participants"][0]["participantId"],
            participant_id
        );
        assert_eq!(
            regenerated["actionItems"][0]["assigneeParticipantIds"][0],
            participant_id
        );
        assert_eq!(
            regenerated["generationRuns"].as_array().map(Vec::len),
            Some(2)
        );
    }
}
