//! Experimental SSTV codec/service spike. One worker owns one contiguous PCM
//! session. A future engine supervisor must start a new session after a retune
//! or gap. No device, CAT, GUI or transmit access is present here.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub const SAMPLE_RATE: u32 = 48_000;
pub const MAX_BLOCK: usize = 4096;
const HISTORY: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Robot36,
    Martin1,
    Scottie1,
    Pd120,
}
impl Mode {
    pub const ALL: [Self; 4] = [Self::Robot36, Self::Martin1, Self::Scottie1, Self::Pd120];
    fn codec(self) -> ::sstv::Mode {
        use ::sstv::modes::*;
        match self {
            Self::Robot36 => ROBOT_36,
            Self::Martin1 => MARTIN_1,
            Self::Scottie1 => SCOTTIE_1,
            Self::Pd120 => PD_120,
        }
    }
    pub fn dimensions(self) -> (u32, u32) {
        self.codec().resolution()
    }
    fn detected(mode: ::sstv::Mode) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.codec().vis_code() == mode.vis_code())
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub id: u64,
    pub mode: Mode,
    pub rows: u32,
    /// Packed RGB, including black rows not yet received.
    pub rgb: Arc<Vec<u8>>,
}
impl Frame {
    pub fn complete(&self) -> bool {
        self.rows == self.mode.dimensions().1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Eof,
    Cancelled,
    Gap,
    Timeout,
    UnsupportedMode,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub current: Option<Frame>,
    /// Most recent finished or interrupted frames; bounded independently of UI polling.
    pub history: VecDeque<Frame>,
    pub end: Option<End>,
}

/// Pure encoder boundary: exact mode geometry, 48 kHz PCM16, no radio output.
pub fn encode(mode: Mode, rgb: &[u8]) -> Result<impl Iterator<Item = i16> + '_, String> {
    let (w, h) = mode.dimensions();
    if rgb.len() != w as usize * h as usize * 3 {
        return Err("RGB input must exactly match the selected mode's dimensions".into());
    }
    let pixels = rgb
        .chunks_exact(3)
        .map(|p| ::sstv::RgbPixel::new(p[0], p[1], p[2]));
    let encoder = ::sstv::Encoder::new(mode.codec(), pixels).map_err(|e| format!("{e:?}"))?;
    Ok(::sstv::Synthesizer::new(encoder, SAMPLE_RATE))
}

/// Decode all frames in a finite iterator, or in the worker's cancellable input.
/// Progress callbacks are coalesced to 10 Hz, with mandatory first/final updates.
pub fn decode(
    samples: impl Iterator<Item = i16>,
    forced: Option<Mode>,
    mut update: impl FnMut(Frame),
) -> Result<(), End> {
    let mut samples = samples.fuse();
    let mut id = 0;
    loop {
        let decoder = ::sstv::Decoder::new(samples.by_ref(), SAMPLE_RATE);
        let decoder = if let Some(mode) = forced {
            decoder.with_mode(mode.codec())
        } else {
            decoder
        };
        let Some(rows) = decoder.rows() else {
            return Ok(());
        };
        let mode = Mode::detected(rows.mode()).ok_or(End::UnsupportedMode)?;
        let (w, h) = mode.dimensions();
        id += 1;
        let mut frame = Frame {
            id,
            mode,
            rows: 0,
            rgb: Arc::new(vec![0; w as usize * h as usize * 3]),
        };
        update(frame.clone());
        let mut published = Instant::now();
        for row in rows {
            let start = row.index() * w as usize * 3;
            let pixels = Arc::make_mut(&mut frame.rgb);
            for (out, pixel) in pixels[start..start + w as usize * 3]
                .chunks_exact_mut(3)
                .zip(row.pixels())
            {
                out.copy_from_slice(&[pixel.red(), pixel.green(), pixel.blue()]);
            }
            frame.rows = row.index() as u32 + 1;
            if published.elapsed() >= Duration::from_millis(100) {
                update(frame.clone());
                published = Instant::now();
            }
        }
        let complete = frame.complete();
        update(frame);
        if !complete {
            return Ok(());
        }
    }
}

enum Input {
    Block { start: u64, samples: Vec<i16> },
    Eof,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    Full,
    Closed,
    InvalidBlock,
}

/// Fixed-rate, contiguous session prototype. Queue capacity is eight blocks.
/// Submission never blocks: on Full, retry the SAME start/sample block or stop
/// the session. Skipping it will be detected as a gap.
pub struct Worker {
    input: mpsc::SyncSender<Input>,
    cancel: Arc<AtomicBool>,
    latest: Arc<Mutex<Snapshot>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(forced: Option<Mode>) -> std::io::Result<Self> {
        let (input, receiver) = mpsc::sync_channel(8);
        let cancel = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(Snapshot::default()));
        let stopping = cancel.clone();
        let output = latest.clone();
        let thread = thread::Builder::new()
            .name("sstv-spike".into())
            .spawn(move || {
                let mut samples = Samples {
                    receiver,
                    cancel: stopping,
                    block: Vec::new().into_iter(),
                    next: 0,
                    end: None,
                    last: Instant::now(),
                };
                let result = decode(&mut samples, forced, |frame| {
                    let mut state = output.lock().unwrap();
                    if state.current.as_ref().is_some_and(|old| old.id != frame.id) {
                        retain_current(&mut state);
                    }
                    state.current = Some(frame);
                });
                let mut state = output.lock().unwrap();
                retain_current(&mut state);
                state.end = Some(result.err().or(samples.end).unwrap_or(End::Eof));
            })?;
        Ok(Self {
            input,
            cancel,
            latest,
            thread: Some(thread),
        })
    }
    pub fn submit(&self, start: u64, samples: &[i16]) -> Result<(), SubmitError> {
        if samples.is_empty() || samples.len() > MAX_BLOCK {
            return Err(SubmitError::InvalidBlock);
        }
        self.send(Input::Block {
            start,
            samples: samples.to_vec(),
        })
    }
    pub fn finish(&self) -> Result<(), SubmitError> {
        self.send(Input::Eof)
    }
    fn send(&self, input: Input) -> Result<(), SubmitError> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        self.input.try_send(input).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => SubmitError::Full,
            mpsc::TrySendError::Disconnected(_) => SubmitError::Closed,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.latest.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn retain_current(state: &mut Snapshot) {
    if let Some(frame) = state.current.take() {
        if state.history.len() == HISTORY {
            state.history.pop_front();
        }
        state.history.push_back(frame);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Samples {
    receiver: mpsc::Receiver<Input>,
    cancel: Arc<AtomicBool>,
    block: std::vec::IntoIter<i16>,
    next: u64,
    end: Option<End>,
    last: Instant,
}
impl Iterator for Samples {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        loop {
            if self.end.is_some() {
                return None;
            }
            if self.cancel.load(Ordering::Acquire) {
                self.end = Some(End::Cancelled);
                return None;
            }
            if let Some(sample) = self.block.next() {
                return Some(sample);
            }
            match self.receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(Input::Block { start, samples }) => {
                    if start != self.next {
                        self.end = Some(End::Gap);
                        return None;
                    }
                    self.next += samples.len() as u64;
                    self.block = samples.into_iter();
                    self.last = Instant::now();
                }
                Ok(Input::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.end = Some(End::Eof);
                    return None;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.last.elapsed() >= Duration::from_secs(2) {
                        self.end = Some(End::Timeout);
                        return None;
                    }
                }
            }
        }
    }
}
