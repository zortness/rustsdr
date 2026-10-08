//! Independent signal generator for listening checks; never opens radio hardware.
use std::{
    f64::consts::TAU,
    fs::OpenOptions,
    io::{BufWriter, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || !["wfm", "stereo", "am", "nfm", "usb", "lsb"].contains(&args[0].as_str())
    {
        return Err("usage: voice_fixture wfm|stereo|am|nfm|usb|lsb OUTPUT.cf32".into());
    }
    let mode = args[0].as_str();
    let mut out = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?,
    );
    let mut phase = 0.0_f64;
    for n in 0..4_800_000 {
        let t = n as f64 / 2_400_000.0;
        let left = 0.3 * (TAU * 1000.0 * t).sin();
        let right = 0.3 * (TAU * 2000.0 * t).sin();
        let mut amplitude = 0.5;
        let deviation = match mode {
            "stereo" => {
                75000.0
                    * ((left + right) * 0.5
                        + (left - right) * 0.5 * (TAU * 38000.0 * t).cos()
                        + 0.1 * (TAU * 19000.0 * t).cos())
            }
            "wfm" => 75000.0 * left,
            "nfm" => 2500.0 * (TAU * 1000.0 * t).sin(),
            "am" => {
                amplitude *= 1.0 + 0.5 * (TAU * 1000.0 * t).sin();
                0.0
            }
            "usb" => 1000.0,
            "lsb" => -1000.0,
            _ => unreachable!(),
        };
        phase = (phase + TAU * (-250000.0 + deviation) / 2_400_000.0).rem_euclid(TAU);
        out.write_all(&((amplitude * phase.cos()) as f32).to_le_bytes())?;
        out.write_all(&((amplitude * phase.sin()) as f32).to_le_bytes())?;
    }
    out.flush()?;
    println!(
        "Created {}: 2 seconds, 2400000 samples/s, center 106950000 Hz, VFO 106700000 Hz, --mode {mode}",
        args[1]
    );
    Ok(())
}
