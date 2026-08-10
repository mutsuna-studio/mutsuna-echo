use std::{collections::HashSet, fs, io::Write, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::TranscriptionProvider;

const SETTINGS_FILE: &str = "context.json";
const LEARNED_CORRECTIONS_FILE: &str = "learned-corrections.json";
const MAX_SETTINGS_BYTES: u64 = 128 * 1024;
const MAX_LEARNED_CORRECTIONS_BYTES: u64 = 512 * 1024;
const MAX_BACKGROUND_CHARS: usize = 10_000;
const MAX_STORED_TERMS: usize = 1_000;
const MAX_STORED_CORRECTIONS: usize = 500;
const MAX_STORED_CONTEXT_CHARS: usize = 20_000;
const MAX_SONIOX_CONTEXT_CHARS: usize = 10_000;
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnedCorrection {
    from: String,
    to: String,
    confirmations: u32,
    updated_at: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnedCorrectionsFile {
    #[serde(default = "learned_corrections_schema_version")]
    schema_version: u8,
    #[serde(default)]
    corrections: Vec<LearnedCorrection>,
}

fn learned_corrections_schema_version() -> u8 {
    1
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionContext {
    #[serde(default)]
    pub(crate) background: String,
    #[serde(default)]
    pub(crate) terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextCorrection {
    pub(crate) from: String,
    pub(crate) to: String,
}

impl TranscriptionContext {
    pub(crate) fn is_empty(&self) -> bool {
        self.background.is_empty() && self.terms.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingTranscriptionContext {
    #[serde(default)]
    pub(crate) background: String,
    #[serde(default)]
    pub(crate) terms: Vec<String>,
    #[serde(default)]
    pub(crate) corrections: Vec<TextCorrection>,
    #[serde(default = "default_true")]
    pub(crate) use_global: bool,
}

impl Default for MeetingTranscriptionContext {
    fn default() -> Self {
        Self {
            background: String::new(),
            terms: Vec::new(),
            corrections: Vec::new(),
            use_global: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GlobalTranscriptionContextSettings {
    #[serde(default)]
    pub(crate) context_enabled: bool,
    #[serde(default)]
    pub(crate) background: String,
    #[serde(default)]
    pub(crate) terms: Vec<String>,
    #[serde(default)]
    pub(crate) corrections: Vec<TextCorrection>,
}

fn default_true() -> bool {
    true
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("transcription").join(SETTINGS_FILE))
        .map_err(|error| format!("文字起こしコンテキストの保存先を取得できませんでした: {error}"))
}

fn learned_corrections_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("transcription").join(LEARNED_CORRECTIONS_FILE))
        .map_err(|error| format!("学習した表記補正の保存先を取得できませんでした: {error}"))
}

fn read_learned_corrections(app: &AppHandle) -> Result<LearnedCorrectionsFile, String> {
    let path = learned_corrections_path(app)?;
    if !path.exists() {
        return Ok(LearnedCorrectionsFile {
            schema_version: learned_corrections_schema_version(),
            corrections: Vec::new(),
        });
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("学習した表記補正を確認できませんでした: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_LEARNED_CORRECTIONS_BYTES
    {
        return Err("学習した表記補正の形式が不正です。".into());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("学習した表記補正を読み込めませんでした: {error}"))?;
    let file: LearnedCorrectionsFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("学習した表記補正が壊れています: {error}"))?;
    if file.schema_version != learned_corrections_schema_version() {
        return Err("学習した表記補正のバージョンに対応していません。".into());
    }
    Ok(file)
}

fn write_learned_corrections(app: &AppHandle, file: &LearnedCorrectionsFile) -> Result<(), String> {
    let path = learned_corrections_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "学習した表記補正の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("学習した表記補正の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(
        ".{LEARNED_CORRECTIONS_FILE}.{}.tmp",
        uuid::Uuid::now_v7()
    ));
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|error| format!("学習した表記補正を作成できませんでした: {error}"))?;
    let result = (|| {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("学習した表記補正を保存できませんでした: {error}"))?;
        output
            .write_all(&bytes)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("学習した表記補正を保存できませんでした: {error}"))?;
        let backup = path.with_extension("json.backup");
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("古い学習済み表記補正を削除できませんでした: {error}"))?;
        }
        if path.exists() {
            fs::rename(&path, &backup).map_err(|error| {
                format!("学習した表記補正を更新用に退避できませんでした: {error}")
            })?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &path);
            }
            return Err(format!("学習した表記補正を確定できませんでした: {error}"));
        }
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!("学習した表記補正の退避ファイルを削除できませんでした: {error}")
            })?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn correction_from_manual_edit(before: &str, after: &str) -> Option<TextCorrection> {
    if before == after || before.contains(['\r', '\n']) || after.contains(['\r', '\n']) {
        return None;
    }
    let before_chars = before.chars().collect::<Vec<_>>();
    let after_chars = after.chars().collect::<Vec<_>>();
    let prefix = before_chars
        .iter()
        .zip(&after_chars)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before_chars[prefix..]
        .iter()
        .rev()
        .zip(after_chars[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let from = before_chars[prefix..before_chars.len() - suffix]
        .iter()
        .collect::<String>();
    let to = after_chars[prefix..after_chars.len() - suffix]
        .iter()
        .collect::<String>();
    let from = from.trim();
    let to = to.trim();
    let contains_word = |value: &str| value.chars().any(char::is_alphanumeric);
    let changes_script = from.chars().any(|value| value.is_ascii_alphabetic())
        != to.chars().any(|value| value.is_ascii_alphabetic());
    let is_small_typo =
        levenshtein_distance(from, to) <= 2.max(from.chars().count().min(to.chars().count()) / 3);
    if from.is_empty()
        || to.is_empty()
        || from == to
        || from.chars().count() > 32
        || to.chars().count() > 32
        || !contains_word(from)
        || !contains_word(to)
        || (!changes_script && !is_small_typo)
    {
        return None;
    }
    Some(TextCorrection {
        from: from.to_string(),
        to: to.to_string(),
    })
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_char) in right.iter().enumerate() {
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

pub(crate) fn learn_corrections(
    app: &AppHandle,
    corrections: Vec<TextCorrection>,
) -> Result<(), String> {
    if corrections.is_empty() {
        return Ok(());
    }
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| "学習した表記補正を保存できませんでした。".to_string())?;
    let mut file = read_learned_corrections(app)?;
    let now = chrono::Utc::now().to_rfc3339();
    for correction in normalize_corrections(corrections) {
        if let Some(existing) = file
            .corrections
            .iter_mut()
            .find(|existing| existing.from == correction.from)
        {
            existing.confirmations = if existing.to == correction.to {
                existing.confirmations.saturating_add(1)
            } else {
                1
            };
            existing.to = correction.to;
            existing.updated_at = now.clone();
        } else {
            file.corrections.push(LearnedCorrection {
                from: correction.from,
                to: correction.to,
                confirmations: 1,
                updated_at: now.clone(),
            });
        }
    }
    file.corrections
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    file.corrections.truncate(MAX_STORED_CORRECTIONS);
    write_learned_corrections(app, &file)
}

fn normalize_terms(terms: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    terms
        .into_iter()
        .map(|term| term.trim().to_string())
        .filter(|term| !term.is_empty() && seen.insert(term.clone()))
        .collect()
}

fn normalize_corrections(corrections: Vec<TextCorrection>) -> Vec<TextCorrection> {
    let mut seen = HashSet::new();
    corrections
        .into_iter()
        .map(|correction| TextCorrection {
            from: correction.from.trim().to_string(),
            to: correction.to.trim().to_string(),
        })
        .filter(|correction| {
            !correction.from.is_empty()
                && !correction.to.is_empty()
                && correction.from != correction.to
                && seen.insert(correction.from.clone())
        })
        .collect()
}

fn normalize_context(context: TranscriptionContext) -> TranscriptionContext {
    TranscriptionContext {
        background: context.background.trim().to_string(),
        terms: normalize_terms(context.terms),
    }
}

fn validate_stored(
    background: &str,
    terms: &[String],
    corrections: &[TextCorrection],
) -> Result<(), String> {
    if background.chars().count() > MAX_BACKGROUND_CHARS {
        return Err(format!(
            "背景情報は{MAX_BACKGROUND_CHARS}文字以内にしてください。"
        ));
    }
    if terms.len() > MAX_STORED_TERMS {
        return Err(format!(
            "重要用語は{MAX_STORED_TERMS}件以内にしてください。"
        ));
    }
    if corrections.len() > MAX_STORED_CORRECTIONS {
        return Err(format!(
            "表記補正は{MAX_STORED_CORRECTIONS}件以内にしてください。"
        ));
    }
    if corrections.iter().any(|correction| {
        correction.from.chars().count() > 100 || correction.to.chars().count() > 100
    }) {
        return Err("表記補正の変換前・変換後はそれぞれ100文字以内にしてください。".into());
    }
    let total_chars = background.chars().count()
        + terms.iter().map(|term| term.chars().count()).sum::<usize>()
        + corrections
            .iter()
            .map(|correction| correction.from.chars().count() + correction.to.chars().count())
            .sum::<usize>();
    if total_chars > MAX_STORED_CONTEXT_CHARS {
        return Err(format!(
            "背景情報、重要用語、表記補正は合計{MAX_STORED_CONTEXT_CHARS}文字以内にしてください。"
        ));
    }
    Ok(())
}

fn read_global_settings(app: &AppHandle) -> Result<GlobalTranscriptionContextSettings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(GlobalTranscriptionContextSettings::default());
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("文字起こしコンテキスト設定を確認できませんでした: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_SETTINGS_BYTES
    {
        return Err("文字起こしコンテキスト設定の形式が不正です。".into());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("文字起こしコンテキスト設定を読み込めませんでした: {error}"))?;
    let mut settings: GlobalTranscriptionContextSettings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("文字起こしコンテキスト設定が壊れています: {error}"))?;
    settings.background = settings.background.trim().to_string();
    settings.terms = normalize_terms(settings.terms);
    settings.corrections = normalize_corrections(settings.corrections);
    validate_stored(&settings.background, &settings.terms, &settings.corrections)?;
    Ok(settings)
}

fn write_global_settings(
    app: &AppHandle,
    mut settings: GlobalTranscriptionContextSettings,
) -> Result<GlobalTranscriptionContextSettings, String> {
    settings.background = settings.background.trim().to_string();
    settings.terms = normalize_terms(settings.terms);
    settings.corrections = normalize_corrections(settings.corrections);
    validate_stored(&settings.background, &settings.terms, &settings.corrections)?;
    let path = settings_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "文字起こしコンテキストの保存先が不正です。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| {
        format!("文字起こしコンテキストの保存先を作成できませんでした: {error}")
    })?;
    let temporary = parent.join(format!(".{SETTINGS_FILE}.{}.tmp", uuid::Uuid::now_v7()));
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("文字起こしコンテキスト設定を作成できませんでした: {error}"))?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!("文字起こしコンテキスト設定を保存できませんでした: {error}")
            })?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| {
                format!("文字起こしコンテキスト設定を保存できませんでした: {error}")
            })?;
        let backup = path.with_extension("json.backup");
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!("古い文字起こしコンテキスト設定を削除できませんでした: {error}")
            })?;
        }
        if path.exists() {
            fs::rename(&path, &backup).map_err(|error| {
                format!("文字起こしコンテキスト設定を更新用に退避できませんでした: {error}")
            })?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &path);
            }
            return Err(format!(
                "文字起こしコンテキスト設定を確定できませんでした: {error}"
            ));
        }
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!("文字起こしコンテキスト設定の退避ファイルを削除できませんでした: {error}")
            })?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|()| settings)
}

