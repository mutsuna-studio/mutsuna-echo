use std::{
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use tauri::{AppHandle, Manager};

pub(crate) struct PcmCacheWriter {
    path: PathBuf,
    writer: Option<BufWriter<fs::File>>,
    byte_buffer: Vec<u8>,
    sample_rate: u32,
    samples_written: u64,
}

pub(crate) struct PcmCacheFile {
    path: PathBuf,
    sample_rate: u32,
    samples: u64,
}

impl PcmCacheWriter {
    pub(crate) fn create(app: &AppHandle, sample_rate: u32) -> Result<Self, String> {
        if sample_rate == 0 {
            return Err("PCMキャッシュのサンプルレートが不正です。".into());
        }
        let directory = app
            .path()
            .app_cache_dir()
            .map_err(|error| format!("PCMキャッシュの保存先を取得できませんでした: {error}"))?
            .join("local-inference-temp");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("PCMキャッシュの保存先を作成できませんでした: {error}"))?;
        let path = directory.join(format!("{}.f32le", uuid::Uuid::now_v7()));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("PCMキャッシュを開始できませんでした: {error}"))?;
        Ok(Self {
            path,
            writer: Some(BufWriter::with_capacity(256 * 1024, file)),
            byte_buffer: Vec::new(),
            sample_rate,
            samples_written: 0,
        })
    }

    pub(crate) fn write(&mut self, samples: &[f32]) -> Result<(), String> {
        self.byte_buffer.clear();
        self.byte_buffer.reserve(samples.len().saturating_mul(4));
        for sample in samples {
            self.byte_buffer.extend_from_slice(&sample.to_le_bytes());
        }
        self.writer
            .as_mut()
            .ok_or_else(|| "PCMキャッシュはすでに確定されています。".to_string())?
            .write_all(&self.byte_buffer)
            .map_err(|error| format!("PCMキャッシュを書き込めませんでした: {error}"))?;
        self.samples_written = self.samples_written.saturating_add(samples.len() as u64);
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<PcmCacheFile, String> {
        self.writer
            .take()
            .ok_or_else(|| "PCMキャッシュはすでに確定されています。".to_string())?
            .flush()
            .map_err(|error| format!("PCMキャッシュを確定できませんでした: {error}"))?;
        Ok(PcmCacheFile {
            path: self.path.clone(),
            sample_rate: self.sample_rate,
            samples: self.samples_written,
        })
    }
}

impl Drop for PcmCacheWriter {
    fn drop(&mut self) {
        if self.writer.is_some() {
            drop(self.writer.take());
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl PcmCacheFile {
    pub(crate) fn read_regions(
        &self,
        windows: &[(u64, u64)],
        mut on_region: impl FnMut(usize, u32, &[f32]) -> Result<(), String>,
    ) -> Result<(), String> {
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("PCMキャッシュを開けませんでした: {error}"))?;
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        for (index, &(start_ms, duration_ms)) in windows.iter().enumerate() {
            let start_sample = start_ms.saturating_mul(self.sample_rate as u64) / 1_000;
            let requested_end = start_ms
                .saturating_add(duration_ms)
                .saturating_mul(self.sample_rate as u64)
                .saturating_add(999)
                / 1_000;
            let end_sample = requested_end.min(self.samples);
            if end_sample <= start_sample {
                return Err("PCMキャッシュの音声区間が空です。".into());
            }
            let sample_count = usize::try_from(
                end_sample
                    .checked_sub(start_sample)
                    .ok_or("PCMキャッシュの音声区間が不正です。")?,
            )
            .map_err(|_| "PCMキャッシュの音声区間が大きすぎます。".to_string())?;
            reader
                .seek(SeekFrom::Start(start_sample.saturating_mul(4)))
                .map_err(|error| format!("PCMキャッシュを移動できませんでした: {error}"))?;
            let mut bytes = vec![0u8; sample_count.saturating_mul(4)];
            reader
                .read_exact(&mut bytes)
                .map_err(|error| format!("PCMキャッシュを読み込めませんでした: {error}"))?;
            let samples = bytes
                .chunks_exact(4)
                .map(|value| f32::from_le_bytes([value[0], value[1], value[2], value[3]]))
                .collect::<Vec<_>>();
            on_region(index, self.sample_rate, &samples)?;
        }
        Ok(())
    }
}

impl Drop for PcmCacheFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    #[test]
    fn f32le_round_trip_preserves_samples_exactly() {
        let path = std::env::temp_dir().join(format!("pcm-cache-{}", uuid::Uuid::now_v7()));
        let source = [0.0f32, -0.25, 0.75, 1.0];
        let mut file = std::fs::File::create(&path).unwrap();
        for value in source {
            file.write_all(&value.to_le_bytes()).unwrap();
        }
        drop(file);
        let bytes = std::fs::read(&path).unwrap();
        let restored = bytes
            .chunks_exact(4)
            .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(restored, source);
        std::fs::remove_file(path).ok();
    }
}
