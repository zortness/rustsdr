use sdr_core::Iq;
use sdr_dsp::{Mode, Receiver, ReceiverConfig};
#[test]
fn channel_power_is_pre_audio_and_available_in_every_mode() {
    for mode in Mode::ALL {
        let config = ReceiverConfig::for_mode(mode);
        let frequency = match mode {
            Mode::Usb => 1000.0,
            Mode::Lsb => -1000.0,
            _ => 0.0,
        };
        let input: Vec<_> = (0..120_000)
            .map(|n| {
                let phase = std::f64::consts::TAU * frequency * n as f64 / 2_400_000.0;
                Iq {
                    i: 0.5 * phase.cos() as f32,
                    q: 0.5 * phase.sin() as f32,
                }
            })
            .collect();
        let mut receiver = Receiver::new(0.0, config).unwrap();
        receiver.process(&input, &mut Vec::new());
        assert!(
            (receiver.channel_power_dbfs() + 6.02).abs() < 1.0,
            "{mode:?}"
        );
        // CW's 257-tap filter at 6 kHz retains 43 ms of history, followed
        // by the power smoother. Allow 100 ms to reach the numerical floor.
        let silence = if mode == Mode::Cw { 240_000 } else { 120_000 };
        receiver.process(&vec![Iq::default(); silence], &mut Vec::new());
        assert!(receiver.channel_power_dbfs() < -100.0, "{mode:?}");
    }
}
