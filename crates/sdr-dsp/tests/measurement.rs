use sdr_core::Iq;
use sdr_dsp::SignalMeter;

#[test]
fn measures_power_and_signed_frequency_across_blocks() {
    let mut meter = SignalMeter::default();
    let samples: Vec<_> = (0..1000)
        .map(|n| {
            let phase = std::f64::consts::TAU * -3000.0 * n as f64 / 48_000.0;
            Iq {
                i: (0.5 * phase.cos()) as f32,
                q: (0.5 * phase.sin()) as f32,
            }
        })
        .collect();
    assert!(meter.report(48_000).is_none());
    for chunk in samples.chunks(127) {
        meter.process(chunk);
    }
    let result = meter.report(48_000).unwrap();
    assert_eq!(result.samples, 1000);
    assert!((result.mean_power - 0.25).abs() < 1e-6);
    assert!((result.tone_offset_hz.unwrap() + 3000.0).abs() < 0.01);
}
