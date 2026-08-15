use std::{fs, io::Write, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "power.json";

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ProcessingPowerSettings {
    keep_display_on: bool,
}

#[derive(Default)]
pub(crate) struct ProcessingPowerState {
    inner: Mutex<ProcessingPowerInner>,
}

#[derive(Default)]
struct ProcessingPowerInner {
    users: usize,
    inhibitor: Option<NativeInhibitor>,
    settings: Option<ProcessingPowerSettings>,
}

pub(crate) struct ProcessingPowerGuard {
    app: AppHandle,
}

impl ProcessingPowerState {
    fn settings(
        &self,
        app: &AppHandle,
        inner: &mut ProcessingPowerInner,
    ) -> Result<ProcessingPowerSettings, String> {
        if let Some(settings) = inner.settings {
            return Ok(settings);
        }
        let settings = read_settings(app)?;
        configure_native_display(settings.keep_display_on)?;
        inner.settings = Some(settings);
        Ok(settings)
    }

    fn acquire(&self, app: &AppHandle, reason: &str) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "スリープ防止状態を更新できませんでした。".to_string())?;
        let settings = self.settings(app, &mut inner)?;
        if inner.users == 0 {
            inner.inhibitor = Some(NativeInhibitor::acquire(reason, settings.keep_display_on)?);
        }
        inner.users = inner.users.saturating_add(1);
        Ok(())
    }

    fn update_settings(&self, settings: ProcessingPowerSettings) -> Result<(), String> {
        configure_native_display(settings.keep_display_on)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "画面点灯設定を更新できませんでした。".to_string())?;
        if let Some(inhibitor) = inner.inhibitor.as_mut() {
            inhibitor.set_display_required(settings.keep_display_on)?;
        }
        inner.settings = Some(settings);
        Ok(())
    }

    fn release(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.users == 0 {
            return;
        }
        inner.users = inner.users.saturating_sub(1);
        if inner.users == 0 {
            inner.inhibitor.take();
        }
    }
}

impl Drop for ProcessingPowerGuard {
    fn drop(&mut self) {
        self.app.state::<ProcessingPowerState>().release();
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("settings").join(SETTINGS_FILE))
        .map_err(|error| format!("画面点灯設定の保存先を取得できませんでした: {error}"))
}

fn read_settings(app: &AppHandle) -> Result<ProcessingPowerSettings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(ProcessingPowerSettings::default());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("画面点灯設定を読み込めませんでした: {error}"))?;
    serde_json::from_slice(&bytes).or_else(|error| {
        eprintln!(
            "Ignoring invalid processing power settings at {}: {error}",
            path.display()
        );
        Ok(ProcessingPowerSettings::default())
    })
}

