#[cfg(not(target_os = "android"))]
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

#[cfg(not(target_os = "android"))]
use tauri::http::{
    header::{
        ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE,
        CONTENT_TYPE,
    },
    Request, Response, StatusCode,
};
use tauri::AppHandle;

#[cfg(target_os = "android")]
mod android_native;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioPlaybackState {
    loaded: bool,
    playing: bool,
    position_ms: u64,
    duration_ms: u64,
    buffered_position_ms: u64,
    buffering: bool,
    ended: bool,
    error: Option<String>,
}

#[cfg(target_os = "android")]
impl From<android_native::NativePlaybackState> for AudioPlaybackState {
    fn from(state: android_native::NativePlaybackState) -> Self {
        Self {
            loaded: state.loaded,
            playing: state.playing,
            position_ms: state.position_ms,
            duration_ms: state.duration_ms,
            buffered_position_ms: state.buffered_position_ms,
            buffering: state.buffering,
            ended: state.ended,
            error: state.error,
        }
    }
}

#[tauri::command]
pub(crate) const fn get_audio_playback_backend() -> &'static str {
    if cfg!(target_os = "android") {
        "android-native"
    } else {
        "web"
    }
}

#[cfg(target_os = "android")]
fn native_result(
    result: Result<android_native::NativePlaybackState, String>,
) -> Result<AudioPlaybackState, String> {
    result.map(Into::into)
}

#[tauri::command]
pub(crate) fn load_selected_audio_for_playback(
    app: AppHandle,
    meeting_id: String,
) -> Result<AudioPlaybackState, String> {
    #[cfg(target_os = "android")]
    {
        let path =
            crate::commands::transcribe::selected_audio_path_for_playback(&app, &meeting_id)?;
        let path = path
            .to_str()
            .ok_or_else(|| "再生する音声ファイルの場所をAndroidで扱えません。".to_string())?;
        native_result(android_native::load(path))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, meeting_id);
        Err("ネイティブ音声再生はAndroid専用です。".into())
    }
}

macro_rules! native_playback_command {
    ($name:ident, $method:literal) => {
        #[tauri::command]
        pub(crate) fn $name() -> Result<AudioPlaybackState, String> {
            #[cfg(target_os = "android")]
            {
                native_result(android_native::simple($method))
            }
            #[cfg(not(target_os = "android"))]
            {
                Err("ネイティブ音声再生はAndroid専用です。".into())
            }
        }
    };
}

native_playback_command!(play_selected_audio, "play");
native_playback_command!(pause_selected_audio, "pause");
native_playback_command!(get_audio_playback_state, "getState");
native_playback_command!(release_audio_playback, "release");

#[tauri::command]
pub(crate) fn seek_selected_audio(position_ms: u64) -> Result<AudioPlaybackState, String> {
    #[cfg(target_os = "android")]
    {
        native_result(android_native::seek(position_ms))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = position_ms;
        Err("ネイティブ音声再生はAndroid専用です。".into())
    }
}

#[tauri::command]
pub(crate) fn set_audio_playback_volume(volume: f32) -> Result<AudioPlaybackState, String> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err("音量は0から1の範囲で指定してください。".into());
    }
    #[cfg(target_os = "android")]
    {
        native_result(android_native::set_float("setVolume", volume))
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("ネイティブ音声再生はAndroid専用です。".into())
    }
}

#[tauri::command]
pub(crate) fn set_audio_playback_rate(rate: f32) -> Result<AudioPlaybackState, String> {
    if !rate.is_finite() || !(0.25..=4.0).contains(&rate) {
        return Err("再生速度は0.25倍から4倍の範囲で指定してください。".into());
    }
    #[cfg(target_os = "android")]
    {
        native_result(android_native::set_float("setPlaybackRate", rate))
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("ネイティブ音声再生はAndroid専用です。".into())
    }
}

