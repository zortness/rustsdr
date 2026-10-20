use super::*;
use sdr_devices::{SyntheticSession, ToneConfig};
use sdr_dsp::zoom_spectrum::{SpectrumView, ZoomSpectrum};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::{self};
use std::time::{Duration, Instant};

fn run_live(
    settings: &mut Settings,
    control: &Mutex<Settings>,
    output: &Outputs,
    cancel: &AtomicBool,
    _volume: Arc<AtomicU32>,
    stop_scan: &AtomicU64,
) -> Result<(), String> {
    if settings.source == Source::Ic705 {
        #[cfg(feature = "ic705")]
        return super::ic705::run(settings, control, output, cancel, _volume);
        #[cfg(not(feature = "ic705"))]
        return Err("Built without IC-705 support".into());
    }
    if settings.source == Source::Qmx {
        #[cfg(feature = "qmx")]
        return super::qmx::run(settings, control, output, cancel, _volume);
        #[cfg(not(feature = "qmx"))]
        return Err("Built without QMX support".into());
    }
    use super::source::RtlSession;
    use crate::scanning::{ScanPhase, ScanStatus, Scanner};
    use sdr_dsp::Receiver;
    let clock = Instant::now();
    let mut scanner = settings.scan.clone().map(Scanner::new).transpose()?;
    if let Some(scan) = &scanner {
        tune_scan(settings, scan.current());
    }
    let mut advance = false;
    let mut offset_hz = settings.station_hz as f64 - settings.center_hz() as f64;
    let mut receiver = Receiver::new(offset_hz, settings.receiver).map_err(str::to_owned)?;
    let mut fft =
        ZoomSpectrum::new(SAMPLE_RATE as f64, settings.spectrum_view).map_err(str::to_owned)?;
    let mut spectrum_row = 0;
    let mut audio_samples = Vec::with_capacity(1024);
    let mut multiplex = Vec::with_capacity(1639);
    let mut rds = new_rds(settings);
    let mut cw = new_cw(settings);
    let capture_revision = settings.revision;
    let mut audio_index = 0_u64;
    let mut discontinuity = 0_u64;
    let mut expected = 0_u64;
    let mut drops = 0_u64;
    #[cfg(feature = "audio")]
    let (mut audio, audio_error) = match crate::audio::AudioOutput::open(_volume) {
        Ok(stream) => (Some(stream), None),
        Err(e) => (None, Some(e)),
    };
    #[cfg(not(feature = "audio"))]
    let audio_error = Some("Built without audio output support".to_owned());
    let mut file = if settings.source == Source::File {
        let config = settings
            .file
            .as_ref()
            .ok_or("Select an IQ file and enter its metadata")?;
        Some(sdr_devices::file::IqFile::open(
            &config.path,
            config.sample_rate,
            config.center_hz,
        )?)
    } else {
        None
    };
    let mut radio = if file.is_none() {
        Some(RtlSession::open_with(
            settings.rtl_backend,
            0,
            settings
                .tuner_center_hz()
                .ok_or("Invalid converted tuner frequency")?,
            SAMPLE_RATE,
        )?)
    } else {
        None
    };
    let mut samples = vec![sdr_core::Iq { i: 0.0, q: 0.0 }; 16_384];
    let mut next_block = Instant::now();
    if let Some(scan) = &mut scanner {
        scan.tuned(clock.elapsed());
    }
    let mut reported_scan = scanner
        .as_ref()
        .map_or(ScanStatus::default(), |scan| scan.status(clock.elapsed()));
    *output.lock().map_err(|_| "display lock poisoned")? = Update {
        rtl: radio.as_ref().map(RtlSession::info),
        revision: settings.revision,
        capture_revision: settings.revision,
        center_hz: settings.center_hz(),
        station_hz: settings.station_hz,
        mode: settings.receiver.mode,
        receiver: settings.receiver,
        scan: reported_scan,
        running: true,
        audio_error,
        total_samples: file.as_ref().map(|f| f.total_samples()),
        ..Update::default()
    };
    output.events.publish(EngineEvent::Applied {
        revision: settings.revision,
    });
    while !cancel.load(Ordering::Acquire) {
        // Keep a stop for an accepted configuration until that revision is applied.
        // Compare/exchange also avoids clearing a newer stop submitted concurrently.
        let stop_revision = stop_scan.load(Ordering::Acquire);
        let consumed_stop = stop_revision != 0
            && stop_revision <= settings.revision
            && stop_scan
                .compare_exchange(stop_revision, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
        if consumed_stop && stop_revision == settings.revision {
            scanner = None;
            advance = false;
            settings.scan = None;
            let mut slot = control.lock().map_err(|_| "control lock poisoned")?;
            if slot.revision == settings.revision {
                *slot = settings.clone();
            }
            reported_scan.phase = ScanPhase::Off;
            reported_scan.remaining_ms = 0;
            output.lock().map_err(|_| "display lock poisoned")?.scan = reported_scan;
        }
        let mut request = control.lock().map_err(|_| "control lock poisoned")?.clone();
        let external = request.revision != settings.revision;
        settings.spectrum_view = request.spectrum_view;
        if external || advance {
            #[cfg(feature = "trace")]
            let _span =
                tracing::info_span!("sdr.receiver.reconfigure", revision = request.revision)
                    .entered();
            if !request.running
                || request.source != settings.source
                || request.rtl_backend != settings.rtl_backend
                || request.file != settings.file
                || request.converter_offset_hz != settings.converter_offset_hz
            {
                break;
            }
            let scan_changed = external
                && match (&request.scan, &settings.scan) {
                    (Some(next), Some(current)) => !Arc::ptr_eq(next, current),
                    (None, None) => false,
                    _ => true,
                };
            if scan_changed {
                scanner = request.scan.clone().map(Scanner::new).transpose()?;
            } else if !external {
                request = settings.clone();
            }
            if let Some(scan) = &scanner {
                tune_scan(&mut request, scan.current());
            }
            let new_offset = request.station_hz as f64 - request.center_hz() as f64;
            let receiver_changed = new_offset != offset_hz
                || request.receiver != settings.receiver
                || request.center_hz() != settings.center_hz();
            #[cfg(feature = "audio")]
            if receiver_changed && let Some(stream) = &mut audio {
                stream.retune();
            }
            if request.center_hz() != settings.center_hz()
                && let Some(radio) = &mut radio
            {
                #[cfg(feature = "trace")]
                let _span = tracing::info_span!("sdr.device.retune").entered();
                radio.retune(
                    request
                        .tuner_center_hz()
                        .ok_or("Invalid converted tuner frequency")?,
                )?;
            }
            if receiver_changed {
                fft = ZoomSpectrum::new(SAMPLE_RATE as f64, request.spectrum_view)
                    .map_err(str::to_owned)?;
                receiver = Receiver::new(new_offset, request.receiver).map_err(str::to_owned)?;
                discontinuity += 1;
                audio_index = 0;
            }
            let reset_rds = receiver_changed || request.rds_enabled != settings.rds_enabled;
            let reset_cw = receiver_changed
                || request.cw_decoder != settings.cw_decoder
                || request.cw_reset != settings.cw_reset;
            if reset_cw {
                cw = new_cw(&request);
            }
            if reset_rds {
                rds = new_rds(&request);
            }
            offset_hz = new_offset;
            *settings = request;
            if external {
                output.events.publish(EngineEvent::Applied {
                    revision: settings.revision,
                });
            }
            if (advance || scan_changed || receiver_changed)
                && let Some(scan) = &mut scanner
            {
                scan.tuned(clock.elapsed());
            }
            advance = false;
            let mut state = output.lock().map_err(|_| "display lock poisoned")?;
            state.revision = settings.revision;
            state.center_hz = settings.center_hz();
            state.station_hz = settings.station_hz;
            state.mode = settings.receiver.mode;
            state.receiver = settings.receiver;
            reported_scan = scanner
                .as_ref()
                .map_or(ScanStatus::default(), |scan| scan.status(clock.elapsed()));
            state.scan = reported_scan;
            state.channel_power_dbfs = None;
            state.stereo_locked = false;
            if reset_rds {
                state.rds = None;
            }
            if reset_cw {
                state.cw = None;
            }
            state.frame = None;
        }
        let (sample_index, count) = if let Some(file) = &mut file {
            while Instant::now() < next_block {
                #[cfg(feature = "trace")]
                let _span = tracing::info_span!("sdr.file.pacing_wait").entered();
                if cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                thread::sleep(
                    next_block
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(5)),
                );
            }
            let count = {
                #[cfg(feature = "trace")]
                let _span = tracing::info_span!("sdr.file.read").entered();
                file.read(&mut samples)?
            };
            if count == 0 {
                #[cfg(feature = "audio")]
                if let Some(stream) = &audio {
                    stream.finish();
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while stream.queued_samples() > 0 && Instant::now() < deadline {
                        if cancel.load(Ordering::Acquire)
                            || control
                                .lock()
                                .map_err(|_| "control lock poisoned")?
                                .revision
                                != settings.revision
                        {
                            return Ok(());
                        }
                        if stream.counters.failed.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                let mut state = output.lock().map_err(|_| "display lock poisoned")?;
                state.running = false;
                state.eof = true;
                state.scan.phase = ScanPhase::Off;
                state.scan.remaining_ms = 0;
                output.events.publish(EngineEvent::EndOfFile {
                    revision: settings.revision,
                });
                state.samples = expected;
                return Ok(());
            }
            // Follow the sample clock, but avoid unlimited catch-up after suspension.
            if Instant::now().saturating_duration_since(next_block) > Duration::from_millis(100) {
                next_block = Instant::now();
            }
            next_block += Duration::from_secs_f64(count as f64 / SAMPLE_RATE as f64);
            (expected, count)
        } else {
            #[cfg(feature = "trace")]
            let _span = tracing::info_span!("sdr.iq.receive_wait_and_copy").entered();
            let block = radio
                .as_ref()
                .unwrap()
                .receive(Duration::from_millis(500))?;
            samples[..block.samples().len()].copy_from_slice(block.samples());
            (block.sample_index, block.samples().len())
        };
        if sample_index != expected {
            fft = ZoomSpectrum::new(SAMPLE_RATE as f64, settings.spectrum_view)
                .map_err(str::to_owned)?;
            if let Some(scan) = &mut scanner {
                scan.tuned(clock.elapsed());
            }
            discontinuity += 1;
            audio_index = 0;
            #[cfg(feature = "audio")]
            if let Some(stream) = &mut audio {
                stream.retune();
            }
            receiver = Receiver::new(offset_hz, settings.receiver).map_err(str::to_owned)?;
            rds = new_rds(settings);
            cw = new_cw(settings);
            output.lock().map_err(|_| "display lock poisoned")?.cw = None;
            output.lock().map_err(|_| "display lock poisoned")?.rds = None;
        }
        output.iq.publish(|| super::IqBlock {
            revision: settings.revision,
            capture_revision,
            discontinuity,
            input_start: sample_index,
            output_start: sample_index,
            sample_rate: SAMPLE_RATE,
            center_hz: settings.center_hz(),
            station_hz: settings.station_hz as u64,
            end_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
            samples: samples[..count].into(),
        });
        expected = sample_index + count as u64;
        audio_samples.clear();
        {
            #[cfg(feature = "trace")]
            let _span = tracing::info_span!("sdr.dsp.receiver", samples = count, mode = ?settings.receiver.mode).entered();
            if let Some(decoder) = &mut rds {
                multiplex.clear();
                receiver.process_with_multiplex(
                    &samples[..count],
                    &mut audio_samples,
                    &mut multiplex,
                );
                #[cfg(feature = "trace")]
                let _rds_span =
                    tracing::info_span!("sdr.dsp.rds", samples = multiplex.len()).entered();
                decoder.process(&multiplex);
            } else if let Some(decoder) = &mut cw {
                multiplex.clear();
                receiver.process_with_cw_power(
                    &samples[..count],
                    &mut audio_samples,
                    &mut multiplex,
                );
                #[cfg(feature = "trace")]
                let _cw_span =
                    tracing::info_span!("sdr.dsp.cw", samples = multiplex.len()).entered();
                decoder.process(&multiplex);
            } else {
                receiver.process(&samples[..count], &mut audio_samples);
            }
        }
        let power = receiver.channel_power_dbfs();
        let scan_status = if let Some(scan) = &mut scanner {
            let previous = scan.status(clock.elapsed());
            advance = scan.observe(clock.elapsed(), power);
            if advance {
                previous
            } else {
                scan.status(clock.elapsed())
            }
        } else {
            reported_scan
        };
        reported_scan = scan_status;
        if let Ok(mut state) = output.try_lock() {
            state.scan = scan_status;
            state.channel_power_dbfs = Some(power);
            state.receiver = settings.receiver;
            if let Some(decoder) = &cw {
                if state.cw.as_deref() != Some(decoder.status()) {
                    state.cw = Some(Arc::new(decoder.status().clone()));
                }
            } else {
                state.cw = None;
            }
            if let Some(decoder) = &rds {
                if state.rds.as_deref() != Some(decoder.status()) {
                    state.rds = Some(Arc::new(decoder.status().clone()));
                }
            } else {
                state.rds = None;
            }
        }
        output.publish_audio(
            settings.revision,
            capture_revision,
            sample_index,
            audio_index,
            discontinuity,
            &audio_samples,
        );
        audio_index += audio_samples.len() as u64;
        // Monitor-only scanning gate. Decoder audio subscriptions remain independent.
        if scanner.is_some() && scan_status.phase != ScanPhase::Holding {
            audio_samples.fill(sdr_dsp::StereoFrame::default());
        }
        #[cfg(feature = "audio")]
        if let Some(stream) = &mut audio {
            stream.push(&audio_samples);
        }
        if fft.view() != settings.spectrum_view {
            fft = ZoomSpectrum::new(SAMPLE_RATE as f64, settings.spectrum_view)
                .map_err(str::to_owned)?;
        }
        let analysis_rate_hz = fft.sample_rate();
        let analysis = {
            #[cfg(feature = "trace")]
            let _span =
                tracing::info_span!("sdr.dsp.zoom_fft", span_hz = settings.spectrum_view.span_hz)
                    .entered();
            fft.process(&samples[..count])
        };
        if let Some(dbfs) = analysis {
            spectrum_row += 1;
            if let Ok(mut state) = output.try_lock() {
                if state.frame.is_some() {
                    drops += 1;
                }
                state.frame = Some(SpectrumFrame {
                    end_time: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default(),
                    stream_seconds: audio_index as f64 / 48_000.0,
                    revision: settings.revision,
                    capture_revision,
                    sample_index,
                    center_hz: settings.center_hz(),
                    row: spectrum_row,
                    view: settings.spectrum_view,
                    analysis_rate_hz,
                    dbfs: dbfs.to_vec(),
                });
                state.samples = expected;
                state.display_drops = drops;
                state.rtl = radio.as_ref().map(RtlSession::info);
                state.iq_drops = radio.as_ref().map_or(0, |r| r.dropped_samples());
                state.retune_discarded = radio.as_ref().map_or(0, |r| r.retune_discarded_samples());
                if !audio_samples.is_empty() {
                    state.stereo_locked = receiver.stereo_locked();
                    state.audio_rms = (audio_samples
                        .iter()
                        .map(|v| (v.left * v.left + v.right * v.right) * 0.5)
                        .sum::<f32>()
                        / audio_samples.len() as f32)
                        .sqrt();
                }
                #[cfg(feature = "audio")]
                if let Some(stream) = &audio {
                    state.audio_underruns = stream.counters.underruns.load(Ordering::Relaxed);
                    state.audio_overruns = stream.counters.overruns.load(Ordering::Relaxed);
                    state.audio_queue_frames = Some(stream.queued_samples());
                    state.audio_output_rate_hz = Some(stream.sample_rate());
                    state.audio_output_device = Some(stream.device_label().to_owned());
                    if stream.counters.failed.load(Ordering::Relaxed) {
                        state.audio_error =
                            Some("Audio output disconnected or failed; stop/start to retry".into());
                    }
                }
            } else {
                drops += 1;
            }
        }
    }
    if let Some(radio) = &mut radio {
        radio.stop()?;
    }
    Ok(())
}

pub(super) fn run_worker(
    control: Arc<Mutex<Settings>>,
    output: Arc<Outputs>,
    cancel: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    stop_scan: Arc<AtomicU64>,
) -> Result<(), String> {
    let mut fft = ZoomSpectrum::new(SAMPLE_RATE as f64, SpectrumView::full(SAMPLE_RATE as f64))
        .map_err(str::to_owned)?;
    let mut spectrum_row = 0;
    let mut active: Option<SyntheticSession> = None;
    let mut revision = 0;
    let mut blocks = 0_u64;
    let mut drops = 0_u64;
    let mut next_block = Instant::now();
    while !cancel.load(Ordering::Acquire) {
        let mut request = control.lock().map_err(|_| "control lock poisoned")?.clone();
        if request.revision != revision {
            // Old session is shut down here, never on the rendering thread.
            drop(active.take());
            revision = request.revision;
            blocks = 0;
            spectrum_row = 0;
            fft = ZoomSpectrum::new(SAMPLE_RATE as f64, request.spectrum_view)
                .map_err(str::to_owned)?;
            if request.running && request.source != Source::Synthetic {
                if let Err(error) = run_live(
                    &mut request,
                    &control,
                    &output,
                    &cancel,
                    volume.clone(),
                    &stop_scan,
                ) {
                    let mut state = output.lock().map_err(|_| "display lock poisoned")?;
                    output.events.publish(EngineEvent::Fault {
                        revision: request.revision,
                        message: error.clone(),
                    });
                    *state = Update {
                        revision: request.revision,
                        center_hz: request.center_hz(),
                        station_hz: request.station_hz,
                        receiver: request.receiver,
                        mode: request.receiver.mode,
                        error: Some(error),
                        ..Update::default()
                    };
                }
                revision = request.revision;
                continue;
            }
            if request.running {
                let mut session = SyntheticSession::new(
                    ToneConfig {
                        sample_rate: SAMPLE_RATE,
                        center_hz: CENTER_HZ,
                        offset_hz: request.tone_hz,
                        amplitude: 0.5,
                    },
                    FFT_SIZE,
                    4,
                )
                .map_err(|e| e.to_string())?;
                session.start().map_err(|e| e.to_string())?;
                active = Some(session);
            }
            next_block = Instant::now();
            output.events.publish(EngineEvent::Applied { revision });
            let mut state = output.lock().map_err(|_| "display lock poisoned")?;
            *state = Update {
                revision,
                capture_revision: revision,
                center_hz: CENTER_HZ,
                running: active.is_some(),
                display_drops: drops,
                ..Update::default()
            };
        }
        if let Some(session) = &active {
            let block = session
                .receive(Duration::from_millis(100))
                .map_err(|e| e.to_string())?;
            output.iq.publish(|| super::IqBlock {
                revision,
                capture_revision: revision,
                discontinuity: 0,
                input_start: blocks * FFT_SIZE as u64,
                output_start: blocks * FFT_SIZE as u64,
                sample_rate: SAMPLE_RATE,
                center_hz: CENTER_HZ,
                station_hz: request.station_hz as u64,
                end_time: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default(),
                samples: block.samples().into(),
            });
            blocks += 1;
            if fft.view() != request.spectrum_view {
                fft = ZoomSpectrum::new(SAMPLE_RATE as f64, request.spectrum_view)
                    .map_err(str::to_owned)?;
            }
            let analysis_rate_hz = fft.sample_rate();
            let analysis = {
                #[cfg(feature = "trace")]
                let _span = tracing::info_span!(
                    "sdr.dsp.zoom_fft",
                    span_hz = request.spectrum_view.span_hz
                )
                .entered();
                fft.process(block.samples())
            };
            if let Some(dbfs) = analysis {
                spectrum_row += 1;
                if let Ok(mut state) = output.try_lock() {
                    if state.frame.is_some() {
                        drops += 1;
                    }
                    state.frame = Some(SpectrumFrame {
                        end_time: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default(),
                        stream_seconds: blocks as f64 * FFT_SIZE as f64 / SAMPLE_RATE as f64,
                        revision,
                        capture_revision: revision,
                        sample_index: (blocks - 1) * FFT_SIZE as u64,
                        center_hz: CENTER_HZ,
                        row: spectrum_row,
                        view: request.spectrum_view,
                        analysis_rate_hz,
                        dbfs: dbfs.to_vec(),
                    });
                    state.samples = blocks * FFT_SIZE as u64;
                    state.display_drops = drops;
                } else {
                    drops += 1;
                }
            }
            drop(block);
            next_block += Duration::from_secs_f64(FFT_SIZE as f64 / SAMPLE_RATE as f64);
            // Avoid an unlimited catch-up burst after the OS suspends the worker.
            if next_block < Instant::now() {
                next_block = Instant::now();
            }
            thread::sleep(next_block.saturating_duration_since(Instant::now()));
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}

fn new_rds(settings: &Settings) -> Option<sdr_dsp::rds::RdsDecoder> {
    (settings.rds_enabled && settings.receiver.mode.is_wfm()).then(sdr_dsp::rds::RdsDecoder::new)
}

fn new_cw(settings: &Settings) -> Option<sdr_dsp::cw::CwDecoder> {
    (settings.receiver.mode == sdr_dsp::Mode::Cw)
        .then_some(settings.cw_decoder)
        .flatten()
        .and_then(|config| sdr_dsp::cw::CwDecoder::new(config).ok())
}

fn tune_scan(settings: &mut Settings, channel: crate::scanning::ScanChannel) {
    let mut tuning = crate::tuning::Tuning {
        converter_offset_hz: settings.converter_offset_hz,
        station_hz: settings.station_hz,
        center_hz: settings.center_hz() as u32,
    };
    tuning.tune(channel.frequency_hz, 1, settings.source == Source::File);
    settings.station_hz = tuning.station_hz;
    if settings.source == Source::RtlSdr {
        settings.rtl_center_hz = Some(tuning.center_hz);
    }
    settings.receiver = channel.receiver;
}
