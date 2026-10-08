use sdr_core::Iq;
use sdr_dsp::FmReceiver;

fn fixture(count: usize, offset: f64) -> Vec<Iq> {
    let mut phase = 0.0_f64;
    (0..count)
        .map(|n| {
            let message = (std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0).sin();
            phase += std::f64::consts::TAU * (offset + 37_500.0 * message) / 2_400_000.0;
            Iq {
                i: phase.cos() as f32 * 0.5,
                q: phase.sin() as f32 * 0.5,
            }
        })
        .collect()
}

#[test]
fn recovers_offset_fm_tone_and_is_independent_of_block_boundaries() {
    let iq = fixture(480_000, -250_000.0);
    let mut whole = Vec::new();
    FmReceiver::new(-250_000.0, 0)
        .unwrap()
        .process(&iq, &mut whole);
    let mut chunks = Vec::new();
    let mut receiver = FmReceiver::new(-250_000.0, 0).unwrap();
    for chunk in iq.chunks(4093) {
        receiver.process(chunk, &mut chunks);
    }
    assert_eq!(whole.len(), 9600);
    assert_eq!(whole, chunks);
    let audio = &whole[4800..];
    let (mut sin, mut cos, mut energy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (n, &value) in audio.iter().enumerate() {
        let phase = std::f64::consts::TAU * 1000.0 * n as f64 / 48_000.0;
        sin += value as f64 * phase.sin();
        cos += value as f64 * phase.cos();
        energy += (value as f64).powi(2);
    }
    let amplitude = 2.0 * sin.hypot(cos) / audio.len() as f64;
    assert!((amplitude - 0.5).abs() < 0.025, "amplitude {amplitude}");
    let signal_energy = audio.len() as f64 * amplitude * amplitude / 2.0;
    assert!(
        signal_energy / energy > 0.99,
        "recovered audio is not a clean 1 kHz tone"
    );
}

#[test]
fn rejects_invalid_receiver_configuration() {
    assert!(FmReceiver::new(f64::NAN, 75).is_err());
    assert!(FmReceiver::new(0.0, 20).is_err());
}
