use std::sync::{atomic::AtomicBool, mpsc, Arc, Mutex};

use tauri::AppHandle;

use super::{
    publish_status,
    session::RecordingPaths,
    types::{RecordingPhase, RecordingStatus, StartRecordingRequest},
};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::{
    collections::VecDeque,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use chrono::{SecondsFormat, Utc};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use super::{
    manifest::{remove_session, RecordingManifest},
    mixer::drain_mix,
    platform::M4aWriter,
    session::atomic_copy_to_output,
    types::{
        StopReason, VoiceActivityState, CHANNELS, FINAL_BITRATE, MAX_DURATION_MS, SAMPLE_RATE,
        SOURCE_BITRATE,
    },
};

#[cfg(any(target_os = "windows", target_os = "macos"))]
const MANIFEST_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(any(target_os = "windows", target_os = "macos"))]
const STATUS_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(any(target_os = "windows", target_os = "macos"))]
const MAC_FRAGMENT_SECONDS: f64 = 10.0;
#[cfg(any(target_os = "windows", target_os = "macos"))]
const WINDOWS_FRAGMENT_SECONDS: f64 = 2.0;

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn accumulate_meter_samples(level: &mut f32, samples: &[f32]) {
    let peak = samples.iter().copied().fold(0.0f32, |peak, sample| {
        if sample.is_finite() {
            peak.max(sample.abs())
        } else {
            peak
        }
    });
    if peak > *level {
        *level = peak.min(1.0);
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn take_meter_levels(microphone: &mut f32, system: &mut f32) -> (f32, f32) {
    (std::mem::take(microphone), std::mem::take(system))
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
enum CaptureEventEffect {
    None,
    Stalled,
    Recovered,
    Stop(StopReason),
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(super) fn run_recording(
    app: AppHandle,
    request: StartRecordingRequest,
    paths: RecordingPaths,
    status: Arc<Mutex<RecordingStatus>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let _power_guard = match crate::processing_power::acquire(&app, "録音中") {
        Ok(guard) => guard,
        Err(error) => {
            publish_status(&app, &status, |current| {
                current.phase = RecordingPhase::Failed;
                current.error = Some(error.clone());
                current.stop_reason = Some(StopReason::CaptureError);
            });
            let _ = ready.try_send(Err(error));
            return;
        }
    };
    if let Err(error) =
        run_desktop_recording(&app, &request, &paths, &status, &stop, &cancel, &ready)
    {
        publish_status(&app, &status, |current| {
            current.phase = RecordingPhase::Failed;
            current.error = Some(error.clone());
            current.stop_reason = Some(StopReason::CaptureError);
        });
        let _ = ready.try_send(Err(error));
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(super) fn run_input_monitor(
    app: AppHandle,
    request: StartRecordingRequest,
    status: Arc<Mutex<RecordingStatus>>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    use flexaudio::{open, OutputFormat, SourceKind, Stream, StreamConfig};

    let output = OutputFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
    };
    let make_stream = |kind, device_id| -> Result<Stream, String> {
        let mut stream = open(StreamConfig {
            kind,
            device_id,
            output,
            ring_capacity_chunks: 16,
            ..StreamConfig::default()
        })
        .map_err(|error| capture_start_error(kind, &error.to_string()))?;
        stream
            .start()
            .map_err(|error| capture_start_error(kind, &error.to_string()))?;
        Ok(stream)
    };

    let result = (|| -> Result<(), String> {
        let mut microphone = request
            .microphone
            .then(|| make_stream(SourceKind::Mic, request.microphone_device_id.clone()))
            .transpose()?;
        let mut system = request
            .system_audio
            .then(|| make_stream(SourceKind::SystemLoopback, request.system_device_id.clone()))
            .transpose()?;
        let _ = ready.send(Ok(()));
        let mut last_status = Instant::now() - STATUS_INTERVAL;
        let mut microphone_level = 0.0;
        let mut system_level = 0.0;
        while !stop.load(Ordering::Acquire) {
            if let Some(stream) = microphone.as_mut() {
                while let Some(chunk) = stream.poll_chunk() {
                    accumulate_meter_samples(&mut microphone_level, &chunk.data);
                }
            }
            if let Some(stream) = system.as_mut() {
                while let Some(chunk) = stream.poll_chunk() {
                    accumulate_meter_samples(&mut system_level, &chunk.data);
                }
            }
            if last_status.elapsed() >= STATUS_INTERVAL {
                let (published_microphone_level, published_system_level) =
                    take_meter_levels(&mut microphone_level, &mut system_level);
                publish_status(&app, &status, |current| {
                    current.microphone_level = published_microphone_level;
                    current.system_level = published_system_level;
                });
                last_status = Instant::now();
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = ready.try_send(Err(error.clone()));
        publish_status(&app, &status, |current| current.warning = Some(error));
    }
    publish_status(&app, &status, |current| {
        current.microphone_level = 0.0;
        current.system_level = 0.0;
    });
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn run_input_monitor(
    _app: AppHandle,
    _request: StartRecordingRequest,
    _status: Arc<Mutex<RecordingStatus>>,
    _stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let _ = ready.send(Err("このOSでは録音前の入力確認を利用できません。".into()));
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn run_recording(
    app: AppHandle,
    _request: StartRecordingRequest,
    _paths: RecordingPaths,
    status: Arc<Mutex<RecordingStatus>>,
    _stop: Arc<AtomicBool>,
    _cancel: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let error = "このOSでは現在、アプリ内録音を利用できません。".to_string();
    publish_status(&app, &status, |current| {
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
    use flexaudio::{open, OutputFormat, SourceKind, Stream, StreamConfig};

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
    // Enhance only the microphone. System playback is already a clean digital
    // source and must not pass through speech-oriented noise suppression/AGC.
    let mut microphone_enhancer = request
        .microphone
        .then(|| crate::audio_enhancement::StreamingAudioEnhancer::sonora(SAMPLE_RATE))
        .transpose()?;

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

    let mut live_vad = create_live_vad(app);
    let mut voice_activity = if live_vad.is_some() {
        VoiceActivityState::Listening
    } else {
        VoiceActivityState::Unavailable
    };

    publish_status(app, status, |current| {
        current.phase = RecordingPhase::Recording;
        current.voice_activity = voice_activity;
    });
    let _ = ready.send(Ok(()));
    let mut last_manifest = Instant::now();
    let mut last_status = Instant::now();
    let mut mic_queue = VecDeque::new();
    let mut sys_queue = VecDeque::new();
    let mut mix_buffer = Vec::new();
    let mut waveform = crate::audio_waveform::LiveWaveformAccumulator::new(SAMPLE_RATE);
    let mut microphone_level = 0.0;
    let mut system_level = 0.0;
    let mut stop_reason = StopReason::User;
    let mut microphone_stalled = false;
    let mut system_stalled = false;
    let mut audio_processing_cpu = Duration::ZERO;
    while !stop.load(Ordering::Acquire) {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if elapsed_ms >= MAX_DURATION_MS {
            stop_reason = StopReason::DurationLimit;
            break;
        }

        if let Some(stream) = microphone.as_mut() {
            while let Some(event) = stream.poll_event() {
                apply_capture_event(&event, &mut microphone_stalled, &mut stop_reason, stop);
                update_capture_warning(app, status, microphone_stalled, system_stalled);
            }
            while let Some(chunk) = stream.poll_chunk() {
                accumulate_meter_samples(&mut microphone_level, &chunk.data);
                let enhancement_started = Instant::now();
                let enhanced = microphone_enhancer
                    .as_mut()
                    .expect("microphone enhancer exists when microphone capture is enabled")
                    .accept(&chunk.data)?;
                audio_processing_cpu += enhancement_started.elapsed();
                if !enhanced.is_empty() {
                    if let Some(writer) = microphone_writer.as_mut() {
                        writer.write(&enhanced)?;
                    }
                    mic_queue.push_back((chunk.pts_ns, enhanced));
                }
            }
        }

        if let Some(stream) = system.as_mut() {
            while let Some(event) = stream.poll_event() {
                apply_capture_event(&event, &mut system_stalled, &mut stop_reason, stop);
                update_capture_warning(app, status, microphone_stalled, system_stalled);
            }
            while let Some(chunk) = stream.poll_chunk() {
                if let Some(writer) = system_writer.as_mut() {
                    writer.write(&chunk.data)?;
                }
                accumulate_meter_samples(&mut system_level, &chunk.data);
                sys_queue.push_back((chunk.pts_ns, chunk.data));
            }
        }

        let mixing_started = Instant::now();
        drain_mix(
            &mut mixed_writer,
            &mut mic_queue,
            &mut sys_queue,
            &mut mix_buffer,
            request,
            false,
            |samples| {
                waveform.accept(samples);
                if let Some(detector) = live_vad.as_mut() {
                    voice_activity = if detector.accept_waveform(samples) {
                        VoiceActivityState::SpeechDetected
                    } else {
                        VoiceActivityState::Listening
                    };
                }
            },
        )?;
        audio_processing_cpu += mixing_started.elapsed();
        if last_status.elapsed() >= STATUS_INTERVAL {
            let (published_microphone_level, published_system_level) =
                take_meter_levels(&mut microphone_level, &mut system_level);
            publish_status(app, status, |current| {
                current.elapsed_ms = elapsed_ms;
                current.microphone_level = published_microphone_level;
                current.system_level = published_system_level;
                current.voice_activity = voice_activity;
            });
            last_status = Instant::now();
        }

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
    let mixing_started = Instant::now();
    drain_mix(
        &mut mixed_writer,
        &mut mic_queue,
        &mut sys_queue,
        &mut mix_buffer,
        request,
        true,
        |samples| {
            waveform.accept(samples);
            if let Some(detector) = live_vad.as_mut() {
                let _ = detector.accept_waveform(samples);
            }
        },
    )?;
    audio_processing_cpu += mixing_started.elapsed();
    let enhanced_tail = microphone_enhancer
        .take()
        .map(|mut enhancer| enhancer.finish())
        .transpose()?
        .unwrap_or_default();
    if !enhanced_tail.is_empty() {
        if let Some(writer) = microphone_writer.as_mut() {
            writer.write(&enhanced_tail)?;
        }
        waveform.accept(&enhanced_tail);
        if let Some(detector) = live_vad.as_mut() {
            let _ = detector.accept_waveform(&enhanced_tail);
        }
        // At shutdown no system samples remain queued, so the microphone tail
        // is also the final mixed tail.
        mixed_writer.write(&enhanced_tail)?;
    }

    if cancel.load(Ordering::Acquire) {
        drop(microphone_writer);
        drop(system_writer);
        drop(mixed_writer);
        remove_session(&paths.directory)?;
        publish_status(app, status, |current| *current = RecordingStatus::default());
        return Ok(());
    }

    publish_status(app, status, |current| {
        current.phase = RecordingPhase::Finalizing;
        current.stop_reason = Some(stop_reason);
    });
    let finalization_timer = crate::processing_metrics::StageTimer::start(
        "recording",
        "finalize_files",
        Some(started.elapsed().as_millis() as u64),
    );
    if let Some(writer) = microphone_writer {
        writer
            .finish()
            .map_err(|error| format!("マイク音声のM4A確定に失敗しました: {error}"))?;
    }
    if let Some(writer) = system_writer {
        writer
            .finish()
            .map_err(|error| format!("システム音声のM4A確定に失敗しました: {error}"))?;
    }
    mixed_writer
        .finish()
        .map_err(|error| format!("会議音声のM4A確定に失敗しました: {error}"))?;

    atomic_copy_to_output(&paths.mixed, &paths.final_file)?;
    manifest.duration_ms = started.elapsed().as_millis() as u64;
    manifest.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    manifest.finalized = true;
    manifest.stop_reason = Some(stop_reason);
    manifest.save(&paths.directory)?;

    let selected =
        crate::commands::transcribe::set_selected_audio_path(app, paths.final_file.clone())?;
    crate::meeting_store::store_recording_tracks(
        app,
        selected.meeting_id(),
        request.microphone.then_some(paths.microphone.as_path()),
        request.system_audio.then_some(paths.system.as_path()),
    )?;
    if let Err(error) =
        crate::audio_waveform::cache_recorded_waveform(app, selected.meeting_id(), waveform)
    {
        eprintln!("Could not cache recorded waveform: {error}");
    }
    finalization_timer.finish();
    crate::processing_metrics::log_timing(
        "recording",
        "realtime_audio_processing_cpu",
        audio_processing_cpu,
        Some(manifest.duration_ms),
    );
    remove_session(&paths.directory)?;
    publish_status(app, status, |current| {
        current.phase = RecordingPhase::Completed;
        current.elapsed_ms = manifest.duration_ms;
        current.output_path = Some(paths.final_file.to_string_lossy().into_owned());
        current.stop_reason = Some(stop_reason);
        current.warning = None;
        current.microphone_level = 0.0;
        current.system_level = 0.0;
        current.voice_activity = VoiceActivityState::Unavailable;
    });
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn create_live_vad(
    app: &AppHandle,
) -> Option<crate::transcription::vad::LiveVoiceActivityDetector> {
    let model = match crate::transcription::vad_models::installed_model_path(app) {
        Ok(Some(path)) => path,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("Could not verify live VAD model: {error}");
            return None;
        }
    };
    let preset = match crate::transcription::vad_settings::current_preset(app) {
        Ok(preset) => preset,
        Err(error) => {
            eprintln!("Could not read live VAD settings: {error}");
            return None;
        }
    };
    match crate::transcription::vad::LiveVoiceActivityDetector::create(
        app,
        &model,
        SAMPLE_RATE,
        preset,
    ) {
        Ok(detector) => Some(detector),
        Err(error) => {
            eprintln!("Could not initialize live VAD: {error}");
            None
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn apply_capture_event(
    event: &flexaudio::Event,
    stalled: &mut bool,
    stop_reason: &mut StopReason,
    stop: &AtomicBool,
) {
    match capture_event_effect(event) {
        CaptureEventEffect::None => {}
        CaptureEventEffect::Stalled => *stalled = true,
        CaptureEventEffect::Recovered => *stalled = false,
        CaptureEventEffect::Stop(reason) => {
            *stop_reason = reason;
            stop.store(true, Ordering::Release);
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn capture_event_effect(event: &flexaudio::Event) -> CaptureEventEffect {
    use flexaudio::Event;

    match event {
        Event::StreamStalled => CaptureEventEffect::Stalled,
        Event::StreamRecovered => CaptureEventEffect::Recovered,
        Event::DeviceLost => CaptureEventEffect::Stop(StopReason::SourceDisconnected),
        Event::PermissionDenied => CaptureEventEffect::Stop(StopReason::CaptureError),
        Event::Error(detail) if detail.starts_with("reopen failed:") => CaptureEventEffect::Stalled,
        Event::Error(_) => CaptureEventEffect::Stop(StopReason::CaptureError),
        _ => CaptureEventEffect::None,
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn update_capture_warning(
    app: &AppHandle,
    status: &Arc<Mutex<RecordingStatus>>,
    microphone_stalled: bool,
    system_stalled: bool,
) {
    let warning = match (microphone_stalled, system_stalled) {
        (true, true) => Some("マイクとシステム音声を再接続しています。録音は継続中です。"),
        (true, false) => Some("マイクを再接続しています。録音は継続中です。"),
        (false, true) => Some("システム音声を再接続しています。録音は継続中です。"),
        (false, false) => None,
    };
    publish_status(app, status, |current| {
        current.warning = warning.map(str::to_owned)
    });
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

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    use super::{
        accumulate_meter_samples, capture_event_effect, take_meter_levels, CaptureEventEffect,
    };

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn meter_uses_the_loudest_chunk_in_each_status_interval() {
        let mut microphone = 0.0;
        let mut system = 0.0;
        accumulate_meter_samples(&mut system, &[0.1, -0.42, 0.2]);
        accumulate_meter_samples(&mut system, &[0.0, 0.0]);
        accumulate_meter_samples(&mut microphone, &[0.18, -0.12]);

        assert_eq!(
            take_meter_levels(&mut microphone, &mut system),
            (0.18, 0.42)
        );
        assert_eq!((microphone, system), (0.0, 0.0));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn temporary_stream_stalls_do_not_stop_recording() {
        assert_eq!(
            capture_event_effect(&flexaudio::Event::StreamStalled),
            CaptureEventEffect::Stalled
        );
        assert_eq!(
            capture_event_effect(&flexaudio::Event::Error(
                "reopen failed: device is temporarily unavailable".into()
            )),
            CaptureEventEffect::Stalled
        );
        assert_eq!(
            capture_event_effect(&flexaudio::Event::StreamRecovered),
            CaptureEventEffect::Recovered
        );
    }
}
