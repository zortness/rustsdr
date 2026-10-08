use sdr_app::engine::{
    Command, Engine, EngineEvent, EventStream, FileSettings, Settings, Source, SubmitError,
};
use sdr_dsp::{Mode, ReceiverConfig};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn zoom_analysis_preserves_capture_audio_clock_and_radio_revision() {
    use sdr_dsp::zoom_spectrum::SpectrumView;
    let path = std::env::temp_dir().join(format!("rustsdr-zoom-{}.cf32", std::process::id()));
    std::fs::write(&path, vec![0_u8; 2_400_000 * 8 * 2]).unwrap();
    let settings = Settings {
        source: Source::File,
        receiver: ReceiverConfig::for_mode(Mode::Usb),
        station_hz: 7_074_000,
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 7_324_000,
        })),
        ..Default::default()
    };
    let engine = Engine::with_settings(settings).unwrap();
    let client = engine.client();
    client.set_volume(0.0);
    let audio = client.subscribe_audio();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut changed = false;
    let mut before = 0;
    let mut output_end = None;
    let mut after_audio = false;
    let view = SpectrumView {
        center_offset_hz: -248_350.0,
        span_hz: 4000.0,
    };
    loop {
        while let Some(block) = audio.next() {
            assert_eq!(block.discontinuity, 0);
            assert_eq!(block.capture_revision, 1);
            assert_eq!(block.revision, 1);
            if let Some(end) = output_end {
                assert_eq!(
                    block.output_start, end,
                    "zoom interrupted the audio sample clock"
                );
            }
            output_end = Some(block.output_start + block.frames.len() as u64);
            if changed {
                after_audio = true;
            }
        }
        if let Some(update) = client.poll() {
            assert!(update.error.is_none(), "{:?}", update.error);
            if !changed && update.samples > 100_000 {
                if client.submit(Command::SpectrumView(view)).is_ok() {
                    before = update.samples;
                    changed = true;
                }
            } else if changed
                && let Some(frame) = update.frame
                && frame.view == view
            {
                assert_eq!(update.revision, 1);
                assert_eq!(frame.capture_revision, 1);
                assert!(update.samples > before);
                assert!(frame.analysis_rate_hz / (frame.dbfs.len() as f64) < 3.0);
                assert!(after_audio);
                break;
            }
        }
        assert!(Instant::now() < deadline, "zoom analysis stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn client_rejects_stale_invalid_and_closed_commands() {
    let engine = Engine::new().unwrap();
    let client = engine.client();
    assert!(!client.capabilities().transmit);
    assert_eq!(
        client.submit(Command::Configure(Box::new(Settings {
            revision: 0,
            ..Settings::default()
        }))),
        Err(SubmitError::Invalid)
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match client.submit(Command::Configure(Box::default())) {
            Err(SubmitError::Busy) => assert!(Instant::now() < deadline),
            result => {
                assert_eq!(result, Err(SubmitError::Stale));
                break;
            }
        }
    }
    while !client.poll().is_some_and(|state| state.running) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(engine);
    let final_state = client.poll().unwrap();
    assert!(!final_state.running);
    assert!(final_state.frame.is_none());
    assert_eq!(
        client.submit(Command::MonitorGain(0.5)),
        Err(SubmitError::Closed)
    );
}

#[test]
fn ordered_events_are_bounded_and_report_loss() {
    let stream = EventStream::bounded(2);
    for n in 0..10 {
        stream.publish(n);
    }
    assert_eq!(stream.dropped(), 8);
    assert_eq!(stream.next(), Some(0));
    assert_eq!(stream.next(), Some(1));
    assert_eq!(stream.next(), None);
}

#[test]
fn mode_preferences_survive_round_trip() {
    let mut saved = sdr_app::mode_settings::ModeSettings::default();
    let mut am = ReceiverConfig::for_mode(Mode::Am);
    am.bandwidth_hz = 6_000;
    am.agc = false;
    let (nfm, step) = saved.switch(am, 500, Mode::Nfm);
    assert_eq!(step, 12_500);
    assert_eq!(saved.switch(nfm, step, Mode::Am), (am, 500));
}

#[test]
fn muted_headless_receiver_exposes_timed_audio_and_finishes_with_stalled_consumers() {
    let path = std::env::temp_dir().join(format!("rustsdr-tap-{}.cf32", std::process::id()));
    let mut iq = Vec::with_capacity(1_200_000 * 8);
    for sample in 0..1_200_000 {
        let amplitude = 0.5 + 0.2 * (std::f32::consts::TAU * sample as f32 / 2400.0).sin();
        iq.extend_from_slice(&amplitude.to_le_bytes());
        iq.extend_from_slice(&0_f32.to_le_bytes());
    }
    std::fs::write(&path, iq).unwrap();
    let settings = Settings {
        running: false,
        source: Source::File,
        receiver: ReceiverConfig::for_mode(Mode::Am),
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 106_700_000,
        })),
        ..Settings::default()
    };
    let engine = Engine::with_settings(settings.clone()).unwrap();
    let client = engine.client();
    let audio = client.subscribe_audio();
    let events = client.events();
    client.submit(Command::MonitorGain(0.0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match client.submit(Command::Configure(Box::new(Settings {
            running: true,
            revision: 2,
            ..settings.clone()
        }))) {
            Ok(()) => break,
            Err(SubmitError::Busy) => assert!(Instant::now() < deadline),
            error => panic!("{error:?}"),
        }
    }
    loop {
        if matches!(events.next(), Some(EngineEvent::EndOfFile { revision: 2 })) {
            break;
        }
        assert!(Instant::now() < deadline, "stalled consumers blocked EOF");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(audio.dropped() > 0);
    let first = audio.next().unwrap();
    let second = audio.next().unwrap();
    assert_eq!(first.sample_rate, 48_000);
    assert_eq!(first.output_start, 0);
    assert_eq!(first.revision, 2);
    assert_eq!(first.capture_revision, 2);
    assert_eq!(second.output_start, first.frames.len() as u64);
    assert!(second.input_start > first.input_start);
    assert_eq!(first.discontinuity, second.discontinuity);
    assert!(
        second.frames.iter().any(|frame| frame.left.abs() > 0.01),
        "monitor mute altered receiver output"
    );
    drop(engine);
    std::fs::remove_file(path).unwrap();
}
