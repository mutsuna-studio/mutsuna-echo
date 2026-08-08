use std::{fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

pub(crate) fn decode_mono(
    path: &Path,
    mut on_samples: impl FnMut(u32, &[f32]) -> Result<(), String>,
) -> Result<u64, String> {
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
        let mono: Vec<f32> = decoded_samples
            .samples()
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        frames = frames.saturating_add(mono.len() as u64);
        on_samples(sample_rate, &mono)?;
    }
    if sample_rate == 0 || frames == 0 {
        return Err("音声データが含まれていません。".into());
    }
    Ok(frames.saturating_mul(1_000) / sample_rate as u64)
}
