use sdr_dsp::keyer::{Contacts, Keyer, KeyerConfig, KeyerMode};

fn keyed(keyer: &mut Keyer, samples: usize) -> Vec<bool> {
    (0..samples).map(|_| keyer.next_key()).collect()
}

#[test]
fn straight_key_follows_contacts_without_paddle_timing_or_release_tail() {
    let mut keyer = Keyer::new(
        1000,
        KeyerConfig {
            mode: KeyerMode::Straight,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(keyed(&mut keyer, 100).iter().all(|&v| !v));
    keyer.contacts(Contacts {
        straight: true,
        ..Default::default()
    });
    assert!(keyed(&mut keyer, 83).iter().all(|&v| v));
    keyer.contacts(Contacts::default());
    assert!(keyed(&mut keyer, 200).iter().all(|&v| !v));
}

#[test]
fn paddles_repeat_exact_elements_and_a_b_differ_after_squeeze_release() {
    for mode in [KeyerMode::IambicA, KeyerMode::IambicB] {
        let mut keyer = Keyer::new(
            1000,
            KeyerConfig {
                mode,
                ..Default::default()
            },
        )
        .unwrap();
        keyer.contacts(Contacts {
            dit: true,
            dah: true,
            straight: false,
        });
        assert_eq!(
            keyed(&mut keyer, 120),
            [vec![true; 60], vec![false; 60]].concat()
        );
        assert!(keyed(&mut keyer, 30).iter().all(|&v| v));
        keyer.contacts(Contacts::default());
        assert_eq!(
            keyed(&mut keyer, 210),
            [vec![true; 150], vec![false; 60]].concat()
        );
        let tail = keyed(&mut keyer, 120);
        assert_eq!(
            tail,
            if mode == KeyerMode::IambicB {
                [vec![true; 60], vec![false; 60]].concat()
            } else {
                vec![false; 120]
            }
        );
        assert!(keyed(&mut keyer, 500).iter().all(|&v| !v));
    }
}

#[test]
fn a_brief_opposite_paddle_tap_is_remembered_during_an_element() {
    let mut keyer = Keyer::new(
        1000,
        KeyerConfig {
            mode: KeyerMode::IambicA,
            wpm: 30.0,
            ..Default::default()
        },
    )
    .unwrap();
    keyer.contacts(Contacts {
        dah: true,
        ..Default::default()
    });
    keyed(&mut keyer, 10);
    keyer.contacts(Contacts::default());
    keyer.contacts(Contacts {
        dit: true,
        ..Default::default()
    });
    keyed(&mut keyer, 5);
    keyer.contacts(Contacts::default());
    assert_eq!(
        keyed(&mut keyer, 225),
        [
            vec![true; 105],
            vec![false; 40],
            vec![true; 40],
            vec![false; 40]
        ]
        .concat()
    );
}

#[test]
fn cancellation_clears_mode_b_memory_and_tone_has_smooth_edges() {
    use sdr_dsp::keyer::Sidetone;
    let mut keyer = Keyer::new(
        48_000,
        KeyerConfig {
            mode: KeyerMode::IambicB,
            ..Default::default()
        },
    )
    .unwrap();
    keyer.contacts(Contacts {
        dit: true,
        dah: true,
        straight: false,
    });
    keyed(&mut keyer, 1000);
    keyer.cancel();
    assert!(keyed(&mut keyer, 48_000).iter().all(|&v| !v));
    let mut tone = Sidetone::new(48_000, 600.0);
    let samples: Vec<_> = (0..2000).map(|n| tone.sample(n < 1000)).collect();
    assert!(samples.iter().all(|v| v.is_finite() && v.abs() <= 0.5));
    assert!(samples.windows(2).all(|v| (v[1] - v[0]).abs() < 0.05));
    assert!(samples[1400..].iter().all(|v| *v == 0.0));
    assert!(Keyer::new(0, KeyerConfig::default()).is_err());
    assert!(
        Keyer::new(
            48_000,
            KeyerConfig {
                wpm: f32::NAN,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn a_new_gap_tap_after_mode_a_squeeze_release_is_not_discarded() {
    let mut keyer = Keyer::new(
        1000,
        KeyerConfig {
            mode: KeyerMode::IambicA,
            ..Default::default()
        },
    )
    .unwrap();
    keyer.contacts(Contacts {
        dit: true,
        dah: true,
        straight: false,
    });
    keyed(&mut keyer, 20);
    keyer.contacts(Contacts::default());
    keyed(&mut keyer, 50);
    keyer.contacts(Contacts {
        dah: true,
        ..Default::default()
    });
    keyed(&mut keyer, 5);
    keyer.contacts(Contacts::default());
    keyed(&mut keyer, 45);
    assert!(keyed(&mut keyer, 180).iter().all(|&v| v));
}
