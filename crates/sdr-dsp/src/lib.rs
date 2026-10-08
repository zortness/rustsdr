//! Headless sample processing.

pub mod channel;
mod spectrum;
pub mod vor;
pub mod zoom_spectrum;
pub use spectrum::SpectrumAnalyzer;
mod fm;
pub use fm::FmReceiver;
pub mod cw;
pub mod keyer;
pub mod rds;
mod receiver;
pub use receiver::{Mode, Receiver, ReceiverConfig, StereoFrame};

/// Continuous, anti-aliased 48 kHz monitor audio to 12 kHz decoder audio.
/// Reset on a stream gap, never at an audio callback boundary.
pub struct DecoderAudio(fm::Decimator);
impl Default for DecoderAudio {
    fn default() -> Self {
        Self(fm::Decimator::new(48_000.0, 5000.0, 129, 4))
    }
}
impl DecoderAudio {
    pub fn push(&mut self, sample: f32) -> Option<i16> {
        self.0
            .push(Iq { i: sample, q: 0.0 })
            .map(|v| (v.i.clamp(-1.0, 1.0) * 32767.0).round() as i16)
    }
}

use sdr_core::Iq;

/// Diagnostic estimator for a single steady tone, not a spectrum analyzer.
#[derive(Default)]
pub struct SignalMeter {
    count: u64,
    power: f64,
    correlation_i: f64,
    correlation_q: f64,
    previous: Option<Iq>,
}

#[derive(Debug)]
pub struct SignalReport {
    pub samples: u64,
    pub mean_power: f64,
    /// None for silence or insufficient samples. Valid for a single steady tone.
    pub tone_offset_hz: Option<f64>,
}

impl SignalMeter {
    /// Call reset at a discontinuity or generation change before processing.
    pub fn process(&mut self, samples: &[Iq]) {
        for &sample in samples {
            let (i, q) = (sample.i as f64, sample.q as f64);
            self.power += i * i + q * q;
            if let Some(previous) = self.previous {
                self.correlation_i += i * previous.i as f64 + q * previous.q as f64;
                self.correlation_q += q * previous.i as f64 - i * previous.q as f64;
            }
            self.previous = Some(sample);
            self.count += 1;
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn report(&self, sample_rate: u32) -> Option<SignalReport> {
        if self.count == 0 || sample_rate == 0 {
            return None;
        }
        let tone_offset_hz = if self.correlation_i.hypot(self.correlation_q) > 0.0 {
            Some(
                self.correlation_q.atan2(self.correlation_i) * sample_rate as f64
                    / std::f64::consts::TAU,
            )
        } else {
            None
        };
        Some(SignalReport {
            samples: self.count,
            mean_power: self.power / self.count as f64,
            tone_offset_hz,
        })
    }
}
