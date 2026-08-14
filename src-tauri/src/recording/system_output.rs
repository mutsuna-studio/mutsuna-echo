use super::types::SystemOutputStatus;

struct PlatformOutputStatus {
    volume: f32,
    muted: bool,
    mute_supported: bool,
}

pub fn status() -> Result<SystemOutputStatus, String> {
    match platform_status() {
        Ok(current) => Ok(public_status(current)),
        Err(OutputError::Unsupported(message)) => Ok(SystemOutputStatus {
            supported: false,
            volume: 0,
            muted: false,
            mute_supported: false,
            limitation: Some(message),
        }),
        Err(OutputError::Failure(message)) => Err(message),
    }
}

pub fn set_volume(volume: u8) -> Result<SystemOutputStatus, String> {
    let requested = normalize_volume(volume)?;
    platform_set_volume(requested).map_err(OutputError::into_message)?;
    let actual = platform_status().map_err(OutputError::into_message)?;
    verify_volume(requested, actual.volume)?;
    Ok(public_status(actual))
}

pub fn set_muted(muted: bool) -> Result<SystemOutputStatus, String> {
    platform_set_muted(muted).map_err(OutputError::into_message)?;
    let actual = platform_status().map_err(OutputError::into_message)?;
    if actual.muted != muted {
        return Err("OSへ出力ミュートの変更を要求しましたが、状態を確認できませんでした。".into());
    }
    Ok(public_status(actual))
}

fn public_status(current: PlatformOutputStatus) -> SystemOutputStatus {
    SystemOutputStatus {
        supported: true,
        volume: (current.volume.clamp(0.0, 1.0) * 100.0).round() as u8,
        muted: current.muted,
        mute_supported: current.mute_supported,
        limitation: (!current.mute_supported).then(|| {
            "この出力デバイスはミュート操作に対応していません。音量調整は利用できます。".into()
        }),
    }
}

fn normalize_volume(volume: u8) -> Result<f32, String> {
    if volume > 100 {
        Err("システム音量は0から100の範囲で指定してください。".into())
    } else {
        Ok(f32::from(volume) / 100.0)
    }
}

fn verify_volume(requested: f32, actual: f32) -> Result<(), String> {
    if (requested - actual).abs() <= 0.011 {
        Ok(())
    } else {
        Err("OSへシステム音量の変更を要求しましたが、状態を確認できませんでした。".into())
    }
}

#[derive(Debug)]
enum OutputError {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Unsupported(String),
    Failure(String),
}

impl OutputError {
    fn into_message(self) -> String {
        match self {
            Self::Unsupported(message) | Self::Failure(message) => message,
        }
    }
}

#[cfg(target_os = "windows")]
fn platform_status() -> Result<PlatformOutputStatus, OutputError> {
    with_windows_endpoint(|endpoint| unsafe {
        Ok(PlatformOutputStatus {
            volume: endpoint.GetMasterVolumeLevelScalar()?,
            muted: endpoint.GetMute()?.as_bool(),
            mute_supported: true,
        })
    })
}

#[cfg(target_os = "windows")]
fn platform_set_volume(volume: f32) -> Result<(), OutputError> {
    with_windows_endpoint(|endpoint| unsafe {
        endpoint.SetMasterVolumeLevelScalar(volume, std::ptr::null())
    })
}

#[cfg(target_os = "windows")]
fn platform_set_muted(muted: bool) -> Result<(), OutputError> {
    with_windows_endpoint(|endpoint| unsafe { endpoint.SetMute(muted, std::ptr::null()) })
}

#[cfg(target_os = "windows")]
fn with_windows_endpoint<T>(
    operation: impl FnOnce(
        &windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
    ) -> windows::core::Result<T>,
) -> Result<T, OutputError> {
    use windows::{
        core::Result as WindowsResult,
        Win32::{
            Media::Audio::{
                eConsole, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
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
        Err(error) if error.code().0 as u32 == 0x8001_0106 => ComGuard(false),
        Err(error) => {
            return Err(OutputError::Failure(format!(
                "Windowsの音声制御を初期化できませんでした: {error}"
            )))
        }
    };

    let result = (|| -> WindowsResult<T> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole)? };
        let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None)? };
        operation(&endpoint)
    })();
    drop(guard);
    result.map_err(|error| {
        OutputError::Failure(format!(
            "Windowsの既定出力を操作できませんでした。出力デバイスを確認してください: {error}"
        ))
    })
}

#[cfg(target_os = "macos")]
fn platform_status() -> Result<PlatformOutputStatus, OutputError> {
    let device = macos_default_output_device()?;
    let volume_addresses = macos_settable_addresses(device, VOLUME_SELECTOR)?;
    if volume_addresses.is_empty() {
        return Err(OutputError::Unsupported(
            "この出力デバイスはmacOSの音量操作に対応していません。".into(),
        ));
    }
    let volume = volume_addresses
        .iter()
        .map(|address| macos_read_f32(device, address))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<f32>()
        / volume_addresses.len() as f32;
    let mute_addresses = macos_settable_addresses(device, MUTE_SELECTOR)?;
    let muted = if mute_addresses.is_empty() {
        false
    } else {
        mute_addresses
            .iter()
            .map(|address| macos_read_u32(device, address))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .all(|value| value != 0)
    };
    Ok(PlatformOutputStatus {
        volume,
        muted,
        mute_supported: !mute_addresses.is_empty(),
    })
}

