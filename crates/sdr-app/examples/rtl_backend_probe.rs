//! Receive-only Ham It Up FT8 comparison; never loads or writes user preferences.
use sdr_app::{
    digital::{Config, Protocol},
    engine::{Command, Engine, Settings, Source},
};
use sdr_devices::RtlBackend;
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let seconds = args.get(1).map_or(Ok(35_u64), |s| s.parse())?;
    let correction = args.get(2).map_or(Ok(0_i64), |s| s.parse())?;
    let mut settings = Settings {
        running: false,
        source: Source::RtlSdr,
        station_hz: 7_074_000,
        converter_offset_hz: -125_000_000 + correction,
        receiver: sdr_dsp::ReceiverConfig {
            bandwidth_hz: 4700,
            agc: false,
            ..sdr_dsp::ReceiverConfig::for_mode(sdr_dsp::Mode::Usb)
        },
        ..Default::default()
    };
    let engine = Engine::with_settings(settings.clone())?;
    let client = engine.client();
    client.set_volume(0.0);
    let decoder = client.digital();
    for backend in [
        RtlBackend::Librtlsdr,
        RtlBackend::Rust,
        RtlBackend::Librtlsdr,
    ] {
        settings.rtl_backend = backend;
        settings.running = true;
        settings.revision += 1;
        decoder.configure(Config {
            protocol: Some(Protocol::Ft8),
            revision: settings.revision,
            dial_hz: 7_074_000,
            file_clock: false,
        });
        client
            .submit(Command::Configure(Box::new(settings.clone())))
            .map_err(|e| format!("{e:?}"))?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut messages = 0;
        let mut announced = false;
        let mut samples = 0;
        let mut drops = 0;
        while Instant::now() < deadline {
            if let Some(state) = client.poll()
                && state.revision == settings.revision
            {
                if let Some(error) = state.error {
                    return Err(error.into());
                }
                if state.running && !announced {
                    println!(
                        "Active {:?}; capture revision {}",
                        state.rtl, state.capture_revision
                    );
                    announced = true;
                }
                samples = state.samples;
                drops = state.iq_drops;
            }
            while let Some(batch) = decoder.results.next() {
                if batch.config.revision == settings.revision {
                    match batch.result {
                        Ok(decoded) => {
                            messages += decoded.len();
                            println!(
                                "{} slot {}: {} messages",
                                backend.key(),
                                batch.slot_ms,
                                decoded.len()
                            );
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !announced || samples == 0 {
            return Err("Backend did not start".into());
        }
        println!(
            "{}: {samples} samples, {drops} host drops, {messages} FT8 messages",
            backend.key()
        );
    }
    drop(engine);
    println!("Both backend handoffs and shutdown completed");
    Ok(())
}
