//! Headless spike: deterministic gradient -> WAV, or WAV -> last image (PPM).
use sdr_app::sstv::{self, Mode};
use std::{fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: sstv_probe encode Robot36|Martin1|Scottie1|Pd120 out.wav OR decode in.wav out.ppm".into());
    }
    match args[0].as_str() {
        "encode" => {
            let mode = Mode::ALL
                .into_iter()
                .find(|m| format!("{m:?}") == args[1])
                .ok_or("unknown mode")?;
            let (w, h) = mode.dimensions();
            let rgb: Vec<_> = (0..h)
                .flat_map(|y| {
                    (0..w).flat_map(move |x| {
                        [(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 96]
                    })
                })
                .collect();
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[2])?;
            let mut wav = hound::WavWriter::new(
                std::io::BufWriter::new(file),
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 48_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )?;
            let mut samples = 0;
            for sample in sstv::encode(mode, &rgb)? {
                wav.write_sample(sample)?;
                samples += 1;
            }
            wav.finalize()?;
            println!(
                "{mode:?}: {samples} samples, {:.3} seconds",
                samples as f64 / 48_000.0
            );
        }
        "decode" => {
            let mut wav = hound::WavReader::open(&args[1])?;
            let spec = wav.spec();
            if spec.channels != 1
                || spec.sample_rate != 48_000
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
                || wav.len() > 48_000 * 600
            {
                return Err("Spike input: mono PCM16 48 kHz WAV, at most ten minutes".into());
            }
            let samples = wav.samples::<i16>().collect::<Result<Vec<_>, _>>()?;
            let start = std::time::Instant::now();
            let mut frame = None;
            sstv::decode(samples.into_iter(), None, |f| frame = Some(f))
                .map_err(|e| format!("{e:?}"))?;
            let frame = frame.ok_or("no SSTV image detected")?;
            let (w, h) = frame.mode.dimensions();
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[2])?;
            write!(file, "P6\n{w} {h}\n255\n")?;
            file.write_all(&frame.rgb)?;
            println!(
                "{:?}: {w}x{h}, {} rows, complete={}, decode {:.3}s",
                frame.mode,
                frame.rows,
                frame.complete(),
                start.elapsed().as_secs_f64()
            );
        }
        _ => return Err("expected encode or decode".into()),
    }
    Ok(())
}
