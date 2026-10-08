//! Explicitly opt-in, single three-second TX test through the production engine.
use sdr_app::engine::{Engine, Ic705Settings, Settings, Source};
use sdr_devices::ic705::Command;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 6 || args[5] != "--dummy-load-test" {
        return Err("Expected PORT INPUT MONITOR TX_OUTPUT EXPECTED_HZ --dummy-load-test. Sends ONE 3-second tone at minimum RF power (not zero watts).".into());
    }
    let expected_hz: u32 = args[4].parse()?;
    let mut settings = Settings {
        source: Source::Ic705,
        ic705: Ic705Settings {
            port: args[0].clone(),
            input: args[1].clone(),
            output: args[2].clone(),
            tx_output: args[3].clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone())?;
    engine.set_volume(0.0);
    let start = Instant::now();
    let mut sent = false;
    let mut saw_tx = false;
    let mut recovered = false;
    let mut previous = None;
    while start.elapsed() < Duration::from_secs(12) {
        if let Some(update) = engine.poll() {
            if let Some(error) = update.error {
                return Err(error.into());
            }
            if let Some(radio) = update.ic705 {
                let status = (update.running, radio.transmitting);
                if previous != Some(status) {
                    println!(
                        "{:.3}s running={} TX={} samples={}",
                        start.elapsed().as_secs_f64(),
                        status.0,
                        status.1,
                        update.samples
                    );
                    previous = Some(status);
                }
                if !sent && update.running && update.samples > 48_000 {
                    if radio.frequency_hz != expected_hz || radio.transmitting {
                        return Err("Radio dial/state differs from requested test".into());
                    }
                    settings.revision += 1;
                    settings.ic705.command = Some((
                        1,
                        Command::TestTone {
                            frequency_hz: expected_hz,
                            power_percent: 0,
                        },
                    ));
                    if !engine.request(settings.clone()) {
                        return Err("Test request rejected".into());
                    }
                    sent = true;
                    println!("Requested one test");
                }
                saw_tx |= radio.transmitting;
                recovered |=
                    saw_tx && !radio.transmitting && update.running && update.frame.is_some();
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(engine);
    if !recovered {
        return Err("Did not observe TX followed by RX spectrum recovery".into());
    }
    println!("TX -> RX and spectrum recovery verified; session stopped");
    Ok(())
}
