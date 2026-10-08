use sdr_app::engine::{
    ChannelEvent, ChannelRequest, Command, Engine, FileSettings, Settings, Source,
};
use std::{
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn engine_owns_decoder_even_when_handle_outlives_it() {
    let engine = Engine::with_settings(Settings {
        running: false,
        ..Default::default()
    })
    .unwrap();
    let handle = engine.client().digital();
    let config = sdr_app::digital::Config {
        protocol: Some(sdr_app::digital::Protocol::Ft4),
        ..Default::default()
    };
    handle
        .import(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/digital/ft4.wav"),
            config,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(batch) = handle.results.next() {
            assert!(
                batch
                    .result
                    .unwrap()
                    .iter()
                    .any(|m| m.text == "CQ RU AB5XS EM12")
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(engine);
    assert!(handle.import("unused.wav".into(), config).is_err());
}

#[test]
fn headless_iq_file_feeds_vor_and_independent_channel_reader() {
    let path = std::env::temp_dir().join(format!("rustsdr-vor-{}.cf32", std::process::id()));
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    for n in 0..7_200_000 {
        let t = n as f64 / 2_400_000.0;
        let p = std::f64::consts::TAU * 30.0 * t;
        let a = 0.5
            * (1.0
                + 0.3 * (p - 120.0_f64.to_radians()).cos()
                + 0.3 * (std::f64::consts::TAU * 9960.0 * t + 16.0 * p.sin()).cos());
        let carrier = -std::f64::consts::TAU * 250_000.0 * t;
        file.write_all(&((a * carrier.cos()) as f32).to_le_bytes())
            .unwrap();
        file.write_all(&((a * carrier.sin()) as f32).to_le_bytes())
            .unwrap();
    }
    file.flush().unwrap();
    drop(file);
    let mut settings = Settings {
        running: false,
        source: Source::File,
        station_hz: 113_000_000,
        receiver: sdr_dsp::ReceiverConfig::for_mode(sdr_dsp::Mode::Am),
        file: Some(Arc::new(FileSettings {
            path: path.clone(),
            sample_rate: 2_400_000,
            center_hz: 113_250_000,
        })),
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone()).unwrap();
    let client = engine.client();
    let extra = client
        .subscribe_channel(ChannelRequest {
            frequency_hz: None,
            sample_rate: 48_000,
            bandwidth_hz: 32_000,
        })
        .unwrap();
    let vor = client.vor();
    vor.enable(true);
    std::thread::sleep(Duration::from_millis(60));
    settings.running = true;
    settings.revision += 1;
    client
        .submit(Command::Configure(Box::new(settings)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_iq = false;
    loop {
        while let Some(event) = extra.events.next() {
            if let ChannelEvent::Samples(block) = event {
                assert_eq!(block.center_hz, 113_000_000);
                assert_eq!(block.sample_rate, 48_000);
                assert_eq!(block.revision, 2);
                saw_iq = true;
            }
        }
        if let Some(status) = vor.results.next()
            && let Some(m) = status.measurement
        {
            assert!((m.radial_deg - 120.0).abs() < 2.0, "{m:?}");
            assert!(saw_iq);
            break;
        }
        assert!(Instant::now() < deadline, "VOR failed to acquire");
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(engine);
    assert!(
        client
            .subscribe_channel(ChannelRequest {
                frequency_hz: None,
                sample_rate: 48_000,
                bandwidth_hz: 32_000
            })
            .is_err()
    );
    std::fs::remove_file(path).unwrap();
}
