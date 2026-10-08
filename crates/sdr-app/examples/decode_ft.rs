//! Offline slot decoder: cargo run -p sdr-app --example decode_ft -- ft8 FILE.wav
use sdr_app::digital::{Protocol, decode, read_wav};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let protocol = match args.get(1).map(String::as_str) {
        Some("ft8") => Protocol::Ft8,
        Some("ft4") => Protocol::Ft4,
        _ => return Err("Usage: decode_ft ft8|ft4 FILE.wav".into()),
    };
    let pcm = read_wav(
        std::path::Path::new(args.get(2).ok_or("Missing WAV path")?),
        protocol,
    )?;
    for m in decode(protocol, &pcm)? {
        println!(
            "{:>5.0} Hz {:>5.1} dB DT {:+.2}  {}",
            m.audio_hz, m.snr, m.dt, m.text
        );
    }
    Ok(())
}
