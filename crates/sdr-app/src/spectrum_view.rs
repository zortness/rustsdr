//! Frequency-aware display reduction. Historical rows retain their own coverage.
pub fn project(
    values: &[f32],
    source_center: f64,
    source_span: f64,
    center: f64,
    span: f64,
    width: usize,
) -> Vec<f32> {
    if values.is_empty() || source_span <= 0.0 || width == 0 {
        return vec![f32::NAN; width];
    }
    let source_left = source_center - source_span / 2.0;
    let bin = source_span / values.len() as f64;
    (0..width)
        .map(|x| {
            let low = (center - span / 2.0 + span * x as f64 / width as f64 - source_left) / bin;
            let high = low + span / width as f64 / bin;
            if high <= 0.0 || low >= values.len() as f64 {
                return f32::NAN;
            }
            let start = low.floor().max(0.0) as usize;
            let end = (high.ceil().max(1.0) as usize).min(values.len());
            values[start..end].iter().copied().fold(f32::NAN, f32::max)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_preserves_coarse_history_and_missing_coverage() {
        assert_eq!(
            project(&[1., 2., 3., 4.], 100., 40., 100., 20., 4),
            vec![2., 2., 3., 3.]
        );
        let wide = project(&[1., 9., 2., 3.], 100., 20., 100., 40., 4);
        assert!(wide[0].is_nan() && wide[3].is_nan());
        assert_eq!(wide[1], 9.);
    }
}
