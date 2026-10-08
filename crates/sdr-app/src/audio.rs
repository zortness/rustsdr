use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};
use sdr_dsp::StereoFrame;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

#[path = "radio_input.rs"]
mod radio_input;
pub use radio_input::RadioInput;

#[derive(Default)]
pub struct AudioCounters {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub failed: AtomicBool,
    draining: AtomicBool,
    epoch: AtomicU64,
}

#[derive(Clone, Copy)]
struct QueuedSample {
    value: StereoFrame,
    epoch: u64,
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    producer: HeapProd<QueuedSample>,
    pub counters: Arc<AudioCounters>,
    rate: u32,
    previous: Option<StereoFrame>,
    position: f64,
}

impl AudioOutput {
    /// Invalidate old-station audio without rebuilding the output device.
    pub fn retune(&mut self) {
        self.counters.epoch.fetch_add(1, Ordering::AcqRel);
        self.previous = None;
        self.position = 0.0;
    }
    /// Permit short final recordings to play without waiting for startup prefill.
    pub fn finish(&self) {
        self.counters.draining.store(true, Ordering::Release);
    }

    pub fn queued_samples(&self) -> usize {
        self.producer.occupied_len()
    }

    pub fn open(volume: Arc<AtomicU32>) -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No default audio output device")?;
        Self::open_device(device, volume)
    }
    pub fn open_named(name: &str, volume: Arc<AtomicU32>) -> Result<Self, String> {
        let device = select_device(name, false)?;
        Self::open_device(device, volume)
    }
    fn open_device(device: cpal::Device, volume: Arc<AtomicU32>) -> Result<Self, String> {
        if is_radio_output(&device_label(&device)?) {
            return Err(
                "Radio audio cannot be used as a monitor output; select speakers/headphones".into(),
            );
        }
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let rate = config.sample_rate();
        if !(32_000..=192_000).contains(&rate) {
            return Err(format!(
                "Unsupported output rate {rate}; select a 44.1 or 48 kHz output device"
            ));
        }
        let (producer, consumer) = HeapRb::<QueuedSample>::new(rate as usize / 2).split();
        let counters = Arc::new(AudioCounters::default());
        let stream_config: cpal::StreamConfig = config.clone().into();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                stream::<f32>(&device, &stream_config, consumer, counters.clone(), volume)
            }
            cpal::SampleFormat::I16 => {
                stream::<i16>(&device, &stream_config, consumer, counters.clone(), volume)
            }
            cpal::SampleFormat::U16 => {
                stream::<u16>(&device, &stream_config, consumer, counters.clone(), volume)
            }
            other => return Err(format!("Unsupported output sample format {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            producer,
            counters,
            rate,
            previous: None,
            position: 0.0,
        })
    }

    /// Producer-side linear conversion from band-limited 48 kHz audio to the
    /// output device's rate. Slow occupancy feedback compensates clock drift.
    pub fn push(&mut self, samples: &[StereoFrame]) {
        #[cfg(feature = "trace")]
        let _span =
            tracing::info_span!("sdr.audio.resample_and_queue", frames = samples.len()).entered();
        let target = self.rate as f64 * 0.04;
        let epoch = self.counters.epoch.load(Ordering::Acquire);
        let error = (self.producer.occupied_len() as f64 - target) / target;
        let step = 48_000.0 / self.rate as f64 * (1.0 + (error * 0.0005).clamp(-0.001, 0.001));
        for &sample in samples {
            if let Some(previous) = self.previous {
                while self.position < 1.0 {
                    let value = StereoFrame {
                        left: previous.left + (sample.left - previous.left) * self.position as f32,
                        right: previous.right
                            + (sample.right - previous.right) * self.position as f32,
                    };
                    if self
                        .producer
                        .try_push(QueuedSample { value, epoch })
                        .is_err()
                    {
                        self.counters.overruns.fetch_add(1, Ordering::Relaxed);
                    }
                    self.position += step;
                }
                self.position -= 1.0;
            }
            self.previous = Some(sample);
        }
    }
}

pub fn device_label(device: &cpal::Device) -> Result<String, String> {
    let description = device.description().map_err(|e| e.to_string())?;
    Ok(format!(
        "{} ({})",
        description.name(),
        description.driver().unwrap_or("unknown")
    ))
}
pub fn device_names(input: bool) -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = if input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| e.to_string())?;
    devices.map(|device| device_label(&device)).collect()
}
pub fn select_device(name: &str, input: bool) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    let devices = if input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| e.to_string())?;
    let mut matches = devices.filter(|d| device_label(d).is_ok_and(|label| label == name));
    let device = matches
        .next()
        .ok_or("Selected audio endpoint is unavailable")?;
    if matches.next().is_some() {
        return Err("Ambiguous endpoint name; rename devices in Windows".into());
    }
    Ok(device)
}

fn stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: HeapCons<QueuedSample>,
    counters: Arc<AudioCounters>,
    volume: Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let prefill = (config.sample_rate as f64 * 0.04) as usize;
    let mut primed = false;
    let mut epoch = 0;
    let mut ramp = 0.0_f32;
    let errors = counters.clone();
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
                let latest_epoch = counters.epoch.load(Ordering::Acquire);
                if latest_epoch != epoch {
                    epoch = latest_epoch;
                    primed = false;
                    ramp = 0.0;
                }
                // The queue is bounded; discard only obsolete audio. New-station
                // samples remain queued for a short prefill, even if already sent.
                while consumer.first().is_some_and(|sample| sample.epoch != epoch) {
                    consumer.try_pop();
                }
                let draining = counters.draining.load(Ordering::Acquire);
                if !primed && (consumer.occupied_len() >= prefill || draining) {
                    primed = true;
                }
                let gain = f32::from_bits(volume.load(Ordering::Relaxed)).clamp(0.0, 1.0);
                let mut missing = 0;
                for frame in output.chunks_mut(channels) {
                    let value = if primed {
                        consumer
                            .try_pop()
                            .map(|sample| sample.value)
                            .unwrap_or_else(|| {
                                if !draining {
                                    missing += 1;
                                }
                                StereoFrame::default()
                            })
                    } else {
                        StereoFrame::default()
                    };
                    if primed {
                        ramp = (ramp + 1.0 / 240.0).min(1.0);
                    }
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        *sample = T::from_sample(
                            (value.channel(channel, channels) * gain * ramp).clamp(-1.0, 1.0),
                        );
                    }
                }
                if missing > 0 {
                    counters.underruns.fetch_add(missing, Ordering::Relaxed);
                    primed = false;
                }
            },
            move |_| {
                errors.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// Known radio codecs are reserved for explicit transmit paths.
pub fn is_radio_output(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["qmx", "ic-705", "usb audio codec"]
        .iter()
        .any(|v| name.contains(v))
}

#[cfg(test)]
mod routing_tests {
    #[test]
    fn radio_codecs_cannot_be_monitor_outputs() {
        for name in ["QMX", "Speakers (USB Audio CODEC )", "IC-705"] {
            assert!(super::is_radio_output(name));
        }
        assert!(!super::is_radio_output("Headphones (ROG CETRA)"));
    }
}
