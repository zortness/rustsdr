use super::*;
use cpal::traits::DeviceTrait;
use ringbuf::{HeapRb, traits::*};
use sdr_dsp::{Mode, ReceiverConfig, SpectrumAnalyzer, StereoFrame};
use std::time::{Duration, Instant};

pub(super) fn run(
    settings: &mut Settings,
    control: &Mutex<Settings>,
    output: &Outputs,
    cancel: &AtomicBool,
    volume: Arc<AtomicU32>,
) -> Result<(), String> {
    let config = settings.qmx.clone();
    // Validate endpoints before opening CAT or capture; no fallback to default playback.
    let mut monitor = crate::audio::AudioOutput::open_named(&config.output, volume)?;
    let device = crate::audio::select_device(&config.input, true)?;
    let format = device.default_input_config().map_err(|e| e.to_string())?;
    if format.sample_format() != cpal::SampleFormat::F32
        || format.sample_rate() != 48_000
        || format.channels() != 2
    {
        return Err("QMX input must be configured as 48 kHz stereo float32 in Windows".into());
    }
    let mut radio = sdr_devices::qmx::open(&config.port).map_err(|e| e.to_string())?;
    let capture_revision = settings.revision;
    // A new connection adopts the radio. Never replay a stale tuning command on reconnect.
    let mut command = config.command.clone();
    let (mut producer, mut consumer) = HeapRb::<StereoFrame>::new(24_000).split();
    let lost = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let callback_lost = lost.clone();
    let callback_failed = failed.clone();
    let stream_failed = failed.clone();
    let stream = crate::audio::RadioInput::open(
        &device,
        format,
        move |samples: &[f32]| {
            for pair in samples.chunks_exact(2) {
                if !pair.iter().all(|v| v.is_finite()) {
                    callback_failed.store(true, Ordering::Relaxed);
                    return;
                }
                if producer
                    .try_push(StereoFrame {
                        left: pair[0],
                        right: pair[1],
                    })
                    .is_err()
                {
                    callback_lost.fetch_add(1, Ordering::Relaxed);
                }
            }
        },
        move || {
            stream_failed.store(true, Ordering::Relaxed);
        },
    )?;
    let mut fft = SpectrumAnalyzer::new(FFT_SIZE).map_err(str::to_owned)?;
    let mut fft_samples = Vec::with_capacity(FFT_SIZE);
    let mut audio_samples = Vec::with_capacity(512);
    let mut monitor_samples = Vec::with_capacity(512);
    let mut samples = 0;
    let mut row = 0;
    let mut discontinuity = 0;
    let mut prior_lost = 0;
    let mut prior_capture_gaps = 0;
    let mut next_poll = Instant::now();
    let mut last_samples = Instant::now();
    let mut discard_until = Instant::now() + Duration::from_millis(100);
    *output.lock().map_err(|_| "display lock poisoned")? = Update::default();
    while !cancel.load(Ordering::Acquire) {
        if failed.load(Ordering::Relaxed) {
            return Err("QMX audio capture failed; restart reception".into());
        }
        let request = control.lock().map_err(|_| "control lock poisoned")?.clone();
        if !request.running
            || request.source != Source::Qmx
            || request.qmx.port != config.port
            || request.qmx.input != config.input
            || request.qmx.output != config.output
        {
            break;
        }
        if request.revision != settings.revision {
            *settings = request;
            if settings.qmx.command != command {
                command = settings.qmx.command.clone();
                if let Some((_, operation)) = &command {
                    radio.apply(operation).map_err(|e| e.to_string())?;
                    monitor.retune();
                    fft_samples.clear();
                    consumer.clear();
                    discontinuity += 1;
                    discard_until = Instant::now() + Duration::from_millis(100);
                }
            }
            next_poll = Instant::now();
        }
        if Instant::now() >= next_poll {
            let before = (
                radio.state.frequency_hz,
                radio.state.mode,
                radio.state.vfo_b,
            );
            radio.refresh().map_err(|e| e.to_string())?;
            if before
                != (
                    radio.state.frequency_hz,
                    radio.state.mode,
                    radio.state.vfo_b,
                )
            {
                monitor.retune();
                consumer.clear();
                fft_samples.clear();
                discontinuity += 1;
                discard_until = Instant::now() + Duration::from_millis(100);
            }
            next_poll = Instant::now() + Duration::from_millis(300);
        }
        let lost_now = lost.load(Ordering::Relaxed);
        let capture_gaps = stream.discontinuities();
        if lost_now != prior_lost || capture_gaps != prior_capture_gaps {
            prior_capture_gaps = capture_gaps;
            monitor.retune();
            fft_samples.clear();
            consumer.clear();
            discontinuity += 1;
            prior_lost = lost_now;
        }
        audio_samples.clear();
        while audio_samples.len() < 512 {
            let Some(frame) = consumer.try_pop() else {
                break;
            };
            last_samples = Instant::now();
            if Instant::now() < discard_until {
                continue;
            }
            audio_samples.push(frame);
        }
        if last_samples.elapsed() > Duration::from_secs(2) {
            return Err("QMX audio input stopped; stop/start reception to reconnect".into());
        }
        if monitor.counters.failed.load(Ordering::Relaxed) {
            return Err(
                "Monitor output disconnected; select an available output and restart".into(),
            );
        }
        let start = samples;
        samples += audio_samples.len() as u64;
        monitor_samples.clear();
        let gain = 10.0_f32.powf(settings.qmx.audio_gain_db.min(60) as f32 / 20.0);
        monitor_samples.extend(audio_samples.iter().map(|v| StereoFrame {
            left: (v.left * gain).clamp(-1.0, 1.0),
            right: (v.right * gain).clamp(-1.0, 1.0),
        }));
        monitor.push(&monitor_samples);
        if !audio_samples.is_empty() {
            output.publish_audio(
                settings.revision,
                capture_revision,
                start,
                start,
                discontinuity,
                &audio_samples,
            );
        }
        let mut frame = None;
        for sample in &audio_samples {
            fft_samples.push(sdr_core::Iq {
                i: (sample.left + sample.right) * 0.5,
                q: 0.0,
            });
            if fft_samples.len() == FFT_SIZE {
                row += 1;
                frame = Some(SpectrumFrame {
                    end_time: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default(),
                    stream_seconds: samples as f64 / 48_000.0,
                    view: sdr_dsp::zoom_spectrum::SpectrumView::full(SAMPLE_RATE as f64),
                    analysis_rate_hz: SAMPLE_RATE as f64,
                    revision: settings.revision,
                    capture_revision,
                    sample_index: samples,
                    center_hz: radio.state.frequency_hz.into(),
                    row,
                    dbfs: fft.analyze(&fft_samples).map_err(str::to_owned)?.to_vec(),
                });
                fft_samples.clear();
            }
        }
        let mode = match radio.state.mode {
            1 | 9 => Mode::Lsb,
            2 | 6 => Mode::Usb,
            _ => Mode::Cw,
        };
        if let Ok(mut state) = output.try_lock() {
            let changed = state.revision != settings.revision;
            state.revision = settings.revision;
            state.capture_revision = capture_revision;
            state.station_hz = radio.state.frequency_hz;
            state.center_hz = radio.state.frequency_hz.into();
            state.qmx = Some(Arc::new(radio.state.clone()));
            state.receiver = ReceiverConfig::for_mode(mode);
            state.mode = mode;
            state.running = true;
            state.samples = samples;
            state.iq_drops = lost_now;
            state.audio_underruns = monitor.counters.underruns.load(Ordering::Relaxed);
            state.audio_overruns = monitor.counters.overruns.load(Ordering::Relaxed);
            if !audio_samples.is_empty() {
                state.audio_rms = (audio_samples
                    .iter()
                    .map(|v| (v.left * v.left + v.right * v.right) * 0.5)
                    .sum::<f32>()
                    / audio_samples.len() as f32)
                    .sqrt();
            }
            if frame.is_some() {
                state.frame = frame;
            }
            if changed {
                output.events.publish(EngineEvent::Applied {
                    revision: settings.revision,
                });
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(stream);
    Ok(())
}
