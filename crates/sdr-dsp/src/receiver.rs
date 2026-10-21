use crate::fm::Decimator;
use sdr_core::Iq;
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    WfmMono,
    WfmStereo,
    Am,
    Nfm,
    Usb,
    Lsb,
    Cw,
}
impl Mode {
    pub const ALL: [Self; 7] = [
        Self::WfmMono,
        Self::WfmStereo,
        Self::Am,
        Self::Nfm,
        Self::Usb,
        Self::Lsb,
        Self::Cw,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::WfmMono => "WFM mono",
            Self::WfmStereo => "WFM stereo",
            Self::Am => "AM",
            Self::Nfm => "NFM",
            Self::Usb => "USB",
            Self::Lsb => "LSB",
            Self::Cw => "CW",
        }
    }
    pub fn is_wfm(self) -> bool {
        matches!(self, Self::WfmMono | Self::WfmStereo)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReceiverConfig {
    pub mode: Mode,
    pub bandwidth_hz: u32,
    pub deviation_hz: u32,
    pub deemphasis_us: u32,
    pub agc: bool,
    /// None disables squelch. Threshold is normalized channel power in dBFS.
    pub squelch_db: Option<f32>,
}
impl Default for ReceiverConfig {
    fn default() -> Self {
        Self::for_mode(Mode::WfmMono)
    }
}
impl ReceiverConfig {
    pub fn for_mode(mode: Mode) -> Self {
        Self {
            mode,
            bandwidth_hz: match mode {
                Mode::Am => 10_000,
                Mode::Nfm => 12_500,
                Mode::Usb | Mode::Lsb => 2700,
                Mode::Cw => 500,
                _ => 200_000,
            },
            deviation_hz: if mode == Mode::Nfm { 2500 } else { 75_000 },
            deemphasis_us: if mode.is_wfm() { 75 } else { 0 },
            agc: matches!(mode, Mode::Am | Mode::Usb | Mode::Lsb | Mode::Cw),
            squelch_db: None,
        }
    }
    pub fn validate(self) -> Result<(), &'static str> {
        let width_ok = match self.mode {
            Mode::WfmMono | Mode::WfmStereo => self.bandwidth_hz == 200_000,
            Mode::Am => [6000, 10_000, 12_000].contains(&self.bandwidth_hz),
            Mode::Nfm => {
                [12_500, 25_000].contains(&self.bandwidth_hz)
                    && [2500, 5000].contains(&self.deviation_hz)
            }
            Mode::Usb | Mode::Lsb => [1800, 2400, 2700, 4700].contains(&self.bandwidth_hz),
            Mode::Cw => [200, 500, 1000].contains(&self.bandwidth_hz),
        };
        if !width_ok
            || ![0, 50, 75, 300, 750].contains(&self.deemphasis_us)
            || self
                .squelch_db
                .is_some_and(|db| !db.is_finite() || !(-120.0..=0.0).contains(&db))
        {
            return Err("Unsupported receiver filter, deviation, de-emphasis, or squelch");
        }
        Ok(())
    }
    pub fn passband(self) -> (f64, f64) {
        match self.mode {
            Mode::Usb => (300.0, 300.0 + self.bandwidth_hz as f64),
            Mode::Lsb => (-300.0 - self.bandwidth_hz as f64, -300.0),
            _ => (
                -(self.bandwidth_hz as f64) / 2.0,
                self.bandwidth_hz as f64 / 2.0,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}
impl StereoFrame {
    pub fn channel(self, index: usize, channels: usize) -> f32 {
        if channels == 1 {
            (self.left + self.right) * 0.5
        } else {
            match index {
                0 => self.left,
                1 => self.right,
                _ => 0.0,
            }
        }
    }
}

/// 1.92 or 2.4 MS/s complex IQ to 48 kHz audio.
pub struct Receiver {
    config: ReceiverConfig,
    rf: Decimator,
    down: Decimator,
    channel: Decimator,
    cw_down: Decimator,
    cw_previous: Iq,
    cw_next: Iq,
    cw_fraction: u32,
    cw_phase: f64,
    phase: f64,
    step: f64,
    dc: f32,
    envelope: f32,
    previous: Option<Iq>,
    voice: Decimator,
    ssb_phase: f64,
    ssb_step: f64,
    deemp: f32,
    power: f32,
    hang: usize,
    gate: f32,
    stereo: StereoDecoder,
}
impl Receiver {
    pub fn new(offset_hz: f64, config: ReceiverConfig) -> Result<Self, &'static str> {
        Self::with_input_rate(2_400_000, offset_hz, config)
    }
    pub fn with_input_rate(
        input_rate: u32,
        offset_hz: f64,
        config: ReceiverConfig,
    ) -> Result<Self, &'static str> {
        config.validate()?;
        if ![1_920_000, 2_400_000].contains(&input_rate)
            || !offset_hz.is_finite()
            || offset_hz.abs() > input_rate as f64 / 2.0 - 200_000.0
        {
            return Err("Unsupported input rate or VFO outside capture passband");
        }
        let ssb_center = if config.mode == Mode::Usb {
            300.0 + config.bandwidth_hz as f64 / 2.0
        } else if config.mode == Mode::Lsb {
            -300.0 - config.bandwidth_hz as f64 / 2.0
        } else {
            0.0
        };
        Ok(Self {
            config,
            rf: Decimator::new(
                input_rate as f64,
                100_000.0,
                241,
                input_rate as usize / 240_000,
            ),
            down: Decimator::new(240_000.0, 20_000.0, 161, 5),
            channel: Decimator::new(
                if config.mode == Mode::Cw {
                    6000.0
                } else {
                    48_000.0
                },
                (config.bandwidth_hz as f64 / 2.0).min(20_000.0),
                257,
                1,
            ),
            cw_down: Decimator::new(48_000.0, 2000.0, 161, 8),
            cw_previous: Iq::default(),
            cw_next: Iq::default(),
            cw_fraction: 0,
            cw_phase: 0.0,
            phase: 0.0,
            step: -TAU * offset_hz / input_rate as f64,
            dc: 0.0,
            envelope: 0.01,
            previous: None,
            voice: Decimator::new(48_000.0, 3000.0, 161, 1),
            ssb_phase: 0.0,
            ssb_step: TAU * ssb_center / 48_000.0,
            deemp: 0.0,
            power: 0.0,
            hang: 0,
            gate: if config.squelch_db.is_none() {
                1.0
            } else {
                0.0
            },
            stereo: StereoDecoder::new(config),
        })
    }
    pub fn process(&mut self, input: &[Iq], output: &mut Vec<StereoFrame>) {
        self.process_tapped(input, output, None);
    }
    /// Optional 240 kHz WFM multiplex tap, before stereo/audio filtering and
    /// de-emphasis. Caller must clear storage between bounded input blocks.
    /// Non-WFM modes append no samples.
    pub fn process_with_multiplex(
        &mut self,
        input: &[Iq],
        output: &mut Vec<StereoFrame>,
        multiplex: &mut Vec<f32>,
    ) {
        self.process_tapped(
            input,
            output,
            self.config.mode.is_wfm().then_some(multiplex),
        );
    }
    /// Optional 48 kHz CW channel-power tap, before AGC and audio squelch.
    /// Non-CW modes append no samples. Clear the tap between input blocks.
    pub fn process_with_cw_power(
        &mut self,
        input: &[Iq],
        output: &mut Vec<StereoFrame>,
        power: &mut Vec<f32>,
    ) {
        self.process_tapped(
            input,
            output,
            (self.config.mode == Mode::Cw).then_some(power),
        );
    }
    fn process_tapped(
        &mut self,
        input: &[Iq],
        output: &mut Vec<StereoFrame>,
        mut tap: Option<&mut Vec<f32>>,
    ) {
        for sample in input {
            let (sin, cos) = self.phase.sin_cos();
            self.phase = (self.phase + self.step).rem_euclid(TAU);
            let shifted = Iq {
                i: sample.i * cos as f32 - sample.q * sin as f32,
                q: sample.i * sin as f32 + sample.q * cos as f32,
            };
            if let Some(v) = self.rf.push(shifted) {
                if self.config.mode.is_wfm() {
                    self.power += 0.002 * (v.i * v.i + v.q * v.q - self.power);
                    let multiplex = self.previous.map_or(0.0, |p| {
                        (v.q * p.i - v.i * p.q).atan2(v.i * p.i + v.q * p.q)
                            * (240_000.0 / (TAU as f32 * 75_000.0))
                    });
                    self.previous = Some(v);
                    if let Some(tap) = &mut tap {
                        tap.push(multiplex);
                    }
                    if let Some(frame) = self.stereo.push(multiplex) {
                        output.push(frame);
                    }
                    continue;
                }
                let Some(v) = self.down.push(v) else {
                    continue;
                };
                let (s, c) = self.ssb_phase.sin_cos();
                self.ssb_phase = (self.ssb_phase + self.ssb_step).rem_euclid(TAU);
                let v = Iq {
                    i: v.i * c as f32 + v.q * s as f32,
                    q: v.q * c as f32 - v.i * s as f32,
                };
                let v = if self.config.mode == Mode::Cw {
                    if let Some(narrow) = self.cw_down.push(v) {
                        self.cw_previous = self.cw_next;
                        self.cw_next = self.channel.push(narrow).unwrap();
                        self.cw_fraction = 0;
                    }
                    // Interpolate the band-limited complex envelope back to 48 kHz.
                    let f = self.cw_fraction as f32 / 8.0;
                    self.cw_fraction = (self.cw_fraction + 1).min(8);
                    let v = Iq {
                        i: self.cw_previous.i + f * (self.cw_next.i - self.cw_previous.i),
                        q: self.cw_previous.q + f * (self.cw_next.q - self.cw_previous.q),
                    };
                    if let Some(tap) = &mut tap {
                        tap.push(v.i * v.i + v.q * v.q);
                    }
                    v
                } else {
                    self.channel.push(v).unwrap()
                };
                self.power += 0.01 * (v.i * v.i + v.q * v.q - self.power);
                let value = match self.config.mode {
                    Mode::Cw => {
                        // VFO denotes RF carrier; the BFO creates a 600 Hz listening tone.
                        let (s, c) = self.cw_phase.sin_cos();
                        self.cw_phase = (self.cw_phase + TAU * 600.0 / 48_000.0).rem_euclid(TAU);
                        v.i * c as f32 - v.q * s as f32
                    }
                    Mode::Am => v.i.hypot(v.q),
                    Mode::Nfm => {
                        let d = self.previous.map_or(0.0, |p| {
                            (v.q * p.i - v.i * p.q).atan2(v.i * p.i + v.q * p.q)
                                * (48_000.0 / (TAU as f32 * self.config.deviation_hz as f32))
                        });
                        self.previous = Some(v);
                        let d = self.voice.push(Iq { i: d, q: 0.0 }).unwrap().i;
                        let alpha = if self.config.deemphasis_us == 0 {
                            1.0
                        } else {
                            (1.0 - (-1.0 / (48_000.0 * self.config.deemphasis_us as f64 * 1e-6))
                                .exp()) as f32
                        };
                        self.deemp += alpha * (d - self.deemp);
                        self.deemp
                    }
                    _ => v.i * c as f32 - v.q * s as f32,
                };
                self.dc += 0.001 * (value - self.dc);
                let mut audio = value - self.dc;
                self.envelope += (if audio.abs() > self.envelope {
                    0.01
                } else {
                    0.0001
                }) * (audio.abs() - self.envelope);
                if self.config.agc {
                    audio *= (0.15 / self.envelope.max(0.00001)).min(100.0);
                }
                if let Some(db) = self.config.squelch_db {
                    let threshold = 10_f32.powf(db / 10.0);
                    if self.power > threshold * if self.hang > 0 { 0.5 } else { 1.0 } {
                        self.hang = 4800;
                    } else {
                        self.hang = self.hang.saturating_sub(1);
                    }
                    self.gate += 0.01 * ((if self.hang > 0 { 1.0 } else { 0.0 }) - self.gate);
                    audio *= self.gate;
                }
                output.push(StereoFrame {
                    left: audio.clamp(-1.0, 1.0),
                    right: audio.clamp(-1.0, 1.0),
                });
            }
        }
    }
    pub fn stereo_locked(&self) -> bool {
        self.config.mode == Mode::WfmStereo && self.stereo.locked
    }
    /// Smoothed, filtered channel IQ power before demodulation, AGC and audio squelch.
    /// Normalized dBFS, not calibrated antenna power or an S-meter reading.
    pub fn channel_power_dbfs(&self) -> f32 {
        10.0 * self.power.max(1e-16).log10()
    }
}

struct StereoDecoder {
    filter: Decimator,
    stereo: bool,
    phase: f64,
    correction: f64,
    pilot_i: f64,
    pilot_q: f64,
    lock_count: usize,
    locked: bool,
    blend: f32,
    alpha: f32,
    deemp: StereoFrame,
    dc: StereoFrame,
}
impl StereoDecoder {
    fn new(config: ReceiverConfig) -> Self {
        Self {
            filter: Decimator::new(240_000.0, 15_000.0, 161, 5),
            stereo: config.mode == Mode::WfmStereo,
            phase: 0.0,
            correction: 0.0,
            pilot_i: 0.0,
            pilot_q: 0.0,
            lock_count: 0,
            locked: false,
            blend: 0.0,
            alpha: if config.deemphasis_us == 0 {
                1.0
            } else {
                (1.0 - (-1.0 / (48_000.0 * config.deemphasis_us as f64 * 1e-6)).exp()) as f32
            },
            deemp: StereoFrame::default(),
            dc: StereoFrame::default(),
        }
    }
    fn push(&mut self, multiplex: f32) -> Option<StereoFrame> {
        let (s, c) = self.phase.sin_cos();
        self.pilot_i += 0.005 * (multiplex as f64 * c - self.pilot_i);
        self.pilot_q += 0.005 * (-multiplex as f64 * s - self.pilot_q);
        let error = self.pilot_q.atan2(self.pilot_i);
        let pilot_amplitude = 2.0 * self.pilot_i.hypot(self.pilot_q);
        self.correction = (self.correction + 1e-8 * error)
            .clamp(-TAU * 100.0 / 240_000.0, TAU * 100.0 / 240_000.0);
        let difference = 2.0 * multiplex * (2.0 * self.phase).cos() as f32;
        self.phase = (self.phase + TAU * 19_000.0 / 240_000.0 + self.correction + 0.0002 * error)
            .rem_euclid(TAU);
        if pilot_amplitude > 0.025 && error.abs() < 0.3 {
            self.lock_count = (self.lock_count + 1).min(12_000);
        } else {
            self.lock_count = self.lock_count.saturating_sub(4);
        }
        if self.lock_count == 12_000 {
            self.locked = true;
        }
        if self.lock_count == 0 || pilot_amplitude < 0.015 {
            self.locked = false;
        }
        let v = self.filter.push(Iq {
            i: multiplex,
            q: difference,
        })?;
        self.blend += 0.001 * ((if self.stereo && self.locked { 1.0 } else { 0.0 }) - self.blend);
        let l = v.i + v.q * self.blend;
        let r = v.i - v.q * self.blend;
        self.deemp.left += self.alpha * (l - self.deemp.left);
        self.deemp.right += self.alpha * (r - self.deemp.right);
        self.dc.left += 0.001 * (self.deemp.left - self.dc.left);
        self.dc.right += 0.001 * (self.deemp.right - self.dc.right);
        Some(StereoFrame {
            left: (self.deemp.left - self.dc.left).clamp(-1.0, 1.0),
            right: (self.deemp.right - self.dc.right).clamp(-1.0, 1.0),
        })
    }
}

#[cfg(test)]
mod input_rate_tests {
    use super::*;
    #[test]
    fn both_capture_rates_produce_48khz_audio() {
        for rate in [1_920_000, 2_400_000] {
            let mut receiver =
                Receiver::with_input_rate(rate, -250_000.0, ReceiverConfig::default()).unwrap();
            let input = vec![Iq::default(); rate as usize / 100];
            let mut audio = Vec::new();
            receiver.process(&input, &mut audio);
            assert_eq!(audio.len(), 480, "rate {rate}");
        }
        assert!(Receiver::with_input_rate(2_000_000, 0.0, ReceiverConfig::default()).is_err());
        assert!(
            Receiver::with_input_rate(1_920_000, 800_000.0, ReceiverConfig::default()).is_err()
        );
    }
}
