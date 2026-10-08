//! Device adapters and synthetic acquisition.

pub mod file;
pub mod ic705;
pub mod qmx;
#[cfg(feature = "rtl")]
pub mod rtl;

mod session;
pub use session::{BlockLease, SyntheticSession};

use sdr_core::Iq;

#[derive(Clone, Copy, Debug)]
pub struct ToneConfig {
    pub sample_rate: u32,
    pub center_hz: u64,
    pub offset_hz: f64,
    pub amplitude: f32,
}

/// Deterministic oscillator. Filling a block never allocates.
pub struct ToneSource {
    config: ToneConfig,
    phase: f64,
}

impl ToneSource {
    pub fn new(config: ToneConfig) -> Result<Self, &'static str> {
        if config.sample_rate == 0
            || !config.offset_hz.is_finite()
            || config.offset_hz.abs() >= config.sample_rate as f64 / 2.0
        {
            return Err("tone must be strictly inside the nonzero sample rate's Nyquist band");
        }
        if !config.amplitude.is_finite() || !(0.0..=1.0).contains(&config.amplitude) {
            return Err("amplitude must be finite and between zero and one");
        }
        Ok(Self { config, phase: 0.0 })
    }

    pub fn fill(&mut self, samples: &mut [Iq]) {
        let step = std::f64::consts::TAU * self.config.offset_hz / self.config.sample_rate as f64;
        for sample in samples {
            let (q, i) = self.phase.sin_cos();
            *sample = Iq {
                i: i as f32 * self.config.amplitude,
                q: q as f32 * self.config.amplitude,
            };
            self.phase = (self.phase + step).rem_euclid(std::f64::consts::TAU);
        }
    }
}

mod rtl_backend;
pub use rtl_backend::{RtlBackend, RtlInfo};
