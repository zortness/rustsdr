//! Bounded receive-only digital service. Audio collection and expensive decoding
//! run on separate threads; neither blocks acquisition or presentation.
use crate::engine::{AudioBlock, EventStream};
use mfsk_core::{
    ft4::Ft4,
    ft8::Ft8,
    msg::{decode_request::DecodeRequest, wsjt77::unpack77},
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Ft8,
    Ft4,
}
impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ft8 => "FT8",
            Self::Ft4 => "FT4",
        }
    }
    pub fn samples(self) -> usize {
        match self {
            Self::Ft8 => 180_000,
            Self::Ft4 => 90_000,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Message {
    pub text: String,
    /// Sender from a structured protocol field, never guessed from free text.
    pub sender: Option<String>,
    pub audio_hz: f32,
    pub dt: f32,
    pub snr: f32,
}

/// Exactly one 12 kHz slot, zero padded by WAV import when needed. Limited
/// candidate search and a cooperative budget prevent pathological CPU backlog.
pub fn decode(protocol: Protocol, audio: &[i16]) -> Result<Vec<Message>, String> {
    #[cfg(feature = "trace")]
    let _span = tracing::info_span!("sdr.digital.decode", protocol = protocol.label()).entered();
    if audio.len() != protocol.samples() {
        return Err("Expected one 12 kHz FT slot".into());
    }
    let start = Instant::now();
    let budget = || start.elapsed() < Duration::from_secs(4);
    let results = match protocol {
        Protocol::Ft8 => {
            DecodeRequest::<Ft8>::new(audio, 300.0, 5000.0, 1.5, 60)
                .budget(&budget)
                .decode()
                .results
        }
        Protocol::Ft4 => {
            DecodeRequest::<Ft4>::new(audio, 300.0, 5000.0, 1.5, 60)
                .budget(&budget)
                .decode()
                .results
        }
    };
    let mut messages: Vec<_> = results
        .into_iter()
        .filter_map(|r| {
            Some(Message {
                text: unpack77(r.message77())?,
                sender: sender(r.message77()),
                audio_hz: r.freq_hz,
                dt: r.dt_sec,
                snr: r.snr_db,
            })
        })
        .collect();
    messages.sort_by(|a, b| a.audio_hz.total_cmp(&b.audio_hz));
    Ok(messages)
}

fn sender(bits: &[u8]) -> Option<String> {
    use mfsk_core::msg::{
        CallsignHashTable,
        wsjt77::{Wsjt77Fields, unpack77_fields},
    };
    let call = match unpack77_fields(bits, &CallsignHashTable::new())? {
        Wsjt77Fields::Standard { call2, .. }
        | Wsjt77Fields::FieldDay { call2, .. }
        | Wsjt77Fields::RttyRoundup { call2, .. }
        | Wsjt77Fields::EuVhfContest { call2, .. } => call2,
        Wsjt77Fields::Nonstandard {
            call2: Some(call), ..
        } => call,
        _ => return None,
    };
    let call = call.trim_matches(['<', '>']);
    (call.bytes().any(|b| b.is_ascii_digit()) && call.bytes().any(|b| b.is_ascii_uppercase()))
        .then(|| call.to_owned())
}

/// Deliberately strict import: reference slot WAV, mono PCM16 at 12 kHz.
/// Short WSJT-X sample slots are zero padded; long files are not silently truncated.
pub fn read_wav(path: &Path, protocol: Protocol) -> Result<Vec<i16>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 12_000
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("Use a mono 16-bit PCM WAV at 12000 Hz containing one slot".into());
    }
    if reader.len() as usize > protocol.samples() {
        return Err("WAV is longer than one slot".into());
    }
    let mut samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if samples.is_empty() {
        return Err("WAV contains no audio".into());
    }
    samples.resize(protocol.samples(), 0);
    Ok(samples)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub protocol: Option<Protocol>,
    pub revision: u64,
    pub dial_hz: u64,
    pub file_clock: bool,
}
pub struct Batch {
    pub capture_revision: u64,
    pub config: Config,
    pub slot_ms: u64,
    pub imported: bool,
    pub elapsed: Duration,
    pub result: Result<Vec<Message>, String>,
}
enum Input {
    Samples(Vec<i16>),
    Wav(std::path::PathBuf),
}
struct Job {
    capture_revision: u64,
    config: Config,
    slot_ms: u64,
    input: Input,
}

