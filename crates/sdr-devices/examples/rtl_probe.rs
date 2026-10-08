use sdr_devices::rtl::{RtlSession, devices};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend =
        sdr_devices::RtlBackend::from_key(&std::env::args().nth(2).unwrap_or("librtlsdr".into()))
            .ok_or("Use librtlsdr or rs-rtl")?;
    if backend == sdr_devices::RtlBackend::Librtlsdr {
        println!("Devices: {:?}", devices()?);
    }
    let frequency = std::env::args()
        .nth(1)
        .unwrap_or("106700000".into())
        .parse()?;
    let mut radio = RtlSession::open_with(backend, 0, frequency, 2_400_000)?;
    println!("Applied: {:?}", radio.info());
    let start = Instant::now();
    let mut samples = 0_u64;
    let mut power = 0.0_f64;
    while start.elapsed() < Duration::from_secs(3) {
        let block = radio.receive(Duration::from_secs(1))?;
        for sample in block.samples() {
            power += (sample.i * sample.i + sample.q * sample.q) as f64;
        }
        samples += block.samples().len() as u64;
    }
    radio.retune(frequency + 100_000)?;
    let block = radio.receive(Duration::from_secs(2))?;
    println!(
        "Retune: {:?}; first index {}",
        radio.info(),
        block.sample_index
    );
    drop(block);
    // Exercise a full downstream queue before shutdown.
    std::thread::sleep(Duration::from_millis(200));
    let stop = Instant::now();
    radio.stop()?;
    println!(
        "Stop after stalled reader: {:.3}s",
        stop.elapsed().as_secs_f64()
    );
    println!(
        "Frequency: {frequency} Hz; samples: {samples}; mean power: {:.6}; host drops: {}",
        power / samples as f64,
        radio.dropped_samples()
    );
    Ok(())
}
