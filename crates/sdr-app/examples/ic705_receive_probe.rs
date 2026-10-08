//! Bounded receive-only integration probe. No tuning or TX commands.
use sdr_app::engine::{Engine, Ic705Settings, Settings, Source};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("Inputs: {:#?}", sdr_app::audio::device_names(true)?);
        println!("Outputs: {:#?}", sdr_app::audio::device_names(false)?);
        return Ok(());
    }
    if args.len() != 3 && !(args.len() == 5 && args[3] == "--min-dbfs") {
        return Err("Expected PORT INPUT_LABEL MONITOR_LABEL [--min-dbfs LEVEL]".into());
    }
    let minimum = args.get(4).map(|v| v.parse::<f64>()).transpose()?;
    if minimum.is_some_and(|v| !v.is_finite() || !(-180.0..=0.0).contains(&v)) {
        return Err("Invalid input-level threshold".into());
    }
    let engine = Engine::with_settings(Settings {
        source: Source::Ic705,
        ic705: Ic705Settings {
            port: args[0].clone(),
            input: args[1].clone(),
            output: args[2].clone(),
            ..Default::default()
        },
        ..Default::default()
    })?;
    engine.set_volume(0.0);
    let audio = engine.client().subscribe_audio();
    let start = Instant::now();
    let mut frames = 0;
    let mut spectra = 0;
    let mut status = None;
    let mut energy = 0.0f64;
    while start.elapsed() < Duration::from_secs(5) {
        while let Some(block) = audio.next() {
            frames += block.frames.len();
            energy += block
                .frames
                .iter()
                .map(|v| (f64::from(v.left).powi(2) + f64::from(v.right).powi(2)) * 0.5)
                .sum::<f64>();
        }
        if let Some(update) = engine.poll() {
            if let Some(e) = update.error {
                return Err(e.into());
            }
            spectra += usize::from(update.frame.is_some());
            status = Some(update);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = status.ok_or("No engine status")?;
    println!(
        "IC-705 {} Hz {:?}: audio frames={frames}, spectra={spectra}, input drops={}, RMS={}",
        state.station_hz, state.mode, state.iq_drops, state.audio_rms
    );
    if frames < 48_000 || spectra == 0 || state.ic705.is_none() {
        return Err("No continuous radio audio/spectrum".into());
    }
    let dbfs = 10.0 * (energy / frames as f64).max(1e-30).log10();
    println!("Subscribed USB audio: {dbfs:.1} dBFS");
    if minimum.is_some_and(|threshold| dbfs < threshold) {
        return Err("USB samples arrived but audio is below the required level".into());
    }
    drop(engine);
    println!("Receive session stopped");
    Ok(())
}
