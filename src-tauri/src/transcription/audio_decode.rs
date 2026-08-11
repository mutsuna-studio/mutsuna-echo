use std::{collections::VecDeque, fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

pub(crate) fn decode_mono(
    path: &Path,
    mut on_samples: impl FnMut(u32, &[f32]) -> Result<(), String>,
) -> Result<u64, String> {
    decode_mono_sampled(path, 1, |sample_rate, _, samples| {
        on_samples(sample_rate, samples)
    })
}

pub(crate) fn decode_mono_sampled(
    path: &Path,
    stride: usize,
    mut on_samples: impl FnMut(u32, u64, &[f32]) -> Result<(), String>,
) -> Result<u64, String> {
    if stride == 0 {
        return Err("音声のサンプリング間隔が不正です。".into());
    }
    let file =
        File::open(path).map_err(|error| format!("音声ファイルを開けませんでした: {error}"))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("音声形式を読み取れませんでした: {error}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "音声トラックが見つかりません。".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("音声デコーダーを準備できませんでした: {error}"))?;
    let mut sample_rate = 0u32;
    let mut frames = 0u64;
    let mut mono = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(format!("音声を読み込めませんでした: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(format!("音声をデコードできませんでした: {error}")),
        };
        let spec = *decoded.spec();
        if sample_rate == 0 {
            sample_rate = spec.rate;
        }
        if spec.rate != sample_rate {
            return Err("途中でサンプルレートが変わる音声には対応していません。".into());
        }
        let channels = spec.channels.count();
        if channels == 0 {
            continue;
        }
        let mut decoded_samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        decoded_samples.copy_interleaved_ref(decoded);
        mono.clear();
        mono.reserve(decoded_samples.samples().len() / channels);
        let packet_frame_offset = frames;
        let packet_frames = decoded_samples.samples().len() / channels;
        mono.extend(
            decoded_samples
                .samples()
                .chunks_exact(channels)
                .step_by(stride)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        frames = frames.saturating_add(packet_frames as u64);
        on_samples(sample_rate, packet_frame_offset, &mono)?;
    }
    if sample_rate == 0 || frames == 0 {
        return Err("音声データが含まれていません。".into());
    }
    Ok(frames.saturating_mul(1_000) / sample_rate as u64)
}

/// Decodes only short windows around the requested positions. Unlike
/// `decode_mono_sampled`, this does not walk through the compressed stream from
/// beginning to end, so its cost is bounded by the number and size of windows.
pub(crate) fn decode_mono_windows(
    path: &Path,
    windows: &[(u64, u64)],
    mut on_samples: impl FnMut(usize, u32, &[f32]) -> Result<(), String>,
) -> Result<(), String> {
    if windows.is_empty() {
        return Ok(());
    }
    let file =
        File::open(path).map_err(|error| format!("音声ファイルを開けませんでした: {error}"))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let format_options = FormatOptions {
        seek_index_fill_rate: 5,
        ..FormatOptions::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, source, &format_options, &MetadataOptions::default())
        .map_err(|error| format!("音声形式を読み取れませんでした: {error}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "音声トラックが見つかりません。".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|error| format!("音声デコーダーを準備できませんでした: {error}"))?;

    for (window_index, (position_ms, window_ms)) in windows.iter().copied().enumerate() {
        let time = Time::new(position_ms / 1_000, (position_ms % 1_000) as f64 / 1_000.0);
        format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time,
                    track_id: Some(track_id),
                },
            )
            .map_err(|error| format!("音声の読み取り位置を移動できませんでした: {error}"))?;
        decoder.reset();
        let mut decoded_frames = 0u64;
        let mut target_frames = None;
        let mut window_sample_rate = 0u32;
        let mut window_mono = Vec::new();

        while target_frames.is_none_or(|target| decoded_frames < target) {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(error) => return Err(format!("音声を読み込めませんでした: {error}")),
            };
            if packet.track_id() != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(error) => return Err(format!("音声をデコードできませんでした: {error}")),
            };
            let spec = *decoded.spec();
            if window_sample_rate == 0 {
                window_sample_rate = spec.rate;
            } else if window_sample_rate != spec.rate {
                return Err("途中でサンプルレートが変わる音声には対応していません。".into());
            }
            let channels = spec.channels.count();
            if channels == 0 {
                continue;
            }
            let requested_frames = window_ms
                .saturating_mul(spec.rate as u64)
                .saturating_add(999)
                / 1_000;
            let target = *target_frames.get_or_insert(requested_frames.max(1));
            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            samples.copy_interleaved_ref(decoded);
            let remaining = target.saturating_sub(decoded_frames) as usize;
            let frames = samples.samples().len() / channels;
            let accepted_frames = frames.min(remaining);
            window_mono.extend(
                samples
                    .samples()
                    .chunks_exact(channels)
                    .take(accepted_frames)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32),
            );
            decoded_frames = decoded_frames.saturating_add(accepted_frames as u64);
        }
        if window_sample_rate == 0 || window_mono.is_empty() {
            return Err("指定した音声区間を読み取れませんでした。".into());
        }
        on_samples(window_index, window_sample_rate, &window_mono)?;
    }
    Ok(())
}

