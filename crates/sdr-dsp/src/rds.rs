//! Receive-only RDS/RBDS from 240 kHz FM multiplex, before audio filtering.
//! Strict CRC group acceptance; no speculative bit-error correction.
use crate::fm::Decimator;
use sdr_core::Iq;
use std::f64::consts::TAU;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RdsStatus {
    pub locked: bool,
    pub groups: u64,
    pub pi: Option<u16>,
    pub program_service: Option<String>,
    pub radio_text: Option<String>,
    /// Numeric code: RDS and North American RBDS use different label tables.
    pub program_type: Option<u8>,
    pub traffic_program: Option<bool>,
    pub traffic_announcement: Option<bool>,
}

/// Bounded streaming decoder. Independent timing hypotheses avoid depending on
/// stereo pilot lock. Complex differential detection removes carrier phase
/// ambiguity. All timing lanes must pass all four block checks before publication.
pub struct RdsDecoder {
    phase: f64,
    filter: Decimator,
    lanes: [Lane; 8],
    samples: u64,
    last_group: u64,
    status: RdsStatus,
    ps: TextAssembly<8>,
    rt: TextAssembly<64>,
    rt_version: Option<(bool, bool)>,
}

impl Default for RdsDecoder {
    fn default() -> Self {
        Self::new()
    }
}
impl RdsDecoder {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            filter: Decimator::new(240_000.0, 2400.0, 321, 10),
            lanes: std::array::from_fn(|i| Lane::new(i as f64 / 8.0)),
            samples: 0,
            last_group: 0,
            status: RdsStatus::default(),
            ps: TextAssembly::default(),
            rt: TextAssembly::default(),
            rt_version: None,
        }
    }
    pub fn status(&self) -> &RdsStatus {
        &self.status
    }
    pub fn process(&mut self, multiplex: &[f32]) {
        for &value in multiplex {
            self.samples += 1;
            let (s, c) = self.phase.sin_cos();
            self.phase = (self.phase + TAU * 57_000.0 / 240_000.0).rem_euclid(TAU);
            if let Some(baseband) = self.filter.push(Iq {
                i: value * c as f32,
                q: -value * s as f32,
            }) {
                for i in 0..self.lanes.len() {
                    if let Some(group) = self.lanes[i].push(baseband)
                        // The same over-the-air group appears in several lanes.
                        && (self.last_group == 0 || self.samples - self.last_group > 12_000)
                    {
                        self.last_group = self.samples;
                        self.accept(group);
                    }
                }
            }
            if self.status.locked && self.samples - self.last_group > 720_000 {
                // Do not retain text as current after signal loss. Assemblies
                // also clear so reacquisition cannot join pre/post-gap segments.
                let groups = self.status.groups;
                self.status = RdsStatus {
                    groups,
                    ..Default::default()
                };
                self.ps = TextAssembly::default();
                self.rt = TextAssembly::default();
                self.rt_version = None;
            }
        }
    }
    fn accept(&mut self, words: [u16; 4]) {
        let [pi, b, c, d] = words;
        if self.status.pi != Some(pi) {
            let groups = self.status.groups;
            self.status = RdsStatus {
                pi: Some(pi),
                groups,
                ..Default::default()
            };
            self.ps = TextAssembly::default();
            self.rt = TextAssembly::default();
            self.rt_version = None;
        }
        self.status.locked = true;
        self.status.groups += 1;
        self.status.program_type = Some(((b >> 5) & 31) as u8);
        self.status.traffic_program = Some(b & 0x400 != 0);
        match b >> 12 {
            0 => {
                self.status.traffic_announcement = Some(b & 0x10 != 0);
                if let Some(text) = self.ps.put((b as usize & 3) * 2, &d.to_be_bytes(), 8) {
                    self.status.program_service = Some(text);
                }
            }
            2 => {
                let version_b = b & 0x800 != 0;
                let ab = b & 0x10 != 0;
                if self.rt_version != Some((version_b, ab)) {
                    self.rt = TextAssembly::default();
                    self.status.radio_text = None;
                    self.rt_version = Some((version_b, ab));
                }
                let segment = b as usize & 15;
                let bytes = [(c >> 8) as u8, c as u8, (d >> 8) as u8, d as u8];
                let text = if version_b {
                    self.rt.put(segment * 2, &bytes[2..], 32)
                } else {
                    self.rt.put(segment * 4, &bytes, 64)
                };
                if let Some(text) = text {
                    self.status.radio_text = Some(text);
                }
            }
            _ => {}
        }
    }
}

