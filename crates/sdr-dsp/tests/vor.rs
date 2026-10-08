use sdr_dsp::vor::analyze_envelope;
#[test]
fn recorded_vor_radials_agree_with_upstream_reference_ranges() {
    for (name, bytes, low, high) in [
        (
            "KLO",
            include_bytes!("fixtures/desperado/klo.f32").as_slice(),
            115.0,
            125.0,
        ),
        (
            "ARL",
            include_bytes!("fixtures/desperado/arl.f32").as_slice(),
            110.0,
            120.0,
        ),
    ] {
        let audio: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let result = analyze_envelope(&audio, 1_800_000.0 / 38.0).expect(name);
        assert!(
            (low..=high).contains(&result.radial_deg),
            "{name}: {result:?}"
        );
    }
}
#[test]
fn decodes_known_radials_and_rejects_missing_reference() {
    for angle in [0.0_f64, 45.0, 120.0, 270.0, 359.0] {
        let audio: Vec<_> = (0..48_000)
            .map(|n| {
                let t = n as f64 / 48_000.0;
                let phase = std::f64::consts::TAU * 30.0 * t;
                (1.0 + 0.3 * (phase - angle.to_radians()).cos()
                    + 0.3 * (std::f64::consts::TAU * 9960.0 * t + 16.0 * phase.sin()).cos())
                    as f32
            })
            .collect();
        let r = analyze_envelope(&audio, 48_000.0).unwrap();
        assert!(
            ((r.radial_deg - angle + 540.0) % 360.0 - 180.0).abs() < 1.0,
            "{angle}: {r:?}"
        );
    }
    assert!(analyze_envelope(&vec![1.0; 48_000], 48_000.0).is_none());
}

#[test]
fn reference_alone_with_noise_does_not_produce_a_radial() {
    let mut seed = 1234_u32;
    let audio: Vec<_> = (0..48_000)
        .map(|n| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let t = n as f64 / 48_000.0;
            (1.0 + 0.3
                * (std::f64::consts::TAU * 9960.0 * t
                    + 16.0 * (std::f64::consts::TAU * 30.0 * t).sin())
                .cos()
                + 0.02 * seed as i32 as f64 / i32::MAX as f64) as f32
        })
        .collect();
    assert!(analyze_envelope(&audio, 48_000.0).is_none());
}
