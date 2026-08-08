use std::path::Path;

#[cfg(target_os = "windows")]
pub struct M4aWriter {
    sink_writer: windows::Win32::Media::MediaFoundation::IMFSinkWriter,
    stream_index: u32,
    samples_written: u64,
    pcm_buffer: Vec<u8>,
    _media_foundation: MediaFoundation,
}

#[cfg(target_os = "windows")]
struct MediaFoundation;

#[cfg(target_os = "windows")]
impl MediaFoundation {
    fn initialize() -> Result<Self, String> {
        unsafe {
            windows::Win32::Media::MediaFoundation::MFStartup(
                windows::Win32::Media::MediaFoundation::MF_VERSION,
                windows::Win32::Media::MediaFoundation::MFSTARTUP_FULL,
            )
        }
        .map_err(windows_encoder_error)?;
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for MediaFoundation {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Media::MediaFoundation::MFShutdown() };
    }
}

#[cfg(target_os = "windows")]
impl M4aWriter {
    pub fn create(path: &Path, bitrate: u32, fragment_seconds: f64) -> Result<Self, String> {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::{Interface, PCWSTR},
            Win32::Media::MediaFoundation::*,
        };

        let media_foundation = MediaFoundation::initialize()?;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let output_type = MFCreateMediaType().map_err(windows_encoder_error)?;
            configure_audio_type(&output_type, MFAudioFormat_AAC, bitrate / 8, 1, 16)?;
            output_type
                .SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)
                .map_err(windows_encoder_error)?;

            let stream = MFCreateFile(
                MF_ACCESSMODE_WRITE,
                MF_OPENMODE_DELETE_IF_EXIST,
                MF_FILEFLAGS_NONE,
                PCWSTR(path.as_ptr()),
            )
            .map_err(windows_encoder_error)?;
            let sink = MFCreateFMPEG4MediaSink(&stream, None, &output_type)
                .map_err(windows_encoder_error)?;
            let sink_attributes: IMFAttributes = sink.cast().map_err(windows_encoder_error)?;
            sink_attributes
                .SetUINT64(
                    &MF_MPEG4SINK_MIN_FRAGMENT_DURATION,
                    (fragment_seconds * 10_000_000.0) as u64,
                )
                .map_err(windows_encoder_error)?;
            let writer =
                MFCreateSinkWriterFromMediaSink(&sink, None).map_err(windows_encoder_error)?;

            let input_type = MFCreateMediaType().map_err(windows_encoder_error)?;
            configure_audio_type(&input_type, MFAudioFormat_PCM, 48_000 * 2, 2, 16)?;
            writer
                .SetInputMediaType(0, &input_type, None)
                .map_err(windows_encoder_error)?;
            writer.BeginWriting().map_err(windows_encoder_error)?;
            Ok(Self {
                sink_writer: writer,
                stream_index: 0,
                samples_written: 0,
                pcm_buffer: Vec::new(),
                _media_foundation: media_foundation,
            })
        }
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::{MFCreateMemoryBuffer, MFCreateSample};
        samples_to_i16_bytes(samples, &mut self.pcm_buffer);
        unsafe {
            let buffer = MFCreateMemoryBuffer(self.pcm_buffer.len() as u32)
                .map_err(windows_encoder_error)?;
            let mut destination = std::ptr::null_mut();
            buffer
                .Lock(&mut destination, None, None)
                .map_err(windows_encoder_error)?;
            std::ptr::copy_nonoverlapping(
                self.pcm_buffer.as_ptr(),
                destination,
                self.pcm_buffer.len(),
            );
            buffer.Unlock().map_err(windows_encoder_error)?;
            buffer
                .SetCurrentLength(self.pcm_buffer.len() as u32)
                .map_err(windows_encoder_error)?;
            let sample = MFCreateSample().map_err(windows_encoder_error)?;
            sample.AddBuffer(&buffer).map_err(windows_encoder_error)?;
            sample
                .SetSampleTime((self.samples_written as i128 * 10_000_000 / 48_000) as i64)
                .map_err(windows_encoder_error)?;
            sample
                .SetSampleDuration((samples.len() as i128 * 10_000_000 / 48_000) as i64)
                .map_err(windows_encoder_error)?;
            self.sink_writer
                .WriteSample(self.stream_index, &sample)
                .map_err(windows_encoder_error)?;
        }
        self.samples_written = self.samples_written.saturating_add(samples.len() as u64);
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        // Media Foundation buffers complete AAC access units. Finalizing a
        // newly opened or extremely short stream otherwise returns
        // MF_E_SINK_NO_SAMPLES_PROCESSED (0xC00D4A44).
        const MIN_FINALIZE_FRAMES: u64 = 2_048;
        let silence = [0.0; 960];
        while self.samples_written < MIN_FINALIZE_FRAMES {
            let frames = (MIN_FINALIZE_FRAMES - self.samples_written).min(960) as usize;
            self.write(&silence[..frames])?;
        }
        unsafe { self.sink_writer.Finalize().map_err(windows_encoder_error) }
    }
}