pub(crate) fn merged_context(
    global: &GlobalTranscriptionContextSettings,
    meeting: &MeetingTranscriptionContext,
) -> Option<TranscriptionContext> {
    if !global.context_enabled {
        return None;
    }
    let global_background = meeting.use_global.then_some(global.background.as_str());
    let background = [global_background, Some(meeting.background.as_str())]
        .into_iter()
        .flatten()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut terms = if meeting.use_global {
        global.terms.clone()
    } else {
        Vec::new()
    };
    terms.extend(meeting.terms.clone());
    let context = normalize_context(TranscriptionContext { background, terms });
    (!context.is_empty()).then_some(context)
}

pub(crate) fn effective_context(
    app: &AppHandle,
    meeting_id: &str,
    provider: TranscriptionProvider,
) -> Result<Option<TranscriptionContext>, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| "文字起こしコンテキスト設定を読み込めませんでした。".to_string())?;
    let global = read_global_settings(app)?;
    let meeting = crate::meeting_store::transcription_context(app, meeting_id)?;
    prepare_for_provider(merged_context(&global, &meeting), provider)
}

pub(crate) fn effective_corrections(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<Vec<TextCorrection>, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| "表記補正設定を読み込めませんでした。".to_string())?;
    let global = read_global_settings(app)?;
    let meeting = crate::meeting_store::transcription_context(app, meeting_id)?;
    let mut corrections = if meeting.use_global {
        global.corrections
    } else {
        Vec::new()
    };
    corrections.extend(meeting.corrections);
    corrections.extend(
        read_learned_corrections(app)?
            .corrections
            .into_iter()
            .map(|correction| TextCorrection {
                from: correction.from,
                to: correction.to,
            }),
    );
    Ok(normalize_corrections(corrections))
}

