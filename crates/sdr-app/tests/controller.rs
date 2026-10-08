use sdr_app::controller::{Controller, Settings};
use std::time::{Duration, Instant};

#[test]
fn stalled_display_does_not_stop_acquisition_and_commands_are_acknowledged() {
    let controller = Controller::new().unwrap();
    // Do not poll: display slot must replace frames rather than block acquisition.
    std::thread::sleep(Duration::from_millis(350));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(update) = controller.poll() {
            assert!(update.error.is_none());
            if update.samples > 163_840 && update.display_drops > 0 {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "acquisition stalled with the display"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    while !controller.request(Settings {
        revision: 2,
        running: false,
        tone_hz: 125_000.0,
        ..Settings::default()
    }) {
        assert!(Instant::now() < deadline);
    }
    loop {
        if let Some(update) = controller.poll()
            && update.revision == 2
        {
            assert!(!update.running);
            assert!(update.frame.is_none());
            break;
        }
        assert!(Instant::now() < deadline, "stop was not acknowledged");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reconfiguration_replaces_stale_spectra_with_new_revision() {
    let controller = Controller::new().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let requested = Settings {
        revision: 7,
        running: true,
        tone_hz: -300_000.0,
        ..Settings::default()
    };
    while !controller.request(requested.clone()) {
        assert!(Instant::now() < deadline);
    }
    loop {
        if let Some(update) = controller.poll() {
            assert!(update.error.is_none());
            if let Some(frame) = update.frame
                && frame.revision == 7
            {
                let peak = frame
                    .dbfs
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .unwrap();
                assert_eq!(peak.0, 1536);
                assert!((*peak.1 + 6.0206).abs() < 0.001);
                break;
            }
        }
        assert!(Instant::now() < deadline, "new spectrum was not delivered");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!controller.request(Settings {
        tone_hz: f64::NAN,
        ..requested
    }));
}
