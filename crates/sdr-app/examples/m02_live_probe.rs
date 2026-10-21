//! Receive-only M02 device and reconfiguration probe; does not touch preferences.
use sdr_app::engine::{Command, Engine, Settings, Source};
use sdr_devices::{RtlBackend, rtl};
use sdr_dsp::zoom_spectrum::SpectrumView;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = match std::env::args().nth(1).as_deref() {
        Some("librtlsdr") => RtlBackend::Librtlsdr,
        None | Some("rs-rtl") => RtlBackend::Rust,
        Some(_) => return Err("usage: m02_live_probe [rs-rtl|librtlsdr]".into()),
    };
    let devices = rtl::discover(backend)?;
    println!("Available {backend:?} devices: {devices:?}");
    let selected = devices.first().ok_or("No RTL device connected")?;
    let mut settings = Settings {
        running: false,
        source: Source::RtlSdr,
        rtl_backend: backend,
        rtl_device_index: selected.index,
        rtl_device_serial: selected.serial.clone(),
        ..Settings::default()
    };
    let engine = Engine::with_settings(settings.clone())?;
    let client = engine.client();
    client.set_volume(0.0);
    let audio = client.subscribe_audio();
    for (rate, gain) in [(2_400_000, 280), (1_920_000, 328), (2_400_000, 280)] {
        settings.revision += 1;
        settings.running = true;
        settings.rtl_sample_rate = rate;
        settings.rtl_gain_tenth_db = gain;
        settings.spectrum_view = SpectrumView::full(rate as f64);
        client
            .submit(Command::Configure(Box::new(settings.clone())))
            .map_err(|e| format!("{e:?}"))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut frames = 0;
        let mut audio_blocks = 0;
        let mut samples = 0;
        let mut drops = 0;
        while Instant::now() < deadline && (frames < 2 || audio_blocks < 2 || samples == 0) {
            if let Some(state) = client.poll()
                && state.revision == settings.revision
            {
                if let Some(error) = state.error {
                    return Err(error.into());
                }
                if let Some(info) = state.rtl
                    && (info.sample_rate != rate
                        || info.requested_gain_tenth_db != gain
                        || info.device_index != selected.index)
                {
                    return Err(format!("Configuration mismatch: {info:?}").into());
                }
                if let Some(frame) = state.frame {
                    if frame.revision != settings.revision
                        || frame.capture_revision != settings.revision
                        || frame.view.span_hz != rate as f64
                    {
                        return Err("Stale or incorrectly scaled spectrum frame".into());
                    }
                    frames += 1;
                }
                samples = state.samples;
                drops = state.iq_drops;
            }
            while let Some(block) = audio.next() {
                if block.revision == settings.revision {
                    if block.capture_revision != settings.revision {
                        return Err("Stale audio capture revision".into());
                    }
                    audio_blocks += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if frames < 2 || audio_blocks < 2 || samples == 0 {
            return Err(format!("Insufficient live output at rate {rate}: {frames} frames, {audio_blocks} audio blocks, {samples} samples").into());
        }
        println!(
            "revision {}: {rate} samples/s, {:.1} dB, {frames} spectrum frames, {audio_blocks} audio blocks, {samples} samples, {drops} host drops",
            settings.revision,
            gain as f32 / 10.0
        );
    }
    drop(engine);
    println!("M02 live rate/gain handoff completed");
    Ok(())
}