struct TextAssembly<const N: usize> {
    bytes: [u8; N],
    seen: [bool; N],
}
impl<const N: usize> Default for TextAssembly<N> {
    fn default() -> Self {
        Self {
            bytes: [b' '; N],
            seen: [false; N],
        }
    }
}
impl<const N: usize> TextAssembly<N> {
    fn put(&mut self, offset: usize, bytes: &[u8], length: usize) -> Option<String> {
        // A changed segment starts a fresh assembly, avoiding old/new text joins
        // when broadcasters rotate PS or change RT without toggling A/B.
        if bytes
            .iter()
            .enumerate()
            .any(|(i, &v)| self.seen[offset + i] && self.bytes[offset + i] != v)
        {
            self.seen.fill(false);
        }
        self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.seen[offset..offset + bytes.len()].fill(true);
        let end = (0..length)
            .find(|&i| self.seen[i] && self.bytes[i] == 13)
            .unwrap_or(length);
        if self.seen[..end].iter().all(|v| *v) {
            Some(
                self.bytes[..end]
                    .iter()
                    .map(|&b| character(b))
                    .collect::<String>()
                    .trim_end()
                    .to_owned(),
            )
        } else {
            None
        }
    }
}
fn character(b: u8) -> char {
    // Basic RDS alphabet. Unsupported extended characters are explicit rather
    // than incorrectly interpreted as UTF-8 or ISO-8859-1.
    match b {
        0x20..=0x7e => match b {
            0x24 => '¤',
            0x5e => '―',
            0x60 => '‖',
            0x7e => '¯',
            _ => b as char,
        },
        0x0a | 0x0b => ' ',
        _ => '�',
    }
}

struct Lane {
    clock: f64,
    sum: Iq,
    previous: Iq,
    blocks: Blocks,
}
impl Lane {
    fn new(clock: f64) -> Self {
        Self {
            clock,
            sum: Iq::default(),
            previous: Iq::default(),
            blocks: Blocks::default(),
        }
    }
    fn push(&mut self, value: Iq) -> Option<[u16; 4]> {
        let sign = if self.clock < 0.5 { 1.0 } else { -1.0 };
        self.sum.i += sign * value.i;
        self.sum.q += sign * value.q;
        self.clock += 1187.5 / 24_000.0;
        if self.clock < 1.0 {
            return None;
        }
        self.clock -= 1.0;
        let bit = self.sum.i * self.previous.i + self.sum.q * self.previous.q < 0.0;
        self.previous = self.sum;
        self.sum = Iq::default();
        self.blocks.push(bit)
    }
}
#[derive(Default)]
struct Blocks {
    shift: u32,
    bits: u32,
    index: usize,
    remaining: u32,
    words: [u16; 4],
}
impl Blocks {
    fn push(&mut self, bit: bool) -> Option<[u16; 4]> {
        self.shift = ((self.shift << 1) | u32::from(bit)) & 0x3ff_ffff;
        self.bits = (self.bits + 1).min(26);
        if self.bits < 26 {
            return None;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining > 0 {
                return None;
            }
        }
        let offset = match self.index {
            0 => 0xfc,
            1 => 0x198,
            2 if self.words[1] & 0x800 != 0 => 0x350,
            2 => 0x168,
            _ => 0x1b4,
        };
        let mut remainder = self.shift;
        for bit in (10..26).rev() {
            if remainder & (1 << bit) != 0 {
                remainder ^= 0x5b9 << (bit - 10);
            }
        }
        if remainder != offset {
            self.index = 0;
            return None;
        }
        self.words[self.index] = (self.shift >> 10) as u16;
        self.index += 1;
        self.remaining = 26;
        if self.index == 4 {
            self.index = 0;
            // Version B repeats PI in C'; mismatches cannot form one service.
            if self.words[1] & 0x800 != 0 && self.words[2] != self.words[0] {
                return None;
            }
            return Some(self.words);
        }
        None
    }
}
