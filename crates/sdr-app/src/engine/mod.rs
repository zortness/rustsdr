use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};

mod channels;
#[cfg(feature = "ic705")]
mod ic705;
pub use channels::{ChannelEvent, ChannelRequest, ChannelStream, IqBlock};
mod outputs;
#[cfg(feature = "qmx")]
mod qmx;
mod runtime;
mod source;
use outputs::Outputs;
pub use outputs::{AudioBlock, EngineEvent, EventStream};
mod types;
pub use types::*;
/// A single coalescing control slot and a single replaceable display slot.
/// UI methods use try_lock; FFT and session lifecycle never run under those locks.
pub struct Engine {
    vor: Option<crate::vor::Worker>,
    vor_handle: crate::vor::Handle,
    digital: crate::digital::Worker,
    channels: Arc<Mutex<Vec<JoinHandle<()>>>>,
    stop_scan: Arc<AtomicU64>,
    requested: Arc<Mutex<Settings>>,
    latest: Arc<Outputs>,
    cancel: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn new() -> std::io::Result<Self> {
        Self::with_settings(Settings::default())
    }
    pub fn with_settings(initial: Settings) -> std::io::Result<Self> {
        if !valid(&initial) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid radio settings",
            ));
        }
        let requested = Arc::new(Mutex::new(initial));
        let latest = Arc::new(Outputs::default());
        let digital = crate::digital::Worker::new(latest.subscribe_audio())?;
        let cancel = Arc::new(AtomicBool::new(false));
        let volume = Arc::new(AtomicU32::new(0.15_f32.to_bits()));
        let gain = volume.clone();
        let stop_scan = Arc::new(AtomicU64::new(0));
        let scan_stop = stop_scan.clone();
        let (control, output, stopping) = (requested.clone(), latest.clone(), cancel.clone());
        let worker = thread::Builder::new()
            .name("radio-controller".into())
            .spawn(move || {
                if let Err(error) =
                    runtime::run_worker(control, output.clone(), stopping.clone(), gain, scan_stop)
                    && let Ok(mut state) = output.lock()
                {
                    state.running = false;
                    output.events.publish(EngineEvent::Fault {
                        revision: state.revision,
                        message: error.clone(),
                    });
                    state.error = Some(error);
                }
                stopping.store(true, Ordering::Release);
            })?;
        let mut engine = Self {
            vor: None,
            vor_handle: crate::vor::Handle::default(),
            digital,
            channels: Arc::new(Mutex::new(Vec::new())),
            stop_scan,
            requested,
            latest,
            cancel,
            volume,
            worker: Some(worker),
        };
        engine.vor = Some(crate::vor::Worker::new(
            engine.client(),
            engine.vor_handle.clone(),
        )?);
        Ok(engine)
    }

    pub fn client(&self) -> Client {
        Client {
            vor: self.vor_handle.clone(),
            digital: self.digital.client(),
            channels: self.channels.clone(),
            stop_scan: self.stop_scan.clone(),
            requested: self.requested.clone(),
            latest: self.latest.clone(),
            cancel: self.cancel.clone(),
            volume: self.volume.clone(),
        }
    }
    /// Compatibility wrapper; clients should use submit for typed errors.
    pub fn request(&self, settings: Settings) -> bool {
        self.client()
            .submit(Command::Configure(Box::new(settings)))
            .is_ok()
    }
    pub fn set_volume(&self, gain: f32) {
        self.client().set_volume(gain);
    }
    pub fn poll(&self) -> Option<Update> {
        self.client().poll()
    }
}

/// A local command/output handle. Cloning it does not keep the worker alive.
/// Presentation has one consuming reader for spectrum and each ordered stream.
#[derive(Clone)]
pub struct Client {
    vor: crate::vor::Handle,
    digital: crate::digital::Handle,
    channels: Arc<Mutex<Vec<JoinHandle<()>>>>,
    stop_scan: Arc<AtomicU64>,
    requested: Arc<Mutex<Settings>>,
    latest: Arc<Outputs>,
    cancel: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
}

pub enum Command {
    /// Coalesced display-only change, without a radio revision or scan restart.
    SpectrumView(sdr_dsp::zoom_spectrum::SpectrumView),
    /// Hold the engine's actual current channel without recreating its receiver.
    StopScan {
        revision: u64,
    },
    Configure(Box<Settings>),
    MonitorGain(f32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Busy,
    Invalid,
    Stale,
    Closed,
}
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub rtl_rust: bool,
    pub qmx: bool,
    pub ic705: bool,
    pub rtl: bool,
    pub monitor_audio: bool,
    pub transmit: bool,
}

