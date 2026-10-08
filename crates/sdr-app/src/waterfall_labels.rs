//! Decodes bind to the original waterfall row, not the frame they arrive on.
use crate::digital::Batch;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub struct Stamp {
    pub seconds: f64,
    pub revision: u64,
    pub capture: u64,
}
pub struct Label {
    pub call: String,
    pub rf_hz: f64,
    pub audio_hz: f64,
    pub snr: f32,
    row: u64,
}
pub struct History {
    capacity: usize,
    serial: u64,
    rows: VecDeque<(u64, Option<Stamp>)>,
    labels: Vec<Label>,
}
impl History {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            serial: 0,
            rows: VecDeque::new(),
            labels: Vec::new(),
        }
    }
    pub fn clear(&mut self) {
        self.rows.clear();
        self.labels.clear();
        self.serial = 0;
    }
    pub fn push(&mut self, stamp: Option<Stamp>) {
        self.serial += 1;
        self.rows.push_back((self.serial, stamp));
        if self.rows.len() > self.capacity {
            self.rows.pop_front();
        }
        self.labels
            .retain(|l| self.serial - l.row < self.capacity as u64);
    }
    pub fn add(&mut self, batch: &Batch) {
        use crate::digital::Protocol;
        if batch.imported {
            return;
        }
        let (Some(protocol), Ok(messages)) = (batch.config.protocol, &batch.result) else {
            return;
        };
        // End of the active waveform, relative to nominal slot start; DT is
        // the decoder's time displacement. FT4 excludes its short edge ramps.
        let duration = match protocol {
            Protocol::Ft8 => 79.0 * 0.160,
            Protocol::Ft4 => 103.0 * 0.048,
        };
        for m in messages {
            let Some(call) = &m.sender else {
                continue;
            };
            let time = batch.slot_ms as f64 / 1000.0 + 0.5 + duration + m.dt as f64;
            let nearest = self
                .rows
                .iter()
                .filter_map(|(id, stamp)| {
                    let s = stamp.as_ref()?;
                    (s.revision == batch.config.revision && s.capture == batch.capture_revision)
                        .then_some((*id, (s.seconds - time).abs()))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((row, error)) = nearest else {
                continue;
            };
            if error > 0.75
                || self.labels.iter().any(|l| {
                    l.row == row && l.call == *call && (l.audio_hz - m.audio_hz as f64).abs() < 3.0
                })
            {
                continue;
            }
            self.labels.push(Label {
                call: call.clone(),
                rf_hz: batch.config.dial_hz as f64 + m.audio_hz as f64,
                audio_hz: m.audio_hz as f64,
                snr: m.snr,
                row,
            });
        }
        // Bound memory even with a malfunctioning or repeatedly replayed caller.
        if self.labels.len() > 1024 {
            self.labels.drain(..self.labels.len() - 1024);
        }
    }
    pub fn visible(&self) -> impl Iterator<Item = (&Label, f32)> {
        self.labels
            .iter()
            .map(|label| (label, (self.serial - label.row) as f32))
    }
}
