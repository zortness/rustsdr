//! Short, explicit hardware check; never run as an automated unit test.
use sdr_app::{
    engine::{Command, Engine, Settings, Source, SubmitError},
    scanning::{ScanChannel, ScanParameters, ScanPhase, ScanPlan, ScanTargets},
};
use sdr_dsp::{Mode, ReceiverConfig};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plan = Arc::new(ScanPlan {
        targets: ScanTargets::Memories(
            vec![
                ScanChannel {
                    frequency_hz: 106_700_000,
                    receiver: ReceiverConfig::for_mode(Mode::WfmStereo),
                    memory_index: Some(0),
                },
                ScanChannel {
                    frequency_hz: 89_900_000,
                    receiver: ReceiverConfig::for_mode(Mode::WfmMono),
                    memory_index: Some(1),
                },
            ]
            .into(),
        ),
        parameters: ScanParameters {
            hold_ms: 500,
            threshold_db: -35.0,
            ..Default::default()
        },
        start_index: 0,
    });
    let engine = Engine::with_settings(Settings {
        source: Source::RtlSdr,
        scan: Some(plan),
        ..Settings::default()
    })?;
    let client = engine.client();
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut last = (0, ScanPhase::Off);
    loop {
        if let Some(state) = client.poll() {
            if let Some(error) = state.error {
                return Err(error.into());
            }
            if (state.scan.visit, state.scan.phase) != last {
                println!(
                    "visit={} station={} mode={:?} phase={:?} power={:?} iq_drops={} audio_under={} audio_over={} audio_error={:?}",
                    state.scan.visit,
                    state.station_hz,
                    state.mode,
                    state.scan.phase,
                    state.channel_power_dbfs,
                    state.iq_drops,
                    state.audio_underruns,
                    state.audio_overruns,
                    state.audio_error
                );
                last = (state.scan.visit, state.scan.phase);
            }
            if state.scan.visit >= 4 {
                match client.submit(Command::StopScan { revision: 1 }) {
                    Ok(()) => break,
                    Err(SubmitError::Busy) => {}
                    other => return Err(format!("{other:?}").into()),
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("Scan did not advance".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let held = loop {
        if let Some(state) = client.poll()
            && state.scan.phase == ScanPhase::Off
        {
            break state.station_hz;
        }
        if Instant::now() >= deadline {
            return Err("Stop scan was not acknowledged".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    std::thread::sleep(Duration::from_millis(700));
    let state = client.poll().ok_or("State busy")?;
    assert_eq!(state.station_hz, held);
    assert_eq!(state.scan.phase, ScanPhase::Off);
    assert_eq!(state.capture_revision, 1);
    assert!(state.running);
    println!(
        "Stopped and held {held} Hz without reopening the source; iq_drops={} audio_under={} audio_over={}",
        state.iq_drops, state.audio_underruns, state.audio_overruns
    );
    Ok(())
}