impl Client {
    pub fn vor(&self) -> crate::vor::Handle {
        self.vor.clone()
    }
    /// At most eight independently filtered channels, processed off acquisition.
    pub fn subscribe_channel(&self, request: ChannelRequest) -> Result<ChannelStream, String> {
        let mut workers = self.channels.lock().map_err(|_| "Channel lock poisoned")?;
        if self.cancel.load(Ordering::Acquire) {
            return Err("Engine stopped".into());
        }
        workers.retain(|w| !w.is_finished());
        if workers.len() >= 8 {
            return Err("At most eight IQ channels".into());
        }
        let (stream, worker) = channels::spawn(&self.latest.iq, request, self.cancel.clone())?;
        workers.push(worker);
        Ok(stream)
    }
    pub fn digital(&self) -> crate::digital::Handle {
        self.digital.clone()
    }
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            rtl_rust: cfg!(feature = "rtl-rust"),
            qmx: cfg!(feature = "qmx"),
            ic705: cfg!(feature = "ic705"),
            rtl: cfg!(feature = "rtl"),
            monitor_audio: cfg!(feature = "audio"),
            transmit: false,
        }
    }
    pub fn submit(&self, command: Command) -> Result<(), SubmitError> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        match command {
            Command::SpectrumView(view) => {
                if !view.validate(SAMPLE_RATE as f64) {
                    return Err(SubmitError::Invalid);
                }
                self.requested
                    .try_lock()
                    .map_err(|_| SubmitError::Busy)?
                    .spectrum_view = view;
            }
            Command::StopScan { revision } => {
                let slot = self.requested.try_lock().map_err(|_| SubmitError::Busy)?;
                if slot.revision != revision {
                    return Err(SubmitError::Stale);
                }
                self.stop_scan.store(revision, Ordering::Release);
            }
            Command::MonitorGain(gain) => {
                if !gain.is_finite() {
                    return Err(SubmitError::Invalid);
                }
                self.set_volume(gain);
            }
            Command::Configure(settings) => {
                if !valid(&settings) {
                    return Err(SubmitError::Invalid);
                }
                let mut slot = self.requested.try_lock().map_err(|_| SubmitError::Busy)?;
                if settings.revision <= slot.revision {
                    return Err(SubmitError::Stale);
                }
                *slot = *settings;
            }
        }
        Ok(())
    }
    pub fn events(&self) -> EventStream<EngineEvent> {
        self.latest.events.clone()
    }
    /// At most eight blocks, before speaker volume/resampling. Each subscription
    /// is independent; slow readers drop their own blocks. Samples are shared.
    pub fn subscribe_audio(&self) -> EventStream<AudioBlock> {
        self.latest.subscribe_audio()
    }
    pub fn set_volume(&self, gain: f32) {
        if gain.is_finite() {
            self.volume
                .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn poll(&self) -> Option<Update> {
        let mut state = self.latest.try_lock().ok()?;
        Some(Update {
            rtl: state.rtl.clone(),
            ic705: state.ic705.clone(),
            qmx: state.qmx.clone(),
            revision: state.revision,
            capture_revision: state.capture_revision,
            center_hz: state.center_hz,
            station_hz: state.station_hz,
            mode: state.mode,
            receiver: state.receiver,
            scan: state.scan,
            channel_power_dbfs: state.channel_power_dbfs,
            stereo_locked: state.stereo_locked,
            rds: state.rds.clone(),
            cw: state.cw.clone(),
            running: state.running,
            samples: state.samples,
            total_samples: state.total_samples,
            eof: state.eof,
            display_drops: state.display_drops,
            iq_drops: state.iq_drops,
            retune_discarded: state.retune_discarded,
            audio_underruns: state.audio_underruns,
            audio_overruns: state.audio_overruns,
            audio_queue_frames: state.audio_queue_frames,
            audio_output_rate_hz: state.audio_output_rate_hz,
            audio_output_device: state.audio_output_device.clone(),
            audio_error: state.audio_error.clone(),
            audio_rms: state.audio_rms,
            error: state.error.clone(),
            frame: state.frame.take(),
        })
    }
}

fn valid(s: &Settings) -> bool {
    (s.source != Source::Ic705 || (cfg!(feature = "ic705") && s.scan.is_none()))
        && s.revision > 0
        && s.spectrum_view.validate(SAMPLE_RATE as f64)
        && (s.source != Source::Qmx || (cfg!(feature = "qmx") && s.scan.is_none()))
        && s.cw_decoder.is_none_or(|config| config.validate().is_ok())
        && s.tone_hz.is_finite()
        && s.tone_hz.abs() < SAMPLE_RATE as f64 / 2.0
        && (sdr_core::FILE_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&s.station_hz)
        && (s.source != Source::RtlSdr
            || (s.rtl_backend.available()
                && cfg!(feature = "rtl")
                && crate::tuning::tuner_frequency(s.station_hz as u64, s.converter_offset_hz)
                    .is_some()
                && s.tuner_center_hz().is_some()))
        && s.receiver.validate().is_ok()
        && s.scan.as_ref().is_none_or(|plan| {
            let (min, max) = scan_bounds(s);
            s.running && s.source != Source::Synthetic && plan.validate(min, max).is_ok()
        })
        && crate::tuning::converter_bounds(s.converter_offset_hz).is_some()
        && (s.source != Source::RtlSdr
            || s.rtl_center_hz.is_none_or(|hz| {
                crate::tuning::tuner_frequency(hz as u64, s.converter_offset_hz).is_some()
            }))
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        drop(self.vor.take());
        for worker in self.channels.lock().expect("Channel lock").drain(..) {
            let _ = worker.join();
        }
        // Clients may outlive the owner. Do not leave them observing a receiver
        // that appears to be running after its worker has been joined.
        if let Ok(mut state) = self.latest.lock() {
            state.running = false;
            state.frame = None;
            state.scan = crate::scanning::ScanStatus::default();
        }
    }
}

pub fn scan_bounds(settings: &Settings) -> (u32, u32) {
    if settings.source == Source::File {
        let center = settings.center_hz();
        if !(sdr_core::FILE_MIN_HZ as u64..=sdr_core::RX_MAX_HZ as u64).contains(&center) {
            return (1, 0);
        }
        (
            center
                .saturating_sub(1_000_000)
                .max(sdr_core::FILE_MIN_HZ as u64) as u32,
            center
                .saturating_add(1_000_000)
                .min(sdr_core::RX_MAX_HZ as u64) as u32,
        )
    } else {
        crate::tuning::converter_bounds(settings.converter_offset_hz).unwrap_or((1, 0))
    }
}
