use super::Update;
use sdr_dsp::StereoFrame;
use std::{
    collections::VecDeque,
    ops::Deref,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

/// Bounded, ordered output. Overflow drops the incoming item, preserving queued
/// order. Contention also drops rather than blocking acquisition. Loss is explicit.
pub struct EventStream<T>(Arc<Queue<T>>);
struct Queue<T> {
    items: Mutex<VecDeque<T>>,
    capacity: usize,
    dropped: AtomicU64,
}
impl<T> Clone for EventStream<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> EventStream<T> {
    pub fn bounded(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self(Arc::new(Queue {
            items: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            dropped: AtomicU64::new(0),
        }))
    }
    pub fn publish(&self, item: T) {
        if let Ok(mut items) = self.0.items.try_lock()
            && items.len() < self.0.capacity
        {
            items.push_back(item);
            return;
        }
        self.0.dropped.fetch_add(1, Ordering::Relaxed);
    }
    pub fn next(&self) -> Option<T> {
        self.0.items.try_lock().ok()?.pop_front()
    }
    pub fn dropped(&self) -> u64 {
        self.0.dropped.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Applied { revision: u64 },
    EndOfFile { revision: u64 },
    Fault { revision: u64, message: String },
}

/// Receiver audio, before monitor gain/device clock correction. Still includes
/// mode AGC, squelch and de-emphasis; it is NOT raw FM multiplex or channel IQ.
#[derive(Clone)]
pub struct AudioBlock {
    /// Host time at publication of the end of this block. Approximate capture
    /// time; device buffering contributes to the reported digital-mode DT.
    pub end_time: std::time::Duration,
    pub revision: u64,
    pub capture_revision: u64,
    pub input_start: u64,
    pub output_start: u64,
    pub discontinuity: u64,
    pub sample_rate: u32,
    pub frames: Arc<[StereoFrame]>,
}

pub(super) struct Outputs {
    state: Mutex<Update>,
    pub events: EventStream<EngineEvent>,
    audio: Broadcast<AudioBlock>,
    pub iq: Broadcast<super::IqBlock>,
}
impl Default for Outputs {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            events: EventStream::bounded(64),
            audio: Broadcast::default(),
            iq: Broadcast::default(),
        }
    }
}
// State and its replaceable spectrum share a lock for a coherent revision snapshot.
// Ordered events and optional sample-timed audio never occupy this display slot.
impl Deref for Outputs {
    type Target = Mutex<Update>;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
impl Outputs {
    pub fn subscribe_audio(&self) -> EventStream<AudioBlock> {
        self.audio.subscribe()
    }
    pub fn publish_audio(
        &self,
        revision: u64,
        capture_revision: u64,
        input_start: u64,
        output_start: u64,
        discontinuity: u64,
        frames: &[StereoFrame],
    ) {
        self.audio.publish(|| AudioBlock {
            end_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
            revision,
            capture_revision,
            input_start,
            output_start,
            discontinuity,
            sample_rate: 48_000,
            frames: frames.into(),
        });
    }
}

/// Independent bounded readers; immutable sample allocations are shared.
pub(super) struct Broadcast<T> {
    readers: Mutex<Vec<Weak<Queue<T>>>>,
}
impl<T> Default for Broadcast<T> {
    fn default() -> Self {
        Self {
            readers: Mutex::new(Vec::new()),
        }
    }
}
impl<T: Clone> Broadcast<T> {
    pub fn subscribe(&self) -> EventStream<T> {
        let stream = EventStream::bounded(8);
        let mut readers = self.readers.lock().expect("subscription lock");
        readers.retain(|r| r.strong_count() > 0);
        readers.push(Arc::downgrade(&stream.0));
        stream
    }
    pub fn publish(&self, make: impl FnOnce() -> T) {
        if let Ok(mut readers) = self.readers.try_lock() {
            readers.retain(|r| r.strong_count() > 0);
            if readers.is_empty() {
                return;
            }
            let item = make();
            for reader in readers.iter().filter_map(Weak::upgrade) {
                EventStream(reader).publish(item.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_audio_consumers_and_slow_reader_loss() {
        let outputs = Outputs::default();
        let slow = outputs.subscribe_audio();
        let fast = outputs.subscribe_audio();
        for index in 0..12 {
            outputs.publish_audio(1, 1, index, index, 0, &[StereoFrame::default()]);
            assert_eq!(fast.next().unwrap().output_start, index);
        }
        assert_eq!(fast.dropped(), 0);
        assert_eq!(slow.dropped(), 4);
        assert_eq!(slow.next().unwrap().output_start, 0);
    }
}
