use sdr_app::scanning::{ScanParameters, ScanPhase, ScanPlan, ScanTargets, Scanner};
use sdr_dsp::ReceiverConfig;
use std::{sync::Arc, time::Duration};
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn invalid_ranges_thresholds_and_unsupported_source_frequencies_are_rejected() {
    let mut plan = ScanPlan {
        targets: ScanTargets::Range {
            start_hz: 88_000_000,
            stop_hz: 108_000_000,
            step_hz: 0,
            receiver: ReceiverConfig::default(),
        },
        parameters: ScanParameters::default(),
        start_index: 0,
    };
    assert!(Scanner::new(Arc::new(plan.clone())).is_err());
    plan.targets = ScanTargets::Range {
        start_hz: 108_000_000,
        stop_hz: 88_000_000,
        step_hz: 100_000,
        receiver: ReceiverConfig::default(),
    };
    assert!(Scanner::new(Arc::new(plan.clone())).is_err());
    plan.targets = ScanTargets::Range {
        start_hz: 1_000_000,
        stop_hz: 1_000_000,
        step_hz: 1,
        receiver: ReceiverConfig::default(),
    };
    assert!(plan.validate(28_800_000, 1_700_000_000).is_err());
    assert!(plan.validate(1, 1_700_000_000).is_ok());
    plan.parameters.threshold_db = f32::NAN;
    assert!(Scanner::new(Arc::new(plan)).is_err());
}
#[test]
fn scan_settles_pauses_for_fixed_time_and_wraps_even_with_continuous_signal() {
    let plan = Arc::new(ScanPlan {
        targets: ScanTargets::Range {
            start_hz: 100_000_000,
            stop_hz: 100_250_000,
            step_hz: 100_000,
            receiver: ReceiverConfig::default(),
        },
        parameters: ScanParameters {
            settle_ms: 100,
            dwell_ms: 100,
            hold_ms: 1000,
            threshold_db: -50.0,
        },
        start_index: 0,
    });
    let mut scanner = Scanner::new(plan).unwrap();
    scanner.tuned(ms(0));
    assert!(!scanner.observe(ms(99), -20.0));
    assert_eq!(scanner.phase(), ScanPhase::Settling);
    assert!(!scanner.observe(ms(100), -20.0));
    assert_eq!(scanner.phase(), ScanPhase::Holding);
    assert!(!scanner.observe(ms(1099), -20.0));
    assert!(scanner.observe(ms(1100), -20.0));
    assert_eq!(scanner.current().frequency_hz, 100_100_000);
    scanner.tuned(ms(1400)); // Hardware retune time is excluded from the settling interval.
    assert!(!scanner.observe(ms(1500), -100.0));
    assert!(scanner.observe(ms(1600), -100.0));
    assert_eq!(scanner.current().frequency_hz, 100_200_000);
    scanner.tuned(ms(1600));
    scanner.observe(ms(1700), -100.0);
    assert!(scanner.observe(ms(1800), -100.0));
    assert_eq!(scanner.current().frequency_hz, 100_000_000);
}

#[test]
fn engine_scans_without_ui_polling_and_stop_holds_the_actual_channel() {
    use sdr_app::engine::{Command, Engine, FileSettings, Settings, Source, SubmitError};
    let path = std::env::temp_dir().join(format!("rustsdr-scan-{}.cf32", std::process::id()));
    // A strong unmodulated carrier: scan decisions must not depend on audible modulation.
    let pair = [0.5_f32.to_le_bytes(), 0_f32.to_le_bytes()].concat();
    std::fs::write(&path, pair.repeat(2_400_000 * 2)).unwrap();
    let plan = Arc::new(ScanPlan {
        targets: ScanTargets::Range {
            start_hz: 100_000_000,
            stop_hz: 100_100_000,
            step_hz: 100_000,
            receiver: ReceiverConfig::default(),
        },
        parameters: ScanParameters {
            settle_ms: 20,
            dwell_ms: 25,
            hold_ms: 100,
            threshold_db: -20.0,
        },
        start_index: 0,
    });
    let engine = Engine::with_settings(Settings {
        source: Source::File,
        station_hz: 100_000_000,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            center_hz: 100_000_000,
            sample_rate: 2_400_000,
        })),
        scan: Some(plan.clone()),
        ..Settings::default()
    })
    .unwrap();
    let client = engine.client();
    std::thread::sleep(Duration::from_millis(350));
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(update) = client.poll()
            && update.scan.visit >= 3
            && update.scan.phase == ScanPhase::Holding
        {
            assert!(update.error.is_none());
            assert!(update.channel_power_dbfs.unwrap() > -20.0);
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    loop {
        match client.submit(Command::StopScan { revision: 1 }) {
            Ok(()) => break,
            Err(SubmitError::Busy) => {}
            other => panic!("{other:?}"),
        }
    }
    let held = loop {
        if let Some(update) = client.poll()
            && update.scan.phase == ScanPhase::Off
        {
            break update.station_hz;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    };
    std::thread::sleep(Duration::from_millis(250));
    let update = client.poll().unwrap();
    assert_eq!(update.station_hz, held);
    assert_eq!(update.scan.phase, ScanPhase::Off);
    assert_eq!(
        update.scan.count, 2,
        "stop should retain the scan position for recall/resume"
    );
    assert!(update.running);
    assert_eq!(update.capture_revision, 1);
    // A stop accepted immediately after start must survive a not-yet-applied
    // configuration revision; the client must not need to submit it repeatedly.
    let restart = Settings {
        revision: 2,
        source: Source::File,
        station_hz: held,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            center_hz: 100_000_000,
            sample_rate: 2_400_000,
        })),
        scan: Some(plan),
        ..Settings::default()
    };
    while matches!(
        client.submit(Command::Configure(Box::new(restart.clone()))),
        Err(SubmitError::Busy)
    ) {
        assert!(std::time::Instant::now() < deadline);
    }
    while matches!(
        client.submit(Command::StopScan { revision: 2 }),
        Err(SubmitError::Busy)
    ) {
        assert!(std::time::Instant::now() < deadline);
    }
    loop {
        if let Some(state) = client.poll()
            && state.revision == 2
        {
            assert!(!state.eof, "accepted early stop was lost until EOF");
            if state.scan.phase == ScanPhase::Off {
                assert!(state.running);
                break;
            }
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(engine);
    std::fs::remove_file(path).unwrap();
}
