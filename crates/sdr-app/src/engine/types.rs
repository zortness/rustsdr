use sdr_dsp::{Mode, ReceiverConfig};
use std::sync::Arc;
pub const FFT_SIZE: usize = 4096;
pub const SAMPLE_RATE: u32 = 2_400_000;
pub const CENTER_HZ: u64 = 100_000_000;
pub const RTL_OFFSET_HZ: f64 = -250_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Synthetic,
    RtlSdr,
    File,
    Qmx,
    Ic705,
}

impl Source {
    pub fn is_audio_radio(self) -> bool {
        matches!(self, Self::Qmx | Self::Ic705)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Ic705Settings {
    pub step_hz: u32,
    pub port: String,
    pub baud: u32,
    pub address: u8,
    pub input: String,
    pub output: String,
    pub tx_output: String,
    pub audio_gain_db: u8,
    #[serde(skip)]
    pub command: Option<(u64, sdr_devices::ic705::Command)>,
}
impl Default for Ic705Settings {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud: 19200,
            step_hz: 100,
            address: 0xa4,
            input: String::new(),
            output: String::new(),
            tx_output: String::new(),
            audio_gain_db: 0,
            command: None,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct QmxSettings {
    pub audio_gain_db: u8,
    pub port: String,
    pub input: String,
    pub output: String,
    pub command: Option<(u64, sdr_devices::qmx::Command)>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileSettings {
    pub path: std::path::PathBuf,
    pub sample_rate: u32,
    pub center_hz: u64,
}

#[derive(Clone)]
pub struct Settings {
    pub rtl_backend: sdr_devices::RtlBackend,
    pub spectrum_view: sdr_dsp::zoom_spectrum::SpectrumView,
    pub converter_offset_hz: i64,
    pub qmx: QmxSettings,
    pub ic705: Ic705Settings,
    pub revision: u64,
    pub running: bool,
    pub tone_hz: f64,
    pub source: Source,
    pub station_hz: u32,
    pub rtl_center_hz: Option<u32>,
    pub receiver: ReceiverConfig,
    pub file: Option<Arc<FileSettings>>,
    pub scan: Option<Arc<crate::scanning::ScanPlan>>,
    /// Optional WFM data service; independent of speaker mode and mute.
    pub rds_enabled: bool,
    pub cw_decoder: Option<sdr_dsp::cw::CwConfig>,
    pub cw_reset: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rtl_backend: sdr_devices::RtlBackend::default(),
            spectrum_view: sdr_dsp::zoom_spectrum::SpectrumView::full(SAMPLE_RATE as f64),
            converter_offset_hz: 0,
            qmx: QmxSettings::default(),
            ic705: Ic705Settings::default(),
            revision: 1,
            running: true,
            tone_hz: 125_000.0,
            source: Source::Synthetic,
            station_hz: 106_700_000,
            rtl_center_hz: None,
            receiver: ReceiverConfig::default(),
            file: None,
            scan: None,
            rds_enabled: true,
            cw_decoder: Some(sdr_dsp::cw::CwConfig::default()),
            cw_reset: 0,
        }
    }
}
impl Settings {
    pub fn tuner_center_hz(&self) -> Option<u32> {
        crate::tuning::tuner_frequency(self.center_hz(), self.converter_offset_hz)
    }
    pub fn center_hz(&self) -> u64 {
        if self.source == Source::RtlSdr {
            self.rtl_center_hz.map_or(
                (self.station_hz as u64 + 250_000).min(
                    crate::tuning::converter_bounds(self.converter_offset_hz)
                        .map_or(sdr_core::RX_MAX_HZ, |(_, max)| max) as u64,
                ),
                u64::from,
            )
        } else if self.source == Source::File {
            self.file.as_ref().map_or(CENTER_HZ, |f| f.center_hz)
        } else {
            CENTER_HZ
        }
    }
}

pub struct SpectrumFrame {
    /// Approximate FFT observation end, using the same host clock as audio.
    pub end_time: std::time::Duration,
    /// Receiver output sample clock, for aligned IQ-file decoding.
    pub stream_seconds: f64,
    /// FFT coordinates are independent of the capture and the audio receiver.
    pub view: sdr_dsp::zoom_spectrum::SpectrumView,
    pub analysis_rate_hz: f64,
    pub revision: u64,
    pub capture_revision: u64,
    pub sample_index: u64,
    pub center_hz: u64,
    pub row: u64,
    pub dbfs: Vec<f32>,
}

#[derive(Default)]
pub struct Update {
    pub rtl: Option<sdr_devices::RtlInfo>,
    pub ic705: Option<Arc<sdr_devices::ic705::State>>,
    pub qmx: Option<Arc<sdr_devices::qmx::State>>,
    pub revision: u64,
    pub capture_revision: u64,
    pub center_hz: u64,
    pub station_hz: u32,
    pub mode: Mode,
    pub receiver: ReceiverConfig,
    pub scan: crate::scanning::ScanStatus,
    pub channel_power_dbfs: Option<f32>,
    pub stereo_locked: bool,
    /// Belongs to this update's station, revision and capture. Cleared on gaps,
    /// retunes, disabling, mode changes and signal loss; never a text history.
    pub rds: Option<Arc<sdr_dsp::rds::RdsStatus>>,
    /// Bounded transcript for this tuning; cleared on discontinuity/reconfiguration.
    pub cw: Option<Arc<sdr_dsp::cw::CwStatus>>,
    pub running: bool,
    pub samples: u64,
    pub total_samples: Option<u64>,
    pub eof: bool,
    pub display_drops: u64,
    pub iq_drops: u64,
    pub retune_discarded: u64,
    pub audio_underruns: u64,
    pub audio_overruns: u64,
    pub audio_queue_frames: Option<usize>,
    pub audio_output_rate_hz: Option<u32>,
    pub audio_output_device: Option<String>,
    pub audio_error: Option<String>,
    pub audio_rms: f32,
    pub error: Option<String>,
    pub frame: Option<SpectrumFrame>,
}
