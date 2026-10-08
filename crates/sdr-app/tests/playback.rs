use sdr_app::controller::{Controller, FileSettings, Settings, Source};
use sdr_dsp::{Mode, ReceiverConfig};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn every_voice_mode_switch_preserves_the_capture_and_playback_clock() {
    let path = std::env::temp_dir().join(format!("rustsdr-modes-{}.cf32", std::process::id()));
    std::fs::write(&path, vec![0; 2_400_000 * 8 * 2]).unwrap();
    let mut settings = Settings {
        source: Source::File,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_950_000,
        })),
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone()).unwrap();
    let mut previous_samples = 0;
    for mode in Mode::ALL {
        settings.revision += 1;
        settings.receiver = ReceiverConfig::for_mode(mode);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !controller.request(settings.clone()) {
            assert!(Instant::now() < deadline);
        }
        loop {
            if let Some(update) = controller.poll() {
                assert!(update.error.is_none(), "{:?}", update.error);
                if update.revision == settings.revision && update.samples > previous_samples {
                    // The first request may coalesce before initial open. Later requests must retain that open.
                    assert!(update.capture_revision <= 2);
                    assert_eq!(update.mode, mode);
                    assert!(update.running);
                    previous_samples = update.samples;
                    break;
                }
            }
            assert!(Instant::now() < deadline, "mode change stalled: {mode:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    drop(controller);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn vfo_changes_preserve_capture_and_file_position() {
    let path = std::env::temp_dir().join(format!("rustsdr-vfo-{}.cf32", std::process::id()));
    std::fs::write(&path, vec![0; 2_400_000 * 8]).unwrap();
    let mut settings = Settings {
        source: Source::File,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_950_000,
        })),
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone()).unwrap();
    let audio = controller.client().subscribe_audio();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut before = 0;
    loop {
        if let Some(update) = controller.poll() {
            assert!(update.error.is_none());
            if before == 0 && update.samples >= 491_520 {
                before = update.samples;
                settings.revision = 2;
                settings.station_hz = 107_100_000;
                while !controller.request(settings.clone()) {
                    assert!(Instant::now() < deadline);
                }
            } else if update.revision == 2 {
                assert!(update.samples >= before, "VFO tuning rewound playback");
                assert_eq!(update.capture_revision, 1, "VFO tuning reopened the source");
                assert_eq!(update.center_hz, 106_950_000);
                assert_eq!(update.station_hz, 107_100_000);
                break;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    loop {
        if let Some(block) = audio.next()
            && block.revision == 2
        {
            assert_eq!(block.capture_revision, 1);
            assert_eq!(block.discontinuity, 1);
            assert!(block.input_start >= before);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "new generation audio was not delivered"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(controller);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_file_can_recover_and_active_playback_can_stop() {
    let path = std::env::temp_dir().join(format!("rustsdr-recovery-{}.cf32", std::process::id()));
    std::fs::write(&path, [0_u8; 7]).unwrap();
    let mut settings = Settings {
        source: Source::File,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_950_000,
        })),
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(update) = controller.poll()
            && update.error.is_some()
        {
            assert!(!update.running);
            assert!(!update.eof);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(&path, vec![0_u8; 2_400_000 * 8]).unwrap();
    settings.revision = 2;
    while !controller.request(settings.clone()) {
        assert!(Instant::now() < deadline);
    }
    loop {
        if let Some(update) = controller.poll()
            && update.revision == 2
            && update.samples > 0
        {
            assert!(update.error.is_none());
            assert!(update.running);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    settings.revision = 3;
    settings.running = false;
    while !controller.request(settings.clone()) {
        assert!(Instant::now() < deadline);
    }
    loop {
        if let Some(update) = controller.poll()
            && update.revision == 3
        {
            assert!(!update.running);
            assert!(!update.eof);
            assert!(update.frame.is_none());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(controller);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn file_playback_is_paced_reaches_eof_and_can_restart() {
    let path = std::env::temp_dir().join(format!("rustsdr-playback-{}.cf32", std::process::id()));
    let count = 240_003;
    std::fs::write(&path, vec![0; count * 8]).unwrap();
    let mut settings = Settings {
        source: Source::File,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_950_000,
        })),
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone()).unwrap();
    for revision in [1, 2] {
        let start = Instant::now();
        if revision == 2 {
            settings.revision = 2;
            while !controller.request(settings.clone()) {
                std::thread::yield_now();
            }
        }
        loop {
            if let Some(update) = controller.poll() {
                assert!(update.error.is_none(), "{:?}", update.error);
                if update.revision == revision && update.eof {
                    assert!(!update.running);
                    assert_eq!(update.samples, count as u64);
                    assert_eq!(update.total_samples, Some(count as u64));
                    assert!(
                        start.elapsed() >= Duration::from_millis(90),
                        "playback was unpaced"
                    );
                    break;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "EOF not delivered"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    settings.revision = 3;
    settings.file = Some(Arc::new(FileSettings {
        path: path.with_extension("missing"),
        sample_rate: 2_400_000,
        center_hz: 106_950_000,
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !controller.request(settings.clone()) {
        assert!(Instant::now() < deadline);
    }
    loop {
        if let Some(update) = controller.poll()
            && update.revision == 3
            && update.error.is_some()
        {
            assert!(!update.eof, "fault retained the preceding recording's EOF");
            assert_eq!(update.samples, 0);
            assert_eq!(update.total_samples, None);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(controller);
    std::fs::remove_file(path).unwrap();
}
