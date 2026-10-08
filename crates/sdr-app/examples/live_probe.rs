use sdr_app::controller::{Controller, Settings, Source};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let station_hz = std::env::args()
        .nth(1)
        .unwrap_or("106700000".into())
        .parse()?;
    let controller = Controller::with_settings(Settings {
        source: Source::RtlSdr,
        station_hz,
        receiver: sdr_dsp::ReceiverConfig::for_mode(if std::env::args().any(|a| a == "stereo") {
            sdr_dsp::Mode::WfmStereo
        } else {
            sdr_dsp::Mode::WfmMono
        }),
        ..Settings::default()
    })?;
    let start = Instant::now();
    let mut last_report = Instant::now();
    let mut seen = false;
    while start.elapsed() < Duration::from_secs(8) {
        if let Some(update) = controller.poll() {
            if let Some(error) = update.error {
                return Err(error.into());
            }
            if last_report.elapsed() > Duration::from_secs(1) {
                println!(
                    "samples={} iq_drops={} audio_under={} audio_over={} audio_rms={:.4} audio_error={:?} stereo_locked={}",
                    update.samples,
                    update.iq_drops,
                    update.audio_underruns,
                    update.audio_overruns,
                    update.audio_rms,
                    update.audio_error,
                    update.stereo_locked
                );
                last_report = Instant::now();
            }
            seen |= update.frame.is_some();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !seen {
        return Err("No live spectrum received".into());
    }
    Ok(())
}
