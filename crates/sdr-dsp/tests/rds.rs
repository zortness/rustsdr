use sdr_dsp::rds::RdsDecoder;
#[path = "support/rds_signal.rs"]
mod signal;
use signal::*;
#[test]
fn multiplex_tap_decodes_through_rf_filter_without_altering_mono_or_stereo_audio() {
    use sdr_dsp::{Mode, Receiver, ReceiverConfig};
    let signal = multiplex(&bits(&program().repeat(3)), 15.0, 30.0);
    let mut phase = 0.0f64;
    for mode in [Mode::WfmMono, Mode::WfmStereo] {
        let mut tapped = Receiver::new(-250_000.0, ReceiverConfig::for_mode(mode)).unwrap();
        let mut listening = Receiver::new(-250_000.0, ReceiverConfig::for_mode(mode)).unwrap();
        let mut decoder = RdsDecoder::new();
        let mut audio = Vec::new();
        let mut baseline = Vec::new();
        let mut mpx = Vec::new();
        for chunk in signal.chunks(1024) {
            let mut iq = Vec::new();
            for &value in chunk {
                for _ in 0..10 {
                    phase = (phase
                        + std::f64::consts::TAU * (-250_000.0 + 75_000.0 * value as f64)
                            / 2_400_000.0)
                        .rem_euclid(std::f64::consts::TAU);
                    iq.push(sdr_core::Iq {
                        i: 0.6 * phase.cos() as f32,
                        q: 0.6 * phase.sin() as f32,
                    });
                }
            }
            audio.clear();
            baseline.clear();
            mpx.clear();
            tapped.process_with_multiplex(&iq, &mut audio, &mut mpx);
            listening.process(&iq, &mut baseline);
            assert_eq!(audio, baseline, "tap must not change speaker samples");
            decoder.process(&mpx);
        }
        assert_eq!(
            decoder.status().program_service.as_deref(),
            Some("RUST FM"),
            "{mode:?}: {:?}",
            decoder.status()
        );
        assert_eq!(decoder.status().radio_text.as_deref(), Some("Hello radio!"));
    }
}
#[test]
fn decodes_station_and_text_from_multiplex_with_carrier_and_clock_error() {
    let mut decoder = RdsDecoder::new();
    let signal = multiplex(&bits(&program().repeat(4)), 23.0, 45.0);
    for chunk in signal.chunks(1379) {
        decoder.process(chunk);
    }
    let state = decoder.status();
    assert!(state.locked, "{state:?}");
    assert_eq!(state.pi, Some(0x1234));
    assert_eq!(state.program_service.as_deref(), Some("RUST FM"));
    assert_eq!(state.radio_text.as_deref(), Some("Hello radio!"));
    assert_eq!(state.program_type, Some(10));
    assert_eq!(state.traffic_program, Some(true));
}

#[test]
fn rejects_corrupt_groups_and_noise_and_clears_on_signal_loss() {
    let mut decoder = RdsDecoder::new();
    let mut damaged = bits(&program().repeat(3));
    for group in damaged.chunks_exact_mut(104) {
        group[80] = !group[80];
    }
    decoder.process(&multiplex(&damaged, 0.0, 0.0));
    assert_eq!(
        decoder.status().groups,
        0,
        "damaged blocks must not publish text"
    );
    decoder.process(&multiplex(&bits(&program().repeat(3)), -30.0, -60.0));
    assert_eq!(decoder.status().program_service.as_deref(), Some("RUST FM"));
    let mut noise = 0x76543210u32;
    let samples: Vec<f32> = (0..960_000)
        .map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            (noise as i32 as f32) / (i32::MAX as f32) * 0.1
        })
        .collect();
    decoder.process(&samples);
    assert!(!decoder.status().locked);
    assert_eq!(decoder.status().program_service, None);
    assert_eq!(decoder.status().radio_text, None);
    let mut noise_only = RdsDecoder::new();
    noise_only.process(&samples);
    assert_eq!(noise_only.status().groups, 0);
}

#[test]
fn handles_version_b_text_ab_changes_and_new_station_identity() {
    let mut decoder = RdsDecoder::new();
    decoder.process(&multiplex(&bits(&program().repeat(3)), 0.0, 0.0));
    let groups = [
        [0x1234, 0x2810, 0x1234, 0x4e65],
        [0x1234, 0x2811, 0x1234, 0x770d],
    ];
    decoder.process(&multiplex(&bits(&groups.repeat(4)), 0.0, 0.0));
    assert_eq!(decoder.status().radio_text.as_deref(), Some("New"));
    // Changing A/B invalidates old text before all new segments arrive.
    decoder.process(&multiplex(
        &bits(&[[0x1234, 0x2801, 0x1234, 0x7478]; 4]),
        0.0,
        0.0,
    ));
    assert_eq!(decoder.status().radio_text, None);
    decoder.process(&multiplex(
        &bits(&[[0x4321, 0x800, 0x4321, 0x4142]; 4]),
        0.0,
        0.0,
    ));
    assert_eq!(decoder.status().pi, Some(0x4321));
    assert_eq!(decoder.status().program_service, None);
    assert_eq!(decoder.status().radio_text, None);
}
