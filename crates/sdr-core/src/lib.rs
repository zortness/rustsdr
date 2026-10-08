//! Engine-independent radio contracts.

/// Initial tested adapter policy: ordinary tuner operation, not HF direct sampling.
pub const RTL_MIN_HZ: u32 = 28_800_000;
pub const RX_MAX_HZ: u32 = 1_700_000_000;
pub const FILE_MIN_HZ: u32 = 1;

/// Normalized complex baseband sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iq {
    pub i: f32,
    pub q: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioError {
    InvalidConfiguration(&'static str),
    Unsupported(&'static str),
    AlreadyRunning,
    NotRunning,
    Timeout,
    WorkerFailed,
}

impl std::fmt::Display for RadioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RadioError {}

/// Positions are software sample indices, not hardware timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockMetadata {
    pub stream_id: u64,
    pub generation: u64,
    pub sample_index: u64,
    pub sample_rate: u32,
    pub center_hz: u64,
    pub discontinuity: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Running,
}

/// Receive-only command surface. Transmit commands are intentionally absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioCommand {
    Start,
    Stop,
    SetRxCenterHz(u64),
}
