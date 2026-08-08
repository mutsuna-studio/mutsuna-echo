use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

use tauri::{
    http::{
        header::{
            ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH,
            CONTENT_RANGE, CONTENT_TYPE,
        },
        Request, Response, StatusCode,
    },
    AppHandle,
};

const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

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
    let partial = start != 0 || end + 1 != length;
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

fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(CACHE_CONTROL, "no-store")
        .body(Vec::new())
        .expect("valid empty response")
}

fn meeting_id_from_path(path: &str) -> Option<&str> {
    let meeting_id = path.strip_prefix("/selected/")?;
    if meeting_id.is_empty() || meeting_id.contains('/') {
        return None;
    }
    crate::meeting_store::validate_meeting_id(meeting_id)
        .is_ok()
        .then_some(meeting_id)
}

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
        return Some((0, max_end(0, length - 1)));
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
        assert_eq!(byte_range(None, 10_000, 1_000), Some((0, 999)));
        assert!(byte_range(Some("bytes=1000-"), 1_000, 1_024).is_none());
        assert!(byte_range(Some("bytes=0-1,3-4"), 1_000, 1_024).is_none());
    }
}
