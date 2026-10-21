//! Frequency policy shared by pointer, wheel, and numeric controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    /// Antenna frequency = tuner frequency + this signed offset.
    pub converter_offset_hz: i64,
    pub center_hz: u32,
    pub station_hz: u32,
}

/// Positive, application-supported RF range intersected with the translated tuner range.
pub fn converter_bounds(offset: i64) -> Option<(u32, u32)> {
    let low = (sdr_core::RTL_MIN_HZ as i64).checked_add(offset)?.max(1);
    let high = (sdr_core::RX_MAX_HZ as i64)
        .checked_add(offset)?
        .min(sdr_core::RX_MAX_HZ as i64);
    (low <= high).then_some((u32::try_from(low).ok()?, u32::try_from(high).ok()?))
}
pub fn tuner_frequency(rf: u64, offset: i64) -> Option<u32> {
    let hz = i64::try_from(rf).ok()?.checked_sub(offset)?;
    (sdr_core::RTL_MIN_HZ as i64..=sdr_core::RX_MAX_HZ as i64)
        .contains(&hz)
        .then_some(hz as u32)
}

impl Tuning {
    pub fn tune(&mut self, hz: u32, step: u32, fixed_capture: bool) {
        self.tune_with_limit(hz, step, fixed_capture, 1_000_000);
    }
    pub fn tune_with_limit(&mut self, hz: u32, step: u32, fixed_capture: bool, max_offset_hz: u32) {
        let step = step.max(1) as u64;
        let (minimum, maximum) = if fixed_capture {
            (sdr_core::FILE_MIN_HZ, sdr_core::RX_MAX_HZ)
        } else {
            let Some(bounds) = converter_bounds(self.converter_offset_hz) else {
                return;
            };
            bounds
        };
        let hz =
            ((hz as u64 + step / 2) / step * step).clamp(minimum as u64, maximum as u64) as u32;
        if fixed_capture {
            self.station_hz = hz.clamp(
                self.center_hz.saturating_sub(1_000_000),
                self.center_hz.saturating_add(1_000_000),
            );
        } else {
            self.station_hz = hz;
            if hz.abs_diff(self.center_hz) > max_offset_hz
                || !(minimum..=maximum).contains(&self.center_hz)
            {
                self.center_hz = (hz + 250_000).min(maximum);
            }
        }
    }

    /// Shift capture and VFO together, retaining the receiver offset.
    pub fn pan(&mut self, delta: i64) {
        let Some((minimum, maximum)) = converter_bounds(self.converter_offset_hz) else {
            return;
        };
        let low =
            (minimum as i64 - self.station_hz as i64).max(minimum as i64 - self.center_hz as i64);
        let high =
            (maximum as i64 - self.station_hz as i64).min(maximum as i64 - self.center_hz as i64);
        if low > high {
            return;
        }
        let delta = delta.clamp(low, high);
        self.station_hz = (self.station_hz as i64 + delta) as u32;
        self.center_hz = (self.center_hz as i64 + delta) as u32;
    }
}
