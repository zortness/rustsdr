#![cfg(feature = "sstv")]
use sdr_app::sstv::{self, End, Frame, Mode, SubmitError, Worker};
use std::time::{Duration, Instant};

fn gradient(mode: Mode) -> Vec<u8> {
    let (w, h) = mode.dimensions();
    (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| [(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 96])
        })
        .collect()
}
fn error(frame: &Frame, expected: &[u8]) -> f64 {
    frame
        .rgb
        .iter()
        .zip(expected)
        .map(|(a, b)| (*a as f64 - *b as f64).abs())
        .sum::<f64>()
        / expected.len() as f64
}
fn retry(mut action: impl FnMut() -> Result<(), SubmitError>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match action() {
            Ok(()) => return,
            Err(SubmitError::Full) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1))
            }
            error => panic!("submission failed: {error:?}"),
        }
    }
}
fn ended(worker: &Worker) -> sstv::Snapshot {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let state = worker.snapshot();
        if state.end.is_some() {
            return state;
        }
        assert!(Instant::now() < deadline, "worker failed to finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn four_modes_auto_detect_and_reconstruct_gradient() {
    for mode in Mode::ALL {
        let input = gradient(mode);
        let mut final_frame = None;
        sstv::decode(sstv::encode(mode, &input).unwrap(), None, |f| {
            final_frame = Some(f)
        })
        .unwrap();
        let frame = final_frame.expect("image");
        assert_eq!(frame.mode, mode);
        assert!(frame.complete(), "{mode:?}: {} rows", frame.rows);
        let mae = error(&frame, &input);
        assert!(mae < 8.0, "{mode:?} mean RGB error {mae}");
        eprintln!("{mode:?}: {} rows, RGB MAE {mae:.3}", frame.rows);
    }
}

#[test]
fn worker_survives_starvation_and_arbitrary_chunks_and_decodes_two_frames() {
    let mode = Mode::Robot36;
    let rgb = gradient(mode);
    let one: Vec<_> = sstv::encode(mode, &rgb).unwrap().collect();
    let worker = Worker::new(None).unwrap();
    let mut start = 0;
    for signal in [&one, &one] {
        for chunk in signal.chunks(997) {
            retry(|| worker.submit(start, chunk));
            start += chunk.len() as u64;
            if start == 997 {
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    }
    retry(|| worker.finish());
    let state = ended(&worker);
    assert_eq!(state.end, Some(End::Eof));
    assert_eq!(state.history.len(), 2);
    for frame in state.history {
        assert!(frame.complete());
        assert!(error(&frame, &rgb) < 8.0);
    }
}

#[test]
fn gap_preserves_partial_image_and_does_not_stitch() {
    let rgb = gradient(Mode::Robot36);
    let samples: Vec<_> = sstv::encode(Mode::Robot36, &rgb)
        .unwrap()
        .take(48_000 * 5)
        .collect();
    let worker = Worker::new(None).unwrap();
    let mut start = 0;
    for chunk in samples.chunks(4096) {
        retry(|| worker.submit(start, chunk));
        start += chunk.len() as u64;
    }
    retry(|| worker.submit(start + 1, &[0; 128]));
    let state = ended(&worker);
    assert_eq!(state.end, Some(End::Gap));
    let frame = state.history.back().expect("partial image retained");
    assert!(frame.rows > 0 && !frame.complete());
}

#[test]
fn idle_cancel_joins_promptly_and_rejects_invalid_blocks() {
    let worker = Worker::new(None).unwrap();
    assert_eq!(worker.submit(0, &[]), Err(SubmitError::InvalidBlock));
    assert_eq!(worker.submit(0, &[0; 4097]), Err(SubmitError::InvalidBlock));
    worker.cancel();
    assert_eq!(ended(&worker).end, Some(End::Cancelled));
    let start = Instant::now();
    drop(worker);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn missing_audio_times_out_without_an_image() {
    let worker = Worker::new(None).unwrap();
    let state = ended(&worker);
    assert_eq!(state.end, Some(End::Timeout));
    assert!(state.history.is_empty());
}

#[test]
fn active_cancel_preserves_progress_and_bounds_shutdown() {
    let rgb = gradient(Mode::Robot36);
    let samples: Vec<_> = sstv::encode(Mode::Robot36, &rgb)
        .unwrap()
        .take(48_000 * 5)
        .collect();
    let worker = Worker::new(None).unwrap();
    let mut start = 0;
    for chunk in samples.chunks(4096) {
        retry(|| worker.submit(start, chunk));
        start += chunk.len() as u64;
    }
    // The codec may be waiting for the next row before the 10 Hz preview tick.
    std::thread::sleep(Duration::from_millis(100));
    let before = Instant::now();
    worker.cancel();
    let state = ended(&worker);
    assert_eq!(state.end, Some(End::Cancelled));
    assert!(
        state
            .history
            .back()
            .is_some_and(|f| f.rows > 0 && !f.complete())
    );
    drop(worker);
    assert!(before.elapsed() < Duration::from_secs(1));
}

#[test]
fn slow_reader_keeps_only_four_latest_images() {
    let rgb = gradient(Mode::Robot36);
    let signal: Vec<_> = sstv::encode(Mode::Robot36, &rgb).unwrap().collect();
    let worker = Worker::new(None).unwrap();
    let mut start = 0;
    for _ in 0..6 {
        for chunk in signal.chunks(4096) {
            retry(|| worker.submit(start, chunk));
            start += chunk.len() as u64;
        }
    }
    retry(|| worker.finish());
    let state = ended(&worker);
    assert_eq!(
        state.history.iter().map(|f| f.id).collect::<Vec<_>>(),
        [3, 4, 5, 6]
    );
    assert!(state.history.iter().all(Frame::complete));
}

#[test]
fn silence_and_truncation_are_not_complete_frames() {
    let mut count = 0;
    sstv::decode(std::iter::repeat_n(0, 48_000 * 3), None, |_| count += 1).unwrap();
    assert_eq!(count, 0);
    let rgb = gradient(Mode::Martin1);
    let mut frame = None;
    sstv::decode(
        sstv::encode(Mode::Martin1, &rgb).unwrap().take(48_000 * 5),
        Some(Mode::Martin1),
        |f| frame = Some(f),
    )
    .unwrap();
    let frame = frame.unwrap();
    assert!(frame.rows > 0 && !frame.complete());
    assert!(sstv::encode(Mode::Robot36, &[0; 3]).is_err());
}

#[test]
#[ignore = "requires independently generated target/sstv-spike fixtures; see docs/sstv-spike.md"]
fn independent_pysstv_fixtures() {
    for mode in Mode::ALL {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/sstv-spike/{mode:?}-pysstv.wav"));
        let mut reader = hound::WavReader::open(path).expect("run fixture generator first");
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.spec().channels, 1);
        let audio = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut frame = None;
        sstv::decode(audio.into_iter(), None, |f| frame = Some(f)).unwrap();
        let frame = frame.expect("independent image");
        assert_eq!(frame.mode, mode);
        assert!(frame.complete(), "{mode:?}: {} rows", frame.rows);
        let mae = error(&frame, &gradient(mode));
        assert!(mae < 10.0, "{mode:?} independent RGB error {mae}");
        eprintln!("PySSTV {mode:?}: {} rows, RGB MAE {mae:.3}", frame.rows);
    }
}