#[cfg(not(target_os = "android"))]
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[cfg(not(target_os = "android"))]
pub(crate) fn response(
    app: &AppHandle,
    webview_label: &str,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if webview_label != "main" {
        return empty_response(StatusCode::FORBIDDEN);
    }
    if request.method() != "GET" && request.method() != "HEAD" {
        return empty_response(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(meeting_id) = meeting_id_from_path(request.uri().path()) else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    let path = match crate::commands::transcribe::selected_audio_path_for_playback(app, meeting_id)
    {
        Ok(path) => path,
        Err(_) => return empty_response(StatusCode::FORBIDDEN),
    };
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty_response(StatusCode::NOT_FOUND)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return empty_response(StatusCode::FORBIDDEN)
        }
        Err(_) => return empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let length = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => return empty_response(StatusCode::NOT_FOUND),
    };
    if length == 0 {
        return empty_response(StatusCode::NO_CONTENT);
    }

    let range_header = request
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok());
    let (start, end) = match byte_range(range_header, length, MAX_RESPONSE_BYTES) {
        Some(range) => range,
        None => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(CONTENT_RANGE, format!("bytes */{length}"))
                .body(Vec::new())
                .expect("valid range response")
        }
    };
    let response_length = end - start + 1;
    // A 206 response is valid only when the client requested a range. Android
    // WebView commonly starts media loading with a plain GET and rejects an
    // unsolicited, truncated 206 response as PIPELINE_ERROR_READ.
    let partial = range_header.is_some();
    let body = if request.method() == "HEAD" {
        Vec::new()
    } else {
        if file.seek(SeekFrom::Start(start)).is_err() {
            return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let mut bytes = Vec::with_capacity(response_length as usize);
        if file.take(response_length).read_to_end(&mut bytes).is_err() {
            return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
        bytes
    };

    let mut builder = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(CONTENT_TYPE, content_type(&path))
        .header(CONTENT_LENGTH, response_length)
        .header(ACCEPT_RANGES, "bytes")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "no-store");
    if partial {
        builder = builder.header(CONTENT_RANGE, format!("bytes {start}-{end}/{length}"));
    }
    builder.body(body).expect("valid audio response")
}

#[cfg(not(target_os = "android"))]
fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(CACHE_CONTROL, "no-store")
        .body(Vec::new())
        .expect("valid empty response")
}

#[cfg(not(target_os = "android"))]
fn meeting_id_from_path(path: &str) -> Option<&str> {
    let meeting_id = path.strip_prefix("/selected/")?;
    if meeting_id.is_empty() || meeting_id.contains('/') {
        return None;
    }
    crate::meeting_store::validate_meeting_id(meeting_id)
        .is_ok()
        .then_some(meeting_id)
}

#[cfg(not(target_os = "android"))]
fn byte_range(header: Option<&str>, length: u64, maximum: u64) -> Option<(u64, u64)> {
    if length == 0 || maximum == 0 {
        return None;
    }
    let max_end = |start: u64, requested_end: u64| {
        requested_end
            .min(length - 1)
            .min(start.saturating_add(maximum - 1))
    };
    let Some(header) = header else {
        return Some((0, length - 1));
    };
    let value = header.strip_prefix("bytes=")?;
    if value.contains(',') {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(length).min(maximum);
        return (suffix > 0).then_some((length - suffix, length - 1));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= length {
        return None;
    }
    let requested_end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?
    };
    if requested_end < start {
        return None;
    }
    Some((start, max_end(start, requested_end)))
}

#[cfg(not(target_os = "android"))]
fn content_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{byte_range, meeting_id_from_path};

    #[test]
    fn accepts_only_selected_uuid_v7_routes() {
        let id = uuid::Uuid::now_v7().to_string();
        assert_eq!(
            meeting_id_from_path(&format!("/selected/{id}")),
            Some(id.as_str())
        );
        assert!(meeting_id_from_path("/selected/../secret").is_none());
        assert!(meeting_id_from_path("/other/audio").is_none());
    }

    #[test]
    fn parses_and_caps_audio_ranges() {
        assert_eq!(
            byte_range(Some("bytes=100-199"), 1_000, 1_024),
            Some((100, 199))
        );
        assert_eq!(
            byte_range(Some("bytes=100-"), 10_000, 1_000),
            Some((100, 1_099))
        );
        assert_eq!(
            byte_range(Some("bytes=-200"), 1_000, 1_024),
            Some((800, 999))
        );
        assert_eq!(byte_range(None, 10_000, 1_000), Some((0, 9_999)));
        assert!(byte_range(Some("bytes=1000-"), 1_000, 1_024).is_none());
        assert!(byte_range(Some("bytes=0-1,3-4"), 1_000, 1_024).is_none());
    }
}
