use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use tauri::AppHandle;

use super::{
    desktop::run_recording,
    session::RecordingPaths,
    set_status,
    types::{RecordingPhase, RecordingStatus, StartRecordingRequest},
};

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
