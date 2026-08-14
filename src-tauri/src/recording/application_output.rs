use super::types::{ApplicationOutput, ApplicationOutputIcon, ApplicationOutputStatus};

pub fn status() -> Result<ApplicationOutputStatus, String> {
    #[cfg(target_os = "windows")]
    {
        windows_status()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(ApplicationOutputStatus {
            supported: false,
            applications: Vec::new(),
            limitation: Some("アプリごとの音量調整はWindows版でのみ利用できます。".into()),
        })
    }
}

pub fn icon(application_id: &str) -> Result<Option<ApplicationOutputIcon>, String> {
    validate_application_id(application_id)?;
    #[cfg(target_os = "windows")]
    {
        windows_icon_for_application(application_id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(None)
    }
}

pub fn set_volume(application_id: &str, volume: u8) -> Result<ApplicationOutput, String> {
    validate_application_id(application_id)?;
    let volume = normalize_volume(volume)?;
    #[cfg(target_os = "windows")]
    {
        windows_update(application_id, |session| unsafe {
            session.SetMasterVolume(volume, std::ptr::null())
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = volume;
        Err("アプリごとの音量調整はWindows版でのみ利用できます。".into())
    }
}

pub fn set_muted(application_id: &str, muted: bool) -> Result<ApplicationOutput, String> {
    validate_application_id(application_id)?;
    #[cfg(target_os = "windows")]
    {
        windows_update(application_id, |session| unsafe {
            session.SetMute(muted, std::ptr::null())
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = muted;
        Err("アプリごとの音量調整はWindows版でのみ利用できます。".into())
    }
}

fn validate_application_id(application_id: &str) -> Result<(), String> {
    if application_id.len() == 64
        && application_id
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err("操作対象のアプリIDが正しくありません。一覧を更新してから再度お試しください。".into())
    }
}

fn normalize_volume(volume: u8) -> Result<f32, String> {
    if volume > 100 {
        Err("アプリ音量は0から100の範囲で指定してください。".into())
    } else {
        Ok(f32::from(volume) / 100.0)
    }
}

#[cfg(target_os = "windows")]
struct WindowsSession {
    application_id: String,
    name: String,
    icon_path: Option<String>,
    volume: windows::Win32::Media::Audio::ISimpleAudioVolume,
}

#[cfg(target_os = "windows")]
fn windows_status() -> Result<ApplicationOutputStatus, String> {
    with_windows_sessions(|sessions| {
        Ok(ApplicationOutputStatus {
            supported: true,
            applications: aggregate_sessions(sessions)?,
            limitation: None,
        })
    })
}

#[cfg(target_os = "windows")]
fn windows_icon_for_application(
    application_id: &str,
) -> Result<Option<ApplicationOutputIcon>, String> {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };

    static ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<ApplicationOutputIcon>>>> =
        OnceLock::new();
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(icon) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(application_id)
        .cloned()
    {
        return Ok(icon);
    }

    let icon = with_windows_sessions(|sessions| {
        let path = sessions
            .iter()
            .find(|session| session.application_id == application_id)
            .and_then(|session| session.icon_path.as_deref());
        Ok(path.and_then(extract_windows_icon))
    })?;
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(application_id.to_owned(), icon.clone());
    Ok(icon)
}

#[cfg(target_os = "windows")]
fn windows_update(
    application_id: &str,
    update: impl Fn(&windows::Win32::Media::Audio::ISimpleAudioVolume) -> windows::core::Result<()>,
) -> Result<ApplicationOutput, String> {
    with_windows_sessions(|sessions| {
        let matching = sessions
            .iter()
            .filter(|session| session.application_id == application_id)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return Err("対象の音声セッションが終了しました。一覧を更新してください。".into());
        }
        for session in matching {
            update(&session.volume)
                .map_err(|error| format!("Windowsでアプリの音量を変更できませんでした: {error}"))?;
        }
        aggregate_sessions(sessions)?
            .into_iter()
            .find(|application| application.id == application_id)
            .ok_or_else(|| "変更後のアプリ音量を確認できませんでした。".into())
    })
}

#[cfg(target_os = "windows")]
fn aggregate_sessions(sessions: &[WindowsSession]) -> Result<Vec<ApplicationOutput>, String> {
    use std::collections::BTreeMap;

    struct Group {
        name: String,
        volume_total: f32,
        muted: bool,
        count: usize,
    }

    let mut groups = BTreeMap::<String, Group>::new();
    for session in sessions {
        let volume = unsafe { session.volume.GetMasterVolume() }
            .map_err(|error| format!("Windowsからアプリ音量を取得できませんでした: {error}"))?;
        let muted = unsafe { session.volume.GetMute() }
            .map_err(|error| {
                format!("Windowsからアプリのミュート状態を取得できませんでした: {error}")
            })?
            .as_bool();
        let group = groups
            .entry(session.application_id.clone())
            .or_insert_with(|| Group {
                name: session.name.clone(),
                volume_total: 0.0,
                muted: true,
                count: 0,
            });
        group.volume_total += volume.clamp(0.0, 1.0);
        group.muted &= muted;
        group.count += 1;
    }

    let mut applications = groups
        .into_iter()
        .map(|(id, group)| ApplicationOutput {
            id,
            name: group.name,
            volume: ((group.volume_total / group.count as f32) * 100.0).round() as u8,
            muted: group.muted,
            session_count: group.count,
        })
        .collect::<Vec<_>>();
    applications.sort_by_key(|application| {
        (
            application_sort_priority(&application.name),
            application.name.to_lowercase(),
        )
    });
    Ok(applications)
}

fn application_sort_priority(name: &str) -> u8 {
    let normalized = name.to_lowercase();
    const MEETING_APPLICATIONS: &[&str] = &[
        "zoom",
        "teams",
        "webex",
        "ciscocollabhost",
        "discord",
        "slack",
        "skype",
        "gotomeeting",
        "whereby",
        "around",
    ];
    const MEDIA_APPLICATIONS: &[&str] = &[
        "spotify",
        "itunes",
        "apple music",
        "applemusic",
        "amazon music",
        "amazonmusic",
        "tidal",
        "deezer",
        "foobar",
        "winamp",
        "musicbee",
        "vlc",
        "wmplayer",
        "media player",
        "mediaplayer",
        "groove",
        "audacious",
        "soundcloud",
        "youtube music",
        "youtubemusic",
    ];
    const AGGREGATE_OUTPUT_APPLICATIONS: &[&str] = &[
        "fxsound",
        "voicemeeter",
        "vb-audio",
        "vbcable",
        "steelseries sonar",
        "steelseriessonar",
        "nahimic",
        "soundid reference",
        "soundidreference",
        "boom 3d",
        "boom3d",
        "wave link",
        "wavelink",
        "nvidia broadcast",
        "krisp",
        "audiodg",
    ];

    if AGGREGATE_OUTPUT_APPLICATIONS
        .iter()
        .any(|candidate| normalized.contains(candidate))
    {
        4
    } else if MEETING_APPLICATIONS
        .iter()
        .any(|candidate| normalized.contains(candidate))
    {
        0
    } else if MEDIA_APPLICATIONS
        .iter()
        .any(|candidate| normalized.contains(candidate))
    {
        1
    } else if normalized == "システム サウンド" {
        3
    } else {
        2
    }
}

#[cfg(target_os = "windows")]
fn with_windows_sessions<T>(
    operation: impl FnOnce(&[WindowsSession]) -> Result<T, String>,
) -> Result<T, String> {
    use windows::{
        core::Interface,
        Win32::{
            Media::Audio::{
                eConsole, eRender, AudioSessionStateExpired, IAudioSessionControl2,
                IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
            },
        },
    };

    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok();
    let guard = match initialized {
        Ok(()) => ComGuard(true),
        Err(error) if error.code().0 as u32 == 0x8001_0106 => ComGuard(false),
        Err(error) => {
            return Err(format!(
                "Windowsの音声セッション制御を初期化できませんでした: {error}"
            ))
        }
    };

    let result = (|| -> Result<T, String> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(windows_audio_error)?;
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(windows_audio_error)?;
        let manager: IAudioSessionManager2 =
            unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(windows_audio_error)?;
        let session_enumerator =
            unsafe { manager.GetSessionEnumerator() }.map_err(windows_audio_error)?;
        let count = unsafe { session_enumerator.GetCount() }.map_err(windows_audio_error)?;
        let mut sessions = Vec::new();
        for index in 0..count {
            let control = match unsafe { session_enumerator.GetSession(index) } {
                Ok(control) => control,
                Err(_) => continue,
            };
            if unsafe { control.GetState() }.ok() == Some(AudioSessionStateExpired) {
                continue;
            }
            let control2: IAudioSessionControl2 = match control.cast() {
                Ok(control) => control,
                Err(_) => continue,
            };
            let volume: ISimpleAudioVolume = match control.cast() {
                Ok(volume) => volume,
                Err(_) => continue,
            };
            let process_id = match unsafe { control2.GetProcessId() } {
                Ok(process_id) => process_id,
                Err(_) => continue,
            };
            let (identity, name, icon_path) = application_identity(process_id, &control2);
            sessions.push(WindowsSession {
                application_id: hash_identity(&identity),
                name,
                icon_path,
                volume,
            });
        }
        operation(&sessions)
    })();
    drop(guard);
    result
}

#[cfg(target_os = "windows")]
fn application_identity(
    process_id: u32,
    control: &windows::Win32::Media::Audio::IAudioSessionControl2,
) -> (String, String, Option<String>) {
    if process_id == 0 {
        return (
            "windows-system-sounds".into(),
            "システム サウンド".into(),
            None,
        );
    }
    if let Some(path) = process_image_path(process_id) {
        let name = std::path::Path::new(&path)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("不明なアプリ")
            .to_owned();
        return (path.to_lowercase(), name, Some(path));
    }
    if let Some(display_name) = session_display_name(control) {
        return (
            format!("display:{display_name}:pid:{process_id}"),
            display_name,
            None,
        );
    }
    (
        format!("pid:{process_id}"),
        format!("アプリ ({process_id})"),
        None,
    )
}

#[cfg(target_os = "windows")]
fn extract_windows_icon(path: &str) -> Option<ApplicationOutputIcon> {
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::SIZE,
            Graphics::Gdi::{
                CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO,
                BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
            },
            UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY},
        },
    };

    const ICON_SIZE: i32 = 64;
    let path = HSTRING::from(path);
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(&path, None) }.ok()?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: ICON_SIZE,
                cy: ICON_SIZE,
            },
            SIIGBF_ICONONLY,
        )
    }
    .ok()?;

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        let _ = unsafe { DeleteObject(HGDIOBJ::from(bitmap)) };
        return None;
    }
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut source = vec![0_u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let copied = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            ICON_SIZE as u32,
            Some(source.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    let icon = (copied == ICON_SIZE).then(|| {
        let has_alpha = source.chunks_exact(4).any(|bgra| bgra[3] != 0);
        let mut pixels = Vec::with_capacity(source.len());
        for bgra in source.chunks_exact(4) {
            let alpha = if has_alpha {
                bgra[3]
            } else if bgra[0] != 0 || bgra[1] != 0 || bgra[2] != 0 {
                255
            } else {
                0
            };
            let unpremultiply = |channel: u8| {
                if has_alpha && alpha > 0 && alpha < 255 {
                    ((u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255)
                        as u8
                } else {
                    channel
                }
            };
            pixels.extend_from_slice(&[
                unpremultiply(bgra[2]),
                unpremultiply(bgra[1]),
                unpremultiply(bgra[0]),
                alpha,
            ]);
        }
        ApplicationOutputIcon {
            width: ICON_SIZE as u32,
            height: ICON_SIZE as u32,
            pixels,
        }
    });

    unsafe {
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
        let _ = DeleteDC(hdc);
    }
    icon
}

#[cfg(target_os = "windows")]
fn process_image_path(process_id: u32) -> Option<String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0_u16; 32_768];
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
    result.ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

#[cfg(target_os = "windows")]
fn session_display_name(
    control: &windows::Win32::Media::Audio::IAudioSessionControl2,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;

    let value = unsafe { control.GetDisplayName() }.ok()?;
    if value.is_null() {
        return None;
    }
    let display_name = unsafe { value.to_string() }
        .ok()
        .filter(|name| !name.trim().is_empty());
    unsafe { CoTaskMemFree(Some(value.as_ptr().cast())) };
    display_name
}

#[cfg(target_os = "windows")]
fn hash_identity(identity: &str) -> String {
    use sha2::{Digest, Sha256};

    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

#[cfg(target_os = "windows")]
fn windows_audio_error(error: windows::core::Error) -> String {
    format!(
        "Windowsの音声セッションを取得できませんでした。既定の出力デバイスを確認してください: {error}"
    )
}

#[cfg(test)]
mod tests {
    use super::{application_sort_priority, normalize_volume, validate_application_id};

    #[test]
    fn application_id_accepts_only_sha256_hex() {
        assert!(validate_application_id(&"a".repeat(64)).is_ok());
        assert!(validate_application_id(&"G".repeat(64)).is_err());
        assert!(validate_application_id("short").is_err());
    }

    #[test]
    fn application_volume_is_normalized_and_bounded() {
        assert_eq!(normalize_volume(0), Ok(0.0));
        assert_eq!(normalize_volume(50), Ok(0.5));
        assert_eq!(normalize_volume(100), Ok(1.0));
        assert!(normalize_volume(101).is_err());
    }

    #[test]
    fn meeting_and_media_applications_sort_before_other_sessions() {
        assert_eq!(application_sort_priority("ms-teams"), 0);
        assert_eq!(application_sort_priority("Zoom"), 0);
        assert_eq!(application_sort_priority("Spotify"), 1);
        assert_eq!(application_sort_priority("mutsuna-echo"), 2);
        assert_eq!(application_sort_priority("システム サウンド"), 3);
        assert_eq!(application_sort_priority("FxSound"), 4);
        assert_eq!(application_sort_priority("Voicemeeter Banana"), 4);
        assert_eq!(application_sort_priority("SteelSeriesSonar"), 4);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_rgba_icon_from_a_windows_executable() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .expect("COM initialization");
        let executable = std::env::current_exe().expect("test executable path");
        let icon = super::extract_windows_icon(
            executable
                .to_str()
                .expect("test executable path must be Unicode"),
        )
        .expect("Windows should provide an executable icon");
        assert_eq!((icon.width, icon.height), (64, 64));
        assert_eq!(icon.pixels.len(), 64 * 64 * 4);
        assert!(icon.pixels.chunks_exact(4).any(|rgba| rgba[3] != 0));
        unsafe { CoUninitialize() };
    }
}