#[cfg(target_os = "windows")]
unsafe fn configure_audio_type(
    media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    subtype: windows::core::GUID,
    average_bytes_per_second: u32,
    block_alignment: u32,
    bits_per_sample: u32,
) -> Result<(), String> {
    use windows::Win32::Media::MediaFoundation::*;
    media_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
        .map_err(windows_encoder_error)?;
    media_type
        .SetGUID(&MF_MT_SUBTYPE, &subtype)
        .map_err(windows_encoder_error)?;
    media_type
        .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)
        .map_err(windows_encoder_error)?;
    media_type
        .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)
        .map_err(windows_encoder_error)?;
    media_type
        .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, average_bytes_per_second)
        .map_err(windows_encoder_error)?;
    media_type
        .SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_alignment)
        .map_err(windows_encoder_error)?;
    media_type
        .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, bits_per_sample)
        .map_err(windows_encoder_error)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_encoder_error(error: windows::core::Error) -> String {
    format!("WindowsのAAC処理に失敗しました: {error}")
}

#[cfg(target_os = "macos")]
pub struct M4aWriter {
    writer: avassetwriter::Writer,
    input: avassetwriter::InputId,
    frames_written: i64,
    path: std::path::PathBuf,
    bitrate: u32,
    pcm_buffer: Vec<u8>,
}

