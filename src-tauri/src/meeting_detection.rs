#![cfg(desktop)]

use std::{
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{
    plugin::{Builder as PluginBuilder, TauriPlugin},
    AppHandle, Manager, Runtime, WebviewWindowBuilder,
};

use crate::recording::{types::RecordingPhase, RecordingService};

const OVERLAY_LABEL: &str = "meeting-overlay";
const SCAN_INTERVAL: Duration = Duration::from_secs(5);
const REQUIRED_CONSECUTIVE_SCANS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MeetingProvider {
    Zoom,
    GoogleMeet,
    MicrosoftTeams,
}

impl MeetingProvider {
    fn label(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::GoogleMeet => "Google Meet",
            Self::MicrosoftTeams => "Microsoft Teams",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetection {
    provider: MeetingProvider,
    provider_label: &'static str,
    window_title: String,
    detected_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MeetingCandidate {
    provider: MeetingProvider,
    window_title: String,
}

#[derive(Default)]
struct DetectionTracker {
    observed_provider: Option<MeetingProvider>,
    consecutive_scans: u8,
    active: Option<MeetingDetection>,
    suppressed_provider: Option<MeetingProvider>,
}

#[derive(Default)]
pub struct MeetingDetectionState {
    tracker: Mutex<DetectionTracker>,
}

impl MeetingDetectionState {
    fn observe(&self, candidate: Option<MeetingCandidate>) -> Option<MeetingDetection> {
        let mut tracker = self
            .tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(candidate) = candidate else {
            tracker.observed_provider = None;
            tracker.consecutive_scans = 0;
            tracker.active = None;
            tracker.suppressed_provider = None;
            return None;
        };

        if tracker.suppressed_provider == Some(candidate.provider) {
            return None;
        }
        if let Some(active) = tracker.active.as_mut() {
            if active.provider == candidate.provider {
                active.window_title = candidate.window_title;
                return None;
            }
        }

        if tracker.observed_provider == Some(candidate.provider) {
            tracker.consecutive_scans = tracker.consecutive_scans.saturating_add(1);
        } else {
            tracker.observed_provider = Some(candidate.provider);
            tracker.consecutive_scans = 1;
        }
        if tracker.consecutive_scans < REQUIRED_CONSECUTIVE_SCANS {
            return None;
        }

        let detection = MeetingDetection {
            provider: candidate.provider,
            provider_label: candidate.provider.label(),
            window_title: candidate.window_title,
            detected_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        tracker.active = Some(detection.clone());
        Some(detection)
    }

    fn suppress(&self, provider: Option<MeetingProvider>) {
        let mut tracker = self
            .tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        tracker.suppressed_provider =
            provider.or_else(|| tracker.active.as_ref().map(|detection| detection.provider));
        tracker.active = None;
        tracker.observed_provider = None;
        tracker.consecutive_scans = 0;
    }

    fn current(&self) -> Option<MeetingDetection> {
        self.tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
            .clone()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("meeting-detection")
        .setup(|app, _| {
            start_watcher(app.clone());
            Ok(())
        })
        .build()
}

#[tauri::command]
pub fn get_meeting_detection(
    state: tauri::State<'_, MeetingDetectionState>,
) -> Option<MeetingDetection> {
    state.current()
}

#[tauri::command]
pub fn dismiss_meeting_overlay(state: tauri::State<'_, MeetingDetectionState>) {
    state.suppress(None);
}

fn start_watcher<R: Runtime>(app: AppHandle<R>) {
    if let Err(error) = std::thread::Builder::new()
        .name("mutsuna-meeting-detection".into())
        .spawn(move || loop {
            std::thread::sleep(SCAN_INTERVAL);
            let candidate = detect_meeting();
            let recording_active = matches!(
                app.state::<RecordingService>().status().phase,
                RecordingPhase::Starting | RecordingPhase::Recording | RecordingPhase::Finalizing
            );
            let state = app.state::<MeetingDetectionState>();
            if candidate.is_none() {
                state.observe(None);
                destroy_overlay(&app);
                continue;
            }
            if recording_active {
                state.suppress(candidate.map(|item| item.provider));
                destroy_overlay(&app);
                continue;
            }
            if state.observe(candidate).is_some() {
                destroy_overlay(&app);
                if let Err(error) = show_overlay(&app) {
                    eprintln!("Could not show meeting overlay: {error}");
                }
            }
        })
    {
        eprintln!("Could not start meeting detection: {error}");
    }
}

fn show_overlay<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.show();
        return Ok(());
    }

    let width = 390.0;
    let height = 230.0;
    // メイン画面の設定を複製すると、開発時はdevUrl、本番時はfrontendDistが
    // Tauriによって同じように解決される。
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .cloned()
        .ok_or_else(|| "main window configuration is missing".to_string())?;
    config.label = OVERLAY_LABEL.into();
    config.title = "Mutsuna Echo - 会議を検出".into();
    config.width = width;
    config.height = height;
    config.min_width = Some(width);
    config.min_height = Some(height);
    config.max_width = Some(width);
    config.max_height = Some(height);
    config.resizable = false;
    config.maximizable = false;
    config.minimizable = false;
    config.decorations = false;
    config.always_on_top = true;
    config.skip_taskbar = true;
    config.focus = false;
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let position = monitor.position();
        config.x = Some(position.x as f64 / scale + size.width as f64 / scale - width - 24.0);
        config.y = Some(position.y as f64 / scale + size.height as f64 / scale - height - 48.0);
    }
    WebviewWindowBuilder::from_config(app, &config)
        .map_err(|error| error.to_string())?
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn destroy_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        if let Err(error) = window.destroy() {
            eprintln!("Could not destroy meeting overlay: {error:?}");
        }
    }
}

fn classify_window(owner: &str, title: &str) -> Option<MeetingCandidate> {
    let owner = owner.to_lowercase();
    let title_lower = title.to_lowercase();
    let has_call_cue = ["meeting", "call", "会議", "通話", "ミーティング"]
        .iter()
        .any(|cue| title_lower.contains(cue));

    let provider = if title_lower.contains("zoom meeting")
        || title_lower.contains("zoomミーティング")
        || ((owner.contains("zoom") || title_lower.contains("zoom")) && has_call_cue)
    {
        MeetingProvider::Zoom
    } else if (title_lower.contains("google meet") && title_lower.trim() != "google meet")
        || looks_like_google_meet_code(&title_lower)
    {
        MeetingProvider::GoogleMeet
    } else if (owner.contains("teams") || title_lower.contains("microsoft teams")) && has_call_cue {
        MeetingProvider::MicrosoftTeams
    } else {
        return None;
    };

    Some(MeetingCandidate {
        provider,
        window_title: title.trim().to_string(),
    })
}

fn looks_like_google_meet_code(title: &str) -> bool {
    let Some(rest) = title.strip_prefix("meet - ") else {
        return false;
    };
    let code = rest.split_whitespace().next().unwrap_or_default();
    let parts: Vec<_> = code.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 3
        && parts[1].len() == 4
        && parts[2].len() == 3
        && parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_alphabetic()))
}

