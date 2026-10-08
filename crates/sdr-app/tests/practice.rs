use sdr_app::practice::{MidiBinding, MidiEvent, PracticeSession, midi_contacts, midi_event};
use sdr_dsp::keyer::{KeyerConfig, KeyerMode};
use std::time::{Duration, Instant};

#[test]
fn midi_note_off_velocity_zero_and_cc_switches_match_channels_and_release() {
    let note = MidiBinding::Note {
        channel: 2,
        note: 60,
    };
    let cc = MidiBinding::Control {
        channel: 2,
        control: 64,
    };
    let bindings = [Some(note), Some(cc), None];
    let mut held = [false; 3];
    for bytes in [[0x92, 60, 100], [0xb2, 64, 127]] {
        midi_contacts(&bindings, &mut held, midi_event(&bytes).unwrap());
    }
    assert_eq!(held, [true, true, false]);
    midi_contacts(&bindings, &mut held, midi_event(&[0x91, 60, 0]).unwrap());
    assert!(held[0], "wrong channel must not release");
    midi_contacts(&bindings, &mut held, midi_event(&[0x92, 60, 0]).unwrap());
    assert!(!held[0], "velocity-zero Note On means Note Off");
    midi_contacts(&bindings, &mut held, midi_event(&[0xb2, 64, 63]).unwrap());
    assert!(!held[1]);
    midi_contacts(&bindings, &mut held, midi_event(&[0x92, 60, 100]).unwrap());
    midi_contacts(&bindings, &mut held, midi_event(&[0x82, 60, 127]).unwrap());
    assert!(!held[0]);
    assert_eq!(
        midi_event(&[0xb2, 123, 0]),
        Some(MidiEvent::ReleaseChannel(2))
    );
    assert_eq!(midi_event(&[0xff]), Some(MidiEvent::ReleaseAll));
    assert_eq!(midi_event(&[0xf8]), None);
    assert_eq!(midi_event(&[0x90, 60]), None);
    assert_eq!(midi_event(&[0x90, 60, 255]), None);
}

fn pump(session: &PracticeSession, millis: u64) {
    let until = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < until {
        session.input.heartbeat();
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn local_practice_decodes_while_muted_combines_sources_and_watchdog_releases() {
    let session = PracticeSession::start(
        KeyerConfig {
            mode: KeyerMode::Straight,
            ..Default::default()
        },
        0.0,
        false,
    )
    .unwrap();
    pump(&session, 30);
    assert!(session.status().ready);
    session.input.set(0, [true, false, false]);
    pump(&session, 30);
    session.input.set(1, [true, false, false]);
    session.input.set(0, [false; 3]);
    pump(&session, 30);
    assert!(
        session.status().key_down,
        "keyboard release must not release MIDI"
    );
    session.input.set(1, [false; 3]);
    pump(&session, 250);
    assert_eq!(session.status().decoded.unwrap().text, "E");
    session.input.set(0, [true, false, false]);
    pump(&session, 50);
    std::thread::sleep(Duration::from_millis(420));
    let status = session.status();
    assert!(!status.running && !status.key_down);
    assert!(status.error.is_some());
}

#[test]
fn overwhelmed_input_queue_stops_instead_of_leaving_a_stuck_contact() {
    let session = PracticeSession::start(KeyerConfig::default(), 0.0, false).unwrap();
    for _ in 0..100_000 {
        session.input.set(0, [true, false, false]);
    }
    let until = Instant::now() + Duration::from_secs(2);
    while session.status().running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!session.status().key_down);
}
