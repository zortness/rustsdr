use super::{EventStream, outputs::Broadcast};
use sdr_core::Iq;
use sdr_dsp::channel::{Channel, ChannelSpec};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone)]
pub struct IqBlock {
    pub revision: u64,
    pub capture_revision: u64,
    pub discontinuity: u64,
    pub input_start: u64,
    pub output_start: u64,
    pub sample_rate: u32,
    pub center_hz: u64,
    pub station_hz: u64,
    /// Approximate host capture-end time, not a hardware timestamp.
    pub end_time: Duration,
    pub samples: Arc<[Iq]>,
}
#[derive(Clone, Copy, Debug)]
pub struct ChannelRequest {
    /// None follows the receiver VFO; Some selects an absolute RF frequency.
    pub frequency_hz: Option<u64>,
    pub sample_rate: u32,
    pub bandwidth_hz: u32,
}
#[derive(Clone)]
pub enum ChannelEvent {
    Samples(IqBlock),
    Unavailable { revision: u64, reason: &'static str },
}
pub struct ChannelStream {
    pub events: EventStream<ChannelEvent>,
    _lease: Arc<()>,
}
pub(super) fn spawn(
    source: &Broadcast<IqBlock>,
    request: ChannelRequest,
    stop: Arc<AtomicBool>,
) -> Result<(ChannelStream, thread::JoinHandle<()>), String> {
    Channel::new(
        2_400_000,
        ChannelSpec {
            offset_hz: 0.0,
            sample_rate: request.sample_rate,
            bandwidth_hz: request.bandwidth_hz,
        },
    )
    .map_err(str::to_owned)?;
    let input = source.subscribe();
    let events = EventStream::bounded(8);
    let output = events.clone();
    let lease = Arc::new(());
    let alive = Arc::downgrade(&lease);
    let worker = thread::Builder::new()
        .name("iq-channel".into())
        .spawn(move || {
            let mut identity = None;
            let mut next = 0;
            let mut index = 0;
            let mut generation = 0;
            let mut channel = None;
            let mut buffer = Vec::new();
            while !stop.load(Ordering::Acquire) && alive.strong_count() > 0 {
                let Some(block) = input.next() else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let frequency = request.frequency_hz.unwrap_or(block.station_hz);
                let key = (
                    block.revision,
                    block.capture_revision,
                    block.discontinuity,
                    block.center_hz,
                    frequency,
                );
                if identity != Some(key) || next != block.input_start {
                    identity = Some(key);
                    index = 0;
                    generation += 1;
                    channel = match Channel::new(
                        block.sample_rate,
                        ChannelSpec {
                            offset_hz: frequency as f64 - block.center_hz as f64,
                            sample_rate: request.sample_rate,
                            bandwidth_hz: request.bandwidth_hz,
                        },
                    ) {
                        Ok(c) => Some(c),
                        Err(reason) => {
                            output.publish(ChannelEvent::Unavailable {
                                revision: block.revision,
                                reason,
                            });
                            None
                        }
                    };
                }
                next = block.input_start + block.samples.len() as u64;
                if let Some(channel) = &mut channel {
                    buffer.clear();
                    channel.process(&block.samples, &mut buffer);
                    let len = buffer.len() as u64;
                    output.publish(ChannelEvent::Samples(IqBlock {
                        sample_rate: request.sample_rate,
                        center_hz: frequency,
                        output_start: index,
                        discontinuity: generation,
                        samples: buffer.as_slice().into(),
                        ..block
                    }));
                    index += len;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((
        ChannelStream {
            events,
            _lease: lease,
        },
        worker,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_samples_and_retunes_reset_channel_clock() {
        let source = Broadcast::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (stream, worker) = spawn(
            &source,
            ChannelRequest {
                frequency_hz: Some(100_000_000),
                sample_rate: 48_000,
                bandwidth_hz: 32_000,
            },
            stop.clone(),
        )
        .unwrap();
        let receive = || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(event) = stream.events.next() {
                    break event;
                }
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        };
        for (start, center, expected_generation) in [
            (0, 100_000_000, 1),
            (1000, 100_000_000, 1),
            (3000, 100_000_000, 2),
            (4000, 110_000_000, 3),
        ] {
            source.publish(|| IqBlock {
                revision: 1,
                capture_revision: 1,
                discontinuity: 0,
                input_start: start,
                output_start: start,
                sample_rate: 2_400_000,
                center_hz: center,
                station_hz: 100_000_000,
                end_time: Duration::ZERO,
                samples: vec![Iq::default(); 1000].into(),
            });
            match receive() {
                ChannelEvent::Samples(block) => {
                    assert_eq!(block.discontinuity, expected_generation);
                    assert_eq!(block.output_start, if start == 1000 { 20 } else { 0 });
                }
                ChannelEvent::Unavailable { .. } => assert_eq!(center, 110_000_000),
            }
        }
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
    }
}
