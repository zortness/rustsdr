//! Locally generated IQ only; this does not interface with a transmitter.
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
pub fn write_recording(path: &Path, repeats: usize) {
    let mut file = BufWriter::new(File::create(path).unwrap());
    let mut sample = 0_u64;
    let mut run = |on: bool, ms: usize| {
        // Periodic 12-sample oscillator is exact at -250 kHz / 2.4 MHz.
        for _ in 0..ms * 2400 {
            let p = -std::f64::consts::TAU * 5.0 * (sample % 48) as f64 / 48.0;
            let a = if on { 0.5 } else { 0.0 };
            file.write_all(&(p.cos() as f32 * a).to_le_bytes()).unwrap();
            file.write_all(&(p.sin() as f32 * a).to_le_bytes()).unwrap();
            sample += 1;
        }
    };
    run(false, 500);
    for _ in 0..repeats {
        for (i, ch) in "CQ RUST SDR".chars().enumerate() {
            if ch == ' ' {
                run(false, 240);
                continue;
            }
            let pattern = sdr_dsp::cw::ALPHABET
                .iter()
                .find(|entry| entry.0 == ch)
                .unwrap()
                .1;
            for (j, element) in pattern.chars().enumerate() {
                // Deliberately irregular keying, about 20 WPM.
                let jitter = [0.88, 1.12, 0.96, 1.06][(i + j) % 4];
                run(
                    true,
                    ((if element == '-' { 180.0 } else { 60.0 }) * jitter) as usize,
                );
                run(false, 60);
            }
            run(false, 120);
        }
        run(false, 800);
    }
    run(false, 500);
    file.flush().unwrap();
}
