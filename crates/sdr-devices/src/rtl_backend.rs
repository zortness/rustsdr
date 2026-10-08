/// Backend selection is independent of the physical RTL device and RF settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtlBackend {
    Librtlsdr,
    Rust,
}
impl Default for RtlBackend {
    fn default() -> Self {
        if cfg!(feature = "rtl-rust") {
            Self::Rust
        } else {
            Self::Librtlsdr
        }
    }
}
impl RtlBackend {
    pub fn key(self) -> &'static str {
        match self {
            Self::Librtlsdr => "librtlsdr",
            Self::Rust => "rs-rtl",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Librtlsdr => "RTL-SDR - librtlsdr",
            Self::Rust => "RTL-SDR - Rust",
        }
    }
    pub fn available(self) -> bool {
        match self {
            Self::Librtlsdr => cfg!(feature = "rtl"),
            Self::Rust => cfg!(feature = "rtl-rust"),
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "librtlsdr" => Some(Self::Librtlsdr),
            "rs-rtl" => Some(Self::Rust),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct RtlInfo {
    pub backend: RtlBackend,
    /// Driver-reported configuration, not a calibrated RF measurement.
    pub center_hz: u32,
    pub sample_rate: u32,
    pub gain_tenth_db: Option<i32>,
}
