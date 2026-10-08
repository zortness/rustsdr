use sdr_devices::{ToneConfig, ToneSource};

#[test]
fn restart_invalidates_previous_generation_and_control_is_explicit() {
    use sdr_core::{RadioCommand, RadioError, SessionState};
    use sdr_devices::SyntheticSession;
    use std::time::Duration;
    let mut session = SyntheticSession::new(
        ToneConfig {
            sample_rate: 48_000,
            center_hz: 100_000_000,
            offset_hz: 3000.0,
            amplitude: 0.5,
        },
        128,
        1,
    )
    .unwrap();
    assert_eq!(session.state(), SessionState::Stopped);
    assert!(matches!(
        session.receive(Duration::ZERO),
        Err(RadioError::NotRunning)
    ));
    assert_eq!(
        session.command(RadioCommand::SetRxCenterHz(5)),
        Err(RadioError::Unsupported(
            "synthetic center frequency is fixed for the session"
        ))
    );
    session.command(RadioCommand::Start).unwrap();
    assert_eq!(
        session.command(RadioCommand::Start),
        Err(RadioError::AlreadyRunning)
    );
    let old = session.receive(Duration::from_secs(1)).unwrap();
    let (done, completed) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        session.command(RadioCommand::Stop).unwrap();
        session.command(RadioCommand::Stop).unwrap();
        session.command(RadioCommand::Start).unwrap();
        let new = session.receive(Duration::from_secs(1)).unwrap();
        assert!(new.metadata().generation > old.metadata().generation);
        assert_eq!(new.metadata().stream_id, old.metadata().stream_id);
        assert_eq!(new.metadata().sample_index, 0);
        assert!(new.metadata().discontinuity);
        drop(old);
        // Old storage cannot enter the new generation's single-buffer pool.
        assert!(matches!(
            session.receive(Duration::from_millis(20)),
            Err(RadioError::Timeout)
        ));
        drop(session);
        done.send(()).unwrap();
    });
    completed
        .recv_timeout(Duration::from_secs(2))
        .expect("shutdown/restart hung");
    worker.join().unwrap();
}

#[test]
fn session_delivers_ordered_blocks_and_recycles_a_fixed_pool() {
    use sdr_devices::SyntheticSession;
    use std::time::Duration;
    let mut session = SyntheticSession::new(
        ToneConfig {
            sample_rate: 48_000,
            center_hz: 100_000_000,
            offset_hz: 3000.0,
            amplitude: 0.5,
        },
        128,
        2,
    )
    .unwrap();
    session.start().unwrap();
    let first = session.receive(Duration::from_secs(1)).unwrap();
    let second = session.receive(Duration::from_secs(1)).unwrap();
    assert_eq!(first.metadata().sample_index, 0);
    assert!(first.metadata().discontinuity);
    assert_eq!(second.metadata().sample_index, 128);
    assert!(!second.metadata().discontinuity);
    assert_eq!(first.samples().len(), 128);
    // Holding every lease exhausts the pool: the producer cannot allocate more.
    assert!(matches!(
        session.receive(Duration::from_millis(30)),
        Err(sdr_core::RadioError::Timeout)
    ));
    let address = first.samples().as_ptr();
    drop(first);
    let third = session.receive(Duration::from_secs(1)).unwrap();
    assert_eq!(third.samples().as_ptr(), address);
    assert_eq!(third.metadata().sample_index, 256);
    session.stop().unwrap();
}

#[test]
fn rejects_invalid_tone_parameters() {
    let valid = ToneConfig {
        sample_rate: 48_000,
        center_hz: 0,
        offset_hz: 1000.0,
        amplitude: 0.5,
    };
    for config in [
        ToneConfig {
            sample_rate: 0,
            ..valid
        },
        ToneConfig {
            offset_hz: f64::NAN,
            ..valid
        },
        ToneConfig {
            offset_hz: 24_000.0,
            ..valid
        },
        ToneConfig {
            offset_hz: -24_000.0,
            ..valid
        },
        ToneConfig {
            amplitude: f32::INFINITY,
            ..valid
        },
        ToneConfig {
            amplitude: -0.1,
            ..valid
        },
        ToneConfig {
            amplitude: 1.1,
            ..valid
        },
    ] {
        assert!(ToneSource::new(config).is_err(), "accepted {config:?}");
    }
}

#[test]
fn tone_is_phase_continuous_across_reused_blocks() {
    let mut source = ToneSource::new(ToneConfig {
        sample_rate: 48_000,
        center_hz: 100_000_000,
        offset_hz: -3_000.0,
        amplitude: 0.5,
    })
    .unwrap();
    let mut samples = vec![sdr_core::Iq::default(); 127];
    for block in 0..10 {
        source.fill(&mut samples);
        for (index, sample) in samples.iter().enumerate() {
            let phase = std::f64::consts::TAU * -3_000.0 * (block * 127 + index) as f64 / 48_000.0;
            assert!((sample.i as f64 - 0.5 * phase.cos()).abs() < 1e-6);
            assert!((sample.q as f64 - 0.5 * phase.sin()).abs() < 1e-6);
        }
    }
}
