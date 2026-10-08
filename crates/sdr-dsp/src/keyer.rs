//! Local practice timing only. No device, PTT, or transmitter access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyerMode {
    #[default]
    Straight,
    IambicA,
    IambicB,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyerConfig {
    pub mode: KeyerMode,
    pub wpm: f32,
    pub pitch_hz: f32,
}
impl Default for KeyerConfig {
    fn default() -> Self {
        Self {
            mode: KeyerMode::Straight,
            wpm: 20.0,
            pitch_hz: 600.0,
        }
    }
}
impl KeyerConfig {
    pub fn validate(self) -> Result<(), &'static str> {
        if !(5.0..=60.0).contains(&self.wpm) || !(200.0..=1200.0).contains(&self.pitch_hz) {
            Err("Practice speed must be 5–60 WPM and pitch 200–1200 Hz")
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Contacts {
    pub straight: bool,
    pub dit: bool,
    pub dah: bool,
}
pub struct Keyer {
    contacts: Contacts,
    mode: KeyerMode,
    dot: u32,
    remaining: u32,
    marking: bool,
    last: Option<bool>, // true = dah
    memory: [bool; 2],
    squeezed: bool,
}
impl Keyer {
    pub fn new(rate: u32, config: KeyerConfig) -> Result<Self, &'static str> {
        config.validate()?;
        if !(1000..=192_000).contains(&rate) {
            return Err("Invalid keyer sample rate");
        }
        Ok(Self {
            contacts: Contacts::default(),
            mode: config.mode,
            dot: (rate as f64 * 1.2 / config.wpm as f64).round() as u32,
            remaining: 0,
            marking: false,
            last: None,
            memory: [false; 2],
            squeezed: false,
        })
    }
    pub fn contacts(&mut self, contacts: Contacts) {
        let pressed = [
            contacts.dit && !self.contacts.dit,
            contacts.dah && !self.contacts.dah,
        ];
        for (index, down) in pressed.into_iter().enumerate() {
            if down && (!self.marking || self.last != Some(index == 1)) {
                self.memory[index] = true;
            }
        }
        self.contacts = contacts;
        if self.marking && contacts.dit && contacts.dah {
            self.squeezed = true;
        }
        if self.mode == KeyerMode::IambicA && self.squeezed && !contacts.dit && !contacts.dah {
            self.memory = [false; 2];
            self.squeezed = false;
        }
        if self.mode == KeyerMode::IambicB && self.marking && contacts.dit && contacts.dah {
            self.memory[usize::from(!self.last.unwrap_or(false))] = true;
        }
    }
    /// Cancel immediately, including any queued iambic-B tail.
    pub fn cancel(&mut self) {
        self.contacts = Contacts::default();
        self.memory = [false; 2];
        self.remaining = 0;
        self.marking = false;
        self.last = None;
        self.squeezed = false;
    }
    pub fn next_key(&mut self) -> bool {
        if self.mode == KeyerMode::Straight {
            return self.contacts.straight;
        }
        if self.remaining == 0 {
            if self.marking {
                self.marking = false;
                self.remaining = self.dot;
            } else {
                let dit = self.contacts.dit || self.memory[0];
                let dah = self.contacts.dah || self.memory[1];
                let next = match (dit, dah) {
                    (true, true) => Some(!self.last.unwrap_or(true)),
                    (true, false) => Some(false),
                    (false, true) => Some(true),
                    _ => None,
                };
                let Some(dah) = next else {
                    self.last = None;
                    return false;
                };
                self.memory[usize::from(dah)] = false;
                self.last = Some(dah);
                self.marking = true;
                self.remaining = self.dot * if dah { 3 } else { 1 };
                self.squeezed = self.contacts.dit && self.contacts.dah;
                if self.squeezed && self.mode == KeyerMode::IambicB {
                    self.memory[usize::from(!dah)] = true;
                }
            }
        }
        self.remaining -= 1;
        self.marking
    }
}

/// Click-reduced sidetone; key timing and practice decoding use the raw gate.
pub struct Sidetone {
    phase: f64,
    step: f64,
    envelope: f32,
    ramp: f32,
}
impl Sidetone {
    pub fn new(rate: u32, pitch_hz: f32) -> Self {
        Self {
            phase: 0.0,
            step: std::f64::consts::TAU * pitch_hz as f64 / rate as f64,
            envelope: 0.0,
            ramp: 1.0 / (rate as f32 * 0.004),
        }
    }
    pub fn sample(&mut self, down: bool) -> f32 {
        self.envelope = (self.envelope + if down { self.ramp } else { -self.ramp }).clamp(0.0, 1.0);
        let value =
            self.phase.sin() as f32 * (0.5 - 0.5 * (std::f32::consts::PI * self.envelope).cos());
        self.phase = (self.phase + self.step).rem_euclid(std::f64::consts::TAU);
        value * 0.5
    }
}