fn detect_meeting() -> Option<MeetingCandidate> {
    #[cfg(debug_assertions)]
    if let Ok(title) = std::env::var("MUTSUNA_ECHO_MEETING_DETECTION_TEST_TITLE") {
        return classify_window("Mutsuna Echo test", &title);
    }
    platform::visible_windows()
        .into_iter()
        .find_map(|window| classify_window(&window.owner, &window.title))
}

#[derive(Debug)]
struct VisibleWindow {
    owner: String,
    title: String,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::VisibleWindow;
    use windows::{
        core::{BOOL, PWSTR},
        Win32::{
            Foundation::{CloseHandle, HWND, LPARAM},
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
                IsWindowVisible,
            },
        },
    };

    pub fn visible_windows() -> Vec<VisibleWindow> {
        let mut windows = Vec::new();
        let pointer = &mut windows as *mut Vec<VisibleWindow>;
        unsafe {
            let _ = EnumWindows(Some(collect_window), LPARAM(pointer as isize));
        }
        windows
    }

    unsafe extern "system" fn collect_window(window: HWND, parameter: LPARAM) -> BOOL {
        if !unsafe { IsWindowVisible(window) }.as_bool() {
            return BOOL(1);
        }
        let length = unsafe { GetWindowTextLengthW(window) };
        if length <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0_u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(window, &mut buffer) };
        if copied > 0 {
            let windows = unsafe { &mut *(parameter.0 as *mut Vec<VisibleWindow>) };
            windows.push(VisibleWindow {
                owner: process_name(window),
                title: String::from_utf16_lossy(&buffer[..copied as usize]),
            });
        }
        BOOL(1)
    }

    fn process_name(window: HWND) -> String {
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
        if process_id == 0 {
            return String::new();
        }
        let Ok(process) =
            (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
        else {
            return String::new();
        };
        let mut buffer = vec![0_u16; 1024];
        let mut length = buffer.len() as u32;
        let result = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        };
        let _ = unsafe { CloseHandle(process) };
        if result.is_err() {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        std::path::Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::{
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        string::CFString,
    };
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowOwnerName,
    };

    use super::VisibleWindow;

    pub fn visible_windows() -> Vec<VisibleWindow> {
        let Some(items) = copy_window_info(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        ) else {
            return Vec::new();
        };
        items
            .get_all_values()
            .into_iter()
            .filter_map(|pointer| unsafe {
                let dictionary: CFDictionary<CFString, CFType> =
                    TCFType::wrap_under_get_rule(pointer.cast());
                let owner = dictionary_string(&dictionary, kCGWindowOwnerName)?;
                let title = dictionary_string(&dictionary, kCGWindowName)?;
                (!title.trim().is_empty()).then_some(VisibleWindow { owner, title })
            })
            .collect()
    }

    unsafe fn dictionary_string(
        dictionary: &CFDictionary<CFString, CFType>,
        key: core_foundation::string::CFStringRef,
    ) -> Option<String> {
        let key = unsafe { CFString::wrap_under_get_rule(key) };
        dictionary
            .find(&key)
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::VisibleWindow;

    pub fn visible_windows() -> Vec<VisibleWindow> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_window, DetectionTracker, MeetingCandidate, MeetingDetectionState, MeetingProvider,
    };

    #[test]
    fn recognizes_supported_meeting_titles_but_not_landing_pages() {
        assert_eq!(
            classify_window("", "Zoom Meeting").unwrap().provider,
            MeetingProvider::Zoom
        );
        assert_eq!(
            classify_window("Google Chrome", "Meet - abc-defg-hij")
                .unwrap()
                .provider,
            MeetingProvider::GoogleMeet
        );
        assert_eq!(
            classify_window("Microsoft Teams", "Weekly meeting | Microsoft Teams")
                .unwrap()
                .provider,
            MeetingProvider::MicrosoftTeams
        );
        assert!(classify_window("Google Chrome", "Google Meet").is_none());
        assert!(classify_window("Microsoft Teams", "Chat | Microsoft Teams").is_none());
    }

    #[test]
    fn requires_two_scans_and_suppresses_until_the_meeting_disappears() {
        let state = MeetingDetectionState {
            tracker: std::sync::Mutex::new(DetectionTracker::default()),
        };
        let candidate = MeetingCandidate {
            provider: MeetingProvider::Zoom,
            window_title: "Zoom Meeting".into(),
        };
        assert!(state.observe(Some(candidate.clone())).is_none());
        assert!(state.observe(Some(candidate.clone())).is_some());
        state.suppress(None);
        assert!(state.observe(Some(candidate.clone())).is_none());
        assert!(state.observe(None).is_none());
        assert!(state.observe(Some(candidate.clone())).is_none());
        assert!(state.observe(Some(candidate)).is_some());
    }
}
