//! Receive-only, interactive physical USB reconnect check for M02.
use sdr_app::engine::{Command, Engine, Settings, Source};
use sdr_devices::{RtlBackend, rtl};
use std::time::{Duration, Instant};

fn present(serial: Option<&str>, index: u32) -> bool {
    rtl::discover(RtlBackend::Rust).is_ok_and(|list| {
        list.iter()
            .any(|d| serial.map_or(d.index == index, |s| d.serial.as_deref() == Some(s)))
    })
}

fn wait_presence(serial: Option<&str>, index: u32, expected: bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        if present(serial, index) == expected {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "Timed out waiting for RTL device presence={expected}"
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let selected = rtl::discover(RtlBackend::Rust)?
        .into_iter()
        .next()
        .ok_or("No RTL device")?;
    println!("Selected {}", selected.label());
    let mut settings = Settings {
        running: false,
        source: Source::RtlSdr,
        rtl_device_index: selected.index,
        rtl_device_serial: selected.serial.clone(),
        ..Settings::default()
    };
    let engine = Engine::with_settings(settings.clone())?;
    let client = engine.client();
    client.set_volume(0.0);
    for cycle in 0..=3 {
        settings.revision += 1;
        settings.running = true;
        client
            .submit(Command::Configure(Box::new(settings.clone())))
            .map_err(|e| format!("{e:?}"))?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut frames = 0;
        let mut samples = 0;
        while Instant::now() < deadline && (frames < 2 || samples == 0) {
            if let Some(state) = client.poll()
                && state.revision == settings.revision
            {
                if let Some(error) = state.error {
                    return Err(error.into());
                }
                if let Some(frame) = state.frame {
                    if frame.capture_revision != settings.revision {
                        return Err("Stale frame after reconnect".into());
                    }
                    frames += 1;
                }
                samples = state.samples;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if frames < 2 || samples == 0 {
            return Err(format!("Cycle {cycle}: reception did not resume").into());
        }
        println!("Cycle {cycle}: receiving, {samples} samples and {frames} fresh frames");
        if cycle == 3 {
            break;
        }
        println!("Please unplug the RTL-SDR now (cycle {}).", cycle + 1);
        wait_presence(selected.serial.as_deref(), selected.index, false)?;
        println!("Cycle {}: removal detected", cycle + 1);
        settings.revision += 1;
        settings.running = false;
        client
            .submit(Command::Configure(Box::new(settings.clone())))
            .map_err(|e| format!("{e:?}"))?;
        println!("Please reconnect the RTL-SDR now (cycle {}).", cycle + 1);
        wait_presence(selected.serial.as_deref(), selected.index, true)?;
        println!("Cycle {}: USB device present again", cycle + 1);
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(engine);
    println!("Three physical reconnect cycles passed in one process");
    Ok(())
}
