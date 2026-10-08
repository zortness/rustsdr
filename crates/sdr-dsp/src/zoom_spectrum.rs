//! Streaming zoom FFT: frequency translation, anti-alias filtering, then decimation.
//! Changes affect only analysis; no audio receiver or capture state is owned here.
use crate::SpectrumAnalyzer;
use sdr_core::Iq;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectrumView {
    pub center_offset_hz: f64,
    pub span_hz: f64,
}
impl SpectrumView {
    pub fn full(sample_rate: f64) -> Self {
        Self {
            center_offset_hz: 0.0,
            span_hz: sample_rate,
        }
    }
    pub fn validate(self, sample_rate: f64) -> bool {
        sample_rate.is_finite()
            && sample_rate >= 1000.0
            && self.center_offset_hz.is_finite()
            && (200.0..=sample_rate).contains(&self.span_hz)
            && self.center_offset_hz.abs() + self.span_hz / 2.0 <= sample_rate / 2.0 + 0.001
    }
}
struct HalfBand {
    samples: [Iq; 64],
    position: usize,
    phase: bool,
    coefficients: [f32; 16],
    center: f32,
}
impl HalfBand {
    fn new() -> Self {
        let coefficient = |n: usize| {
            let x = n as f64 - 31.0;
            let sinc = if x == 0.0 {
                0.5
            } else {
                (std::f64::consts::PI * x / 2.0).sin() / (std::f64::consts::PI * x)
            };
            let angle = std::f64::consts::TAU * n as f64 / 62.0;
            sinc * (0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos())
        };
        let sum: f64 = (0..63).map(coefficient).sum();
        Self {
            samples: [Iq::default(); 64],
            position: 0,
            phase: false,
            coefficients: std::array::from_fn(|k| (coefficient(k * 2) / sum) as f32),
            center: (0.5 / sum) as f32,
        }
    }
    fn push(&mut self, sample: Iq) -> Option<Iq> {
        self.samples[self.position] = sample;
        self.position = (self.position + 1) & 63;
        self.phase = !self.phase;
        if self.phase {
            return None;
        }
        let middle = self.samples[self.position.wrapping_sub(32) & 63];
        let mut result = Iq {
            i: middle.i * self.center,
            q: middle.q * self.center,
        };
        for (k, &c) in self.coefficients.iter().enumerate() {
            let a = self.samples[self.position.wrapping_sub(1 + 2 * k) & 63];
            let b = self.samples[self.position.wrapping_sub(63 - 2 * k) & 63];
            result.i += (a.i + b.i) * c;
            result.q += (a.q + b.q) * c;
        }
        Some(result)
    }
}
pub struct ZoomSpectrum {
    view: SpectrumView,
    rate: f64,
    stages: Vec<HalfBand>,
    fft: SpectrumAnalyzer,
    ring: Vec<Iq>,
    linear: Vec<Iq>,
    position: usize,
    filled: usize,
    since: usize,
    hop: usize,
    settling: usize,
    oscillator: (f64, f64),
    rotation: (f64, f64),
    phase_count: usize,
    ready: bool,
}
impl ZoomSpectrum {
    pub fn new(sample_rate: f64, view: SpectrumView) -> Result<Self, &'static str> {
        if !view.validate(sample_rate) {
            return Err("Invalid spectrum view");
        }
        let mut rate = sample_rate;
        let mut stages = Vec::new();
        // Keep the visible span inside the flat central half of the final filter.
        while rate / 2.0 >= view.span_hz * 2.0 {
            stages.push(HalfBand::new());
            rate /= 2.0;
        }
        let mut size = 4096;
        while size > 256 && size as f64 / rate > 0.5 {
            size /= 2;
        }
        let hop = (rate / 30.0).round() as usize;
        let angle = -std::f64::consts::TAU * view.center_offset_hz / sample_rate;
        Ok(Self {
            view,
            rate,
            settling: if stages.is_empty() { 0 } else { 64 },
            stages,
            fft: SpectrumAnalyzer::new(size)?,
            ring: vec![Iq::default(); size],
            linear: vec![Iq::default(); size],
            position: 0,
            filled: 0,
            since: 0,
            hop: hop.max(size / 4),
            oscillator: (1.0, 0.0),
            rotation: (angle.cos(), angle.sin()),
            phase_count: 0,
            ready: false,
        })
    }
    pub fn view(&self) -> SpectrumView {
        self.view
    }
    pub fn sample_rate(&self) -> f64 {
        self.rate
    }
    pub fn bin_hz(&self) -> f64 {
        self.rate / self.ring.len() as f64
    }
    pub fn observation_seconds(&self) -> f64 {
        self.ring.len() as f64 / self.rate
    }
    pub fn row_seconds(&self) -> f64 {
        self.hop as f64 / self.rate
    }
    pub fn process(&mut self, input: &[Iq]) -> Option<&[f32]> {
        self.ready = false;
        for &sample in input {
            let mut sample = if self.view.center_offset_hz == 0.0 {
                sample
            } else {
                let (i, q) = self.oscillator;
                let translated = Iq {
                    i: (sample.i as f64 * i - sample.q as f64 * q) as f32,
                    q: (sample.i as f64 * q + sample.q as f64 * i) as f32,
                };
                let (ri, rq) = self.rotation;
                self.oscillator = (i * ri - q * rq, i * rq + q * ri);
                self.phase_count += 1;
                if self.phase_count.is_multiple_of(4096) {
                    let norm = self.oscillator.0.hypot(self.oscillator.1);
                    self.oscillator.0 /= norm;
                    self.oscillator.1 /= norm;
                }
                translated
            };
            let mut emit = true;
            for stage in &mut self.stages {
                match stage.push(sample) {
                    Some(next) => sample = next,
                    None => {
                        emit = false;
                        break;
                    }
                }
            }
            if !emit {
                continue;
            }
            if self.settling > 0 {
                self.settling -= 1;
                continue;
            }
            self.ring[self.position] = sample;
            self.position = (self.position + 1) % self.ring.len();
            self.since += 1;
            self.filled = (self.filled + 1).min(self.ring.len());
            if self.filled == self.ring.len() && self.since >= self.hop {
                let tail = self.ring.len() - self.position;
                self.linear[..tail].copy_from_slice(&self.ring[self.position..]);
                self.linear[tail..].copy_from_slice(&self.ring[..self.position]);
                self.fft.analyze(&self.linear).expect("fixed analysis size");
                self.since = 0;
                self.ready = true;
            }
        }
        if self.ready {
            Some(self.fft.values())
        } else {
            None
        }
    }
}
