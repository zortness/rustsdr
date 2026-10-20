//! Live check for reception with an unavailable output, then recovery after
//! that output is restored. `--hot` waits for a running endpoint to be disabled.
use sdr_app::controller::{Controller, Settings, Source};
use std::{
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

fn submit(controller: &Controller, settings: &Settings) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !controller.request(settings.clone()) {
        if Instant::now() >= deadline {
            return Err(format!(
                "revision {} could not be submitted",
                settings.revision
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expected_output = std::env::args()
        .nth(1)
        .ok_or("usage: audio_recovery_probe EXPECTED_OUTPUT_NAME_FRAGMENT [--hot]")?;
    let hot = std::env::args().any(|arg| arg == "--hot");
    let mut settings = Settings {
        source: Source::RtlSdr,
        ..Settings::default()
    };
    let controller = Controller::with_settings(settings.clone())?;

    let deadline = Instant::now() + Duration::from_secs(if hot { 180 } else { 12 });
    let mut first_loss = None;
    let mut saw_frame = false;
    let mut baseline_reported = false;
    loop {
        if let Some(update) = controller.poll() {
            if let Some(error) = update.error {
                return Err(format!("receiver failed without audio output: {error}").into());
            }
            if let Some(device) = &update.audio_output_device {
                if !device.contains(&expected_output) {
                    return Err(format!("Windows selected another audio output: {device}").into());
                }
                if !hot {
                    return Err(format!("audio output is still available: {device}").into());
                }
                if !baseline_reported
                    && update.running
                    && update.samples >= 4_000_000
                    && update.audio_error.is_none()
                    && update.audio_queue_frames.is_some()
                {
                    println!(
                        "BASELINE: samples={} output={device}; power it off now",
                        update.samples
                    );
                    io::stdout().flush()?;
                    baseline_reported = true;
                }
            }
            if hot && !baseline_reported && update.audio_error.is_some() {
                return Err("audio output failed before the healthy baseline".into());
            }
            saw_frame |= update.frame.is_some();
            if update.running && update.audio_error.is_some() && update.samples > 0 {
                let (started, initial_samples) =
                    *first_loss.get_or_insert((Instant::now(), update.samples));
                if started.elapsed() >= Duration::from_secs(3)
                    && update.samples >= initial_samples + 4_000_000
                    && saw_frame
                {
                    println!(
                        "UNAVAILABLE: receiver running, samples {} -> {}, audio_error={:?}, iq_drops={}",
                        initial_samples, update.samples, update.audio_error, update.iq_drops
                    );
                    break;
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "no sustained reception with an unavailable audio output before timeout".into(),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    settings.revision += 1;
    settings.running = false;
    submit(&controller, &settings)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(update) = controller.poll()
            && update.revision == settings.revision
            && !update.running
        {
            if let Some(error) = update.error {
                return Err(format!("receiver failed to stop: {error}").into());
            }
            println!("STOPPED: revision {}", update.revision);
            break;
        }
        if Instant::now() >= deadline {
            return Err("receiver did not stop within five seconds".into());
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!("Restore {expected_output} as the default output, then press Enter to restart.");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    settings.revision += 1;
    settings.running = true;
    submit(&controller, &settings)?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut saw_recovered_frame = false;
    loop {
        if let Some(update) = controller.poll() {
            if let Some(error) = update.error {
                return Err(format!("receiver failed after audio restoration: {error}").into());
            }
            if update.revision == settings.revision {
                saw_recovered_frame |= update.frame.is_some();
                if let Some(device) = &update.audio_output_device
                    && update.running
                    && update.samples >= 4_000_000
                    && update.audio_error.is_none()
                    && update.audio_queue_frames.is_some()
                    && saw_recovered_frame
                {
                    if !device.contains(&expected_output) {
                        return Err(format!("recovered on unexpected output: {device}").into());
                    }
                    println!(
                        "RECOVERED: revision {} samples={} output={} rate={:?} queue={:?} audio_rms={:.4} iq_drops={}",
                        update.revision,
                        update.samples,
                        device,
                        update.audio_output_rate_hz,
                        update.audio_queue_frames,
                        update.audio_rms,
                        update.iq_drops
                    );
                    break;
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("audio output did not recover within twelve seconds".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(controller);
    println!("PASS: unavailable output and same-process stop/start recovery");
    Ok(())
}
