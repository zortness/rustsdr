//! Deterministic scan policy. The native engine supplies the clock and channel power.
use sdr_dsp::ReceiverConfig;
use std::{sync::Arc, time::Duration};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanChannel {
    pub frequency_hz: u32,
    pub receiver: ReceiverConfig,
    pub memory_index: Option<usize>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ScanTargets {
    Memories(Arc<[ScanChannel]>),
    Range {
        start_hz: u32,
        stop_hz: u32,
        step_hz: u32,
        receiver: ReceiverConfig,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanParameters {
    pub settle_ms: u32,
    pub dwell_ms: u32,
    pub hold_ms: u32,
    pub threshold_db: f32,
}
impl Default for ScanParameters {
    fn default() -> Self {
        Self {
            settle_ms: 150,
            dwell_ms: 250,
            hold_ms: 5000,
            threshold_db: -55.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScanPlan {
    pub targets: ScanTargets,
    pub parameters: ScanParameters,
    pub start_index: usize,
}
impl ScanPlan {
    pub fn validate(&self, minimum_hz: u32, maximum_hz: u32) -> Result<(), String> {
        let p = self.parameters;
        if !(20..=5000).contains(&p.settle_ms)
            || !(25..=10_000).contains(&p.dwell_ms)
            || !(100..=300_000).contains(&p.hold_ms)
            || !p.threshold_db.is_finite()
            || !(-120.0..=0.0).contains(&p.threshold_db)
        {
            return Err("Scan needs settling 20–5000 ms, dwell 25–10000 ms, pause 0.1–300 s and threshold -120–0 dBFS".into());
        }
        let valid_channel = |hz, receiver: ReceiverConfig| -> Result<(), String> {
            if !(minimum_hz..=maximum_hz).contains(&hz) {
                return Err(format!(
                    "Scan frequency {hz} Hz is outside this source's usable range ({minimum_hz}–{maximum_hz} Hz)"
                ));
            }
            receiver.validate().map_err(str::to_owned)
        };
        match &self.targets {
            ScanTargets::Memories(channels) => {
                if channels.is_empty() || channels.len() > crate::memories::MAX_ENTRIES {
                    return Err("Select 1–100,000 memories for scanning".into());
                }
                for channel in channels.iter() {
                    valid_channel(channel.frequency_hz, channel.receiver)?;
                }
            }
            ScanTargets::Range {
                start_hz,
                stop_hz,
                step_hz,
                receiver,
            } => {
                if start_hz > stop_hz || *step_hz == 0 {
                    return Err(
                        "Scan start must not exceed stop; step must be at least 1 Hz".into(),
                    );
                }
                valid_channel(*start_hz, *receiver)?;
                valid_channel(*stop_hz, *receiver)?;
            }
        }
        if self.start_index >= self.len() {
            return Err("Invalid scan starting position".into());
        }
        Ok(())
    }
    pub fn len(&self) -> usize {
        match &self.targets {
            ScanTargets::Memories(channels) => channels.len(),
            ScanTargets::Range {
                start_hz,
                stop_hz,
                step_hz,
                ..
            } => {
                if *step_hz == 0 || start_hz > stop_hz {
                    0
                } else {
                    ((stop_hz - start_hz) / step_hz) as usize + 1
                }
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn channel(&self, index: usize) -> ScanChannel {
        match &self.targets {
            ScanTargets::Memories(channels) => channels[index],
            ScanTargets::Range {
                start_hz,
                step_hz,
                receiver,
                ..
            } => ScanChannel {
                frequency_hz: (*start_hz as u64 + index as u64 * *step_hz as u64) as u32,
                receiver: *receiver,
                memory_index: None,
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanPhase {
    #[default]
    Off,
    Settling,
    Listening,
    Holding,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScanStatus {
    pub phase: ScanPhase,
    pub index: usize,
    pub memory_index: Option<usize>,
    pub count: usize,
    pub visit: u64,
    pub remaining_ms: u32,
}
pub struct Scanner {
    plan: Arc<ScanPlan>,
    index: usize,
    phase: ScanPhase,
    deadline: Duration,
    visit: u64,
}
impl Scanner {
    pub fn new(plan: Arc<ScanPlan>) -> Result<Self, String> {
        plan.validate(sdr_core::FILE_MIN_HZ, sdr_core::RX_MAX_HZ)?;
        Ok(Self {
            index: plan.start_index,
            plan,
            phase: ScanPhase::Settling,
            deadline: Duration::MAX,
            visit: 1,
        })
    }
    pub fn current(&self) -> ScanChannel {
        self.plan.channel(self.index)
    }
    pub fn phase(&self) -> ScanPhase {
        self.phase
    }
    pub fn tuned(&mut self, now: Duration) {
        self.phase = ScanPhase::Settling;
        self.deadline = now + Duration::from_millis(self.plan.parameters.settle_ms as u64);
    }
    /// Returns true when the engine must tune the new current channel. Call tuned
    /// only after that tune completes; stale measurements must not advance a scan.
    pub fn observe(&mut self, now: Duration, dbfs: f32) -> bool {
        let p = self.plan.parameters;
        if self.phase == ScanPhase::Settling {
            if now < self.deadline {
                return false;
            }
            self.phase = ScanPhase::Listening;
            self.deadline = now + Duration::from_millis(p.dwell_ms as u64);
        }
        if self.phase == ScanPhase::Listening && dbfs.is_finite() && dbfs >= p.threshold_db {
            self.phase = ScanPhase::Holding;
            self.deadline = now + Duration::from_millis(p.hold_ms as u64);
        }
        if now >= self.deadline {
            self.index = (self.index + 1) % self.plan.len();
            self.visit += 1;
            self.phase = ScanPhase::Settling;
            self.deadline = Duration::MAX;
            return true;
        }
        false
    }
    pub fn status(&self, now: Duration) -> ScanStatus {
        ScanStatus {
            phase: self.phase,
            index: self.index,
            memory_index: self.current().memory_index,
            count: self.plan.len(),
            visit: self.visit,
            remaining_ms: self
                .deadline
                .saturating_sub(now)
                .as_millis()
                .min(u32::MAX as u128) as u32,
        }
    }
}
