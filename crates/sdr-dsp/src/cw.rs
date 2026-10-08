//! Streaming, single-channel Morse decoding from 48 kHz pre-AGC channel power.
//! Timing estimates are heuristic, not error correction. No language guessing.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CwConfig {
    pub seed_wpm: f32,
    pub adaptive: bool,
    pub threshold_dbfs: f32,
    /// Word-boundary multiplier for stretched spacing (Farnsworth).
    pub spacing: f32,
}
impl Default for CwConfig {
    fn default() -> Self {
        Self {
            seed_wpm: 20.0,
            adaptive: true,
            threshold_dbfs: -45.0,
            spacing: 1.0,
        }
    }
}
impl CwConfig {
    pub fn validate(self) -> Result<(), &'static str> {
        if !(5.0..=60.0).contains(&self.seed_wpm)
            || !(-110.0..=-5.0).contains(&self.threshold_dbfs)
            || !(1.0..=4.0).contains(&self.spacing)
        {
            return Err("CW speed must be 5–60 WPM, threshold -110 to -5 dBFS, spacing 1–4");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CwStatus {
    pub text: String,
    pub wpm: f32,
    pub key_down: bool,
    pub rejected_marks: u64,
    pub unknown_characters: u64,
}

pub struct CwDecoder {
    config: CwConfig,
    status: CwStatus,
    dot_ms: f32,
    sum: f32,
    samples: u32,
    candidate_ms: u32,
    duration_ms: u32,
    pattern: u16,
    elements: u8,
    invalid: bool,
    word_pending: bool,
}
impl CwDecoder {
    pub fn new(config: CwConfig) -> Result<Self, &'static str> {
        config.validate()?;
        Ok(Self {
            config,
            status: CwStatus {
                text: String::new(),
                wpm: config.seed_wpm,
                key_down: false,
                rejected_marks: 0,
                unknown_characters: 0,
            },
            dot_ms: 1200.0 / config.seed_wpm,
            sum: 0.0,
            samples: 0,
            candidate_ms: 0,
            duration_ms: 0,
            pattern: 1,
            elements: 0,
            invalid: false,
            word_pending: false,
        })
    }
    pub fn status(&self) -> &CwStatus {
        &self.status
    }
    pub fn process(&mut self, power: &[f32]) {
        for &power in power {
            self.sum += if power.is_finite() {
                power.max(0.0)
            } else {
                0.0
            };
            self.samples += 1;
            if self.samples == 48 {
                let threshold = 10_f32.powf(
                    (self.config.threshold_dbfs - if self.status.key_down { 4.0 } else { 0.0 })
                        / 10.0,
                );
                let key = self.sum / 48.0 >= threshold;
                self.tick(key);
                self.samples = 0;
                self.sum = 0.0;
            }
        }
    }
    fn tick(&mut self, key: bool) {
        self.duration_ms = self.duration_ms.saturating_add(1);
        if key != self.status.key_down {
            self.candidate_ms += 1;
            if self.candidate_ms >= 3 {
                let duration = self.duration_ms.saturating_sub(3) as f32;
                if self.status.key_down {
                    self.mark(duration);
                }
                self.status.key_down = key;
                self.duration_ms = 3;
                self.candidate_ms = 0;
            }
        } else {
            self.candidate_ms = 0;
        }
        if !self.status.key_down {
            let gap = self.duration_ms as f32;
            if gap >= 2.0 * self.dot_ms {
                self.flush();
            }
            if gap >= 5.0 * self.config.spacing * self.dot_ms && !self.status.text.is_empty() {
                self.word_pending = true;
            }
        }
    }
    fn mark(&mut self, duration: f32) {
        if duration < self.dot_ms * 0.25 {
            self.status.rejected_marks += 1;
            return;
        }
        if duration > self.dot_ms * 5.0 {
            self.status.rejected_marks += 1;
            self.pattern = 1;
            self.elements = 0;
            self.invalid = false;
            return;
        }
        let dash = duration >= self.dot_ms * 2.0;
        let estimate = duration / if dash { 3.0 } else { 1.0 };
        // Slow, bounded adaptation tolerates jitter without chasing a single bad edge.
        if self.config.adaptive && (self.dot_ms * 0.6..=self.dot_ms * 1.5).contains(&estimate) {
            self.dot_ms = (self.dot_ms + 0.08 * (estimate - self.dot_ms)).clamp(20.0, 240.0);
            self.status.wpm = 1200.0 / self.dot_ms;
        }
        // SOS has nine elements. Keep an overflow flag rather than accepting
        // a recognized prefix of an arbitrarily long/noisy run.
        if self.elements < 9 {
            self.pattern = (self.pattern << 1) | u16::from(dash);
            self.elements += 1;
        } else {
            self.invalid = true;
        }
    }
    fn flush(&mut self) {
        if self.elements == 0 {
            return;
        }
        let matches = |code: &str| {
            code.bytes()
                .fold(1_u16, |p, c| (p << 1) | u16::from(c == b'-'))
                == self.pattern
        };
        let prosign = (!self.invalid)
            .then(|| {
                PROSIGNS
                    .iter()
                    .find_map(|&(label, code)| matches(code).then_some(label))
            })
            .flatten();
        let ch = if self.invalid {
            None
        } else {
            ALPHABET
                .iter()
                .find_map(|&(ch, code)| matches(code).then_some(ch))
        };
        if self.word_pending {
            self.status.text.push(' ');
            self.word_pending = false;
        }
        if ch.is_none() && prosign.is_none() {
            self.status.unknown_characters += 1;
        }
        if let Some(prosign) = prosign {
            self.status.text.push_str(prosign);
        } else {
            self.status.text.push(ch.unwrap_or('?'));
        }
        // ASCII output; never leave a partial <PROSIGN> at the history boundary.
        if self.status.text.len() > 4096 {
            let mut remove = self.status.text.len() - 4096;
            let prefix = &self.status.text[..remove];
            if prefix
                .rfind('<')
                .is_some_and(|open| prefix.rfind('>').is_none_or(|close| open > close))
            {
                remove += self.status.text[remove..]
                    .find('>')
                    .expect("complete prosign")
                    + 1;
            }
            self.status.text.drain(..remove);
        }
        self.elements = 0;
        self.pattern = 1;
        self.invalid = false;
    }
}

/// Canonical display labels; aliases with identical keying cannot be inferred
/// from timing. AR/BT take display precedence over '+'/'=' respectively.
pub const PROSIGNS: &[(&str, &str)] = &[
    ("<BK>", "-...-.-"),
    ("<AR>", ".-.-."),
    ("<AS>", ".-..."),
    ("<BT>", "-...-"),
    ("<SK>", "...-.-"),
    ("<KN>", "-.--."),
    ("<KA>", "-.-.-"),
    ("<AA>", ".-.-"),
    ("<VE>", "...-."),
    ("<HH>", "........"),
    ("<SOS>", "...---..."),
];

/// International Morse alphabet used by the decoder and deterministic fixtures.
pub const ALPHABET: &[(char, &str)] = &[
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
    ('.', ".-.-.-"),
    (',', "--..--"),
    ('?', "..--.."),
    ('/', "-..-."),
    ('=', "-...-"),
    ('+', ".-.-."),
    ('-', "-....-"),
    ('@', ".--.-."),
];
