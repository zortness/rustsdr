use sdr_core::Iq;
use sdr_dsp::{Mode, Receiver, ReceiverConfig, StereoFrame};

fn amplitude(audio: &[StereoFrame], hz: f64, right: bool) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, s) in audio.iter().enumerate() {
        let phase = std::f64::consts::TAU * hz * n as f64 / 48_000.0;
        let v = if right { s.right } else { s.left } as f64;
        re += v * phase.cos();
        im += v * phase.sin();
    }
    2.0 * re.hypot(im) / audio.len() as f64
}

#[test]
fn stereo_endpoint_mapping_and_invalid_configurations_are_explicit() {
    let frame = StereoFrame {
        left: 0.2,
        right: 0.6,
    };
    assert_eq!(frame.channel(0, 2), 0.2);
    assert_eq!(frame.channel(1, 2), 0.6);
    assert_eq!(frame.channel(0, 1), 0.4);
    assert_eq!(frame.channel(2, 6), 0.0);
    assert!(Receiver::new(f64::NAN, ReceiverConfig::default()).is_err());
    assert!(Receiver::new(1_100_000.0, ReceiverConfig::default()).is_err());
    assert!(
        Receiver::new(
            0.0,
            ReceiverConfig {
                bandwidth_hz: 0,
                ..ReceiverConfig::default()
            }
        )
        .is_err()
    );
    assert!(
        Receiver::new(
            0.0,
            ReceiverConfig {
                squelch_db: Some(f32::NAN),
                ..ReceiverConfig::for_mode(Mode::Nfm)
            }
        )
        .is_err()
    );
}

#[test]
fn stereo_without_pilot_plays_mono_and_all_modes_are_chunk_invariant() {
    let mut phase = 0.0_f64;
    let iq: Vec<_> = (0..480_000)
        .map(|n| {
            phase += std::f64::consts::TAU
                * 15000.0
                * (std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0).sin()
                / 2_400_000.0;
            Iq {
                i: phase.cos() as f32 * 0.5,
                q: phase.sin() as f32 * 0.5,
            }
        })
        .collect();
    for mode in Mode::ALL {
        let config = ReceiverConfig {
            deemphasis_us: 0,
            ..ReceiverConfig::for_mode(mode)
        };
        let mut whole = Receiver::new(0.0, config).unwrap();
        let mut split = Receiver::new(0.0, config).unwrap();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        whole.process(&iq, &mut a);
        for chunk in iq.chunks(4093) {
            split.process(chunk, &mut b);
        }
        assert_eq!(a, b, "{mode:?}");
        assert_eq!(a.len(), 9600);
        if mode == Mode::WfmStereo {
            assert!(!whole.stereo_locked());
            assert!(a.iter().all(|s| s.left == s.right));
            assert!((amplitude(&a[4800..], 1000.0, false) - 0.2).abs() < 0.025);
        }
    }
}

#[test]
fn am_recovers_modulation_without_carrier_dc_and_preserves_chunks() {
    let iq: Vec<_> = (0..480_000)
        .map(|n| {
            let t = n as f64 / 2_400_000.0;
            let a = 0.5 * (1.0 + 0.5 * (std::f64::consts::TAU * 1000.0 * t).sin());
            let p = std::f64::consts::TAU * -250_000.0 * t;
            Iq {
                i: (a * p.cos()) as f32,
                q: (a * p.sin()) as f32,
            }
        })
        .collect();
    let config = ReceiverConfig {
        agc: false,
        ..ReceiverConfig::for_mode(Mode::Am)
    };
    let mut a = Receiver::new(-250_000.0, config).unwrap();
    let mut b = Receiver::new(-250_000.0, config).unwrap();
    let (mut whole, mut split) = (Vec::new(), Vec::new());
    a.process(&iq, &mut whole);
    for chunk in iq.chunks(4093) {
        b.process(chunk, &mut split);
    }
    assert_eq!(whole, split);
    assert_eq!(whole.len(), 9600);
    assert!((amplitude(&whole[4800..], 1000.0, false) - 0.25).abs() < 0.025);
    assert!(
        whole[4800..]
            .iter()
            .map(|s| s.left as f64)
            .sum::<f64>()
            .abs()
            / 4800.0
            < 0.005
    );
}

#[test]
fn am_and_ssb_agc_normalize_different_signal_levels() {
    for mode in [Mode::Am, Mode::Usb, Mode::Lsb] {
        let mut levels = Vec::new();
        for level in [0.05, 0.5] {
            let input: Vec<_> = (0..1_200_000)
                .map(|n| {
                    let phase = std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0;
                    if mode == Mode::Am {
                        Iq {
                            i: (level * (1.0 + 0.5 * phase.sin())) as f32,
                            q: 0.0,
                        }
                    } else {
                        Iq {
                            i: (level * phase.cos()) as f32,
                            q: (level * phase.sin() * if mode == Mode::Usb { 1.0 } else { -1.0 })
                                as f32,
                        }
                    }
                })
                .collect();
            let mut receiver = Receiver::new(0.0, ReceiverConfig::for_mode(mode)).unwrap();
            let mut audio = Vec::new();
            receiver.process(&input, &mut audio);
            assert!(
                audio
                    .iter()
                    .all(|s| s.left.is_finite() && s.left.abs() <= 1.0)
            );
            levels.push(amplitude(&audio[19_200..], 1000.0, false));
        }
        assert!(levels[0] > 0.1 && levels[0] < 0.25, "{mode:?}: {levels:?}");
        assert!(
            (levels[0] / levels[1] - 1.0).abs() < 0.1,
            "{mode:?}: {levels:?}"
        );
    }
}

