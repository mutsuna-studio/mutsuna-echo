#[cfg(any(target_os = "android", test))]
#[allow(dead_code)]
pub mod android;
mod manifest;
mod platform;
pub mod types;

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::{Local, SecondsFormat, Utc};
use tauri::{AppHandle, Manager};

use manifest::{remove_session, RecordingManifest};
use platform::{mix_with_limiter, M4aWriter};
use types::{
    AudioDevice, RecordingCapabilities, RecordingPhase, RecordingStatus, RecoverableRecording,
    StartRecordingRequest, StopReason, CHANNELS, FINAL_BITRATE, MAX_DURATION_MS, SAMPLE_RATE,
    SOURCE_BITRATE,
};

const MANIFEST_INTERVAL: Duration = Duration::from_secs(2);
const MAC_FRAGMENT_SECONDS: f64 = 10.0;
const WINDOWS_FRAGMENT_SECONDS: f64 = 2.0;
const CHUNK_NS: i64 = 20_000_000;

struct ActiveRecording {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

#[derive(Default)]
pub struct RecordingService {
    status: Arc<Mutex<RecordingStatus>>,
    active: Mutex<Option<ActiveRecording>>,
}

impl RecordingService {
    pub fn status(&self) -> RecordingStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn start(
        &self,
        app: AppHandle,
        request: StartRecordingRequest,
    ) -> Result<RecordingStatus, String> {
        request.validate()?;
        self.reap_finished();

        let mut active = self
            .active
            .lock()
            .map_err(|_| "録音状態を開始できませんでした。".to_string())?;
        if active.is_some() {
            return Err("録音はすでに実行中です。".into());
        }

        let paths = RecordingPaths::create(&app)?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        set_status(&status, |current| {
            *current = RecordingStatus {
                phase: RecordingPhase::Starting,
                session_id: Some(paths.session_id.clone()),
                microphone: request.microphone,
                system_audio: request.system_audio,
                ..RecordingStatus::default()
            };
        });

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_stop = stop.clone();
        let worker_cancel = cancel.clone();
        let worker = thread::Builder::new()
            .name("mutsuna-recording".into())
            .spawn(move || {
                run_recording(
                    app,
                    request,
                    paths,
                    status,
                    worker_stop,
                    worker_cancel,
                    ready_tx,
                )
            })
            .map_err(|error| format!("録音処理を開始できませんでした: {error}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {
                *active = Some(ActiveRecording {
                    stop,
                    cancel,
                    worker,
                });
                Ok(self.status())
            }
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(
                    "録音デバイスの開始がタイムアウトしました。OSの音声権限を確認してください。"
                        .into(),
                )
            }
        }
    }

    pub fn request_stop(&self, cancel_recording: bool) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "録音状態を停止できませんでした。".to_string())?;
        let recording = active
            .as_ref()
            .ok_or_else(|| "現在、録音していません。".to_string())?;
        if cancel_recording {
            recording.cancel.store(true, Ordering::Release);
        }
        recording.stop.store(true, Ordering::Release);
        Ok(())
    }

    pub fn wait_for_stop(&self) -> Result<RecordingStatus, String> {
        let recording = self
            .active
            .lock()
            .map_err(|_| "録音状態を確定できませんでした。".to_string())?
            .take();
        if let Some(recording) = recording {
            recording
                .worker
                .join()
                .map_err(|_| "録音処理が予期せず終了しました。".to_string())?;
        }
        Ok(self.status())
    }

    fn reap_finished(&self) {
        let finished = self
            .active
            .lock()
            .map(|active| {
                active
                    .as_ref()
                    .is_some_and(|recording| recording.worker.is_finished())
            })
            .unwrap_or(false);
        if finished {
            let _ = self.wait_for_stop();
        }
    }
}

struct RecordingPaths {
    session_id: String,
    directory: PathBuf,
    microphone: PathBuf,
    system: PathBuf,
    mixed: PathBuf,
    final_file: PathBuf,
}