fn prepare_for_provider(
    mut context: Option<TranscriptionContext>,
    provider: TranscriptionProvider,
) -> Result<Option<TranscriptionContext>, String> {
    match provider {
        TranscriptionProvider::Local => {
            if let Some(value) = context.as_mut() {
                // ReazonSpeech currently consumes only contextual hotwords.
                value.background.clear();
            }
        }
        TranscriptionProvider::ElevenLabs => {
            if let Some(value) = context.as_mut() {
                value.background.clear();
            }
        }
        TranscriptionProvider::Soniox => {}
    }
    if context.as_ref().is_some_and(TranscriptionContext::is_empty) {
        context = None;
    }
    if let Some(value) = &context {
        validate_for_provider(value, provider)?;
    }
    Ok(context)
}

pub(crate) fn validate_for_provider(
    context: &TranscriptionContext,
    provider: TranscriptionProvider,
) -> Result<(), String> {
    match provider {
        TranscriptionProvider::ElevenLabs => {
            if context.terms.len() > 1_000 {
                return Err("ElevenLabsで使用できる重要用語は1000件までです。".into());
            }
            for term in &context.terms {
                if term.chars().count() >= 50 {
                    return Err(format!("重要用語「{term}」は50文字未満にしてください。"));
                }
                if term.split_whitespace().count() > 5 {
                    return Err(format!("重要用語「{term}」は5語以内にしてください。"));
                }
                if term
                    .chars()
                    .any(|character| matches!(character, '<' | '>' | '{' | '}' | '[' | ']' | '\\'))
                {
                    return Err(format!(
                        "重要用語「{term}」には使用できない記号が含まれています。"
                    ));
                }
            }
        }
        TranscriptionProvider::Soniox => {
            let total_chars = context.background.chars().count()
                + context
                    .terms
                    .iter()
                    .map(|term| term.chars().count())
                    .sum::<usize>();
            if total_chars > MAX_SONIOX_CONTEXT_CHARS {
                return Err(format!(
                    "Sonioxへ送る背景情報と重要用語は合計{MAX_SONIOX_CONTEXT_CHARS}文字以内にしてください。"
                ));
            }
        }
        TranscriptionProvider::Local => {
            if context.terms.len() > 500 {
                return Err("ローカル文字起こしで使用できる重要用語は500件までです。".into());
            }
            if context
                .terms
                .iter()
                .any(|term| term.chars().count() > 64 || term.contains(['/', ':']))
            {
                return Err(
                    "ローカル文字起こしの重要用語は64文字以内で、/ と : は使用できません。".into(),
                );
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_global_transcription_context(
    app: AppHandle,
) -> Result<GlobalTranscriptionContextSettings, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| "文字起こしコンテキスト設定を読み込めませんでした。".to_string())?;
    read_global_settings(&app)
}

#[tauri::command]
pub(crate) fn set_global_transcription_context(
    app: AppHandle,
    settings: GlobalTranscriptionContextSettings,
) -> Result<GlobalTranscriptionContextSettings, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| "文字起こしコンテキスト設定を保存できませんでした。".to_string())?;
    write_global_settings(&app, settings)
}