#[test]
fn narrow_fm_recovers_tone_and_squelch_closes_for_silence() {
    let config = ReceiverConfig {
        squelch_db: Some(-60.0),
        ..ReceiverConfig::for_mode(Mode::Nfm)
    };
    let mut receiver = Receiver::new(0.0, config).unwrap();
    let mut phase = 0.0;
    let input: Vec<_> = (0..480_000)
        .map(|n| {
            phase += std::f64::consts::TAU
                * 1250.0
                * (std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0).sin()
                / 2_400_000.0;
            Iq {
                i: phase.cos() as f32 * 0.5,
                q: phase.sin() as f32 * 0.5,
            }
        })
        .collect();
    let mut output = Vec::new();
    receiver.process(&input, &mut output);
    assert!((amplitude(&output[4800..], 1000.0, false) - 0.5).abs() < 0.04);
    output.clear();
    receiver.process(&vec![Iq::default(); 960_000], &mut output);
    assert!(output[14_400..].iter().all(|s| s.left.abs() < 0.0001));
}

#[test]
fn ssb_selects_correct_sideband() {
    let input: Vec<_> = (0..480_000)
        .map(|n| {
            let t = std::f64::consts::TAU * n as f64 / 2_400_000.0;
            Iq {
                i: (0.3 * (1000.0 * t).cos() + 0.3 * (-2000.0 * t).cos()) as f32,
                q: (0.3 * (1000.0 * t).sin() + 0.3 * (-2000.0 * t).sin()) as f32,
            }
        })
        .collect();
    for (mode, wanted, unwanted) in [(Mode::Usb, 1000.0, 2000.0), (Mode::Lsb, 2000.0, 1000.0)] {
        let mut receiver = Receiver::new(
            0.0,
            ReceiverConfig {
                agc: false,
                ..ReceiverConfig::for_mode(mode)
            },
        )
        .unwrap();
        let mut output = Vec::new();
        receiver.process(&input, &mut output);
        let a = amplitude(&output[4800..], wanted, false);
        let b = amplitude(&output[4800..], unwanted, false);
        assert!((a - 0.3).abs() < 0.03, "{mode:?}: {a}");
        assert!(b / a < 0.01, "opposite sideband leaked: {mode:?} {b}/{a}");
    }
}

#[test]
fn stereo_wfm_locks_and_separates_left_and_right_then_falls_back_to_mono() {
    let mut phase = 0.0_f64;
    let input: Vec<_> = (0..2_400_000)
        .map(|n| {
            let t = n as f64 / 2_400_000.0;
            let l = 0.3 * (std::f64::consts::TAU * 1000.0 * t).sin();
            let r = 0.3 * (std::f64::consts::TAU * 2000.0 * t).sin();
            let pilot_phase = std::f64::consts::TAU * 19002.0 * t + 0.7;
            let multiplex =
                (l + r) * 0.5 + (l - r) * 0.5 * (2.0 * pilot_phase).cos() + 0.1 * pilot_phase.cos();
            phase += std::f64::consts::TAU * 75000.0 * multiplex / 2_400_000.0;
            Iq {
                i: phase.cos() as f32 * 0.5,
                q: phase.sin() as f32 * 0.5,
            }
        })
        .collect();
    let mut receiver = Receiver::new(
        0.0,
        ReceiverConfig {
            deemphasis_us: 0,
            ..ReceiverConfig::for_mode(Mode::WfmStereo)
        },
    )
    .unwrap();
    let mut audio = Vec::new();
    for chunk in input.chunks(8191) {
        receiver.process(chunk, &mut audio);
    }
    assert!(receiver.stereo_locked(), "pilot not locked");
    let audio = &audio[36_000..];
    for (right, desired, leak) in [(false, 1000.0, 2000.0), (true, 2000.0, 1000.0)] {
        let a = amplitude(audio, desired, right);
        let b = amplitude(audio, leak, right);
        assert!((a - 0.3).abs() < 0.04, "channel gain {a}");
        assert!(b / a < 0.032, "stereo leakage {b}/{a}");
    }
    let mut silence = Vec::new();
    receiver.process(&vec![Iq::default(); 2_400_000], &mut silence);
    assert!(!receiver.stereo_locked());
    assert!(
        silence[36_000..]
            .iter()
            .all(|s| s.left.abs() < 0.001 && s.right.abs() < 0.001)
    );
}
