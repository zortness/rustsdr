//! Receive-only USB audio probe. Never opens an output endpoint.
#[cfg(feature = "audio")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use ringbuf::{HeapRb, traits::*};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use std::time::{Duration, Instant};
    let host = cpal::default_host();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        for device in host.input_devices()? {
            println!(
                "INPUT: {:?} / {:?}",
                device.description()?,
                device.default_input_config()
            );
        }
        if let Some(device) = host.default_output_device() {
            println!("Default output (NOT opened): {:?}", device.description()?);
        }
        println!("Capture: radio_audio_probe <exact input name> <new .f32le path>");
        return Ok(());
    }
    if args.len() != 2 {
        return Err("Expected exact input name and new output file".into());
    }
    let mut matches = host
        .input_devices()?
        .filter(|d| d.description().is_ok_and(|v| v.name() == args[0]));
    let device = matches.next().ok_or("Input device not found")?;
    if matches.next().is_some() {
        return Err("Ambiguous input name".into());
    }
    let config = device.default_input_config()?;
    println!("Capture {}: {config:?}", args[0]);
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err("This probe requires a float32 host capture format".into());
    }
    let channels = usize::from(config.channels());
    let rate = config.sample_rate();
    let (mut producer, mut consumer) = HeapRb::<f32>::new(rate as usize * channels).split();
    let lost = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let callback_lost = lost.clone();
    let callback_errors = errors.clone();
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
            let accepted = producer.push_slice(data);
            callback_lost.fetch_add((data.len() - accepted) as u64, Ordering::Relaxed);
        },
        move |_| {
            callback_errors.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )?;
    use std::io::Write;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    let mut file = std::io::BufWriter::new(file);
    let mut count = 0u64;
    let mut energy = vec![0.0f64; channels];
    let mut peak = vec![0.0f32; channels];
    let mut clipped = 0u64;
    let mut nonfinite = 0u64;
    stream.play()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        while let Some(value) = consumer.try_pop() {
            let channel = count as usize % channels;
            count += 1;
            if value.is_finite() {
                energy[channel] += f64::from(value).powi(2);
                peak[channel] = peak[channel].max(value.abs());
                clipped += u64::from(value.abs() >= 1.0);
            } else {
                nonfinite += 1;
            }
            file.write_all(&value.to_le_bytes())?;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(stream);
    file.flush()?;
    println!(
        "frames={} rate={rate} channels={channels} lost_samples={} stream_errors={} clipped={clipped} nonfinite={nonfinite}",
        count / channels as u64,
        lost.load(Ordering::Relaxed),
        errors.load(Ordering::Relaxed)
    );
    for ch in 0..channels {
        println!(
            "channel {ch}: RMS {:.2} dBFS, peak {:.2} dBFS",
            10.0 * (energy[ch] / (count / channels as u64).max(1) as f64).log10(),
            20.0 * peak[ch].log10()
        );
    }
    if count == 0
        || lost.load(Ordering::Relaxed) != 0
        || errors.load(Ordering::Relaxed) != 0
        || nonfinite != 0
    {
        return Err("Incomplete/invalid capture; inspect counters".into());
    }
    Ok(())
}

#[cfg(not(feature = "audio"))]
fn main() {
    eprintln!("Enable --features audio");
    std::process::exit(1);
}
