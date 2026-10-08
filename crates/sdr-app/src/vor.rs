//! Engine-owned experimental VOR service. No audio output or device ownership.
use crate::engine::{ChannelEvent, ChannelRequest, Client, EventStream};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Status {
    pub revision: u64,
    pub frequency_hz: u64,
    pub measurement: Option<sdr_dsp::vor::Measurement>,
    pub message: String,
}
#[derive(Clone)]
pub struct Handle {
    enabled: Arc<AtomicBool>,
    pub results: EventStream<Status>,
}
impl Default for Handle {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            results: EventStream::bounded(8),
        }
    }
}
impl Handle {
    pub fn enable(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }
}
pub(crate) struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(client: Client, handle: Handle) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("vor-decode".into())
            .spawn(move || {
                let mut channel = None;
                let mut identity = None;
                let mut next = 0;
                let mut decoder = sdr_dsp::vor::Decoder::default();
                let mut last = Instant::now();
                while !stopping.load(Ordering::Acquire) {
                    if !handle.enabled.load(Ordering::Acquire) {
                        channel = None;
                        identity = None;
                        decoder = Default::default();
                        thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    if channel.is_none() {
                        match client.subscribe_channel(ChannelRequest {
                            frequency_hz: None,
                            sample_rate: 48_000,
                            bandwidth_hz: 32_000,
                        }) {
                            Ok(stream) => {
                                channel = Some(stream);
                                last = Instant::now();
                            }
                            Err(e) => {
                                handle.results.publish(Status {
                                    revision: 0,
                                    frequency_hz: 0,
                                    measurement: None,
                                    message: e,
                                });
                                thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                        }
                    }
                    if let Some(event) = channel.as_ref().unwrap().events.next() {
                        last = Instant::now();
                        match event {
                            ChannelEvent::Unavailable { revision, reason } => {
                                identity = None;
                                decoder = Default::default();
                                handle.results.publish(Status {
                                    revision,
                                    frequency_hz: 0,
                                    measurement: None,
                                    message: reason.into(),
                                });
                            }
                            ChannelEvent::Samples(block) => {
                                let key = (
                                    block.revision,
                                    block.capture_revision,
                                    block.discontinuity,
                                    block.center_hz,
                                );
                                if identity != Some(key) || next != block.output_start {
                                    decoder = Default::default();
                                    identity = Some(key);
                                    handle.results.publish(Status {
                                        revision: block.revision,
                                        frequency_hz: block.center_hz,
                                        measurement: None,
                                        message: "Acquiring 30 Hz reference".into(),
                                    });
                                }
                                next = block.output_start + block.samples.len() as u64;
                                for measurement in decoder.process(&block.samples) {
                                    let message = if measurement.is_some() {
                                        "Experimental radial FROM station"
                                    } else {
                                        "No reliable VOR reference"
                                    }
                                    .into();
                                    handle.results.publish(Status {
                                        revision: block.revision,
                                        frequency_hz: block.center_hz,
                                        measurement,
                                        message,
                                    });
                                }
                            }
                        }
                    } else {
                        if last.elapsed() > Duration::from_secs(2) {
                            decoder = Default::default();
                            identity = None;
                            last = Instant::now();
                            handle.results.publish(Status {
                                revision: 0,
                                frequency_hz: 0,
                                measurement: None,
                                message: "Waiting for IQ samples".into(),
                            });
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