/// Sample-clock slot assembly. Partial first slots and slots interrupted by
/// gaps are discarded. Wall-clock adjustment is checked at each input block.
struct Slots {
    config: Config,
    identity: Option<(u64, u64, u64)>,
    next: u64,
    anchor: i128,
    slot: u64,
    complete: bool,
    pcm: Vec<i16>,
    resampler: sdr_dsp::DecoderAudio,
}
impl Slots {
    fn new(config: Config) -> Self {
        Self {
            config,
            identity: None,
            next: 0,
            anchor: 0,
            slot: 0,
            complete: false,
            pcm: Vec::new(),
            resampler: Default::default(),
        }
    }
    fn push(&mut self, b: AudioBlock) -> Vec<Job> {
        let Some(protocol) = self.config.protocol else {
            return Vec::new();
        };
        if b.sample_rate != 48_000 || b.revision != self.config.revision {
            return Vec::new();
        }
        let identity = (b.revision, b.capture_revision, b.discontinuity);
        let wall_start = (b.end_time.as_secs_f64() * 48_000.0) as i128 - b.frames.len() as i128;
        let expected_wall = self.anchor + b.output_start as i128;
        let clock_jump = !self.config.file_clock && (wall_start - expected_wall).abs() > 24_000;
        if self.identity != Some(identity) || b.output_start != self.next || clock_jump {
            self.identity = Some(identity);
            self.anchor = if self.config.file_clock {
                0
            } else {
                wall_start - b.output_start as i128
            };
            self.resampler = Default::default();
            self.pcm.clear();
            self.complete = false;
            self.slot = u64::MAX;
        }
        self.next = b.output_start + b.frames.len() as u64;
        let size = protocol.samples() as u64 * 4;
        let mut jobs = Vec::new();
        for (i, frame) in b.frames.iter().enumerate() {
            let tick = (self.anchor + b.output_start as i128 + i as i128).max(0) as u64;
            let slot = tick / size;
            if slot != self.slot {
                self.slot = slot;
                self.complete = tick.is_multiple_of(size);
                self.pcm.clear();
            }
            if let Some(sample) = self.resampler.push((frame.left + frame.right) * 0.5) {
                self.pcm.push(sample);
            }
            if tick % size == size - 1 && self.complete && self.pcm.len() == protocol.samples() {
                jobs.push(Job {
                    capture_revision: b.capture_revision,
                    config: self.config,
                    slot_ms: slot * size / 48,
                    input: Input::Samples(std::mem::take(&mut self.pcm)),
                });
            }
        }
        jobs
    }
}

