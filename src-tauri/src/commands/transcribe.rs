use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use lofty::{config::ParseOptions, file::AudioFile, probe::Probe};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::transcription::{Transcript, TranscriptionProvider};

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "wav", "flac"];
const MAX_AUDIO_FILE_SIZE: u64 = 5_000_000_000;

#[derive(Default)]
pub(crate) struct AudioSelectionState {
    selected: Mutex<Option<SelectedAudio>>,
    meeting_id: Mutex<Option<String>>,
    transcribing: AtomicBool,
    progress: Mutex<Option<TranscriptionProgress>>,
}

const TRANSCRIPTION_PROGRESS_EVENT: &str = "transcription-progress";

#[derive(Debug, Clone)]
struct SelectedAudio {
    path: PathBuf,
    descriptor: SelectedAudioFile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedAudioFile {
    meeting_id: String,
    name: String,
    size_bytes: u64,
    duration_ms: u64,
    playback_url: String,
}

impl SelectedAudioFile {
    pub(crate) fn meeting_id(&self) -> &str {
        &self.meeting_id
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionSession {
    selected_audio: Option<SelectedAudioFile>,
    transcribing: bool,
    progress: Option<TranscriptionProgress>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TranscriptionStage {
    Preparing,
    DetectingSpeech,
    Transcribing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionProgress {
    stage: TranscriptionStage,
    completed_chunks: u32,
    total_chunks: Option<u32>,
}

impl TranscriptionProgress {
    pub(crate) const fn new(
        stage: TranscriptionStage,
        completed_chunks: u32,
        total_chunks: Option<u32>,
    ) -> Self {
        Self {
            stage,
            completed_chunks,
            total_chunks,
        }
    }
}

pub(crate) fn publish_transcription_progress(app: &AppHandle, progress: TranscriptionProgress) {
    let state = app.state::<AudioSelectionState>();
    *state
        .progress
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(progress.clone());
    if let Err(error) = app.emit(TRANSCRIPTION_PROGRESS_EVENT, progress) {
        eprintln!("Could not emit transcription progress: {error:?}");
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionResult {
    transcript: Transcript,
    run: Option<crate::transcript_store::TranscriptionRunDetail>,
    persistence_warning: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionRequest {
    provider: TranscriptionProvider,
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectTranscriptionRunRequest {
    transcription_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateTranscriptDocumentRequest {
    transcription_id: String,
    expected_revision: u64,
    #[serde(default)]
    changes: Vec<crate::transcript_store::TranscriptSegmentChange>,
    #[serde(default)]
    speaker_labels: Vec<crate::transcript_store::TranscriptSpeakerLabelChange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetTranscriptDocumentRequest {
    transcription_id: String,
    expected_revision: u64,
}

fn describe_audio_path(path: &Path, meeting_id: String) -> Result<SelectedAudioFile, String> {
    let size_bytes = validate_audio_file(path)?;
    let audio = inspect_audio(path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("選択した音声ファイル")
        .to_string();
    Ok(SelectedAudioFile {
        playback_url: playback_url(&meeting_id),
        meeting_id,
        name,
        size_bytes,
        duration_ms: audio.duration_ms,
    })
}

fn playback_url(meeting_id: &str) -> String {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://mutsuna-audio.localhost/selected/{meeting_id}")
    } else {
        format!("mutsuna-audio://localhost/selected/{meeting_id}")
    }
}

pub(crate) fn validate_audio_path(path: &Path) -> Result<(), String> {
    validate_audio_file(path)?;
    inspect_audio(path)?;
    Ok(())
}

pub(crate) fn set_selected_audio_path(
    app: &AppHandle,
    path: PathBuf,
) -> Result<SelectedAudioFile, String> {
    let meeting_id = crate::meeting_store::resolve_or_create(app, &path)?;
    set_selected_audio(app, path, meeting_id)
}

pub(crate) fn set_selected_audio_with_meeting(
    app: &AppHandle,
    path: PathBuf,
    meeting_id: String,
) -> Result<SelectedAudioFile, String> {
    crate::meeting_store::link_existing(app, &meeting_id, &path)?;
    set_selected_audio(app, path, meeting_id)
}

pub(crate) fn restore_selected_meeting(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<SelectedAudioFile, String> {
    let path = crate::meeting_store::local_audio_path(app, meeting_id)?;
    set_selected_audio(app, path, meeting_id.to_string())
}

pub(crate) fn selected_meeting_id(app: &AppHandle) -> Result<String, String> {
    app.state::<AudioSelectionState>()
        .meeting_id
        .lock()
        .map_err(|_| "選択したMeetingの状態を取得できませんでした。".to_string())?
        .clone()
        .ok_or_else(|| "Meetingが選択されていません。".to_string())
}

pub(crate) fn select_meeting_without_audio(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<(), String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let state = app.state::<AudioSelectionState>();
    *state
        .selected
        .lock()
        .map_err(|_| "選択した音声の状態を更新できませんでした。".to_string())? = None;
    *state
        .meeting_id
        .lock()
        .map_err(|_| "選択したMeetingの状態を更新できませんでした。".to_string())? =
        Some(meeting_id.to_string());
    Ok(())
}

pub(crate) fn clear_selected_meeting(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    let state = app.state::<AudioSelectionState>();
    let mut selected_meeting_id = state
        .meeting_id
        .lock()
        .map_err(|_| "選択したMeetingの状態を更新できませんでした。".to_string())?;
    if selected_meeting_id.as_deref() == Some(meeting_id) {
        *state
            .selected
            .lock()
            .map_err(|_| "選択した音声の状態を更新できませんでした。".to_string())? = None;
        *selected_meeting_id = None;
    }
    Ok(())
}

pub(crate) fn selected_audio_path_for_playback(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<PathBuf, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let state = app.state::<AudioSelectionState>();
    let selected = state
        .selected
        .lock()
        .map_err(|_| "再生する音声の状態を取得できませんでした。".to_string())?;
    let selected = selected
        .as_ref()
        .ok_or_else(|| "再生する音声が選択されていません。".to_string())?;
    if selected.descriptor.meeting_id != meeting_id {
        return Err("選択中のMeetingと再生対象が一致しません。".into());
    }
    Ok(selected.path.clone())
}

pub(crate) fn selected_audio_for_waveform(
    app: &AppHandle,
    meeting_id: &str,
) -> Result<(PathBuf, u64), String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let state = app.state::<AudioSelectionState>();
    let selected = state
        .selected
        .lock()
        .map_err(|_| "波形を生成する音声の状態を取得できませんでした。".to_string())?;
    let selected = selected
        .as_ref()
        .ok_or_else(|| "波形を生成する音声が選択されていません。".to_string())?;
    if selected.descriptor.meeting_id != meeting_id {
        return Err("選択中のMeetingと波形生成対象が一致しません。".into());
    }
    Ok((selected.path.clone(), selected.descriptor.duration_ms))
}

fn set_selected_audio(
    app: &AppHandle,
    path: PathBuf,
    meeting_id: String,
) -> Result<SelectedAudioFile, String> {
    let descriptor = describe_audio_path(&path, meeting_id)?;
    let state = app.state::<AudioSelectionState>();
    *state
        .meeting_id
        .lock()
        .map_err(|_| "選択したMeetingの状態を更新できませんでした。".to_string())? =
        Some(descriptor.meeting_id.clone());
    *state
        .selected
        .lock()
        .map_err(|_| "選択したファイルの状態を更新できませんでした。".to_string())? =
        Some(SelectedAudio {
            path,
            descriptor: descriptor.clone(),
        });
    Ok(descriptor)
}

#[tauri::command]
pub(crate) fn get_transcription_session(
    state: State<'_, AudioSelectionState>,
) -> Result<TranscriptionSession, String> {
    let selected_audio = {
        let mut selection = state
            .selected
            .lock()
            .map_err(|_| "選択したファイルの状態を取得できませんでした。".to_string())?;
        match selection.as_ref() {
            Some(selected) if selected.path.is_file() => Some(selected.descriptor.clone()),
            Some(_) => {
                *selection = None;
                None
            }
            None => None,
        }
    };
    Ok(TranscriptionSession {
        selected_audio,
        transcribing: state.transcribing.load(Ordering::Acquire),
        progress: state
            .progress
            .lock()
            .map_err(|_| "文字起こし進捗を取得できませんでした。".to_string())?
            .clone(),
    })
}

struct AudioMetadata {
    duration_ms: u64,
}

struct TranscriptionGuard<'a>(&'a AudioSelectionState);

impl Drop for TranscriptionGuard<'_> {
    fn drop(&mut self) {
        self.0.transcribing.store(false, Ordering::Release);
        *self
            .0
            .progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

fn validate_audio_file(path: &Path) -> Result<u64, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "音声ファイルの拡張子を確認できませんでした。".to_string())?;

    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err("MP3、M4A、WAV、FLACのいずれかを選択してください。".to_string());
    }

    let metadata = std::fs::metadata(path).map_err(|error| {
        eprintln!("Could not inspect selected audio file: {error:?}");
        "選択した音声ファイルを読み込めませんでした。".to_string()
    })?;

    if !metadata.is_file() {
        return Err("音声ファイルを選択してください。".to_string());
    }

    if metadata.len() == 0 {
        return Err("選択した音声ファイルが空です。".to_string());
    }

    if metadata.len() >= MAX_AUDIO_FILE_SIZE {
        return Err("音声ファイルは5GB未満にしてください。".to_string());
    }

    Ok(metadata.len())
}

fn inspect_audio(path: &Path) -> Result<AudioMetadata, String> {
    let is_m4a = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("m4a"));
    let mut duration = match Probe::open(path)
        .and_then(|probe| probe.options(ParseOptions::new().read_tags(false)).read())
    {
        Ok(tagged_file) => tagged_file.properties().duration(),
        Err(error) if is_m4a => {
            eprintln!("Could not read audio duration: {error:?}");
            std::time::Duration::ZERO
        }
        Err(error) => {
            eprintln!("Could not read audio duration: {error:?}");
            return Err(
                "音声の再生時間を取得できませんでした。ファイル内容を確認してください。"
                    .to_string(),
            );
        }
    };
    if duration.is_zero() && is_m4a {
        duration = fragmented_m4a_duration(path).unwrap_or_default();
    }

    if duration.is_zero() {
        return Err("音声の再生時間が0秒です。別のファイルを選択してください。".to_string());
    }

    let duration_ms = u64::try_from(duration.as_millis())
        .map_err(|_| "音声の再生時間が長すぎます。".to_string())?;
    Ok(AudioMetadata { duration_ms })
}

#[derive(Default)]
struct FragmentedMp4Timing {
    timescale: u32,
    end_time: u64,
}

pub(crate) fn fragmented_m4a_duration(path: &Path) -> Option<std::time::Duration> {
    const MAX_METADATA_BOX_SIZE: u64 = 16 * 1024 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut timing = FragmentedMp4Timing::default();
    let mut cursor = 0u64;
    while let Some((kind, payload_start, box_end)) = file_mp4_box(&mut file, cursor, length) {
        if matches!(&kind, b"moov" | b"moof") {
            let payload_length = box_end.checked_sub(payload_start)?;
            if payload_length > MAX_METADATA_BOX_SIZE {
                return None;
            }
            file.seek(SeekFrom::Start(payload_start)).ok()?;
            let mut payload = vec![0; usize::try_from(payload_length).ok()?];
            file.read_exact(&mut payload).ok()?;
            if &kind == b"moov" {
                visit_mp4_boxes(&payload, 0, payload.len(), &mut timing);
            } else {
                visit_moof(&payload, 0, payload.len(), &mut timing);
            }
        }
        if box_end <= cursor {
            break;
        }
        cursor = box_end;
    }
    (timing.timescale > 0 && timing.end_time > 0).then(|| {
        std::time::Duration::from_secs_f64(timing.end_time as f64 / timing.timescale as f64)
    })
}

fn file_mp4_box(file: &mut std::fs::File, start: u64, limit: u64) -> Option<([u8; 4], u64, u64)> {
    if start.checked_add(8)? > limit {
        return None;
    }
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header[..8]).ok()?;
    let size32 = u32::from_be_bytes(header[..4].try_into().ok()?) as u64;
    let kind = header[4..8].try_into().ok()?;
    let (size, header_size) = if size32 == 1 {
        file.read_exact(&mut header[8..16]).ok()?;
        (u64::from_be_bytes(header[8..16].try_into().ok()?), 16u64)
    } else if size32 == 0 {
        (limit.checked_sub(start)?, 8u64)
    } else {
        (size32, 8u64)
    };
    if size < header_size {
        return None;
    }
    let box_end = start.checked_add(size)?;
    (box_end <= limit).then_some((kind, start + header_size, box_end))
}

fn visit_mp4_boxes(data: &[u8], start: usize, end: usize, timing: &mut FragmentedMp4Timing) {
    let mut cursor = start;
    while let Some((kind, payload_start, box_end)) = mp4_box(data, cursor, end) {
        match &kind {
            b"moov" | b"trak" | b"mdia" => visit_mp4_boxes(data, payload_start, box_end, timing),
            b"mdhd" => {
                if let Some((timescale, duration)) =
                    parse_mdhd_timing(&data[payload_start..box_end])
                {
                    timing.timescale = timescale;
                    timing.end_time = timing.end_time.max(duration);
                }
            }
            b"moof" => visit_moof(data, payload_start, box_end, timing),
            _ => {}
        }
        if box_end <= cursor {
            break;
        }
        cursor = box_end;
    }
}

fn visit_moof(data: &[u8], start: usize, end: usize, timing: &mut FragmentedMp4Timing) {
    let mut cursor = start;
    while let Some((kind, payload_start, box_end)) = mp4_box(data, cursor, end) {
        if &kind == b"traf" {
            if let Some((base_time, duration)) = parse_traf(data, payload_start, box_end) {
                timing.end_time = if let Some(base_time) = base_time {
                    timing.end_time.max(base_time.saturating_add(duration))
                } else {
                    timing.end_time.saturating_add(duration)
                };
            }
        }
        if box_end <= cursor {
            break;
        }
        cursor = box_end;
    }
}

fn parse_traf(data: &[u8], start: usize, end: usize) -> Option<(Option<u64>, u64)> {
    let mut default_duration = None;
    let mut base_time = None;
    let mut runs = Vec::new();
    let mut cursor = start;
    while let Some((kind, payload_start, box_end)) = mp4_box(data, cursor, end) {
        let payload = &data[payload_start..box_end];
        match &kind {
            b"tfhd" => default_duration = parse_tfhd_default_duration(payload),
            b"tfdt" => base_time = parse_tfdt_base_time(payload),
            b"trun" => runs.push(payload),
            _ => {}
        }
        if box_end <= cursor {
            break;
        }
        cursor = box_end;
    }
    let mut duration = 0u64;
    for run in runs {
        duration = duration.checked_add(parse_trun_duration(run, default_duration)?)?;
    }
    Some((base_time, duration))
}

fn mp4_box(data: &[u8], start: usize, limit: usize) -> Option<([u8; 4], usize, usize)> {
    if start.checked_add(8)? > limit || limit > data.len() {
        return None;
    }
    let size32 = read_u32(data, start)? as u64;
    let kind = data.get(start + 4..start + 8)?.try_into().ok()?;
    let (size, header) = if size32 == 1 {
        (read_u64(data, start + 8)?, 16usize)
    } else if size32 == 0 {
        ((limit - start) as u64, 8usize)
    } else {
        (size32, 8usize)
    };
    if size < header as u64 {
        return None;
    }
    let box_end = start.checked_add(usize::try_from(size).ok()?)?;
    (box_end <= limit).then_some((kind, start + header, box_end))
}

fn parse_mdhd_timing(payload: &[u8]) -> Option<(u32, u64)> {
    match *payload.first()? {
        0 => Some((read_u32(payload, 12)?, u64::from(read_u32(payload, 16)?))),
        1 => Some((read_u32(payload, 20)?, read_u64(payload, 24)?)),
        _ => None,
    }
}

fn parse_tfhd_default_duration(payload: &[u8]) -> Option<u32> {
    let flags = read_u32(payload, 0)? & 0x00ff_ffff;
    let mut cursor = 8usize;
    if flags & 0x000001 != 0 {
        cursor = cursor.checked_add(8)?;
    }
    if flags & 0x000002 != 0 {
        cursor = cursor.checked_add(4)?;
    }
    (flags & 0x000008 != 0)
        .then(|| read_u32(payload, cursor))
        .flatten()
}

fn parse_tfdt_base_time(payload: &[u8]) -> Option<u64> {
    match *payload.first()? {
        0 => read_u32(payload, 4).map(u64::from),
        1 => read_u64(payload, 4),
        _ => None,
    }
}

fn parse_trun_duration(payload: &[u8], default_duration: Option<u32>) -> Option<u64> {
    let flags = read_u32(payload, 0)? & 0x00ff_ffff;
    let sample_count = read_u32(payload, 4)? as usize;
    let mut cursor = 8usize;
    if flags & 0x000001 != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if flags & 0x000004 != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if flags & 0x000100 == 0 {
        return Some(u64::from(default_duration?) * sample_count as u64);
    }
    let mut duration = 0u64;
    for _ in 0..sample_count {
        duration = duration.checked_add(u64::from(read_u32(payload, cursor)?))?;
        cursor = cursor.checked_add(4)?;
        if flags & 0x000200 != 0 {
            cursor = cursor.checked_add(4)?;
        }
        if flags & 0x000400 != 0 {
            cursor = cursor.checked_add(4)?;
        }
        if flags & 0x000800 != 0 {
            cursor = cursor.checked_add(4)?;
        }
    }
    Some(duration)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_be_bytes(
        data.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}

#[tauri::command]
pub(crate) async fn select_audio_file(app: AppHandle) -> Result<Option<SelectedAudioFile>, String> {
    // Android needs the activity event loop to remain available while the system
    // picker is open. `blocking_pick_file` keeps the command alive but can block
    // that loop, so a cancelled picker never resolves the webview invocation.
    // Use the callback API instead; it reports cancellation as `None`.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.dialog()
        .file()
        .add_filter("Audio", AUDIO_EXTENSIONS)
        .pick_file(move |selected| {
            let _ = sender.send(selected);
        });

    let selected = tauri::async_runtime::spawn_blocking(move || receiver.recv().ok().flatten())
        .await
        .map_err(|error| format!("音声ファイルの選択結果を取得できませんでした: {error}"))?;

    let Some(selected) = selected else {
        return Ok(None);
    };

    let path = selected_file_path(selected)?;
    let selected =
        tauri::async_runtime::spawn_blocking(move || set_selected_audio_path(&app, path))
            .await
            .map_err(|error| {
                format!("音声ファイルのMeeting情報を準備できませんでした: {error}")
            })??;
    Ok(Some(selected))
}

fn selected_file_path(selected: FilePath) -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    if let FilePath::Url(url) = &selected {
        if url.scheme() == "content" {
            return crate::recording::android::copy_content_uri(url.as_str());
        }
    }
    selected
        .into_path()
        .map_err(|_| "選択したファイルのパスを取得できませんでした。".to_string())
}

#[tauri::command]
pub(crate) async fn transcribe_selected_audio(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
    request: TranscriptionRequest,
) -> Result<TranscriptionResult, String> {
    if state
        .transcribing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("文字起こしはすでに実行中です。".to_string());
    }
    let _guard = TranscriptionGuard(&state);
    publish_transcription_progress(
        &app,
        TranscriptionProgress::new(TranscriptionStage::Preparing, 0, None),
    );

    let selected = state
        .selected
        .lock()
        .map_err(|_| "選択したファイルの状態を取得できませんでした。".to_string())?
        .clone()
        .ok_or_else(|| "先に音声ファイルを選択してください。".to_string())?;

    // Selection metadata is cached for lightweight session polling, so validate
    // the actual file once more at the boundary where it is consumed.
    validate_audio_path(&selected.path)?;

    if matches!(
        request.provider,
        TranscriptionProvider::ElevenLabs | TranscriptionProvider::Soniox
    ) {
        publish_transcription_progress(
            &app,
            TranscriptionProgress::new(TranscriptionStage::Transcribing, 0, None),
        );
    }
    let outcome = crate::transcription::transcribe(
        &app,
        &selected.path,
        request.provider,
        request.model_id.as_deref(),
    )
    .await?;
    let (run, persistence_warning) = match crate::transcript_store::create_run(
        &app,
        &selected.descriptor.meeting_id,
        &selected.path,
        &outcome.transcript,
        outcome.cost_usd,
    ) {
        Ok(run) => (
            Some(run),
            crate::meeting_store::mark_updated(&app, &selected.descriptor.meeting_id).err(),
        ),
        Err(error) => (None, Some(error)),
    };
    Ok(TranscriptionResult {
        transcript: outcome.transcript,
        run,
        persistence_warning,
    })
}

#[tauri::command]
pub(crate) fn get_selected_transcription_history(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
) -> Result<crate::transcript_store::TranscriptionHistory, String> {
    let meeting_id = selected_meeting_id_from_state(&state)?;
    let audio_path = state
        .selected
        .lock()
        .map_err(|_| "選択したファイルの状態を取得できませんでした。".to_string())?
        .as_ref()
        .filter(|selected| selected.descriptor.meeting_id == meeting_id)
        .map(|selected| selected.path.clone());
    crate::transcript_store::history(&app, &meeting_id, audio_path.as_deref())
}

#[tauri::command]
pub(crate) fn get_selected_transcription_run(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
) -> Result<Option<crate::transcript_store::TranscriptionRunDetail>, String> {
    let meeting_id = selected_meeting_id_from_state(&state)?;
    crate::transcript_store::selected_run(&app, &meeting_id)
}

#[tauri::command]
pub(crate) fn select_transcription_run(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
    request: SelectTranscriptionRunRequest,
) -> Result<crate::transcript_store::TranscriptionRunDetail, String> {
    let meeting_id = selected_meeting_id_from_state(&state)?;
    crate::transcript_store::select_run(&app, &meeting_id, &request.transcription_id)
}

#[tauri::command]
pub(crate) fn update_transcript_document(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
    request: UpdateTranscriptDocumentRequest,
) -> Result<crate::transcript_store::TranscriptionRunDetail, String> {
    let meeting_id = selected_meeting_id_from_state(&state)?;
    crate::transcript_store::update_run_segments(
        &app,
        &meeting_id,
        &request.transcription_id,
        request.expected_revision,
        request.changes,
        request.speaker_labels,
    )
}

#[tauri::command]
pub(crate) fn reset_transcript_document(
    app: AppHandle,
    state: State<'_, AudioSelectionState>,
    request: ResetTranscriptDocumentRequest,
) -> Result<crate::transcript_store::TranscriptionRunDetail, String> {
    let meeting_id = selected_meeting_id_from_state(&state)?;
    crate::transcript_store::reset_run_document(
        &app,
        &meeting_id,
        &request.transcription_id,
        request.expected_revision,
    )
}

fn selected_meeting_id_from_state(
    state: &State<'_, AudioSelectionState>,
) -> Result<String, String> {
    state
        .meeting_id
        .lock()
        .map_err(|_| "選択したMeetingの状態を取得できませんでした。".to_string())?
        .clone()
        .ok_or_else(|| "先にMeetingを選択してください。".to_string())
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write, path::Path};

    use super::{inspect_audio, parse_mdhd_timing, validate_audio_file, TranscriptionRequest};

    fn write_one_second_wav(path: &Path) {
        const SAMPLE_RATE: u32 = 8_000;
        const CHANNELS: u16 = 1;
        const BITS_PER_SAMPLE: u16 = 16;
        let data_size = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8);
        let mut file = File::create(path).expect("create WAV fixture");

        file.write_all(b"RIFF").expect("write RIFF");
        file.write_all(&(36 + data_size).to_le_bytes())
            .expect("write RIFF size");
        file.write_all(b"WAVEfmt ").expect("write WAVE fmt");
        file.write_all(&16_u32.to_le_bytes())
            .expect("write fmt size");
        file.write_all(&1_u16.to_le_bytes())
            .expect("write PCM format");
        file.write_all(&CHANNELS.to_le_bytes())
            .expect("write channels");
        file.write_all(&SAMPLE_RATE.to_le_bytes())
            .expect("write sample rate");
        file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())
            .expect("write byte rate");
        file.write_all(&2_u16.to_le_bytes())
            .expect("write block align");
        file.write_all(&BITS_PER_SAMPLE.to_le_bytes())
            .expect("write bits per sample");
        file.write_all(b"data").expect("write data marker");
        file.write_all(&data_size.to_le_bytes())
            .expect("write data size");
        file.write_all(&vec![0_u8; data_size as usize])
            .expect("write PCM samples");
    }

    #[test]
    fn rejects_unsupported_file_extension() {
        let path = std::env::temp_dir().join("mutsuna-echo-unsupported.txt");
        let mut file = File::create(&path).expect("create fixture");
        file.write_all(b"not audio").expect("write fixture");

        let result = validate_audio_file(&path);
        let _ = std::fs::remove_file(path);

        assert_eq!(
            result.expect_err("unsupported extension should fail"),
            "MP3、M4A、WAV、FLACのいずれかを選択してください。"
        );
    }

    #[test]
    fn reads_local_audio_duration() {
        let path =
            std::env::temp_dir().join(format!("mutsuna-echo-duration-{}.wav", std::process::id()));
        write_one_second_wav(&path);

        let audio = inspect_audio(&path).expect("inspect WAV audio");
        let _ = std::fs::remove_file(path);

        assert_eq!(audio.duration_ms, 1_000);
    }

    #[test]
    fn reads_duration_from_version_zero_mdhd() {
        let mut payload = vec![0_u8; 20];
        payload[12..16].copy_from_slice(&48_000_u32.to_be_bytes());
        payload[16..20].copy_from_slice(&2_116_608_u32.to_be_bytes());

        assert_eq!(parse_mdhd_timing(&payload), Some((48_000, 2_116_608)));
    }

    #[test]
    fn transcription_request_rejects_unknown_providers() {
        assert!(
            serde_json::from_value::<TranscriptionRequest>(serde_json::json!({
                "provider": "elevenlabs"
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<TranscriptionRequest>(serde_json::json!({
                "provider": "local"
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<TranscriptionRequest>(serde_json::json!({
                "provider": "soniox"
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<TranscriptionRequest>(serde_json::json!({
                "provider": "unknown"
            }))
            .is_err()
        );
    }
}