impl RecordingPaths {
    fn create(app: &AppHandle) -> Result<Self, String> {
        let now = Local::now();
        let base_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let session_id = format!("{}-{}", now.format("%Y%m%d%H%M%S%3f"), std::process::id());
        let directory = recordings_root(app)?.join("in-progress").join(&session_id);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("録音用の一時フォルダーを作成できませんでした: {error}"))?;

        let output_directory = app
            .path()
            .audio_dir()
            .map_err(|error| format!("ミュージックフォルダーを取得できませんでした: {error}"))?
            .join("Mutsuna Echo");
        fs::create_dir_all(&output_directory)
            .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;
        let final_file = unique_output_path(&output_directory, &base_name);

        Ok(Self {
            session_id,
            microphone: directory.join("microphone.partial.m4a"),
            system: directory.join("system.partial.m4a"),
            mixed: directory.join("meeting.partial.m4a"),
            directory,
            final_file,
        })
    }
}

fn unique_output_path(directory: &Path, base_name: &str) -> PathBuf {
    let initial = directory.join(format!("{base_name}.m4a"));
    if !initial.exists() {
        return initial;
    }
    (2..=999)
        .map(|suffix| directory.join(format!("{base_name}_{suffix}.m4a")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| directory.join(format!("{base_name}_{}.m4a", std::process::id())))
}

fn recordings_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("アプリデータフォルダーを取得できませんでした: {error}"))?
        .join("recordings"))
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_recording(
    app: AppHandle,
    request: StartRecordingRequest,
    paths: RecordingPaths,
    status: Arc<Mutex<RecordingStatus>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    if let Err(error) =
        run_desktop_recording(&app, &request, &paths, &status, &stop, &cancel, &ready)
    {
        set_status(&status, |current| {
            current.phase = RecordingPhase::Failed;
            current.error = Some(error.clone());
            current.stop_reason = Some(StopReason::CaptureError);
        });
        let _ = ready.try_send(Err(error));
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn run_recording(
    _app: AppHandle,
    _request: StartRecordingRequest,
    _paths: RecordingPaths,
    status: Arc<Mutex<RecordingStatus>>,
    _stop: Arc<AtomicBool>,
    _cancel: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let error = "このOSでは現在、アプリ内録音を利用できません。".to_string();
    set_status(&status, |current| {
        current.phase = RecordingPhase::Failed;
        current.error = Some(error.clone());
    });
    let _ = ready.send(Err(error));
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_desktop_recording(
    app: &AppHandle,
    request: &StartRecordingRequest,
    paths: &RecordingPaths,
    status: &Arc<Mutex<RecordingStatus>>,
    stop: &Arc<AtomicBool>,
    cancel: &Arc<AtomicBool>,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    use flexaudio::{open, Event, OutputFormat, SourceKind, Stream, StreamConfig};

    let output = OutputFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
    };
    let make_stream = |kind, device_id| -> Result<Stream, String> {
        let mut stream = open(StreamConfig {
            kind,
            device_id,
            output,
            ring_capacity_chunks: 250,
            ..StreamConfig::default()
        })
        .map_err(|error| capture_start_error(kind, &error.to_string()))?;
        stream
            .start()
            .map_err(|error| capture_start_error(kind, &error.to_string()))?;
        Ok(stream)
    };

    let mut microphone = if request.microphone {
        Some(make_stream(
            SourceKind::Mic,
            request.microphone_device_id.clone(),
        )?)
    } else {
        None
    };
    let mut system = if request.system_audio {
        Some(make_stream(
            SourceKind::SystemLoopback,
            request.system_device_id.clone(),
        )?)
    } else {
        None
    };

    let fragment_seconds = if cfg!(target_os = "macos") {
        MAC_FRAGMENT_SECONDS
    } else {
        WINDOWS_FRAGMENT_SECONDS
    };
    let mut microphone_writer = if request.microphone {
        Some(M4aWriter::create(
            &paths.microphone,
            SOURCE_BITRATE,
            fragment_seconds,
        )?)
    } else {
        None
    };
    let mut system_writer = if request.system_audio {
        Some(M4aWriter::create(
            &paths.system,
            SOURCE_BITRATE,
            fragment_seconds,
        )?)
    } else {
        None
    };
    let mut mixed_writer = M4aWriter::create(&paths.mixed, FINAL_BITRATE, fragment_seconds)?;

    let started_at = Utc::now();
    let started = Instant::now();
    let mut manifest = RecordingManifest {
        version: 1,
        session_id: paths.session_id.clone(),
        started_at: started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        updated_at: started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        duration_ms: 0,
        microphone: request.microphone,
        system_audio: request.system_audio,
        microphone_file: request.microphone.then(|| paths.microphone.clone()),
        system_file: request.system_audio.then(|| paths.system.clone()),
        mixed_file: paths.mixed.clone(),
        final_file: paths.final_file.clone(),
        finalized: false,
        stop_reason: None,
    };
    manifest.save(&paths.directory)?;

    set_status(status, |current| current.phase = RecordingPhase::Recording);
    let _ = ready.send(Ok(()));
    let mut last_manifest = Instant::now();
    let mut mic_queue = VecDeque::new();
    let mut sys_queue = VecDeque::new();
    let mut stop_reason = StopReason::User;

    while !stop.load(Ordering::Acquire) {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if elapsed_ms >= MAX_DURATION_MS {
            stop_reason = StopReason::DurationLimit;
            break;
        }

        if let Some(stream) = microphone.as_mut() {
            while let Some(event) = stream.poll_event() {
                if matches!(event, Event::DeviceLost) {
                    stop_reason = StopReason::SourceDisconnected;
                    stop.store(true, Ordering::Release);
                } else if matches!(event, Event::StreamStalled) {
                    stop_reason = StopReason::SourceStalled;
                    stop.store(true, Ordering::Release);
                } else if let Event::Error(_) | Event::PermissionDenied = event {
                    stop_reason = StopReason::CaptureError;
                    stop.store(true, Ordering::Release);
                }
            }
            while let Some(chunk) = stream.poll_chunk() {
                if let Some(writer) = microphone_writer.as_mut() {
                    writer.write(&chunk.data)?;
                }
                set_status(status, |current| {
                    current.microphone_level = chunk.peak.clamp(0.0, 1.0)
                });
                mic_queue.push_back((chunk.pts_ns, chunk.data));
            }
        }

        if let Some(stream) = system.as_mut() {
            while let Some(event) = stream.poll_event() {
                if matches!(event, Event::DeviceLost) {
                    stop_reason = StopReason::SourceDisconnected;
                    stop.store(true, Ordering::Release);
                } else if matches!(event, Event::StreamStalled) {
                    stop_reason = StopReason::SourceStalled;
                    stop.store(true, Ordering::Release);
                } else if let Event::Error(_) | Event::PermissionDenied = event {
                    stop_reason = StopReason::CaptureError;
                    stop.store(true, Ordering::Release);
                }
            }
            while let Some(chunk) = stream.poll_chunk() {
                if let Some(writer) = system_writer.as_mut() {
                    writer.write(&chunk.data)?;
                }
                set_status(status, |current| {
                    current.system_level = chunk.peak.clamp(0.0, 1.0)
                });
                sys_queue.push_back((chunk.pts_ns, chunk.data));
            }
        }

        drain_mix(
            &mut mixed_writer,
            &mut mic_queue,
            &mut sys_queue,
            request,
            false,
        )?;
        set_status(status, |current| current.elapsed_ms = elapsed_ms);

        if last_manifest.elapsed() >= MANIFEST_INTERVAL {
            manifest.duration_ms = elapsed_ms;
            manifest.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            manifest.save(&paths.directory)?;
            last_manifest = Instant::now();
        }
        thread::sleep(Duration::from_millis(5));
    }

    if let Some(stream) = microphone.as_mut() {
        stream.stop();
    }
    if let Some(stream) = system.as_mut() {
        stream.stop();
    }
    drain_mix(
        &mut mixed_writer,
        &mut mic_queue,
        &mut sys_queue,
        request,
        true,
    )?;

    if cancel.load(Ordering::Acquire) {
        drop(microphone_writer);
        drop(system_writer);
        drop(mixed_writer);
        remove_session(&paths.directory)?;
        set_status(status, |current| *current = RecordingStatus::default());
        return Ok(());
    }

    set_status(status, |current| {
        current.phase = RecordingPhase::Finalizing;
        current.stop_reason = Some(stop_reason);
    });
    if let Some(writer) = microphone_writer {
        writer.finish()?;
    }
    if let Some(writer) = system_writer {
        writer.finish()?;
    }
    mixed_writer.finish()?;

    atomic_copy_to_output(&paths.mixed, &paths.final_file)?;
    manifest.duration_ms = started.elapsed().as_millis() as u64;
    manifest.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    manifest.finalized = true;
    manifest.stop_reason = Some(stop_reason);
    manifest.save(&paths.directory)?;

    crate::commands::transcribe::set_selected_audio_path(app, paths.final_file.clone())?;
    remove_session(&paths.directory)?;
    set_status(status, |current| {
        current.phase = RecordingPhase::Completed;
        current.elapsed_ms = manifest.duration_ms;
        current.output_path = Some(paths.final_file.to_string_lossy().into_owned());
        current.stop_reason = Some(stop_reason);
        current.microphone_level = 0.0;
        current.system_level = 0.0;
    });
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn drain_mix(
    writer: &mut M4aWriter,
    microphone: &mut VecDeque<(i64, Vec<f32>)>,
    system: &mut VecDeque<(i64, Vec<f32>)>,
    request: &StartRecordingRequest,
    flush: bool,
) -> Result<(), String> {
    loop {
        if !request.microphone {
            let Some((_, samples)) = system.pop_front() else {
                break;
            };
            writer.write(&samples)?;
            continue;
        }
        if !request.system_audio {
            let Some((_, samples)) = microphone.pop_front() else {
                break;
            };
            writer.write(&samples)?;
            continue;
        }

        match (microphone.front(), system.front()) {
            (Some((mic_pts, _)), Some((sys_pts, _))) if (mic_pts - sys_pts).abs() < CHUNK_NS => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&mix_with_limiter(&mic, &sys))?;
            }
            (Some((mic_pts, _)), Some((sys_pts, _))) if mic_pts < sys_pts => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                writer.write(&mic)?;
            }
            (Some(_), Some(_)) => {
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&sys)?;
            }
            (Some(_), None) if flush || microphone.len() > 3 => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                writer.write(&mic)?;
            }
            (None, Some(_)) if flush || system.len() > 3 => {
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&sys)?;
            }
            _ => break,
        }
    }
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn capture_start_error(kind: flexaudio::SourceKind, detail: &str) -> String {
    let source = match kind {
        flexaudio::SourceKind::Mic => "マイク",
        _ => "システム音声",
    };
    if detail.contains("permission") {
        format!("{source}の録音権限がありません。OSのプライバシー設定でMutsuna Echoを許可してください。")
    } else if detail.contains("device not found") {
        format!("選択した{source}デバイスが見つかりません。接続を確認して選び直してください。")
    } else {
        format!("{source}を開始できませんでした: {detail}")
    }
}

