#[path = "../../sdr-dsp/tests/support/cw_signal.rs"]
mod signal;
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/cw-demo.cf32".into());
    signal::write_recording(std::path::Path::new(&path), 3);
    println!("Wrote {path}; 2400000 samples/s, center 106950000 Hz, CW at 106.7 MHz: CQ RUST SDR");
}
