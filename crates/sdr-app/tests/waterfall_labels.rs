use sdr_app::{
    digital::{Batch, Config, Message, Protocol},
    waterfall_labels::{History, Stamp},
};
fn batch() -> Batch {
    Batch {
        config: Config {
            protocol: Some(Protocol::Ft8),
            revision: 2,
            dial_hz: 7_074_000,
            file_clock: true,
        },
        capture_revision: 3,
        slot_ms: 0,
        imported: false,
        elapsed: Default::default(),
        result: Ok(vec![Message {
            text: "CQ K1ABC FN42".into(),
            sender: Some("K1ABC".into()),
            audio_hz: 1200.0,
            snr: -10.0,
            dt: 0.0,
        }]),
    }
}
#[test]
fn delayed_decode_tracks_original_row_and_expires_at_wrap() {
    let mut history = History::new(256);
    for row in 0..151 {
        history.push(Some(Stamp {
            seconds: row as f64 / 10.0,
            revision: 2,
            capture: 3,
        }));
    }
    history.add(&batch());
    let (label, age) = history.visible().next().unwrap();
    assert_eq!(label.call, "K1ABC");
    assert_eq!(label.rf_hz, 7_075_200.0);
    assert_eq!(age, 19.0); // anchor 13.14 s -> row 13.1 s, NOT decode arrival 15 s
    for _ in 0..10 {
        history.push(None);
    }
    assert_eq!(history.visible().next().unwrap().1, 29.0);
    for _ in 0..227 {
        history.push(None);
    }
    assert_eq!(history.visible().count(), 0);
}
#[test]
fn imports_stale_captures_unknown_senders_and_missing_history_are_not_labelled() {
    for case in 0..5 {
        let mut h = History::new(256);
        h.push(Some(Stamp {
            seconds: 13.14,
            revision: 2,
            capture: 3,
        }));
        let mut b = batch();
        match case {
            0 => b.imported = true,
            1 => b.capture_revision = 4,
            2 => b.config.revision = 1,
            3 => b.result.as_mut().unwrap()[0].sender = None,
            _ => b.slot_ms = 15000,
        }
        h.add(&b);
        assert_eq!(h.visible().count(), 0);
    }
}
#[test]
fn repeated_batch_deduplicates_and_clear_removes_labels() {
    let mut h = History::new(256);
    h.push(Some(Stamp {
        seconds: 13.14,
        revision: 2,
        capture: 3,
    }));
    h.add(&batch());
    h.add(&batch());
    assert_eq!(h.visible().count(), 1);
    h.clear();
    assert_eq!(h.visible().count(), 0);
}
