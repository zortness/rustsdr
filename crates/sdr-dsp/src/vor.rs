//! Experimental VOR radial measurement from wide AM envelope, before voice filters.
//! A radial is FROM the station. This is a signal-analysis prototype.
use rustfft::{FftPlanner, num_complex::Complex};
#[derive(Clone, Debug)]
pub struct Measurement {
    pub radial_deg: f64,
    pub reference_deviation_hz: f64,
    /// Fraction of reference FM energy explained by its 30 Hz component.
    pub coherence: f64,
}
/// Analyzes 1–3 seconds. FFT isolation forms the analytic 9960 Hz subcarrier;
/// discriminator and direct AM phases are measured over the same time window.
pub fn analyze_envelope(audio: &[f32], rate: f64) -> Option<Measurement> {
    if !rate.is_finite()
        || !(32_000.0..=96_000.0).contains(&rate)
        || audio.len() < rate as usize
        || audio.len() > (rate * 3.1) as usize
        || audio.iter().any(|v| !v.is_finite())
    {
        return None;
    }
    let n = audio.len();
    let mut planner = FftPlanner::<f64>::new();
    let mut spectrum: Vec<_> = audio.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    planner.plan_fft_forward(n).process(&mut spectrum);
    for (k, value) in spectrum.iter_mut().enumerate() {
        let hz = k as f64 * rate / n as f64;
        if !(8500.0..=11_400.0).contains(&hz) {
            *value = Complex::default();
        } else {
            *value *= 2.0 / n as f64;
        }
    }
    planner.plan_fft_inverse(n).process(&mut spectrum);
    let start = (rate * 0.1) as usize;
    let end = n - start;
    let count = end - start;
    let mut am = Complex::<f64>::default();
    let mut fm = Complex::<f64>::default();
    let mut energy = 0.0;
    let mut weight = 0.0;
    let mut subcarrier = 0.0;
    for i in start..end {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * (i - start) as f64 / count as f64).cos();
        let phase = -std::f64::consts::TAU * 30.0 * i as f64 / rate;
        am += Complex::from_polar(audio[i] as f64 * w, phase);
        let deviation =
            (spectrum[i] * spectrum[i - 1].conj()).arg() * rate / std::f64::consts::TAU - 9960.0;
        // Discriminator measures phase over the preceding sample interval.
        fm += Complex::from_polar(deviation * w, phase + std::f64::consts::PI * 30.0 / rate);
        energy += deviation * deviation * w;
        subcarrier += spectrum[i].norm() * w;
        weight += w;
    }
    let deviation = 2.0 * fm.norm() / weight;
    let coherence = (2.0 * fm.norm_sqr() / (weight * energy)).clamp(0.0, 1.0);
    let am_amplitude = 2.0 * am.norm() / weight;
    // No plausible 30 Hz/FM reference -> no bearing. Silence/noise must not
    // become a confident zero-degree measurement.
    if !(300.0..=650.0).contains(&deviation)
        || coherence < 0.65
        || am_amplitude < 1e-5
        || subcarrier / weight < 1e-5
        || am_amplitude < 0.08 * subcarrier / weight
    {
        return None;
    }
    Some(Measurement {
        radial_deg: (fm.arg() - am.arg()).to_degrees().rem_euclid(360.0),
        reference_deviation_hz: deviation,
        coherence,
    })
}

/// Fixed-rate channel IQ collector. Caller resets it on gaps and retunes.
#[derive(Default)]
pub struct Decoder {
    envelope: Vec<f32>,
}
impl Decoder {
    pub fn process(&mut self, iq: &[sdr_core::Iq]) -> Vec<Option<Measurement>> {
        let mut results = Vec::new();
        for s in iq {
            self.envelope.push(s.i.hypot(s.q));
            if self.envelope.len() == 48_000 {
                results.push(analyze_envelope(&self.envelope, 48_000.0));
                self.envelope.clear();
            }
        }
        results
    }
}
