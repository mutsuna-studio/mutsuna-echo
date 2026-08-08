#![cfg(desktop)]

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem},
    plugin::{Builder as PluginBuilder, TauriPlugin},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, Runtime, WebviewWindowBuilder, WindowEvent,
};

use crate::recording::{types::RecordingPhase, RecordingService};

const MAIN_WINDOW_LABEL: &str = "main";
const MENU_OPEN: &str = "open";
const MENU_QUIT: &str = "quit";

#[derive(Default)]
pub struct ResidentState {
    quitting: AtomicBool,
    opening: AtomicBool,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("resident-shell")
        .setup(|app, _| {
            create_tray(app)?;
            Ok(())
        })
        .on_event(|app, event| match event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } if label == MAIN_WINDOW_LABEL
                && !app
                    .state::<ResidentState>()
                    .quitting
                    .load(Ordering::Acquire) =>
            {
                api.prevent_close();
                destroy_main_window(app);
            }
            RunEvent::ExitRequested { api, .. }
                if !app
                    .state::<ResidentState>()
                    .quitting
                    .load(Ordering::Acquire) =>
            {
                api.prevent_exit();
            }
            _ => {}
        })
        .build()
}

fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Mutsuna Echoを開く", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("Mutsuna Echo")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_OPEN => request_main_window(app),
            MENU_QUIT => quit_application(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                request_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn destroy_main_window<R: Runtime>(app: &AppHandle<R>) {
    let destroyed = match app.get_webview_window(MAIN_WINDOW_LABEL) {
        Some(window) => match window.destroy() {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Could not destroy main webview window: {error:?}");
                false
            }
        },
        None => true,
    };
    #[cfg(not(target_os = "macos"))]
    let _ = destroyed;
    #[cfg(target_os = "macos")]
    if destroyed {
        if let Err(error) = app.set_dock_visibility(false) {
            eprintln!("Could not hide the macOS dock icon: {error:?}");
        }
    }
}

fn request_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = show_or_create_main_window(app) {
        eprintln!("Could not request main webview window: {error}");
    }
}

fn show_or_create_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
            .unminimize()
            .map_err(|error| format!("メイン画面を元のサイズへ戻せませんでした: {error}"))?;
        window
            .show()
            .map_err(|error| format!("メイン画面を表示できませんでした: {error}"))?;
        window
            .set_focus()
            .map_err(|error| format!("メイン画面を前面へ移動できませんでした: {error}"))?;
        return Ok(());
    }

    let state = app.state::<ResidentState>();
    if state
        .opening
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    let app = app.clone();
    let fallback_app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("mutsuna-window-open".into())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            if let Err(error) = app.set_dock_visibility(true) {
                eprintln!("Could not show the macOS dock icon: {error:?}");
            }
            let result = app
                .config()
                .app
                .windows
                .iter()
                .find(|config| config.label == MAIN_WINDOW_LABEL)
                .cloned()
                .ok_or_else(|| "main window configuration is missing".to_string())
                .and_then(|config| {
                    WebviewWindowBuilder::from_config(&app, &config)
                        .map_err(|error| error.to_string())?
                        .build()
                        .map_err(|error| error.to_string())
                });
            app.state::<ResidentState>()
                .opening
                .store(false, Ordering::Release);
            if let Err(error) = result {
                eprintln!("Could not recreate main webview window: {error}");
            }
        })
    {
        fallback_app
            .state::<ResidentState>()
            .opening
            .store(false, Ordering::Release);
        return Err(format!(
            "メイン画面の起動処理を開始できませんでした: {error}"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn prepare_transcription_handoff(
    app: AppHandle,
) -> Result<crate::pending_action::PendingAction, String> {
    let meeting_id = crate::commands::transcribe::selected_meeting_id(&app)?;
    let action = crate::pending_action::prepare_transcription(&app, &meeting_id)?;
    show_or_create_main_window(&app)?;
    app.emit(crate::pending_action::AVAILABLE_EVENT, action.clone())
        .map_err(|error| format!("メイン画面へ文字起こし待ちを通知できませんでした: {error}"))?;
    Ok(action)
}

fn quit_application<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<ResidentState>();
    if state.quitting.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("mutsuna-safe-exit".into())
        .spawn(move || {
            let service = app.state::<RecordingService>();
            if matches!(
                service.status().phase,
                RecordingPhase::Starting | RecordingPhase::Recording | RecordingPhase::Finalizing
            ) {
                if let Err(error) = service
                    .request_stop(false)
                    .and_then(|_| service.wait_for_stop().map(|_| ()))
                {
                    eprintln!("Could not finalize recording before exit: {error}");
                }
            }
            app.exit(0);
        })
    {
        state.quitting.store(false, Ordering::Release);
        eprintln!("Could not start safe application exit: {error}");
    }
}
