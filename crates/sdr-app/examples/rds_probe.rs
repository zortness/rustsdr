//! Explicit hardware receive check: cargo run -p sdr-app --release --example rds_probe -- 106700000 20
use sdr_app::engine::{Engine, Settings, Source};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let station_hz = args.get(1).map_or(Ok(106_700_000), |v| v.parse())?;
    let seconds: u64 = args.get(2).map_or(Ok(20), |v| v.parse())?;
    let engine = Engine::with_settings(Settings {
        source: Source::RtlSdr,
        station_hz,
        ..Default::default()
    })?;
    let client = engine.client();
    client.set_volume(0.0);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last = None;
    let mut acquired = false;
    while Instant::now() < deadline {
        if let Some(state) = client.poll() {
            if let Some(error) = state.error {
                return Err(error.into());
            }
            if state.rds != last {
                println!(
                    "{:.1}s {station_hz} Hz {:?} iq_drops={}",
                    seconds as f64
                        - deadline
                            .saturating_duration_since(Instant::now())
                            .as_secs_f64(),
                    state.rds,
                    state.iq_drops
                );
                acquired |= state.rds.as_ref().is_some_and(|v| v.locked);
                last = state.rds;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !acquired {
        return Err("No checked RDS groups received during this probe".into());
    }
    Ok(())
}
