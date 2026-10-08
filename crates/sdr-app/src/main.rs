use sdr_core::RadioCommand;
use sdr_devices::{SyntheticSession, ToneConfig};
use sdr_dsp::SignalMeter;
use std::time::{Duration, Instant};

#[cfg(feature = "gui")]
mod gui;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--gui") {
        #[cfg(feature = "gui")]
        {
            return gui::run();
        }
        #[cfg(not(feature = "gui"))]
        {
            return Err(
                "GUI support requires: cargo run -p sdr-app --features gui -- --gui".into(),
            );
        }
    }
    let mut config = ToneConfig {
        sample_rate: 2_400_000,
        center_hz: 100_000_000,
        offset_hz: 125_000.0,
        amplitude: 0.5,
    };
    let mut blocks = 256_u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "RustSDR headless synthetic receiver\n\
                Usage: sdr-app [--blocks N] [--sample-rate HZ] [--tone-hz HZ] [--amplitude 0..1]\n\
                Defaults: 256 blocks, 2400000 samples/s, +125000 Hz tone, amplitude 0.5.\n\
                Each block contains 4096 IQ samples; the pool contains 4 reusable blocks.\n\
                Runs unpaced; no hardware, audio, or GUI is opened."
            );
            println!("GUI (build with --features gui): sdr-app --gui [--rtl] [--smoke-test]");
            println!("IQ playback: sdr-app --gui --file PATH --center-hz HZ --sample-rate 2400000");
            return Ok(());
        }
        if !matches!(
            arg.as_str(),
            "--blocks" | "--sample-rate" | "--tone-hz" | "--amplitude"
        ) {
            return Err(format!("unknown option {arg}; use --help").into());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--blocks" => blocks = value.parse()?,
            "--sample-rate" => config.sample_rate = value.parse()?,
            "--tone-hz" => config.offset_hz = value.parse()?,
            "--amplitude" => config.amplitude = value.parse()?,
            _ => unreachable!(),
        }
    }
    if blocks == 0 {
        return Err("--blocks must be greater than zero".into());
    }
    let expected_samples = blocks.checked_mul(4096).ok_or("sample count overflow")?;
    let mut session = SyntheticSession::new(config, 4096, 4)?;
    let mut meter = SignalMeter::default();
    let started = Instant::now();
    session.command(RadioCommand::Start)?;
    let mut generation = None;
    let mut next_index = 0;
    for _ in 0..blocks {
        let block = session.receive(Duration::from_secs(2))?;
        let metadata = block.metadata();
        if generation != Some(metadata.generation) || metadata.discontinuity {
            meter.reset();
            next_index = 0;
            generation = Some(metadata.generation);
        }
        if metadata.sample_index != next_index {
            return Err("unexpected IQ discontinuity".into());
        }
        meter.process(block.samples());
        next_index += block.samples().len() as u64;
    }
    session.command(RadioCommand::Stop)?;
    let elapsed = started.elapsed();
    let report = meter
        .report(config.sample_rate)
        .ok_or("no samples received")?;
    if report.samples != expected_samples {
        return Err("sample count mismatch".into());
    }
    println!("RustSDR headless synthetic receiver");
    println!("Samples: {}", report.samples);
    println!("Sample rate: {} samples/s (unpaced)", config.sample_rate);
    println!("Mean power: {:.6}", report.mean_power);
    match report.tone_offset_hz {
        Some(hz) => println!("Tone offset: {hz:.2} Hz"),
        None => println!("Tone offset: unavailable (silence or insufficient samples)"),
    }
    println!("IQ pool: 4 x 4096 samples (131072 bytes of sample storage)");
    println!(
        "Elapsed: {:.3} s; throughput: {:.2} MS/s",
        elapsed.as_secs_f64(),
        report.samples as f64 / elapsed.as_secs_f64() / 1e6
    );
    println!("State: {:?}", session.state());
    Ok(())
}
