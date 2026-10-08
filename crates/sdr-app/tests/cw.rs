use sdr_app::engine::{
    Client, Command, Engine, FileSettings, Settings, Source, SubmitError, Update,
};
use sdr_dsp::{Mode, ReceiverConfig};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[path = "../../sdr-dsp/tests/support/cw_signal.rs"]
mod signal;
fn submit(client: &Client, settings: &Settings) {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match client.submit(Command::Configure(Box::new(settings.clone()))) {
            Ok(()) => return,
            Err(SubmitError::Busy) => assert!(Instant::now() < until),
            result => panic!("{result:?}"),
        }
    }
}
fn wait(client: &Client, predicate: impl Fn(&Update) -> bool) -> Update {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(state) = client.poll() {
            assert!(state.error.is_none(), "{:?}", state.error);
            if predicate(&state) {
                return state;
            }
            assert!(!state.eof, "ended before expected state: {:?}", state.cw);
        }
        assert!(Instant::now() < until, "CW update timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn muted_cw_decodes_and_reset_disable_retune_preserve_correct_epochs() {
    let path = std::env::temp_dir().join(format!("rustsdr-cw-{}.cf32", std::process::id()));
    signal::write_recording(&path, 2);
    let mut settings = Settings {
        source: Source::File,
        receiver: ReceiverConfig::for_mode(Mode::Cw),
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
    wait(&client, |s| {
        s.cw.as_ref().is_some_and(|cw| cw.text.contains("CQ RUST"))
    });
    settings.revision += 1;
    settings.cw_reset += 1;
    submit(&client, &settings);
    let cleared = wait(&client, |s| s.revision == 2);
    assert!(cleared.cw.as_ref().is_none_or(|cw| cw.text.is_empty()));
    assert_eq!(cleared.capture_revision, 1);
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(block) = audio.next()
            && block.revision == 2
        {
            assert_eq!(block.discontinuity, 0, "decoder reset must not reset audio");
            assert!(block.output_start > 0);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    settings.revision += 1;
    settings.cw_decoder = None;
    submit(&client, &settings);
    assert!(wait(&client, |s| s.revision == 3).cw.is_none());
    settings.revision += 1;
    settings.cw_decoder = Some(Default::default());
    settings.station_hz += 100_000;
    submit(&client, &settings);
    let retuned = wait(&client, |s| {
        s.revision == 4 && s.samples > cleared.samples + 240_000
    });
    assert!(retuned.cw.is_some_and(|cw| cw.text.is_empty()));
    settings.revision += 1;
    settings.receiver = ReceiverConfig::for_mode(Mode::Am);
    submit(&client, &settings);
    assert!(wait(&client, |s| s.revision == 5).cw.is_none());
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn cw_memory_csv_uses_narrow_filter_and_round_trips() {
    use sdr_app::memories::Library;
    let library = Library::from_csv(
        b"name,frequency_hz,mode,bandwidth_hz\nCW test,106700000,cw,200\n",
        "CW",
    )
    .unwrap();
    assert_eq!(library.groups[0].entries[0].receiver.mode, Mode::Cw);
    let path = std::env::temp_dir().join(format!("rustsdr-cw-{}.csv", std::process::id()));
    library.save(&path).unwrap();
    assert_eq!(library, Library::load(&path).unwrap());
    std::fs::remove_file(path).unwrap();
}
