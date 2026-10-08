use sdr_app::engine::{
    Client, Command, Engine, FileSettings, Settings, Source, SubmitError, Update,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[path = "../../sdr-dsp/tests/support/rds_signal.rs"]
mod signal;
fn submit(client: &Client, settings: &Settings) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match client.submit(Command::Configure(Box::new(settings.clone()))) {
            Ok(()) => return,
            Err(SubmitError::Busy) => assert!(Instant::now() < deadline),
            result => panic!("{result:?}"),
        }
    }
}
fn wait(client: &Client, predicate: impl Fn(&Update) -> bool) -> Update {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(state) = client.poll() {
            assert!(state.error.is_none(), "{:?}", state.error);
            assert!(!state.eof, "fixture ended before expected state");
            if predicate(&state) {
                return state;
            }
        }
        assert!(Instant::now() < deadline, "RDS state did not arrive");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn muted_playback_decodes_and_decoder_toggle_preserves_audio_epoch_while_retune_clears_text() {
    let path = std::env::temp_dir().join(format!("rustsdr-rds-{}.cf32", std::process::id()));
    signal::write_recording(&path, 10);
    let mut settings = Settings {
        source: Source::File,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_950_000,
        })),
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone()).unwrap();
    let client = engine.client();
    client.set_volume(0.0);
    let audio = client.subscribe_audio();
    let decoded = wait(&client, |s| {
        s.rds
            .as_ref()
            .is_some_and(|d| d.radio_text.as_deref() == Some("Hello radio!"))
    });
    assert_eq!(
        decoded.rds.unwrap().program_service.as_deref(),
        Some("RUST FM")
    );
    while audio.next().is_some() {}
    settings.revision += 1;
    settings.rds_enabled = false;
    submit(&client, &settings);
    let disabled = wait(&client, |s| s.revision == 2 && s.rds.is_none());
    assert_eq!(disabled.capture_revision, 1);
    assert!(disabled.samples >= decoded.samples);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(block) = audio.next()
            && block.revision == 2
        {
            assert_eq!(block.discontinuity, 0, "decoder toggle reset audio");
            assert!(block.output_start > 0);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    settings.revision += 1;
    settings.rds_enabled = true;
    submit(&client, &settings);
    wait(&client, |s| {
        s.revision == 3 && s.rds.as_ref().is_some_and(|d| d.locked)
    });
    settings.revision += 1;
    settings.station_hz = 107_700_000;
    submit(&client, &settings);
    let retuned = wait(&client, |s| s.revision == 4);
    assert!(retuned.rds.as_ref().is_none_or(|d| d.pi.is_none()));
    assert_eq!(retuned.capture_revision, 1);
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn decoder_toggle_does_not_restart_a_scan_pause() {
    use sdr_app::scanning::{ScanParameters, ScanPhase, ScanPlan, ScanTargets};
    let path = std::env::temp_dir().join(format!("rustsdr-rds-scan-{}.cf32", std::process::id()));
    std::fs::write(
        &path,
        [0.5_f32.to_le_bytes(), 0_f32.to_le_bytes()]
            .concat()
            .repeat(2_400_000 * 2),
    )
    .unwrap();
    let mut settings = Settings {
        source: Source::File,
        station_hz: 100_000_000,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 100_000_000,
        })),
        scan: Some(Arc::new(ScanPlan {
            targets: ScanTargets::Range {
                start_hz: 100_000_000,
                stop_hz: 100_000_000,
                step_hz: 1000,
                receiver: Default::default(),
            },
            parameters: ScanParameters {
                settle_ms: 20,
                dwell_ms: 25,
                hold_ms: 1500,
                threshold_db: -20.0,
            },
            start_index: 0,
        })),
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone()).unwrap();
    let client = engine.client();
    let before = wait(&client, |s| {
        s.scan.phase == ScanPhase::Holding && s.scan.remaining_ms < 1200
    });
    settings.revision = 2;
    settings.rds_enabled = false;
    submit(&client, &settings);
    let after = wait(&client, |s| {
        s.revision == 2 && s.scan.phase == ScanPhase::Holding
    });
    assert!(
        after.scan.remaining_ms <= before.scan.remaining_ms,
        "toggle restarted fixed pause: {} -> {}",
        before.scan.remaining_ms,
        after.scan.remaining_ms
    );
    assert_eq!(after.scan.visit, before.scan.visit);
    drop(engine);
    std::fs::remove_file(path).unwrap();
}
