use std::collections::VecDeque;

use super::{
    platform::{mix_with_limiter_into, M4aWriter},
    types::StartRecordingRequest,
};
use crate::audio_enhancement::StreamingAudioEnhancer;

const CHUNK_NS: i64 = 20_000_000;

pub(super) fn drain_mix(
    writer: &mut M4aWriter,
    enhancer: &mut StreamingAudioEnhancer,
    microphone: &mut VecDeque<(i64, Vec<f32>)>,
    system: &mut VecDeque<(i64, Vec<f32>)>,
    mix_buffer: &mut Vec<f32>,
    request: &StartRecordingRequest,
    flush: bool,
    mut on_mixed: impl FnMut(&[f32]),
) -> Result<(), String> {
    loop {
        if !request.microphone {
            let Some((_, samples)) = system.pop_front() else {
                break;
            };
            write_enhanced(writer, enhancer, &samples, &mut on_mixed)?;
            continue;
        }
        if !request.system_audio {
            let Some((_, samples)) = microphone.pop_front() else {
                break;
            };
            write_enhanced(writer, enhancer, &samples, &mut on_mixed)?;
            continue;
        }

        match (microphone.front(), system.front()) {
            (Some((mic_pts, _)), Some((sys_pts, _))) if (mic_pts - sys_pts).abs() < CHUNK_NS => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                let (_, sys) = system.pop_front().expect("front checked");
                mix_with_limiter_into(&mic, &sys, mix_buffer);
                write_enhanced(writer, enhancer, mix_buffer, &mut on_mixed)?;
            }
            (Some((mic_pts, _)), Some((sys_pts, _))) if mic_pts < sys_pts => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                write_enhanced(writer, enhancer, &mic, &mut on_mixed)?;
            }
            (Some(_), Some(_)) => {
                let (_, sys) = system.pop_front().expect("front checked");
                write_enhanced(writer, enhancer, &sys, &mut on_mixed)?;
            }
            (Some(_), None) if flush || microphone.len() > 3 => {
                let (_, mic) = microphone.pop_front().expect("front checked");
                write_enhanced(writer, enhancer, &mic, &mut on_mixed)?;
            }
            (None, Some(_)) if flush || system.len() > 3 => {
                let (_, sys) = system.pop_front().expect("front checked");
                write_enhanced(writer, enhancer, &sys, &mut on_mixed)?;
            }
            _ => break,
        }
    }
    Ok(())
}

fn write_enhanced(
    writer: &mut M4aWriter,
    enhancer: &mut StreamingAudioEnhancer,
    samples: &[f32],
    on_mixed: &mut impl FnMut(&[f32]),
) -> Result<(), String> {
    let enhanced = enhancer.accept(samples)?;
    if !enhanced.is_empty() {
        on_mixed(&enhanced);
        writer.write(&enhanced)?;
    }
    Ok(())
}