fn set_status(status: &Arc<Mutex<RecordingStatus>>, update: impl FnOnce(&mut RecordingStatus)) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut current);
}

pub fn capabilities() -> Result<RecordingCapabilities, String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let devices = flexaudio::devices()
            .map_err(|error| format!("音声デバイスを取得できませんでした: {error}"))?;
        let mut microphones = Vec::new();
        let mut systems = Vec::new();
        for device in devices {
            let item = AudioDevice {
                id: device.id,
                name: device.name,
                is_default: device.is_default,
            };
            if device.is_loopback {
                systems.push(item);
            } else {
                microphones.push(item);
            }
        }
        Ok(RecordingCapabilities {
            platform: if cfg!(target_os = "windows") {
                "windows"
            } else {
                "macos"
            },
            supported: true,
            microphone_supported: true,
            system_audio_supported: true,
            system_audio_limited: false,
            limitation: None,
            microphone_devices: microphones,
            system_devices: systems,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            codec: "AAC-LC",
            bitrate: FINAL_BITRATE,
            max_duration_ms: MAX_DURATION_MS,
        })
    }
    #[cfg(all(
        not(any(target_os = "windows", target_os = "macos")),
        not(target_os = "android")
    ))]
    {
        Ok(RecordingCapabilities {
            platform: if cfg!(target_os = "android") {
                "android"
            } else {
                "unsupported"
            },
            supported: false,
            microphone_supported: false,
            system_audio_supported: false,
            system_audio_limited: cfg!(target_os = "android"),
            limitation: Some("このビルドではアプリ内録音を利用できません。"),
            microphone_devices: Vec::new(),
            system_devices: Vec::new(),
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            codec: "AAC-LC",
            bitrate: FINAL_BITRATE,
            max_duration_ms: MAX_DURATION_MS,
        })
    }
    #[cfg(target_os = "android")]
    {
        android::capabilities()
    }
}

