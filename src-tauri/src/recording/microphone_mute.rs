use super::types::MicrophoneMuteStatus;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const UNSUPPORTED_MESSAGE: &str = "このOSではマイクのミュート操作を利用できません。";

pub fn status() -> Result<MicrophoneMuteStatus, String> {
    match platform_status() {
        Ok(muted) => Ok(MicrophoneMuteStatus {
            supported: true,
            muted,
            limitation: None,
        }),
        Err(PlatformMuteError::Unsupported(message)) => Ok(MicrophoneMuteStatus {
            supported: false,
            muted: false,
            limitation: Some(message),
        }),
        Err(PlatformMuteError::Failure(message)) => Err(message),
    }
}

pub fn set_muted(muted: bool) -> Result<MicrophoneMuteStatus, String> {
    platform_set_muted(muted).map_err(PlatformMuteError::into_message)?;
    let actual = platform_status().map_err(PlatformMuteError::into_message)?;
    verify_mute_transition(muted, actual)?;
    Ok(MicrophoneMuteStatus {
        supported: true,
        muted: actual,
        limitation: None,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum PlatformMuteError {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Unsupported(String),
    Failure(String),
}

impl PlatformMuteError {
    fn into_message(self) -> String {
        match self {
            Self::Unsupported(message) | Self::Failure(message) => message,
        }
    }
}

fn verify_mute_transition(requested: bool, actual: bool) -> Result<(), String> {
    if requested == actual {
        Ok(())
    } else {
        Err(if requested {
            "OSへマイクのミュートを要求しましたが、状態を確認できませんでした。"
        } else {
            "OSへマイクのミュート解除を要求しましたが、状態を確認できませんでした。"
        }
        .into())
    }
}

#[cfg(target_os = "windows")]
fn platform_status() -> Result<bool, PlatformMuteError> {
    with_windows_endpoint(|endpoint| unsafe { endpoint.GetMute().map(|value| value.as_bool()) })
}

#[cfg(target_os = "windows")]
fn platform_set_muted(muted: bool) -> Result<(), PlatformMuteError> {
    with_windows_endpoint(|endpoint| unsafe { endpoint.SetMute(muted, std::ptr::null()) })
}

#[cfg(target_os = "windows")]
fn with_windows_endpoint<T>(
    operation: impl FnOnce(
        &windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
    ) -> windows::core::Result<T>,
) -> Result<T, PlatformMuteError> {
    use windows::{
        core::Result as WindowsResult,
        Win32::{
            Media::Audio::{
                eCapture, eConsole, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                MMDeviceEnumerator,
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
        // RPC_E_CHANGED_MODE means this thread already has another COM apartment.
        Err(error) if error.code().0 as u32 == 0x8001_0106 => ComGuard(false),
        Err(error) => {
            return Err(PlatformMuteError::Failure(format!(
                "Windowsの音声制御を初期化できませんでした: {error}"
            )))
        }
    };

    let result = (|| -> WindowsResult<T> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)? };
        let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None)? };
        operation(&endpoint)
    })();
    drop(guard);
    result.map_err(|error| {
        PlatformMuteError::Failure(format!(
            "Windowsの既定マイクを操作できませんでした。入力デバイスを確認してください: {error}"
        ))
    })
}

#[cfg(target_os = "macos")]
fn platform_status() -> Result<bool, PlatformMuteError> {
    let device = macos_default_input_device()?;
    let address = macos_mute_property();
    if !macos_property_is_settable(device, &address)? {
        return Err(PlatformMuteError::Unsupported(
            "このマイクはmacOSのデバイスミュート操作に対応していません。".into(),
        ));
    }
    let mut value = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut u32).cast(),
        )
    };
    macos_result(
        status,
        "macOSからマイクのミュート状態を取得できませんでした",
    )?;
    Ok(value != 0)
}

#[cfg(target_os = "macos")]
fn platform_set_muted(muted: bool) -> Result<(), PlatformMuteError> {
    let device = macos_default_input_device()?;
    let address = macos_mute_property();
    if !macos_property_is_settable(device, &address)? {
        return Err(PlatformMuteError::Unsupported(
            "このマイクはmacOSのデバイスミュート操作に対応していません。".into(),
        ));
    }
    let value = u32::from(muted);
    let status = unsafe {
        AudioObjectSetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<u32>() as u32,
            (&value as *const u32).cast(),
        )
    };
    macos_result(status, "macOSでマイクのミュート状態を変更できませんでした")
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(object_id: u32, address: *const AudioObjectPropertyAddress) -> u8;
    fn AudioObjectIsPropertySettable(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        settable: *mut u8,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const std::ffi::c_void,
        data_size: *mut u32,
        data: *mut std::ffi::c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const std::ffi::c_void,
        data_size: u32,
        data: *const std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "macos")]
fn macos_default_input_device() -> Result<u32, PlatformMuteError> {
    const SYSTEM_OBJECT: u32 = 1;
    const DEFAULT_INPUT_DEVICE: u32 = u32::from_be_bytes(*b"dIn ");
    const GLOBAL_SCOPE: u32 = u32::from_be_bytes(*b"glob");
    let address = AudioObjectPropertyAddress {
        selector: DEFAULT_INPUT_DEVICE,
        scope: GLOBAL_SCOPE,
        element: 0,
    };
    let mut device = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut device as *mut u32).cast(),
        )
    };
    macos_result(status, "macOSの既定マイクを取得できませんでした")?;
    if device == 0 {
        return Err(PlatformMuteError::Failure(
            "macOSの既定マイクが設定されていません。".into(),
        ));
    }
    Ok(device)
}

#[cfg(target_os = "macos")]
fn macos_mute_property() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        selector: u32::from_be_bytes(*b"mute"),
        scope: u32::from_be_bytes(*b"inpt"),
        element: 0,
    }
}

#[cfg(target_os = "macos")]
fn macos_property_is_settable(
    device: u32,
    address: &AudioObjectPropertyAddress,
) -> Result<bool, PlatformMuteError> {
    if unsafe { AudioObjectHasProperty(device, address) } == 0 {
        return Ok(false);
    }
    let mut settable = 0_u8;
    let status = unsafe { AudioObjectIsPropertySettable(device, address, &mut settable) };
    macos_result(status, "macOSでマイクの操作可否を確認できませんでした")?;
    Ok(settable != 0)
}

#[cfg(target_os = "macos")]
fn macos_result(status: i32, context: &str) -> Result<(), PlatformMuteError> {
    if status == 0 {
        Ok(())
    } else {
        Err(PlatformMuteError::Failure(format!(
            "{context}（OSStatus: {status}）"
        )))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_status() -> Result<bool, PlatformMuteError> {
    Err(PlatformMuteError::Unsupported(UNSUPPORTED_MESSAGE.into()))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_set_muted(_muted: bool) -> Result<(), PlatformMuteError> {
    Err(PlatformMuteError::Unsupported(UNSUPPORTED_MESSAGE.into()))
}

#[cfg(test)]
mod tests {
    use super::verify_mute_transition;

    #[test]
    fn accepts_a_verified_mute_and_unmute_transition() {
        assert!(verify_mute_transition(true, true).is_ok());
        assert!(verify_mute_transition(false, false).is_ok());
    }

    #[test]
    fn rejects_a_transition_when_the_os_state_did_not_change() {
        assert!(verify_mute_transition(true, false).is_err());
        assert!(verify_mute_transition(false, true).is_err());
    }
}
