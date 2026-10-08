use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use sdr_core::Iq;
use std::sync::Arc;

/// Periodic Hann window, FFT-shifted output (-Fs/2 through Fs/2 - Fs/N).
/// A bin-centered complex tone of magnitude 1 has a peak of 0 dBFS.
/// This is coherent-tone scaling, not a noise power spectral density.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex32>,
    scratch: Vec<Complex32>,
    output: Vec<f32>,
    normalization: f32,
}

impl SpectrumAnalyzer {
    pub fn values(&self) -> &[f32] {
        &self.output
    }
    pub fn new(size: usize) -> Result<Self, &'static str> {
        if !(16..=65_536).contains(&size) || !size.is_power_of_two() {
            return Err("FFT size must be a power of two from 16 through 65536");
        }
        let fft = FftPlanner::new().plan_fft_forward(size);
        let window: Vec<_> = (0..size)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / size as f32).cos())
            .collect();
        let normalization = window.iter().sum::<f32>().powi(2);
        let scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        Ok(Self {
            fft,
            window,
            buffer: vec![Complex32::default(); size],
            scratch,
            output: vec![-140.0; size],
            normalization,
        })
    }

    /// Reuses all working storage. Input must contain exactly one FFT frame.
    pub fn analyze(&mut self, samples: &[Iq]) -> Result<&[f32], &'static str> {
        if samples.len() != self.buffer.len() {
            return Err("sample count must match FFT size");
        }
        for ((dest, source), window) in self.buffer.iter_mut().zip(samples).zip(&self.window) {
            *dest = Complex32::new(source.i * window, source.q * window);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let size = self.buffer.len();
        for (bin, output) in self.output.iter_mut().enumerate() {
            let power = self.buffer[(bin + size / 2) % size].norm_sqr() / self.normalization;
            *output = 10.0 * power.max(1e-14).log10();
        }
        Ok(&self.output)
    }
}
