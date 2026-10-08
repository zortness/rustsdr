//! Generate a deterministic RDS-bearing recording centered at 106.95 MHz.
#[path = "../../sdr-dsp/tests/support/rds_signal.rs"]
mod signal;
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/rds-demo.cf32".into());
    signal::write_recording(std::path::Path::new(&path), 24);
    println!("Wrote {path}; center 106950000 Hz, sample rate 2400000, tune 106.7 MHz");
}
