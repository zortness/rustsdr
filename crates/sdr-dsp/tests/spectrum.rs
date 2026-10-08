use sdr_core::Iq;
use sdr_dsp::SpectrumAnalyzer;

#[test]
fn hann_spectrum_has_shifted_bins_and_coherent_tone_dbfs() {
    for size in [256, 4096] {
        for bin in [-31, 0, 29] {
            let samples: Vec<_> = (0..size)
                .map(|n| {
                    let phase = std::f64::consts::TAU * bin as f64 * n as f64 / size as f64;
                    Iq {
                        i: (0.5 * phase.cos()) as f32,
                        q: (0.5 * phase.sin()) as f32,
                    }
                })
                .collect();
            let mut fft = SpectrumAnalyzer::new(size).unwrap();
            let spectrum = fft.analyze(&samples).unwrap();
            let peak = spectrum
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap();
            assert_eq!(peak.0, (size as isize / 2 + bin) as usize);
            assert!((*peak.1 + 6.0206).abs() < 0.001);
        }
    }
}

#[test]
fn silence_is_finite_and_invalid_lengths_are_rejected() {
    assert!(SpectrumAnalyzer::new(0).is_err());
    assert!(SpectrumAnalyzer::new(3).is_err());
    let mut fft = SpectrumAnalyzer::new(256).unwrap();
    assert!(fft.analyze(&[Iq::default(); 10]).is_err());
    assert!(
        fft.analyze(&[Iq::default(); 256])
            .unwrap()
            .iter()
            .all(|value| *value == -140.0)
    );
}
