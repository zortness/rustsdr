use sdr_app::engine::{Command, Engine, QmxSettings, Settings, Source, Update};
use sdr_devices::qmx::Command as CatCommand;
use std::time::{Duration, Instant};

fn wait(engine: &Engine, revision: u64) -> Result<Update, String> {
    wait_samples(engine, revision, 24_000)
}
fn wait_samples(engine: &Engine, revision: u64, minimum: u64) -> Result<Update, String> {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(update) = engine.poll() {
            if let Some(error) = update.error {
                return Err(error);
            }
            if update.revision == revision
                && update.running
                && update.samples > minimum
                && update.qmx.is_some()
            {
                return Ok(update);
            }
        }
        if Instant::now() > deadline {
            return Err("QMX engine timeout".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn submit(engine: &Engine, settings: &Settings) -> Result<(), String> {
    for _ in 0..100 {
        match engine
            .client()
            .submit(Command::Configure(Box::new(settings.clone())))
        {
            Ok(()) => return Ok(()),
            Err(sdr_app::engine::SubmitError::Busy) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(format!("Command rejected: {error:?}")),
        }
    }
    Err("Busy control slot".into())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 && !(args.len() == 4 && args[3] == "--modes") {
        return Err("Expected PORT INPUT_NAME OUTPUT_NAME [--modes]".into());
    }
    let mut settings = Settings {
        source: Source::Qmx,
        qmx: QmxSettings {
            port: args[0].clone(),
            input: args[1].clone(),
            output: args[2].clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone())?;
    engine.set_volume(0.0);
    let audio = engine.client().subscribe_audio();
    let initial = wait(&engine, 1)?;
    let original = initial.qmx.as_ref().unwrap().as_ref().clone();
    println!(
        "Connected {} Hz mode {}, firmware {}, bands {:?}",
        original.frequency_hz,
        original.mode,
        original.firmware,
        original.bands.iter().map(|b| b.meters).collect::<Vec<_>>()
    );
    let result = (|| -> Result<(), String> {
        let block = audio.next().ok_or("No audio subscription frames")?;
        if block.sample_rate != 48_000 || block.frames.is_empty() {
            return Err("Bad audio subscription".into());
        }
        println!(
            "Audio: {} frames at {} Hz; input drops {}",
            block.frames.len(),
            block.sample_rate,
            initial.iq_drops
        );
        if args.len() == 4 {
            let mut samples = initial.samples;
            for mode in [6, 2, 9, 3, original.mode] {
                settings.revision += 1;
                settings.qmx.command = Some((
                    settings.revision,
                    CatCommand::Tune {
                        frequency_hz: original.frequency_hz,
                        mode,
                    },
                ));
                submit(&engine, &settings)?;
                let update = wait_samples(&engine, settings.revision, samples + 12_000)?;
                if update.qmx.as_ref().unwrap().mode != mode {
                    return Err("Mode readback mismatch".into());
                }
                let expected = match mode {
                    1 | 9 => sdr_dsp::Mode::Lsb,
                    2 | 6 => sdr_dsp::Mode::Usb,
                    _ => sdr_dsp::Mode::Cw,
                };
                if update.mode != expected {
                    return Err("Wrong engine audio mode".into());
                }
                if update.capture_revision != initial.capture_revision {
                    return Err("Mode change restarted capture".into());
                }
                println!(
                    "Mode {mode}: audio advanced {} frames; capture {}; drops {}",
                    update.samples - samples,
                    update.capture_revision,
                    update.iq_drops
                );
                samples = update.samples;
            }
            return Ok(());
        }
        let band = original
            .band()
            .ok_or("Current frequency outside band table")?;
        let offset = original
            .frequency_hz
            .checked_sub(1000)
            .filter(|hz| band.contains(*hz))
            .ok_or("No in-band test frequency")?;
        let other = original
            .bands
            .iter()
            .find(|b| b.index != band.index)
            .ok_or("No second band")?;
        for (index, command) in [
            CatCommand::Tune {
                frequency_hz: offset,
                mode: original.mode,
            },
            CatCommand::Band(other.index),
            CatCommand::Band(band.index),
            CatCommand::Tune {
                frequency_hz: original.frequency_hz,
                mode: original.mode,
            },
        ]
        .into_iter()
        .enumerate()
        {
            settings.revision += 1;
            settings.qmx.command = Some((index as u64 + 1, command));
            submit(&engine, &settings)?;
            let update = wait(&engine, settings.revision)?;
            let state = update.qmx.unwrap();
            println!(
                "Applied revision {}: {} Hz mode {}",
                update.revision, state.frequency_hz, state.mode
            );
            let expected = match index {
                0 | 2 => offset,
                1 => other.start_hz,
                _ => original.frequency_hz,
            };
            if state.frequency_hz != expected {
                return Err(format!("Expected {expected} Hz"));
            }
        }
        Ok(())
    })();
    drop(engine);
    // Independent cleanup after any failed assertion/command; failure is reported.
    let mut radio = sdr_devices::qmx::open(&args[0])?;
    radio.apply(&CatCommand::Tune {
        frequency_hz: original.frequency_hz,
        mode: original.mode,
    })?;
    println!(
        "Restored {} Hz mode {}; receive",
        radio.state.frequency_hz, radio.state.mode
    );
    result.map_err(Into::into)
}
