//! Independent channel extraction before voice filtering, AGC or squelch.
use crate::fm::Decimator;
use sdr_core::Iq;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelSpec {
    pub offset_hz: f64,
    pub sample_rate: u32,
    /// Total two-sided passband width.
    pub bandwidth_hz: u32,
}
pub struct Channel {
    stages: Vec<Decimator>,
    phase: f64,
    step: f64,
}
impl Channel {
    pub fn new(input_rate: u32, spec: ChannelSpec) -> Result<Self, &'static str> {
        if input_rate != 2_400_000
            || ![48_000, 240_000].contains(&spec.sample_rate)
            || spec.bandwidth_hz == 0
            || spec.bandwidth_hz > spec.sample_rate * 2 / 3
            || !spec.offset_hz.is_finite()
            || spec.offset_hz.abs() + spec.sample_rate as f64 / 2.0 > input_rate as f64 / 2.0
        {
            return Err(
                "Channel requires 2.4 MS/s capture, 48/240 kS/s output and a covered passband",
            );
        }
        let mut stages = vec![Decimator::new(input_rate as f64, 90_000.0, 241, 10)];
        if spec.sample_rate == 48_000 {
            stages.push(Decimator::new(
                240_000.0,
                spec.bandwidth_hz as f64 / 2.0,
                241,
                5,
            ));
        } else {
            stages.push(Decimator::new(
                240_000.0,
                spec.bandwidth_hz as f64 / 2.0,
                129,
                1,
            ));
        }
        Ok(Self {
            stages,
            phase: 0.0,
            step: -std::f64::consts::TAU * spec.offset_hz / input_rate as f64,
        })
    }
    /// Appends to reusable storage; reconstruct after any source gap or retune.
    pub fn process(&mut self, input: &[Iq], output: &mut Vec<Iq>) {
        for s in input {
            let (sin, cos) = self.phase.sin_cos();
            self.phase = (self.phase + self.step).rem_euclid(std::f64::consts::TAU);
            let mut sample = Some(Iq {
                i: s.i * cos as f32 - s.q * sin as f32,
                q: s.q * cos as f32 + s.i * sin as f32,
            });
            for stage in &mut self.stages {
                sample = sample.and_then(|s| stage.push(s));
            }
            if let Some(s) = sample {
                output.push(s);
            }
        }
    }
}