pub fn recoverable_recordings(app: &AppHandle) -> Result<Vec<RecoverableRecording>, String> {
    let root = recordings_root(app)?.join("in-progress");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut recordings = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("復旧可能な録音を確認できませんでした: {error}"))?
    {
        let directory = match entry {
            Ok(entry) if entry.path().is_dir() => entry.path(),
            _ => continue,
        };
        let manifest = match RecordingManifest::load(&directory) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !manifest.finalized && manifest.mixed_file.exists() {
            recordings.push(RecoverableRecording {
                session_id: manifest.session_id,
                started_at: manifest.started_at,
                duration_ms: manifest.duration_ms,
                microphone: manifest.microphone,
                system_audio: manifest.system_audio,
            });
        }
    }
    recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(recordings)
}

pub fn recover(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    validate_session_id(session_id)?;
    let directory = recordings_root(app)?.join("in-progress").join(session_id);
    let manifest = RecordingManifest::load(&directory)?;
    if !manifest.mixed_file.exists() {
        return Err("復旧できる音声フラグメントが見つかりません。".into());
    }
    crate::commands::transcribe::describe_audio_path(&manifest.mixed_file)
        .map_err(|error| format!("録音フラグメントを再生可能なM4Aとして復旧できませんでした。元データは破棄していません: {error}"))?;
    atomic_copy_to_output(&manifest.mixed_file, &manifest.final_file)?;
    crate::commands::transcribe::set_selected_audio_path(app, manifest.final_file.clone())?;
    remove_session(&directory)?;
    Ok(manifest.final_file)
}

fn atomic_copy_to_output(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "録音の保存先が不正です。".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("録音の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(
        ".{}.partial",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    fs::copy(source, &temporary)
        .map_err(|error| format!("録音ファイルを保存先へコピーできませんでした: {error}"))?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("録音ファイルを安全に書き込めませんでした: {error}"))?;
    fs::rename(&temporary, destination)
        .map_err(|error| format!("録音ファイルを保存先へ確定できませんでした: {error}"))
}

pub fn discard(app: &AppHandle, session_id: &str) -> Result<(), String> {
    validate_session_id(session_id)?;
    remove_session(&recordings_root(app)?.join("in-progress").join(session_id))
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("録音セッションIDが不正です。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{unique_output_path, validate_session_id};

    #[test]
    fn session_id_cannot_escape_recording_root() {
        assert!(validate_session_id("../../secret").is_err());
        assert!(validate_session_id("20260808-42").is_ok());
    }

    #[test]
    fn output_path_uses_m4a_extension() {
        let path = unique_output_path(&std::env::temp_dir(), "2026-08-08_12-00-00");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("m4a")
        );
    }
}
