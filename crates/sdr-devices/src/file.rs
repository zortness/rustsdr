//! Raw interleaved little-endian float32 IQ. Metadata is supplied explicitly.
use sdr_core::Iq;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

pub struct IqFile {
    reader: BufReader<File>,
    remaining: u64,
    total: u64,
}

impl IqFile {
    pub fn open(path: &Path, sample_rate: u32, center_hz: u64) -> Result<Self, String> {
        if sample_rate != 2_400_000 || center_hz == 0 {
            return Err(
                "IQ playback requires 2400000 samples/s and a nonzero capture center".into(),
            );
        }
        let file = File::open(path).map_err(|e| format!("Cannot open IQ file: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() == 0 || !metadata.len().is_multiple_of(8) {
            return Err("IQ file must be nonempty, with complete 8-byte float32 I/Q pairs".into());
        }
        let total = metadata.len() / 8;
        Ok(Self {
            reader: BufReader::new(file),
            remaining: total,
            total,
        })
    }

    pub fn total_samples(&self) -> u64 {
        self.total
    }

    /// Returns zero at EOF. The caller reuses its sample buffer.
    pub fn read(&mut self, output: &mut [Iq]) -> Result<usize, String> {
        if output.is_empty() {
            return Err("IQ read buffer must not be empty".into());
        }
        let count = output.len().min(self.remaining as usize);
        for sample in &mut output[..count] {
            let mut bytes = [0; 8];
            self.reader
                .read_exact(&mut bytes)
                .map_err(|e| format!("IQ file read failed: {e}"))?;
            *sample = Iq {
                i: f32::from_le_bytes(bytes[..4].try_into().unwrap()),
                q: f32::from_le_bytes(bytes[4..].try_into().unwrap()),
            };
            if !sample.i.is_finite() || !sample.q.is_finite() {
                return Err("IQ file contains NaN or infinite samples".into());
            }
        }
        self.remaining -= count as u64;
        Ok(count)
    }
}