#[cfg(target_os = "macos")]
impl M4aWriter {
    pub fn create(path: &Path, bitrate: u32, fragment_seconds: f64) -> Result<Self, String> {
        use avassetwriter::{FileType, Time, Writer};

        let writer = Writer::create(path, FileType::M4a)
            .map_err(|error| format!("macOSのAACエンコーダーを開始できませんでした: {error}"))?;
        writer.set_movie_fragment_interval_seconds(fragment_seconds);
        writer
            .set_initial_movie_fragment_interval(Time::new(1, 1))
            .map_err(|error| format!("最初のM4Aフラグメントを設定できませんでした: {error}"))?;
        let input = writer
            .add_audio_input_pcm(48_000.0, 1, 16)
            .map_err(|error| format!("AAC音声入力を作成できませんでした: {error}"))?;
        writer
            .start_session((0, 48_000))
            .map_err(|error| format!("M4Aの書き込みを開始できませんでした: {error}"))?;
        Ok(Self {
            writer,
            input,
            frames_written: 0,
            path: path.to_path_buf(),
            bitrate,
            pcm_buffer: Vec::new(),
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<(), String> {
        samples_to_i16_bytes(samples, &mut self.pcm_buffer);
        self.writer
            .append_audio_pcm(
                self.input,
                &self.pcm_buffer,
                samples.len(),
                (self.frames_written, 48_000),
            )
            .map_err(|error| format!("AAC音声を書き込めませんでした: {error}"))?;
        self.frames_written += samples.len() as i64;
        Ok(())
    }

    pub fn finish(self) -> Result<(), String> {
        self.writer
            .finish()
            .map_err(|error| format!("M4Aファイルを確定できませんでした: {error}"))?;
        if self.bitrate == 128_000 {
            return Ok(());
        }
        let temporary = self.path.with_extension("transcoded.partial.m4a");
        let _ = std::fs::remove_file(&temporary);
        let output = std::process::Command::new("/usr/bin/afconvert")
            .arg(&self.path)
            .arg(&temporary)
            .args(["-f", "m4af", "-d", "aac", "-b"])
            .arg(self.bitrate.to_string())
            .output()
            .map_err(|error| {
                format!("macOSのAACビットレート変換を開始できませんでした: {error}")
            })?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "macOSのAACビットレート変換に失敗しました: {}",
                detail.trim()
            ));
        }
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| format!("変換したM4Aファイルを確定できませんでした: {error}"))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct M4aWriter;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl M4aWriter {
    pub fn create(_path: &Path, _bitrate: u32, _fragment_seconds: f64) -> Result<Self, String> {
        Err("このOSの録音エンコーダーはまだ利用できません。".into())
    }
    pub fn write(&mut self, _samples: &[f32]) -> Result<(), String> {
        Ok(())
    }
    pub fn finish(self) -> Result<(), String> {
        Ok(())
    }
}

fn samples_to_i16_bytes(samples: &[f32], pcm: &mut Vec<u8>) {
    pcm.clear();
    pcm.reserve(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
pub fn mix_with_limiter(microphone: &[f32], system: &[f32]) -> Vec<f32> {
    let mut output = Vec::with_capacity(microphone.len().max(system.len()));
    mix_with_limiter_into(microphone, system, &mut output);
    output
}

pub fn mix_with_limiter_into(microphone: &[f32], system: &[f32], output: &mut Vec<f32>) {
    let length = microphone.len().max(system.len());
    output.clear();
    output.reserve(length);
    output.extend((0..length).map(|index| {
        let mic = microphone.get(index).copied().unwrap_or(0.0);
        let sys = system.get(index).copied().unwrap_or(0.0);
        soft_limit(mic + sys)
    }));
}

fn soft_limit(sample: f32) -> f32 {
    if sample.abs() <= 0.95 {
        sample
    } else {
        sample.signum() * (0.95 + 0.05 * ((sample.abs() - 0.95) / 0.05).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::mix_with_limiter;

    #[test]
    fn mix_uses_equal_gain_and_never_clips() {
        let mixed = mix_with_limiter(&[1.0, -1.0, 0.5], &[1.0, -1.0, -0.5]);
        assert!(mixed.iter().all(|sample| sample.abs() <= 1.0));
        assert_eq!(mixed[2], 0.0);
    }

    #[test]
    fn missing_samples_are_silence() {
        assert_eq!(mix_with_limiter(&[0.5], &[]), vec![0.5]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_encoder_writes_playable_m4a() {
        use super::M4aWriter;
        use lofty::{file::AudioFile, probe::Probe};

        let path = std::env::temp_dir().join(format!(
            "mutsuna-echo-encoder-smoke-{}.m4a",
            std::process::id()
        ));
        let mut writer = M4aWriter::create(&path, 64_000, 2.0).expect("create Windows encoder");
        for _ in 0..250 {
            writer.write(&[0.1; 960]).expect("write audio chunk");
        }
        writer.finish().expect("finalize M4A");
        let metadata = std::fs::metadata(&path).expect("M4A exists");
        assert!(metadata.len() > 1_000);
        let bytes = std::fs::read(&path).expect("read M4A");
        assert!(
            bytes.windows(4).filter(|atom| *atom == b"moof").count() >= 2,
            "five seconds of audio must contain multiple M4A fragments"
        );
        let tagged = Probe::open(&path)
            .and_then(Probe::read)
            .expect("parse encoded M4A");
        assert_eq!(tagged.properties().sample_rate(), Some(48_000));
        assert_eq!(tagged.properties().channels(), Some(1));
        assert!(matches!(tagged.properties().audio_bitrate(), Some(60..=68)));
        let duration = crate::commands::transcribe::fragmented_m4a_duration(&path)
            .expect("read fragmented duration");
        assert!((4.9..=5.1).contains(&duration.as_secs_f64()));
        crate::commands::transcribe::validate_audio_path(&path)
            .expect("recorded M4A is selectable for transcription");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_encoder_can_finalize_without_captured_samples() {
        use super::M4aWriter;
        use lofty::{file::AudioFile, probe::Probe};

        for captured_frames in [0, 1, 960] {
            let path = std::env::temp_dir().join(format!(
                "mutsuna-echo-short-encoder-{}-{captured_frames}.m4a",
                std::process::id()
            ));
            let mut writer = M4aWriter::create(&path, 96_000, 2.0).expect("create Windows encoder");
            if captured_frames > 0 {
                writer
                    .write(&vec![0.0; captured_frames])
                    .expect("write short audio");
            }
            writer.finish().expect("finalize short M4A");
            let tagged = Probe::open(&path)
                .and_then(Probe::read)
                .expect("parse short M4A");
            assert_eq!(tagged.properties().sample_rate(), Some(48_000));
            assert!(crate::commands::transcribe::fragmented_m4a_duration(&path)
                .is_some_and(|duration| !duration.is_zero()));
            crate::commands::transcribe::validate_audio_path(&path)
                .expect("short M4A is selectable for transcription");
            let bytes = std::fs::read(&path).expect("read short M4A");
            assert!(bytes.windows(4).any(|atom| atom == b"moof"));
            let _ = std::fs::remove_file(path);
        }
    }
}
