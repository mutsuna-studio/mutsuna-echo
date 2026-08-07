use std::collections::VecDeque;

use super::{
    platform::{mix_with_limiter, M4aWriter},
    types::StartRecordingRequest,
};

const CHUNK_NS: i64 = 20_000_000;

pub(super) fn drain_mix(
    writer: &mut M4aWriter,
    microphone: &mut VecDeque<(i64, Vec<f32>)>,
    system: &mut VecDeque<(i64, Vec<f32>)>,
    request: &StartRecordingRequest,
    flush: bool,
) -> Result<(), String> {
    loop {
        if !request.microphone {
            let Some((_, samples)) = system.pop_front() else {
                break;
            };
            writer.write(&samples)?;
            continue;
        }
        if !request.system_audio {
            let Some((_, samples)) = microphone.pop_front() else {
                break;
            };
            writer.write(&samples)?;
            continue;
        }

        match (microphone.front(), system.front()) {
            (Some((mic_pts, _)), Some((sys_pts, _))) if (mic_pts - sys_pts).abs() < CHUNK_NS => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&mix_with_limiter(&mic, &sys))?;
            }
            (Some((mic_pts, _)), Some((sys_pts, _))) if mic_pts < sys_pts => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                writer.write(&mic)?;
            }
            (Some(_), Some(_)) => {
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&sys)?;
            }
            (Some(_), None) if flush || microphone.len() > 3 => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                writer.write(&mic)?;
            }
            (None, Some(_)) if flush || system.len() > 3 => {
                let (_, sys) = system.pop_front().expect("front checked");
                writer.write(&sys)?;
            }
            _ => break,
        }
    }
    Ok(())
}
