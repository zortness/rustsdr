//! Exercise live VFO/center changes and report acknowledgement latency.
use sdr_app::controller::{Controller, Settings, Source};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut settings = Settings {
        source: Source::RtlSdr,
        rtl_center_hz: Some(106_950_000),
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone())?;
    let mut before = 0;
    for (revision, station, center) in [
        (1, 106_700_000, 106_950_000),
        (2, 107_100_000, 106_950_000),
        (3, 107_200_000, 107_050_000),
        (4, 89_900_000, 90_150_000),
    ] {
        settings.revision = revision;
        settings.station_hz = station;
        settings.rtl_center_hz = Some(center);
        let start = Instant::now();
        if revision > 1 {
            while !controller.request(settings.clone()) {
                if start.elapsed() > Duration::from_secs(5) {
                    return Err("control timeout".into());
                }
            }
        }
        loop {
            if let Some(update) = controller.poll() {
                if let Some(error) = update.error {
                    return Err(error.into());
                }
                if update.revision == revision && update.samples > before && update.frame.is_some()
                {
                    assert_eq!(update.capture_revision, 1);
                    assert_eq!(update.center_hz, center as u64);
                    assert_eq!(update.station_hz, station);
                    println!(
                        "revision={revision} center={center} station={station} latency_ms={} samples={} iq_drops={} audio_under={} audio_over={}",
                        start.elapsed().as_millis(),
                        update.samples,
                        update.iq_drops,
                        update.audio_underruns,
                        update.audio_overruns
                    );
                    before = update.samples;
                    break;
                }
            }
            if start.elapsed() > Duration::from_secs(5) {
                return Err("tuning timeout".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}
