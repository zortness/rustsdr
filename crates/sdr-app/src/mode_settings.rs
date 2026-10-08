//! Mode preference bank, independent of rendering and engine lifetime.
use sdr_dsp::{Mode, ReceiverConfig};

pub struct ModeSettings {
    saved: [(ReceiverConfig, u32); 7],
}
pub fn default_step(mode: Mode) -> u32 {
    match mode {
        Mode::Am => 1_000,
        Mode::Nfm => 12_500,
        Mode::Usb | Mode::Lsb | Mode::Cw => 100,
        _ => 100_000,
    }
}
impl Default for ModeSettings {
    fn default() -> Self {
        Self {
            saved: Mode::ALL.map(|mode| (ReceiverConfig::for_mode(mode), default_step(mode))),
        }
    }
}
impl ModeSettings {
    pub fn entries(&self) -> &[(ReceiverConfig, u32); 7] {
        &self.saved
    }
    pub fn restore(&mut self, config: ReceiverConfig, step: u32) {
        if config.validate().is_ok() && (1..=10_000_000).contains(&step) {
            let index = Mode::ALL.iter().position(|m| *m == config.mode).unwrap();
            self.saved[index] = (config, step);
        }
    }
    pub fn switch(
        &mut self,
        current: ReceiverConfig,
        step: u32,
        next: Mode,
    ) -> (ReceiverConfig, u32) {
        let index = |mode| {
            Mode::ALL
                .iter()
                .position(|&m| m == mode)
                .expect("known mode")
        };
        self.saved[index(current.mode)] = (current, step);
        self.saved[index(next)]
    }
}
