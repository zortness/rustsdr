use sdr_core::Iq;
use sdr_dsp::channel::{Channel, ChannelSpec};
#[test]
fn selects_channel_and_preserves_clock_across_chunks() {
    let spec = ChannelSpec {
        offset_hz: 250_000.0,
        sample_rate: 48_000,
        bandwidth_hz: 32_000,
    };
    let mut channel = Channel::new(2_400_000, spec).unwrap();
    let mut output = Vec::new();
    let input: Vec<_> = (0..240_000)
        .map(|n| {
            let phase = std::f64::consts::TAU * 251_000.0 * n as f64 / 2_400_000.0;
            Iq {
                i: phase.cos() as f32,
                q: phase.sin() as f32,
            }
        })
        .collect();
    for chunk in input.chunks(997) {
        channel.process(chunk, &mut output);
    }
    assert_eq!(output.len(), 4800);
    let mut meter = sdr_dsp::SignalMeter::default();
    meter.process(&output[1000..]);
    let report = meter.report(48_000).unwrap();
    assert!((report.tone_offset_hz.unwrap() - 1000.0).abs() < 1.0);
    assert!(report.mean_power > 0.9);
    assert!(
        Channel::new(
            2_400_000,
            ChannelSpec {
                offset_hz: 1_199_999.0,
                ..spec
            }
        )
        .is_err()
    );
}
