use sdr_app::{
    engine::{Engine, FileSettings, Settings, Source},
    recording::Recorder,
};
use sdr_dsp::{Mode, ReceiverConfig};
use std::{
    fs,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn iq_file_can_be_recorded_as_a_playable_wav() {
    let dir = std::env::temp_dir().join(format!("rustsdr-recording-engine-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let iq = dir.join("silence.cf32");
    let wav = dir.join("decoded.wav");
    let _ = fs::remove_file(&wav);
    // A short, valid 2.4 MS/s IQ file exercises the actual receiver audio stream.
    fs::write(&iq, vec![0_u8; 2_400_000 * 8 / 5]).unwrap();
    let settings = Settings {
        source: Source::File,
        station_hz: 100_000_000,
        receiver: ReceiverConfig::for_mode(Mode::Nfm),
        file: Some(Arc::new(FileSettings {
            path: iq.clone(),
            sample_rate: 2_400_000,
            center_hz: 100_000_000,
        })),
        ..Settings::default()
    };
    let engine = Engine::with_settings(settings).unwrap();
    let client = engine.client();
    let recorder = Recorder::start(&client, wav.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(update) = client.poll() {
            assert!(update.error.is_none(), "{:?}", update.error);
            if update.eof {
                break;
            }
        }
        assert!(Instant::now() < deadline, "file playback did not reach EOF");
        std::thread::sleep(Duration::from_millis(5));
    }
    let frames = recorder.finish().unwrap();
    assert!(frames > 1000, "recorded only {frames} frames");
    let reader = hound::WavReader::open(&wav).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48_000);
    assert_eq!(reader.duration() as u64, frames);
    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,sample_rate,channels",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(&wav)
        .output()
    {
        assert!(
            output.status.success(),
            "ffprobe rejected the WAV: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(report.contains("codec_name=pcm_s16le"));
        assert!(report.contains("sample_rate=48000"));
        assert!(report.contains("channels=2"));
    }
    drop(engine);
    fs::remove_file(wav).unwrap();
    fs::remove_file(iq).unwrap();
    fs::remove_dir(dir).unwrap();
}
