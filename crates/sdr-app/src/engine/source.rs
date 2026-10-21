#[cfg(feature = "rtl")]
pub(super) use sdr_devices::rtl::RtlSession;

// Compile-time unavailable adapter: no native loader dependency in file-only builds.
#[cfg(not(feature = "rtl"))]
pub(super) struct RtlSession;
#[cfg(not(feature = "rtl"))]
pub(super) struct UnavailableBlock {
    pub sample_index: u64,
}
#[cfg(not(feature = "rtl"))]
impl UnavailableBlock {
    pub fn samples(&self) -> &[sdr_core::Iq] {
        &[]
    }
}
#[cfg(not(feature = "rtl"))]
impl RtlSession {
    pub fn open_configured(
        _: sdr_devices::RtlBackend,
        _: u32,
        _: Option<&str>,
        _: u32,
        _: u32,
        _: i32,
    ) -> Result<Self, String> {
        Err("RTL support was not compiled in".into())
    }
    pub fn info(&self) -> sdr_devices::RtlInfo {
        unreachable!("Unavailable RTL adapter")
    }
    pub fn retune(&mut self, _: u32) -> Result<(), String> {
        Err("RTL support was not compiled in".into())
    }
    pub fn receive(&self, _: std::time::Duration) -> Result<UnavailableBlock, String> {
        Err("RTL support was not compiled in".into())
    }
    pub fn dropped_samples(&self) -> u64 {
        0
    }
    pub fn retune_discarded_samples(&self) -> u64 {
        0
    }
    pub fn stop(&mut self) -> Result<(), String> {
        Ok(())
    }
}
