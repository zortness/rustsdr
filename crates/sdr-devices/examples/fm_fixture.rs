//! Generate a two-second 1 kHz mono FM recording, without radio hardware.
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: fm_fixture OUTPUT.cf32 (must not exist)")?;
    let mut output = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?,
    );
    let mut phase = 0.0_f64;
    for n in 0..4_800_000 {
        let message = (std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0).sin();
        phase = (phase + std::f64::consts::TAU * (-250_000.0 + 37_500.0 * message) / 2_400_000.0)
            .rem_euclid(std::f64::consts::TAU);
        output.write_all(&(phase.cos() as f32 * 0.5).to_le_bytes())?;
        output.write_all(&(phase.sin() as f32 * 0.5).to_le_bytes())?;
    }
    output.flush()?;
    println!(
        "Wrote {path}: cf32le, 2400000 samples/s, center 106950000 Hz, station 106700000 Hz, duration 2 s"
    );
    Ok(())
}
