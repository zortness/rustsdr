//! Standalone local practice service. It has no radio/engine command handle.
use sdr_dsp::{
    StereoFrame,
    cw::{CwConfig, CwDecoder, CwStatus},
    keyer::{Contacts, Keyer, KeyerConfig, KeyerMode, Sidetone},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MidiBinding {
    Note { channel: u8, note: u8 },
    Control { channel: u8, control: u8 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiEvent {
    Contact(MidiBinding, bool),
    ReleaseChannel(u8),
    ReleaseAll,
}
/// Channel numbers are zero-based internally; UI presents 1–16. CC >=64 is down.
pub fn midi_event(bytes: &[u8]) -> Option<MidiEvent> {
    let (&status, data) = bytes.split_first()?;
    if status == 0xff {
        return Some(MidiEvent::ReleaseAll);
    }
    if data.len() < 2 || data[0] > 127 || data[1] > 127 {
        return None;
    }
    let channel = status & 15;
    match status & 0xf0 {
        0x80 | 0x90 => Some(MidiEvent::Contact(
            MidiBinding::Note {
                channel,
                note: data[0],
            },
            status & 0xf0 == 0x90 && data[1] != 0,
        )),
        0xb0 if [120, 121, 123].contains(&data[0]) => Some(MidiEvent::ReleaseChannel(channel)),
        0xb0 => Some(MidiEvent::Contact(
            MidiBinding::Control {
                channel,
                control: data[0],
            },
            data[1] >= 64,
        )),
        _ => None,
    }
}
impl MidiBinding {
    pub fn channel(self) -> u8 {
        match self {
            Self::Note { channel, .. } | Self::Control { channel, .. } => channel,
        }
    }
    pub fn label(self) -> String {
        match self {
            Self::Note { channel, note } => format!("MIDI ch {} note {note}", channel + 1),
            Self::Control { channel, control } => format!("MIDI ch {} CC {control}", channel + 1),
        }
    }
}
pub fn midi_contacts(bindings: &[Option<MidiBinding>; 3], held: &mut [bool; 3], event: MidiEvent) {
    for (index, binding) in bindings.iter().enumerate() {
        match event {
            MidiEvent::Contact(input, down) if *binding == Some(input) => held[index] = down,
            MidiEvent::ReleaseChannel(channel)
                if binding.is_some_and(|b| b.channel() == channel) =>
            {
                held[index] = false
            }
            MidiEvent::ReleaseAll => held[index] = false,
            _ => {}
        }
    }
}
fn contacts(held: [bool; 3]) -> Contacts {
    Contacts {
        straight: held[0],
        dit: held[1],
        dah: held[2],
    }
}

#[derive(Clone, Default)]
pub struct PracticeStatus {
    pub running: bool,
    pub ready: bool,
    pub key_down: bool,
    pub decoded: Option<CwStatus>,
    pub error: Option<String>,
    pub audio_error: Option<String>,
    pub samples: u64,
}
struct Input {
    when: Instant,
    source: usize,
    held: [bool; 3],
}
#[derive(Clone)]
pub struct PracticeInput {
    sender: SyncSender<Input>,
    stop: Arc<AtomicBool>,
    heartbeat: Arc<Mutex<Instant>>,
}
impl PracticeInput {
    /// GUI and MIDI have separate contact sets, so one source cannot release another.
    pub fn set(&self, source: usize, held: [bool; 3]) {
        if source > 1 || self.stop.load(Ordering::Acquire) {
            return;
        }
        if self
            .sender
            .try_send(Input {
                when: Instant::now(),
                source,
                held,
            })
            .is_err()
        {
            self.stop.store(true, Ordering::Release);
        }
    }
    pub fn heartbeat(&self) {
        *self.heartbeat.lock().unwrap() = Instant::now();
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
pub struct PracticeSession {
    pub input: PracticeInput,
    pub volume: Arc<AtomicU32>,
    status: Arc<Mutex<PracticeStatus>>,
    worker: Option<JoinHandle<()>>,
}
impl PracticeSession {
    pub fn start(config: KeyerConfig, volume: f32, audible: bool) -> Result<Self, String> {
        config.validate().map_err(str::to_owned)?;
        if !(0.0..=1.0).contains(&volume) {
            return Err("Practice volume must be 0–1".into());
        }
        let (sender, receiver) = mpsc::sync_channel::<Input>(256);
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(PracticeStatus {
            running: true,
            ..Default::default()
        }));
        let heartbeat = Arc::new(Mutex::new(Instant::now()));
        let input = PracticeInput {
            sender,
            stop: stop.clone(),
            heartbeat: heartbeat.clone(),
        };
        let gain = Arc::new(AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()));
        let worker_status = status.clone();
        let worker_gain = gain.clone();
        let worker = thread::Builder::new()
            .name("cw-practice".into())
            .spawn(move || {
                #[cfg(feature = "audio")]
                let mut audio = if audible {
                    match crate::audio::AudioOutput::open(worker_gain) {
                        Ok(audio) => Some(audio),
                        Err(error) => {
                            worker_status.lock().unwrap().audio_error = Some(error);
                            None
                        }
                    }
                } else {
                    None
                };
                #[cfg(not(feature = "audio"))]
                {
                    let _ = worker_gain;
                    if audible {
                        worker_status.lock().unwrap().audio_error =
                            Some("Build with audio for sidetone".into());
                    }
                }
                let clock = Instant::now();
                worker_status.lock().unwrap().ready = true;
                *heartbeat.lock().unwrap() = clock;
                let mut keyer = Keyer::new(48_000, config).unwrap();
                let mut tone = Sidetone::new(48_000, config.pitch_hz);
                let mut decoder = CwDecoder::new(CwConfig {
                    seed_wpm: config.wpm,
                    adaptive: config.mode == KeyerMode::Straight,
                    ..Default::default()
                })
                .unwrap();
                let mut held = [[false; 3]; 2];
                let mut index = 0_u64;
                let mut audio_block = Vec::with_capacity(480);
                let mut power = Vec::with_capacity(480);
                let mut last_publish = Instant::now();
                let mut key_down = false;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let now = Instant::now();
                    if now.duration_since(*heartbeat.lock().unwrap()) > Duration::from_millis(300) {
                        worker_status.lock().unwrap().error = Some(
                            "Practice paused: input focus/heartbeat lost. Re-arm to continue."
                                .into(),
                        );
                        break;
                    }
                    let target = |at: Instant| {
                        (at.saturating_duration_since(clock).as_secs_f64() * 48_000.0) as u64
                    };
                    if target(now).saturating_sub(index) > 4800 {
                        worker_status.lock().unwrap().error = Some(
                            "Practice paused after a timing interruption. Re-arm to continue."
                                .into(),
                        );
                        break;
                    }
                    // Render to each timestamp before applying its new contacts, preserving
                    // short MIDI press/release pairs received between worker iterations.
                    let mut pending = receiver.try_recv().ok();
                    loop {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let end =
                            target(pending.as_ref().map_or(now, |event| event.when)).max(index);
                        while index < end {
                            let count = (end - index).min(480) as usize;
                            audio_block.clear();
                            power.clear();
                            for _ in 0..count {
                                key_down = keyer.next_key();
                                let value = tone.sample(key_down);
                                audio_block.push(StereoFrame {
                                    left: value,
                                    right: value,
                                });
                                power.push(if key_down { 0.25 } else { 0.0 });
                            }
                            decoder.process(&power);
                            #[cfg(feature = "audio")]
                            if let Some(audio) = &mut audio {
                                audio.push(&audio_block);
                            }
                            index += count as u64;
                        }
                        if let Some(event) = pending.take() {
                            held[event.source] = event.held;
                            keyer.contacts(contacts(std::array::from_fn(|i| {
                                held[0][i] || held[1][i]
                            })));
                            pending = receiver.try_recv().ok();
                        } else {
                            break;
                        }
                    }
                    if last_publish.elapsed() >= Duration::from_millis(20) {
                        let mut state = worker_status.lock().unwrap();
                        state.key_down = key_down;
                        state.samples = index;
                        state.decoded = Some(decoder.status().clone());
                        #[cfg(feature = "audio")]
                        if audio
                            .as_ref()
                            .is_some_and(|audio| audio.counters.failed.load(Ordering::Relaxed))
                        {
                            state.audio_error =
                                Some("Practice audio disconnected. Stop/re-arm to retry.".into());
                        }
                        last_publish = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                keyer.cancel();
                #[cfg(feature = "audio")]
                if let Some(audio) = &mut audio {
                    audio.retune();
                }
                let mut state = worker_status.lock().unwrap();
                state.running = false;
                state.key_down = false;
                state.decoded = Some(decoder.status().clone());
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            input,
            volume: gain,
            status,
            worker: Some(worker),
        })
    }
    pub fn status(&self) -> PracticeStatus {
        self.status.lock().unwrap().clone()
    }
}
impl Drop for PracticeSession {
    fn drop(&mut self) {
        self.input.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
