//! Decoded receiver audio recording, independent of speaker gain and output device.
use crate::engine::{AudioBlock, Client, EventStream};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct Recorder {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<u64, String>>>,
}

impl Recorder {
    pub fn start(client: &Client, path: PathBuf) -> Result<Self, String> {
        Self::start_stream(client.subscribe_monitor_audio(), path)
    }

    fn start_stream(stream: EventStream<AudioBlock>, path: PathBuf) -> Result<Self, String> {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            return Err("Recording path must end in .wav".into());
        }
        if path.exists() {
            return Err(format!("Recording already exists: {}", path.display()));
        }
        let partial = partial_path(&path);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|e| format!("Cannot create {}: {e}", partial.display()))?;
        let writer = match hound::WavWriter::new(
            file,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        ) {
            Ok(writer) => writer,
            Err(error) => {
                let _ = fs::remove_file(&partial);
                return Err(format!("Cannot start WAV recording: {error}"));
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let cleanup = partial.clone();
        let path_for_status = path.clone();
        let worker = thread::Builder::new()
            .name("audio-recorder".into())
            .spawn(move || {
                let result = write_stream(stream, stopping, writer, &path, &partial);
                if result.is_err() {
                    let _ = fs::remove_file(&partial);
                }
                result
            })
            .map_err(|error| {
                let _ = fs::remove_file(cleanup);
                format!("Cannot start recording worker: {error}")
            })?;
        Ok(Self {
            path: path_for_status,
            stop,
            worker: Some(worker),
        })
    }

    pub fn destination(&self) -> &Path {
        &self.path
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_some_and(JoinHandle::is_finished)
    }

    pub fn finish(mut self) -> Result<u64, String> {
        self.stop.store(true, Ordering::Release);
        self.join()
    }

    fn join(&mut self) -> Result<u64, String> {
        self.worker
            .take()
            .ok_or("Recording already finished".to_owned())?
            .join()
            .map_err(|_| "Recording worker panicked".to_owned())?
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if self.worker.is_some() {
            let _ = self.join();
        }
    }
}

fn partial_path(path: &Path) -> PathBuf {
    path.with_extension("wav.part")
}

fn write_stream(
    stream: EventStream<AudioBlock>,
    stop: Arc<AtomicBool>,
    mut writer: hound::WavWriter<fs::File>,
    path: &Path,
    partial: &Path,
) -> Result<u64, String> {
    let mut frames = 0_u64;
    loop {
        if stream.dropped() > 0 {
            return Err(format!(
                "Recording could not keep up; {} audio blocks were lost",
                stream.dropped()
            ));
        }
        if let Some(block) = stream.next() {
            if block.sample_rate != 48_000 {
                return Err(format!(
                    "Unsupported recording rate: {} Hz",
                    block.sample_rate
                ));
            }
            for frame in block.frames.iter() {
                for sample in [frame.left, frame.right] {
                    let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
                    writer
                        .write_sample(pcm)
                        .map_err(|e| format!("WAV write failed: {e}"))?;
                }
            }
            frames += block.frames.len() as u64;
        } else if stop.load(Ordering::Acquire) {
            break;
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    writer
        .finalize()
        .map_err(|e| format!("Cannot finish WAV header: {e}"))?;
    if path.exists() {
        return Err(format!(
            "Recording destination now exists: {}",
            path.display()
        ));
    }
    fs::rename(partial, path).map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdr_dsp::StereoFrame;

    fn block(samples: &[(f32, f32)]) -> AudioBlock {
        AudioBlock {
            end_time: Duration::ZERO,
            revision: 1,
            capture_revision: 1,
            input_start: 0,
            output_start: 0,
            discontinuity: 0,
            sample_rate: 48_000,
            frames: samples
                .iter()
                .map(|(left, right)| StereoFrame {
                    left: *left,
                    right: *right,
                })
                .collect(),
        }
    }

    #[test]
    fn short_recording_is_playable_and_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("rustsdr-recording-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("short.wav");
        let _ = fs::remove_file(&path);
        let stream = EventStream::bounded(8);
        let recorder = Recorder::start_stream(stream.clone(), path.clone()).unwrap();
        stream.publish(block(&[(0.5, -0.5), (0.0, 1.0)]));
        assert_eq!(recorder.finish().unwrap(), 2);
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.samples::<i16>().count(), 4);
        assert!(Recorder::start_stream(stream, path.clone()).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn dropped_audio_fails_without_publishing_partial_wav() {
        let dir =
            std::env::temp_dir().join(format!("rustsdr-recording-loss-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lost.wav");
        let stream = EventStream::bounded(1);
        let recorder = Recorder::start_stream(stream.clone(), path.clone()).unwrap();
        for _ in 0..100 {
            stream.publish(block(&[(0.2, 0.2)]));
        }
        assert!(recorder.finish().is_err());
        assert!(!path.exists());
        assert!(!partial_path(&path).exists());
        fs::remove_dir(dir).unwrap();
    }
}
