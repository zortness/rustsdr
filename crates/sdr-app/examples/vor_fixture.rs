//! Generate deterministic 120-degree VOR IQ for file playback and GUI testing.
use std::io::Write;
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("target/vor-fixture.cf32".into());
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    for n in 0..7_200_000 {
        let t = n as f64 / 2_400_000.0;
        let p = std::f64::consts::TAU * 30.0 * t;
        let a = 0.5
            * (1.0
                + 0.3 * (p - 120.0_f64.to_radians()).cos()
                + 0.3 * (std::f64::consts::TAU * 9960.0 * t + 16.0 * p.sin()).cos());
        let carrier = -std::f64::consts::TAU * 250_000.0 * t;
        file.write_all(&((a * carrier.cos()) as f32).to_le_bytes())
            .unwrap();
        file.write_all(&((a * carrier.sin()) as f32).to_le_bytes())
            .unwrap();
    }
    file.flush().unwrap();
    drop(file);
    println!("Created {path}: 2.4 MS/s, center 113.250 MHz, VOR 113.000 MHz, radial 120 degrees");
}