#[cfg(target_os = "macos")]
fn platform_set_volume(volume: f32) -> Result<(), OutputError> {
    let device = macos_default_output_device()?;
    let addresses = macos_settable_addresses(device, VOLUME_SELECTOR)?;
    if addresses.is_empty() {
        return Err(OutputError::Unsupported(
            "この出力デバイスはmacOSの音量操作に対応していません。".into(),
        ));
    }
    for address in addresses {
        macos_write(device, &address, &volume)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_set_muted(muted: bool) -> Result<(), OutputError> {
    let device = macos_default_output_device()?;
    let addresses = macos_settable_addresses(device, MUTE_SELECTOR)?;
    if addresses.is_empty() {
        return Err(OutputError::Unsupported(
            "この出力デバイスはmacOSのミュート操作に対応していません。".into(),
        ));
    }
    let value = u32::from(muted);
    for address in addresses {
        macos_write(device, &address, &value)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const VOLUME_SELECTOR: u32 = u32::from_be_bytes(*b"volm");
#[cfg(target_os = "macos")]
const MUTE_SELECTOR: u32 = u32::from_be_bytes(*b"mute");

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
fn macos_default_output_device() -> Result<u32, OutputError> {
    let address = AudioObjectPropertyAddress {
        selector: u32::from_be_bytes(*b"dOut"),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    let mut device = 0_u32;
    macos_read(1, &address, &mut device)?;
    if device == 0 {
        Err(OutputError::Failure(
            "macOSの既定出力が設定されていません。".into(),
        ))
    } else {
        Ok(device)
    }
}

#[cfg(target_os = "macos")]
fn macos_settable_addresses(
    device: u32,
    selector: u32,
) -> Result<Vec<AudioObjectPropertyAddress>, OutputError> {
    let make = |element| AudioObjectPropertyAddress {
        selector,
        scope: u32::from_be_bytes(*b"outp"),
        element,
    };
    let master = make(0);
    if macos_is_settable(device, &master)? {
        return Ok(vec![master]);
    }
    let mut addresses = Vec::new();
    for element in 1..=8 {
        let address = make(element);
        if macos_is_settable(device, &address)? {
            addresses.push(address);
        }
    }
    Ok(addresses)
}

#[cfg(target_os = "macos")]
fn macos_is_settable(
    device: u32,
    address: &AudioObjectPropertyAddress,
) -> Result<bool, OutputError> {
    if unsafe { AudioObjectHasProperty(device, address) } == 0 {
        return Ok(false);
    }
    let mut settable = 0_u8;
    let status = unsafe { AudioObjectIsPropertySettable(device, address, &mut settable) };
    macos_result(status, "macOSで出力の操作可否を確認できませんでした")?;
    Ok(settable != 0)
}

#[cfg(target_os = "macos")]
fn macos_read_f32(device: u32, address: &AudioObjectPropertyAddress) -> Result<f32, OutputError> {
    let mut value = 0.0_f32;
    macos_read(device, address, &mut value)?;
    Ok(value)
}

#[cfg(target_os = "macos")]
fn macos_read_u32(device: u32, address: &AudioObjectPropertyAddress) -> Result<u32, OutputError> {
    let mut value = 0_u32;
    macos_read(device, address, &mut value)?;
    Ok(value)
}

#[cfg(target_os = "macos")]
fn macos_read<T>(
    device: u32,
    address: &AudioObjectPropertyAddress,
    value: &mut T,
) -> Result<(), OutputError> {
    let mut size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            address,
            0,
            std::ptr::null(),
            &mut size,
            (value as *mut T).cast(),
        )
    };
    macos_result(status, "macOSから出力状態を取得できませんでした")
}

#[cfg(target_os = "macos")]
fn macos_write<T>(
    device: u32,
    address: &AudioObjectPropertyAddress,
    value: &T,
) -> Result<(), OutputError> {
    let status = unsafe {
        AudioObjectSetPropertyData(
            device,
            address,
            0,
            std::ptr::null(),
            std::mem::size_of::<T>() as u32,
            (value as *const T).cast(),
        )
    };
    macos_result(status, "macOSで出力状態を変更できませんでした")
}

#[cfg(target_os = "macos")]
fn macos_result(status: i32, context: &str) -> Result<(), OutputError> {
    if status == 0 {
        Ok(())
    } else {
        Err(OutputError::Failure(format!(
            "{context}（OSStatus: {status}）"
        )))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_status() -> Result<PlatformOutputStatus, OutputError> {
    Err(OutputError::Unsupported(
        "このOSではシステム音量の操作を利用できません。".into(),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_set_volume(_volume: f32) -> Result<(), OutputError> {
    Err(OutputError::Unsupported(
        "このOSではシステム音量の操作を利用できません。".into(),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_set_muted(_muted: bool) -> Result<(), OutputError> {
    Err(OutputError::Unsupported(
        "このOSでは出力ミュートの操作を利用できません。".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{normalize_volume, verify_volume};

    #[test]
    fn volume_percent_is_normalized_and_bounded() {
        assert_eq!(normalize_volume(0), Ok(0.0));
        assert_eq!(normalize_volume(50), Ok(0.5));
        assert_eq!(normalize_volume(100), Ok(1.0));
        assert!(normalize_volume(101).is_err());
    }

    #[test]
    fn volume_verification_allows_device_rounding_only() {
        assert!(verify_volume(0.5, 0.505).is_ok());
        assert!(verify_volume(0.5, 0.52).is_err());
    }
}
