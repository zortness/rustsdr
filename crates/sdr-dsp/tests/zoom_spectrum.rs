use sdr_core::Iq;
use sdr_dsp::zoom_spectrum::{SpectrumView, ZoomSpectrum};

#[test]
fn narrow_view_resolves_two_close_tones_at_an_offset() {
    let mut spectrum = ZoomSpectrum::new(
        48_000.0,
        SpectrumView {
            center_offset_hz: 8000.0,
            span_hz: 1000.0,
        },
    )
    .unwrap();
    let input: Vec<_> = (0..48_000)
        .map(|n| {
            let a = std::f64::consts::TAU * 7990.0 * n as f64 / 48000.0;
            let b = std::f64::consts::TAU * 8010.0 * n as f64 / 48000.0;
            Iq {
                i: (0.2 * (a.cos() + b.cos())) as f32,
                q: (0.2 * (a.sin() + b.sin())) as f32,
            }
        })
        .collect();
    let mut last = Vec::new();
    for chunk in input.chunks(137) {
        if let Some(values) = spectrum.process(chunk) {
            last = values.to_vec();
        }
    }
    let spacing = spectrum.bin_hz();
    assert!(spacing < 4.0);
    let middle = last.len() / 2;
    let delta = (10.0 / spacing).round() as usize;
    assert!(
        last[middle - delta] > -18.0,
        "left {}",
        last[middle - delta]
    );
    assert!(
        last[middle + delta] > -18.0,
        "right {}",
        last[middle + delta]
    );
    assert!(last[middle] < -30.0, "valley {}", last[middle]);
}

#[test]
fn decimation_rejects_out_of_band_aliases_and_is_chunk_independent() {
    let view = SpectrumView {
        center_offset_hz: 0.0,
        span_hz: 1000.0,
    };
    let input: Vec<_> = (0..48_000)
        .map(|n| {
            let a = std::f64::consts::TAU * 3100.0 * n as f64 / 48000.0;
            Iq {
                i: a.cos() as f32,
                q: a.sin() as f32,
            }
        })
        .collect();
    let mut a = ZoomSpectrum::new(48000.0, view).unwrap();
    let full = a.process(&input).unwrap().to_vec();
    let mut b = ZoomSpectrum::new(48000.0, view).unwrap();
    let mut split = Vec::new();
    for chunk in input.chunks(71) {
        if let Some(values) = b.process(chunk) {
            split = values.to_vec();
        }
    }
    assert_eq!(full, split);
    assert!(
        full.iter().copied().fold(-140.0, f32::max) < -60.0,
        "alias peak {}",
        full.iter().copied().fold(-140.0, f32::max)
    );
    assert!(a.observation_seconds() <= 0.5);
    assert!(
        ZoomSpectrum::new(
            48000.0,
            SpectrumView {
                center_offset_hz: 24000.0,
                span_hz: 1000.0
            }
        )
        .is_err()
    );
}
