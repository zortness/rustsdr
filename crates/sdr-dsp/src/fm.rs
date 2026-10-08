use sdr_core::Iq;

pub(crate) struct Decimator {
    taps: Vec<f32>,
    delay: Vec<Iq>,
    position: usize,
    phase: usize,
    factor: usize,
}

impl Decimator {
    pub(crate) fn new(rate: f64, cutoff: f64, length: usize, factor: usize) -> Self {
        let mut taps: Vec<f32> = (0..length)
            .map(|n| {
                let x = n as f64 - (length - 1) as f64 / 2.0;
                let sinc = if x == 0.0 {
                    2.0 * cutoff / rate
                } else {
                    (std::f64::consts::TAU * cutoff / rate * x).sin() / (std::f64::consts::PI * x)
                };
                let window = 0.42
                    - 0.5 * (std::f64::consts::TAU * n as f64 / (length - 1) as f64).cos()
                    + 0.08 * (2.0 * std::f64::consts::TAU * n as f64 / (length - 1) as f64).cos();
                (sinc * window) as f32
            })
            .collect();
        let sum: f32 = taps.iter().sum();
        for tap in &mut taps {
            *tap /= sum;
        }
        Self {
            taps,
            delay: vec![Iq::default(); 2 * length],
            position: 0,
            phase: 0,
            factor,
        }
    }
    pub(crate) fn push(&mut self, sample: Iq) -> Option<Iq> {
        let length = self.taps.len();
        self.delay[self.position] = sample;
        self.delay[self.position + length] = sample;
        self.position = (self.position + 1) % length;
        self.phase += 1;
        if self.phase != self.factor {
            return None;
        }
        self.phase = 0;
        let mut result = Iq::default();
        for (sample, tap) in self.delay[self.position..self.position + length]
            .iter()
            .zip(&self.taps)
        {
            result.i += sample.i * tap;
            result.q += sample.q * tap;
        }
        Some(result)
    }
}

/// Mono broadcast FM: 2.4 MS/s IQ -> 240 kS/s discriminator -> 48 kHz audio.
/// The channel is 200 kHz wide; stereo/RDS subcarriers are filtered out.
pub struct FmReceiver {
    channel: Decimator,
    audio: Decimator,
    oscillator: f64,
    step: f64,
    previous: Option<Iq>,
    deemphasis: f32,
    alpha: f32,
    dc: f32,
}

impl FmReceiver {
    pub fn new(offset_hz: f64, deemphasis_us: u32) -> Result<Self, &'static str> {
        if !offset_hz.is_finite()
            || offset_hz.abs() > 1_000_000.0
            || ![0, 50, 75].contains(&deemphasis_us)
        {
            return Err("FM offset must be within +/-1 MHz; de-emphasis must be 0, 50, or 75 us");
        }
        Ok(Self {
            channel: Decimator::new(2_400_000.0, 100_000.0, 241, 10),
            audio: Decimator::new(240_000.0, 15_000.0, 101, 5),
            oscillator: 0.0,
            step: -std::f64::consts::TAU * offset_hz / 2_400_000.0,
            previous: None,
            deemphasis: 0.0,
            alpha: if deemphasis_us == 0 {
                1.0
            } else {
                (1.0 - (-1.0 / (48_000.0 * deemphasis_us as f64 * 1e-6)).exp()) as f32
            },
            dc: 0.0,
        })
    }
    /// Appends audio to reusable caller storage. Recreate on discontinuities.
    pub fn process(&mut self, samples: &[Iq], output: &mut Vec<f32>) {
        for sample in samples {
            let (sin, cos) = self.oscillator.sin_cos();
            let shifted = Iq {
                i: sample.i * cos as f32 - sample.q * sin as f32,
                q: sample.i * sin as f32 + sample.q * cos as f32,
            };
            self.oscillator = (self.oscillator + self.step).rem_euclid(std::f64::consts::TAU);
            if let Some(value) = self.channel.push(shifted) {
                let demod = self.previous.map_or(0.0, |previous| {
                    let re = value.i * previous.i + value.q * previous.q;
                    let im = value.q * previous.i - value.i * previous.q;
                    im.atan2(re) * (240_000.0 / (std::f32::consts::TAU * 75_000.0))
                });
                self.previous = Some(value);
                if let Some(audio) = self.audio.push(Iq { i: demod, q: 0.0 }) {
                    self.deemphasis += self.alpha * (audio.i - self.deemphasis);
                    self.dc += 0.001 * (self.deemphasis - self.dc);
                    output.push((self.deemphasis - self.dc).clamp(-1.0, 1.0));
                }
            }
        }
    }
}