fn write_settings(app: &AppHandle, settings: ProcessingPowerSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let parent = path.parent().ok_or("画面点灯設定の保存先が不正です。")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("画面点灯設定の保存先を作成できませんでした: {error}"))?;
    let temporary = parent.join(format!(".{SETTINGS_FILE}.{}", uuid::Uuid::now_v7()));
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("画面点灯設定を作成できませんでした: {error}"))?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("画面点灯設定を保存できませんでした: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("画面点灯設定を安全に保存できませんでした: {error}"))?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("画面点灯設定を更新できませんでした: {error}"))?;
        }
        fs::rename(&temporary, &path)
            .map_err(|error| format!("画面点灯設定を確定できませんでした: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[tauri::command]
pub(crate) fn get_processing_power_settings(
    app: AppHandle,
) -> Result<ProcessingPowerSettings, String> {
    let settings = read_settings(&app)?;
    app.state::<ProcessingPowerState>()
        .update_settings(settings)?;
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_processing_power_settings(
    app: AppHandle,
    settings: ProcessingPowerSettings,
) -> Result<ProcessingPowerSettings, String> {
    let previous = read_settings(&app)?;
    let state = app.state::<ProcessingPowerState>();
    state.update_settings(settings)?;
    if let Err(error) = write_settings(&app, settings) {
        if let Err(rollback_error) = state.update_settings(previous) {
            eprintln!("Could not restore processing power settings: {rollback_error}");
        }
        return Err(error);
    }
    Ok(settings)
}

#[cfg(target_os = "android")]
pub(crate) fn sync_display_setting(app: &AppHandle) -> Result<(), String> {
    let settings = read_settings(app)?;
    app.state::<ProcessingPowerState>()
        .update_settings(settings)
}

pub(crate) fn acquire(app: &AppHandle, reason: &str) -> Result<ProcessingPowerGuard, String> {
    app.state::<ProcessingPowerState>().acquire(app, reason)?;
    Ok(ProcessingPowerGuard { app: app.clone() })
}

#[cfg(target_os = "windows")]
enum WindowsPowerCommand {
    SetDisplay(bool, std::sync::mpsc::SyncSender<Result<(), String>>),
    Stop,
}

#[cfg(target_os = "windows")]
struct NativeInhibitor {
    command: std::sync::mpsc::Sender<WindowsPowerCommand>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
fn set_windows_execution_state(display_required: bool) -> Result<(), String> {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };

    let mut flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED;
    if display_required {
        flags |= ES_DISPLAY_REQUIRED;
    }
    let result = unsafe { SetThreadExecutionState(flags) };
    if result.0 == 0 {
        Err("Windowsのスリープ防止を更新できませんでした。".into())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl NativeInhibitor {
    fn acquire(_reason: &str, display_required: bool) -> Result<Self, String> {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("mutsuna-sleep-inhibitor".into())
            .spawn(move || {
                // SetThreadExecutionState is thread-scoped, so every update and
                // the final release intentionally live on this dedicated thread.
                let initial = set_windows_execution_state(display_required);
                let initial_succeeded = initial.is_ok();
                if ready_tx.send(initial).is_err() || !initial_succeeded {
                    return;
                }
                while let Ok(command) = command_rx.recv() {
                    match command {
                        WindowsPowerCommand::SetDisplay(required, response) => {
                            let _ = response.send(set_windows_execution_state(required));
                        }
                        WindowsPowerCommand::Stop => break,
                    }
                }
                use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
            })
            .map_err(|error| format!("スリープ防止処理を開始できませんでした: {error}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                command: command_tx,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("スリープ防止処理の開始結果を確認できませんでした。".into())
            }
        }
    }

    fn set_display_required(&mut self, required: bool) -> Result<(), String> {
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
        self.command
            .send(WindowsPowerCommand::SetDisplay(required, response_tx))
            .map_err(|_| "Windowsのスリープ防止処理が停止しています。".to_string())?;
        response_rx
            .recv()
            .map_err(|_| "Windowsの画面点灯設定を確認できませんでした。".to_string())?
    }
}

#[cfg(target_os = "windows")]
impl Drop for NativeInhibitor {
    fn drop(&mut self) {
        let _ = self.command.send(WindowsPowerCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeInhibitor {
    system_assertion: u32,
    display_assertion: Option<u32>,
    reason: String,
}

#[cfg(target_os = "macos")]
fn create_macos_assertion(assertion_type: &str, reason: &str) -> Result<u32, String> {
    use core_foundation::{base::TCFType, string::CFString};

    type CFStringRef = *const std::ffi::c_void;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            level: u32,
            reason: CFStringRef,
            assertion_id: *mut u32,
        ) -> i32;
    }

    let assertion_type = CFString::new(assertion_type);
    let reason = CFString::new(reason);
    let mut assertion_id = 0;
    let result = unsafe {
        IOPMAssertionCreateWithName(
            assertion_type.as_concrete_TypeRef().cast(),
            255,
            reason.as_concrete_TypeRef().cast(),
            &mut assertion_id,
        )
    };
    if result != 0 {
        Err(format!(
            "macOSのスリープ防止を開始できませんでした（IOKit: {result}）。"
        ))
    } else {
        Ok(assertion_id)
    }
}

#[cfg(target_os = "macos")]
fn release_macos_assertion(assertion_id: u32) {
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }
    unsafe {
        IOPMAssertionRelease(assertion_id);
    }
}

#[cfg(target_os = "macos")]
impl NativeInhibitor {
    fn acquire(reason: &str, display_required: bool) -> Result<Self, String> {
        let system_assertion = create_macos_assertion("PreventUserIdleSystemSleep", reason)?;
        let display_assertion = if display_required {
            match create_macos_assertion("PreventUserIdleDisplaySleep", reason) {
                Ok(assertion) => Some(assertion),
                Err(error) => {
                    release_macos_assertion(system_assertion);
                    return Err(error);
                }
            }
        } else {
            None
        };
        Ok(Self {
            system_assertion,
            display_assertion,
            reason: reason.to_string(),
        })
    }

    fn set_display_required(&mut self, required: bool) -> Result<(), String> {
        match (required, self.display_assertion) {
            (true, None) => {
                self.display_assertion = Some(create_macos_assertion(
                    "PreventUserIdleDisplaySleep",
                    &self.reason,
                )?);
            }
            (false, Some(assertion)) => {
                release_macos_assertion(assertion);
                self.display_assertion = None;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl Drop for NativeInhibitor {
    fn drop(&mut self) {
        if let Some(assertion) = self.display_assertion.take() {
            release_macos_assertion(assertion);
        }
        release_macos_assertion(self.system_assertion);
    }
}

#[cfg(target_os = "android")]
struct NativeInhibitor;

#[cfg(target_os = "android")]
impl NativeInhibitor {
    fn acquire(reason: &str, _display_required: bool) -> Result<Self, String> {
        crate::android_context::with_bridge_env(
            "jp.mutsuna.echo.ProcessingPowerBridge",
            "Androidの処理継続サービスへ接続できませんでした",
            |env, context, class| {
                let reason = env
                    .new_string(reason)
                    .map_err(|error| format!("処理内容を準備できませんでした: {error}"))?;
                let reason = jni::objects::JObject::from(reason);
                env.call_static_method(
                    class,
                    "acquire",
                    "(Landroid/content/Context;Ljava/lang/String;)V",
                    &[
                        jni::objects::JValue::Object(context),
                        jni::objects::JValue::Object(&reason),
                    ],
                )
                .map_err(|error| {
                    format!("Androidの処理継続サービスを開始できませんでした: {error}")
                })?;
                Ok(())
            },
        )?;
        Ok(Self)
    }

    fn set_display_required(&mut self, _required: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(target_os = "android")]
impl Drop for NativeInhibitor {
    fn drop(&mut self) {
        let result = crate::android_context::with_bridge_env(
            "jp.mutsuna.echo.ProcessingPowerBridge",
            "Androidの処理継続サービスへ接続できませんでした",
            |env, context, class| {
                env.call_static_method(
                    class,
                    "release",
                    "(Landroid/content/Context;)V",
                    &[jni::objects::JValue::Object(context)],
                )
                .map_err(|error| {
                    format!("Androidの処理継続サービスを停止できませんでした: {error}")
                })?;
                Ok(())
            },
        );
        if let Err(error) = result {
            eprintln!("{error}");
        }
    }
}

#[cfg(target_os = "android")]
fn configure_native_display(required: bool) -> Result<(), String> {
    crate::android_context::with_bridge_env(
        "jp.mutsuna.echo.ProcessingPowerBridge",
        "Androidの画面点灯設定へ接続できませんでした",
        |env, context, class| {
            env.call_static_method(
                class,
                "setDisplayRequired",
                "(Landroid/content/Context;Z)V",
                &[
                    jni::objects::JValue::Object(context),
                    jni::objects::JValue::Bool(required.into()),
                ],
            )
            .map_err(|error| format!("Androidの画面点灯設定を変更できませんでした: {error}"))?;
            Ok(())
        },
    )
}

#[cfg(not(target_os = "android"))]
fn configure_native_display(_required: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
struct NativeInhibitor;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
impl NativeInhibitor {
    fn acquire(_reason: &str, _display_required: bool) -> Result<Self, String> {
        Ok(Self)
    }

    fn set_display_required(&mut self, _required: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessingPowerSettings;

    #[test]
    fn keeping_the_display_on_is_opt_in() {
        assert!(!ProcessingPowerSettings::default().keep_display_on);
    }
}