pub struct Worker {
    client: Handle,
    threads: Vec<thread::JoinHandle<()>>,
}
impl std::ops::Deref for Worker {
    type Target = Handle;
    fn deref(&self) -> &Handle {
        &self.client
    }
}
/// Control/results handle; cloning does not extend worker lifetime.
#[derive(Clone)]
pub struct Handle {
    config: Arc<Mutex<Config>>,
    cancel: Arc<AtomicBool>,
    jobs: mpsc::SyncSender<Job>,
    pub results: EventStream<Batch>,
    pub skipped: Arc<AtomicU64>,
    audio: EventStream<AudioBlock>,
}
impl Worker {
    pub fn new(audio: EventStream<AudioBlock>) -> std::io::Result<Self> {
        let config = Arc::new(Mutex::new(Config::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let skipped = Arc::new(AtomicU64::new(0));
        let results = EventStream::bounded(16);
        let (jobs, rx) = mpsc::sync_channel::<Job>(1);
        let (stop, output) = (cancel.clone(), results.clone());
        let decoder = thread::Builder::new()
            .name("ft-decode".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Ok(job) = rx.recv_timeout(Duration::from_millis(50)) else {
                        continue;
                    };
                    let Some(protocol) = job.config.protocol else {
                        continue;
                    };
                    let start = Instant::now();
                    let imported = matches!(job.input, Input::Wav(_));
                    let result = match job.input {
                        Input::Samples(pcm) => decode(protocol, &pcm),
                        Input::Wav(path) => {
                            read_wav(&path, protocol).and_then(|pcm| decode(protocol, &pcm))
                        }
                    };
                    output.publish(Batch {
                        capture_revision: job.capture_revision,
                        config: job.config,
                        slot_ms: job.slot_ms,
                        imported,
                        elapsed: start.elapsed(),
                        result,
                    });
                }
            })?;
        let (settings, stop, sender, dropped) = (
            config.clone(),
            cancel.clone(),
            jobs.clone(),
            skipped.clone(),
        );
        let audio_status = audio.clone();
        let collector = thread::Builder::new()
            .name("ft-collect".into())
            .spawn(move || {
                let mut slots = Slots::new(Config::default());
                while !stop.load(Ordering::Relaxed) {
                    let cfg = *settings.lock().expect("digital config");
                    if cfg != slots.config {
                        slots = Slots::new(cfg);
                    }
                    if let Some(block) = audio.next() {
                        for job in slots.push(block) {
                            if sender.try_send(job).is_err() {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    } else {
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            });
        match collector {
            Ok(collector) => Ok(Self {
                client: Handle {
                    config,
                    cancel,
                    jobs,
                    results,
                    skipped,
                    audio: audio_status,
                },
                threads: vec![collector, decoder],
            }),
            Err(e) => {
                cancel.store(true, Ordering::Relaxed);
                let _ = decoder.join();
                Err(e)
            }
        }
    }
    pub fn client(&self) -> Handle {
        self.client.clone()
    }
}
impl Handle {
    pub fn configure(&self, config: Config) {
        if self.cancel.load(Ordering::Acquire) {
            return;
        }
        *self.config.lock().expect("digital config") = config;
    }
    pub fn dropped_audio(&self) -> u64 {
        self.audio.dropped()
    }
    pub fn import(&self, path: std::path::PathBuf, config: Config) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("Decoder stopped".into());
        }
        self.jobs
            .try_send(Job {
                capture_revision: 0,
                config,
                slot_ms: 0,
                input: Input::Wav(path),
            })
            .map_err(|_| "Decoder busy; try again after this slot".into())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_text_has_no_inferred_sender() {
        assert_eq!(sender(&[0; 77]), None);
    }
    fn block(start: u64, count: usize, utc_start: f64) -> AudioBlock {
        AudioBlock {
            end_time: Duration::from_secs_f64(utc_start + (start as f64 + count as f64) / 48_000.0),
            revision: 1,
            capture_revision: 1,
            input_start: start * 50,
            output_start: start,
            discontinuity: 0,
            sample_rate: 48_000,
            frames: vec![sdr_dsp::StereoFrame::default(); count].into(),
        }
    }
    fn cfg(file_clock: bool) -> Config {
        Config {
            protocol: Some(Protocol::Ft4),
            revision: 1,
            dial_hz: 7_047_500,
            file_clock,
        }
    }
    #[test]
    fn utc_partial_slot_then_complete_across_arbitrary_blocks() {
        let mut slots = Slots::new(cfg(false));
        let mut jobs = Vec::new();
        let mut start = 0;
        while start < 48_000 * 15 {
            let n = (48_000 * 15 - start).min(997) as usize;
            jobs.extend(slots.push(block(start, n, 1.25)));
            start += n as u64;
        }
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].slot_ms, 7500);
        let Input::Samples(ref pcm) = jobs[0].input else {
            panic!()
        };
        assert_eq!(pcm.len(), 90_000);
    }
    #[test]
    fn gap_revision_and_clock_jump_discard_partial_slots() {
        for scenario in 0..3 {
            let mut slots = Slots::new(cfg(false));
            assert!(slots.push(block(0, 180_000, 0.0)).is_empty());
            let mut b = block(180_000, 180_000, 0.0);
            match scenario {
                0 => b.output_start += 4,
                1 => b.revision = 2,
                _ => b.end_time += Duration::from_secs(1),
            }
            assert!(slots.push(b).is_empty());
        }
    }
    #[test]
    fn reference_audio_through_streaming_resampler_and_slot_collector() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/digital/ft4.wav");
        let pcm = read_wav(&path, Protocol::Ft4).unwrap();
        let mut slots = Slots::new(cfg(true));
        let frames: Vec<_> = pcm
            .iter()
            .flat_map(|&v| {
                std::iter::repeat_n(
                    sdr_dsp::StereoFrame {
                        left: v as f32 / 32768.0,
                        right: v as f32 / 32768.0,
                    },
                    4,
                )
            })
            .collect();
        let mut jobs = Vec::new();
        let mut start = 0;
        for chunk in frames.chunks(1023) {
            let mut b = block(start, 0, 100.0);
            b.frames = chunk.into();
            start += chunk.len() as u64;
            jobs.extend(slots.push(b));
        }
        assert_eq!(jobs.len(), 1);
        let Input::Samples(ref pcm) = jobs[0].input else {
            panic!()
        };
        let results = decode(Protocol::Ft4, pcm).unwrap();
        assert!(results.iter().any(|m| m.text == "CQ RU AB5XS EM12"));
    }
}
