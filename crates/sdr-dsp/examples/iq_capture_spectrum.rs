//! Inspect 48 kHz interleaved float32 two-channel probe captures as ch0 + j*ch1.
use rustfft::{FftPlanner, num_complex::Complex32};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const N: usize = 32768;
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path)?;
        if bytes.len() % 8 != 0 {
            return Err("Expected complete two-channel float32 frames".into());
        }
        let frames: Vec<_> = bytes
            .chunks_exact(8)
            .map(|b| {
                Complex32::new(
                    f32::from_le_bytes(b[..4].try_into().unwrap()),
                    f32::from_le_bytes(b[4..].try_into().unwrap()),
                )
            })
            .collect();
        if frames
            .iter()
            .any(|v| !v.re.is_finite() || !v.im.is_finite())
        {
            return Err("Nonfinite capture".into());
        }
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
        let mut power = vec![0.0f64; N];
        let mut blocks = 0;
        // Exclude the first second, including mode/tuning and capture startup.
        for block in frames.get(48000..).unwrap_or_default().chunks_exact(N) {
            let mut buffer: Vec<_> = block
                .iter()
                .enumerate()
                .map(|(i, v)| v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()))
                .collect();
            fft.process(&mut buffer);
            for (sum, value) in power.iter_mut().zip(buffer) {
                *sum += f64::from(value.norm_sqr());
            }
            blocks += 1;
        }
        if blocks == 0 {
            return Err("Capture too short".into());
        }
        println!(
            "{path}: frames={}, averaged blocks={blocks}, bin width {:.6} Hz",
            frames.len(),
            48000.0 / N as f64
        );
        let mut ranked: Vec<_> = (0..N).collect();
        ranked.sort_unstable_by(|&a, &b| power[b].total_cmp(&power[a]));
        let mut selected = Vec::<usize>::new();
        for bin in ranked {
            if selected.iter().any(|&old| {
                let d = old.abs_diff(bin);
                d.min(N - d) < 30
            }) {
                continue;
            }
            let signed = if bin < N / 2 {
                bin as i32
            } else {
                bin as i32 - N as i32
            };
            let mirror = (N - bin) % N;
            println!(
                "  peak {:+.3} Hz; mirror ratio {:.1} dB",
                signed as f64 * 48000.0 / N as f64,
                10.0 * (power[bin] / power[mirror]).log10()
            );
            selected.push(bin);
            if selected.len() == 5 {
                break;
            }
        }
    }
    Ok(())
}
