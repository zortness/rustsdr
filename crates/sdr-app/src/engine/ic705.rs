use super::*;
use cpal::traits::{DeviceTrait, StreamTrait};
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
    let config = settings.ic705.clone();
    // Validate endpoints before opening CAT or capture; no fallback to default playback.
    let mut monitor = crate::audio::AudioOutput::open_named(&config.output, volume)?;
    let device = crate::audio::select_device(&config.input, true)?;
    let format = device.default_input_config().map_err(|e| e.to_string())?;
    if format.sample_format() != cpal::SampleFormat::F32
        || format.sample_rate() != 48_000
        || !(1..=2).contains(&format.channels())
    {
        return Err(
            "IC-705 input must be configured as 48 kHz mono or stereo float32 in Windows".into(),
        );
    }
    let mut radio = sdr_devices::ic705::open(&config.port, config.baud, config.address)
        .map_err(|e| e.to_string())?;
    let mut radio_state = radio.state().map_err(|e| e.to_string())?;
    if radio_state.usb_if {
        return Err("Select USB AF output on the IC-705".into());
    }
    if radio_state.transmitting {
        return Err("Stop transmitting before starting reception".into());
    }
    let channels = usize::from(format.channels());
    let mut tone: Option<TestTone> = None;
    let mut tx_deadline: Option<Instant> = None;
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
            for pair in samples.chunks_exact(channels) {
                if !pair.iter().all(|v| v.is_finite()) {
                    callback_failed.store(true, Ordering::Relaxed);
                    return;
                }
                if producer
                    .try_push(StereoFrame {
                        left: pair[0],
                        right: pair[channels - 1],
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
    let mut input_watchdog = ReceiveWatchdog::new(Instant::now());
    let mut discard_until = Instant::now() + Duration::from_millis(100);
    *output.lock().map_err(|_| "display lock poisoned")? = Update::default();
    while !cancel.load(Ordering::Acquire) {
        if failed.load(Ordering::Relaxed) {
            return Err("IC-705 audio capture failed; restart reception".into());
        }
        let request = control.lock().map_err(|_| "control lock poisoned")?.clone();
        if !request.running
            || request.source != Source::Ic705
            || request.ic705.port != config.port
            || request.ic705.baud != config.baud
            || request.ic705.address != config.address
            || request.ic705.tx_output != config.tx_output
            || request.ic705.input != config.input
            || request.ic705.output != config.output
        {
            break;
        }
        if request.revision != settings.revision {
            *settings = request;
            if settings.ic705.command != command {
                command = settings.ic705.command.clone();
                if let Some((_, operation)) = &command {
                    match operation {
                        sdr_devices::ic705::Command::Tune {
                            frequency_hz,
                            mode,
                            data,
                        } => {
                            radio
                                .tune(*frequency_hz, *mode, *data)
                                .map_err(|e| e.to_string())?;
                        }
                        sdr_devices::ic705::Command::StopTx => {
                            tone = None;
                            radio.end_test().map_err(|e| e.to_string())?;
                            tx_deadline = None;
                        }
                        sdr_devices::ic705::Command::TestTone {
                            frequency_hz,
                            power_percent,
                        } => {
                            if tx_deadline.is_some() {
                                return Err("TX test already active".into());
                            }
                            let signal = TestTone::open(&config.tx_output)?;
                            radio
                                .begin_test(*frequency_hz, *power_percent)
                                .map_err(|e| e.to_string())?;
                            signal.start();
                            tone = Some(signal);
                            tx_deadline = Some(Instant::now() + Duration::from_secs(3));
                        }
                    }
                    radio_state = radio.state().map_err(|e| e.to_string())?;
                    monitor.retune();
                    fft_samples.clear();
                    consumer.clear();
                    discontinuity += 1;
                    discard_until = Instant::now() + Duration::from_millis(100);
                }
            }
            next_poll = Instant::now();
        }
        if tx_deadline.is_some_and(|deadline| Instant::now() >= deadline)
            || tone
                .as_ref()
                .is_some_and(|v| v.failed.load(Ordering::Acquire))
        {
            tone = None;
            radio.end_test().map_err(|e| e.to_string())?;
            tx_deadline = None;
            next_poll = Instant::now();
        }
        if Instant::now() >= next_poll {
            let before = (
                radio_state.frequency_hz,
                radio_state.mode,
                radio_state.data_mode,
                radio_state.transmitting,
            );
            radio_state = radio.state().map_err(|e| e.to_string())?;
            if radio_state.usb_if {
                return Err("IC-705 changed to IF output; restore AF and restart".into());
            }
            if before
                != (
                    radio_state.frequency_hz,
                    radio_state.mode,
                    radio_state.data_mode,
                    radio_state.transmitting,
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
            input_watchdog.received(Instant::now());
            if Instant::now() < discard_until {
                continue;
            }
            audio_samples.push(frame);
        }
        if input_watchdog.expired(Instant::now(), radio_state.transmitting) {
            return Err("IC-705 audio input stopped; stop/start reception to reconnect".into());
        }
        if monitor.counters.failed.load(Ordering::Relaxed) {
            return Err(
                "Monitor output disconnected; select an available output and restart".into(),
            );
        }
        if radio_state.transmitting {
            audio_samples.clear();
        }
        let start = samples;
        samples += audio_samples.len() as u64;
        monitor_samples.clear();
        let gain = 10.0_f32.powf(settings.ic705.audio_gain_db.min(60) as f32 / 20.0);
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
                    center_hz: radio_state.frequency_hz.into(),
                    row,
                    dbfs: fft.analyze(&fft_samples).map_err(str::to_owned)?.to_vec(),
                });
                fft_samples.clear();
            }
        }
        let mode = match radio_state.mode {
            0 => Mode::Lsb,
            1 => Mode::Usb,
            2 => Mode::Am,
            5 | 0x17 => Mode::Nfm,
            6 => Mode::WfmMono,
            _ => Mode::Cw,
        };
        if let Ok(mut state) = output.try_lock() {
            let changed = state.revision != settings.revision;
            state.revision = settings.revision;
            state.capture_revision = capture_revision;
            state.station_hz = radio_state.frequency_hz;
            state.center_hz = radio_state.frequency_hz.into();
            state.ic705 = Some(Arc::new(radio_state.clone()));
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
    drop(tone);
    radio.end_test().map_err(|e| e.to_string())?;
    drop(stream);
    Ok(())
}

/// USB receive audio may pause while the radio transmits. Start a fresh timeout
/// when CAT confirms RX, while leaving capture/serial errors fatal in either state.
struct ReceiveWatchdog {
    last_samples: Instant,
    transmitting: bool,
}
impl ReceiveWatchdog {
    fn new(now: Instant) -> Self {
        Self {
            last_samples: now,
            transmitting: false,
        }
    }
    fn received(&mut self, now: Instant) {
        self.last_samples = now;
    }
    fn expired(&mut self, now: Instant, transmitting: bool) -> bool {
        if self.transmitting && !transmitting {
            self.last_samples = now;
        }
        self.transmitting = transmitting;
        !transmitting && now.duration_since(self.last_samples) > Duration::from_secs(2)
    }
}

/// Tone is separately routed; never uses the system default playback endpoint.
struct TestTone {
    _stream: cpal::Stream,
    enabled: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}
impl TestTone {
    fn open(name: &str) -> Result<Self, String> {
        if name.is_empty() || !crate::audio::is_radio_output(name) {
            return Err("Select the radio's transmit audio endpoint".into());
        }
        let device = crate::audio::select_device(name, false)?;
        let format = device.default_output_config().map_err(|e| e.to_string())?;
        if format.sample_format() != cpal::SampleFormat::F32
            || !(1..=2).contains(&format.channels())
        {
            return Err("TX audio requires mono/stereo float32 output".into());
        }
        let rate = format.sample_rate() as usize;
        let channels = format.channels() as usize;
        if !(32_000..=192_000).contains(&rate) {
            return Err("Unsupported TX sample rate".into());
        }
        let enabled = Arc::new(AtomicBool::new(false));
        let active = enabled.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let error = failed.clone();
        let mut count = 0usize;
        let stream = device
            .build_output_stream(
                &format.into(),
                move |buffer: &mut [f32], _| {
                    for frame in buffer.chunks_mut(channels) {
                        let sample = if active.load(Ordering::Acquire) && count < rate * 3 {
                            let envelope = ((count as f32 / (rate as f32 * 0.01)).min(1.0))
                                .min(((rate * 3 - count) as f32 / (rate as f32 * 0.01)).min(1.0));
                            let value =
                                (count as f32 * std::f32::consts::TAU * 1000.0 / rate as f32).sin()
                                    * 0.02
                                    * envelope;
                            count += 1;
                            value
                        } else {
                            0.0
                        };
                        frame.fill(sample);
                    }
                },
                move |_| {
                    error.store(true, Ordering::Release);
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            enabled,
            failed,
        })
    }
    fn start(&self) {
        self.enabled.store(true, Ordering::Release);
    }
}
impl Drop for TestTone {
    fn drop(&mut self) {
        self.enabled.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_watchdog_allows_tx_silence_and_audio_recovery() {
        let start = Instant::now();
        let mut watchdog = ReceiveWatchdog::new(start);
        assert!(!watchdog.expired(start, false));
        assert!(!watchdog.expired(start + Duration::from_secs(1), true));
        assert!(!watchdog.expired(start + Duration::from_secs(4), true));
        // Returning to RX grants a full receive timeout, not the remaining TX time.
        assert!(!watchdog.expired(start + Duration::from_secs(5), false));
        assert!(!watchdog.expired(start + Duration::from_secs(6), false));
        watchdog.received(start + Duration::from_secs(6));
        assert!(!watchdog.expired(start + Duration::from_secs(7), false));
        assert!(watchdog.expired(start + Duration::from_secs(9), false));
    }

    #[test]
    fn receive_watchdog_still_detects_missing_audio_at_start_and_after_tx() {
        let start = Instant::now();
        let mut watchdog = ReceiveWatchdog::new(start);
        assert!(watchdog.expired(start + Duration::from_secs(3), false));
        assert!(!watchdog.expired(start + Duration::from_secs(4), true));
        assert!(!watchdog.expired(start + Duration::from_secs(7), false));
        assert!(watchdog.expired(start + Duration::from_secs(10), false));
    }
}