#[tauri::command]
pub(crate) fn get_meeting_transcription_context(
    app: AppHandle,
    meeting_id: String,
) -> Result<MeetingTranscriptionContext, String> {
    crate::meeting_store::transcription_context(&app, &meeting_id)
}

#[tauri::command]
pub(crate) fn set_meeting_transcription_context(
    app: AppHandle,
    meeting_id: String,
    context: MeetingTranscriptionContext,
) -> Result<MeetingTranscriptionContext, String> {
    let context = MeetingTranscriptionContext {
        background: context.background.trim().to_string(),
        terms: normalize_terms(context.terms),
        corrections: normalize_corrections(context.corrections),
        use_global: context.use_global,
    };
    validate_stored(&context.background, &context.terms, &context.corrections)?;
    crate::meeting_store::set_transcription_context(&app, &meeting_id, context.clone())?;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::{
        correction_from_manual_edit, merged_context, prepare_for_provider, validate_for_provider,
        GlobalTranscriptionContextSettings, MeetingTranscriptionContext, TranscriptionContext,
    };
    use crate::transcription::TranscriptionProvider;

    #[test]
    fn disabled_context_is_never_merged() {
        let global = GlobalTranscriptionContextSettings {
            context_enabled: false,
            background: "会社の情報".into(),
            terms: vec!["Mutsuna".into()],
            corrections: Vec::new(),
        };
        assert_eq!(
            merged_context(&global, &MeetingTranscriptionContext::default()),
            None
        );
    }

    #[test]
    fn global_context_is_disabled_by_default() {
        assert!(!GlobalTranscriptionContextSettings::default().context_enabled);
    }

    #[test]
    fn merges_global_then_meeting_and_deduplicates_terms() {
        let global = GlobalTranscriptionContextSettings {
            context_enabled: true,
            background: "会社の情報".into(),
            terms: vec!["Mutsuna".into(), "Echo".into()],
            corrections: Vec::new(),
        };
        let meeting = MeetingTranscriptionContext {
            background: "製品会議".into(),
            terms: vec!["Echo".into(), "Scribe".into(), "  ".into()],
            corrections: Vec::new(),
            use_global: true,
        };
        assert_eq!(
            merged_context(&global, &meeting),
            Some(TranscriptionContext {
                background: "会社の情報\n\n製品会議".into(),
                terms: vec!["Mutsuna".into(), "Echo".into(), "Scribe".into()],
            })
        );
    }

    #[test]
    fn meeting_can_exclude_global_content() {
        let global = GlobalTranscriptionContextSettings {
            context_enabled: true,
            background: "会社の情報".into(),
            terms: vec!["Mutsuna".into()],
            corrections: Vec::new(),
        };
        let meeting = MeetingTranscriptionContext {
            background: "個別会議".into(),
            terms: vec!["限定用語".into()],
            corrections: Vec::new(),
            use_global: false,
        };
        let merged = merged_context(&global, &meeting).expect("meeting context");
        assert_eq!(merged.background, "個別会議");
        assert_eq!(merged.terms, ["限定用語"]);
    }

    #[test]
    fn validates_elevenlabs_keyterm_constraints() {
        let invalid = TranscriptionContext {
            background: String::new(),
            terms: vec!["invalid[term".into()],
        };
        assert!(validate_for_provider(&invalid, TranscriptionProvider::ElevenLabs).is_err());
        assert!(validate_for_provider(&invalid, TranscriptionProvider::Soniox).is_ok());
    }

    #[test]
    fn providers_receive_only_supported_context_sections() {
        let context = Some(TranscriptionContext {
            background: "会議の背景".into(),
            terms: vec!["Mutsuna".into()],
        });
        let elevenlabs = prepare_for_provider(context.clone(), TranscriptionProvider::ElevenLabs)
            .expect("ElevenLabs context")
            .expect("terms remain");
        assert!(elevenlabs.background.is_empty());
        assert_eq!(elevenlabs.terms, ["Mutsuna"]);
        assert_eq!(
            prepare_for_provider(context, TranscriptionProvider::Local)
                .expect("local context")
                .expect("local terms remain"),
            TranscriptionContext {
                background: String::new(),
                terms: vec!["Mutsuna".into()],
            }
        );
    }

    #[test]
    fn extracts_only_the_local_replacement_from_a_manual_edit() {
        let correction = correction_from_manual_edit(
            "今日はむつなエコーについて話します。",
            "今日はMutsuna Echoについて話します。",
        )
        .expect("correction");
        assert_eq!(correction.from, "むつなエコー");
        assert_eq!(correction.to, "Mutsuna Echo");
    }

    #[test]
    fn ignores_deletions_punctuation_and_large_rewrites() {
        assert!(correction_from_manual_edit("えー本題です", "本題です").is_none());
        assert!(correction_from_manual_edit("です。", "です！").is_none());
        assert!(correction_from_manual_edit(
            "短い文です",
            "これは内容そのものを書き換えた非常に長い文章であり、表記補正としては保存しません"
        )
        .is_none());
        assert!(correction_from_manual_edit("テストです。", "修正しました。").is_none());
        assert!(correction_from_manual_edit("こんにちわ", "こんにちは").is_some());
    }
}