/// Decodes sorted time windows in one sequential pass. Only currently active
/// windows are retained, so memory is bounded by the largest region even when
/// processing a long recording with hundreds of VAD segments.
pub(crate) fn decode_mono_regions(
    path: &Path,
    windows: &[(u64, u64)],
    mut on_region: impl FnMut(usize, u32, &[f32]) -> Result<(), String>,
) -> Result<(), String> {
    if windows.is_empty() {
        return Ok(());
    }
    if windows.windows(2).any(|pair| pair[0].0 > pair[1].0) {
        return Err("音声区間が時系列順に並んでいません。".into());
    }

    struct ActiveRegion {
        index: usize,
        start_frame: u64,
        end_frame: u64,
        samples: Vec<f32>,
    }

    let mut next_window = 0usize;
    let mut active = VecDeque::<ActiveRegion>::new();
    let mut completed = 0usize;
    let mut source_rate = 0u32;
    decode_mono_sampled(path, 1, |sample_rate, packet_start, samples| {
        source_rate = sample_rate;
        let packet_end = packet_start.saturating_add(samples.len() as u64);
        while let Some(&(start_ms, duration_ms)) = windows.get(next_window) {
            let start_frame = start_ms.saturating_mul(sample_rate as u64) / 1_000;
            if start_frame >= packet_end {
                break;
            }
            let end_ms = start_ms.saturating_add(duration_ms);
            let end_frame = end_ms
                .saturating_mul(sample_rate as u64)
                .saturating_add(999)
                / 1_000;
            active.push_back(ActiveRegion {
                index: next_window,
                start_frame,
                end_frame: end_frame.max(start_frame.saturating_add(1)),
                samples: Vec::new(),
            });
            next_window += 1;
        }

        for region in &mut active {
            let overlap_start = packet_start.max(region.start_frame);
            let overlap_end = packet_end.min(region.end_frame);
            if overlap_start >= overlap_end {
                continue;
            }
            let start = usize::try_from(overlap_start - packet_start)
                .map_err(|_| "音声区間の開始位置が大きすぎます。".to_string())?;
            let end = usize::try_from(overlap_end - packet_start)
                .map_err(|_| "音声区間の終了位置が大きすぎます。".to_string())?;
            region.samples.extend_from_slice(&samples[start..end]);
        }

        while active
            .front()
            .is_some_and(|region| region.end_frame <= packet_end)
        {
            let region = active.pop_front().expect("front was checked");
            if region.samples.is_empty() {
                return Err("VADが検出した音声区間を読み取れませんでした。".into());
            }
            on_region(region.index, sample_rate, &region.samples)?;
            completed += 1;
        }
        Ok(())
    })?;

    while let Some(region) = active.pop_front() {
        if region.samples.is_empty() {
            return Err("VADが検出した末尾の音声区間を読み取れませんでした。".into());
        }
        on_region(region.index, source_rate, &region.samples)?;
        completed += 1;
    }
    if next_window != windows.len() || completed != windows.len() {
        return Err("VADが検出した音声区間をすべて読み取れませんでした。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{decode_mono_regions, decode_mono_windows};

    #[test]
    fn decodes_only_requested_wav_windows() {
        let path = std::env::temp_dir().join(format!("waveform-seek-{}.wav", uuid::Uuid::now_v7()));
        let sample_rate = 8_000u32;
        let frames = sample_rate * 2;
        let data_bytes = frames * 2;
        let mut file = std::fs::File::create(&path).expect("create wav");
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&sample_rate.to_le_bytes()).unwrap();
        file.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for frame in 0..frames {
            let sample = if frame < sample_rate {
                1_000i16
            } else {
                12_000i16
            };
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
        drop(file);

        let mut maxima = [0.0f32; 2];
        decode_mono_windows(&path, &[(100, 100), (1_100, 100)], |index, _, samples| {
            maxima[index] = samples
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0, f32::max);
            Ok(())
        })
        .expect("decode windows");
        assert!(maxima[0] > 0.02 && maxima[0] < 0.1);
        assert!(maxima[1] > 0.3);

        let mut region_lengths = [0usize; 2];
        decode_mono_regions(&path, &[(100, 200), (150, 200)], |index, rate, samples| {
            assert_eq!(rate, sample_rate);
            region_lengths[index] = samples.len();
            Ok(())
        })
        .expect("decode overlapping regions");
        assert_eq!(region_lengths, [1_600, 1_600]);

        let _ = std::fs::remove_file(path);
    }
}
