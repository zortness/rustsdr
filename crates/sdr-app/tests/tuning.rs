use sdr_app::tuning::Tuning;

#[test]
fn narrow_capture_recenters_before_vfo_leaves_passband() {
    let mut tuning = Tuning {
        converter_offset_hz: 0,
        center_hz: 100_000_000,
        station_hz: 100_000_000,
    };
    tuning.tune_with_limit(100_800_000, 1, false, 760_000);
    assert_eq!(tuning.station_hz, 100_800_000);
    assert_eq!(tuning.center_hz, 101_050_000);
    assert!(tuning.station_hz.abs_diff(tuning.center_hz) <= 760_000);
}

#[test]
fn converter_mapping_bounds_and_hf_tuning() {
    use sdr_app::tuning::{converter_bounds, tuner_frequency};
    assert_eq!(tuner_frequency(7_100_000, -125_000_000), Some(132_100_000));
    assert_eq!(tuner_frequency(435_000_000, 300_000_000), Some(135_000_000));
    assert_eq!(converter_bounds(-125_000_000), Some((1, 1_575_000_000)));
    assert_eq!(
        converter_bounds(300_000_000),
        Some((328_800_000, 1_700_000_000))
    );
    assert_eq!(converter_bounds(i64::MAX), None);
    assert_eq!(converter_bounds(i64::MIN), None);
    assert_eq!(tuner_frequency(7_100_000, 0), None);
    let mut t = Tuning {
        converter_offset_hz: -125_000_000,
        center_hz: 7_350_000,
        station_hz: 7_100_000,
    };
    t.tune(7_101_034, 50, false);
    assert_eq!(t.station_hz, 7_101_050);
    assert_eq!(t.center_hz, 7_350_000);
    t.pan(-10_000);
    assert_eq!(t.center_hz, 7_340_000);
    t.tune(14_200_000, 100, false);
    assert_eq!(t.center_hz, 14_450_000);
    t.pan(-2_000_000_000);
    assert_eq!(t.station_hz, 1);
    assert!(tuner_frequency(t.center_hz as u64, t.converter_offset_hz).is_some());
    t.converter_offset_hz = 0;
    t.tune(t.station_hz, 1, false);
    assert_eq!(t.station_hz, sdr_core::RTL_MIN_HZ);
}

#[test]
fn converted_engine_and_scan_use_rf_coordinates() {
    use sdr_app::engine::{Settings, Source, scan_bounds};
    let s = Settings {
        running: false,
        source: Source::RtlSdr,
        converter_offset_hz: -125_000_000,
        station_hz: 7_100_000,
        rtl_center_hz: Some(7_350_000),
        ..Default::default()
    };
    assert_eq!(s.center_hz(), 7_350_000);
    assert_eq!(s.tuner_center_hz(), Some(132_350_000));
    assert_eq!(scan_bounds(&s), (1, 1_575_000_000));
    let plan = sdr_app::scanning::ScanPlan {
        parameters: Default::default(),
        start_index: 0,
        targets: sdr_app::scanning::ScanTargets::Range {
            start_hz: 7_000_000,
            stop_hz: 7_300_000,
            step_hz: 1000,
            receiver: sdr_dsp::ReceiverConfig::for_mode(sdr_dsp::Mode::Lsb),
        },
    };
    let (min, max) = scan_bounds(&s);
    assert!(plan.validate(min, max).is_ok());
    #[cfg(feature = "rtl")]
    {
        let _engine = sdr_app::engine::Engine::with_settings(s).unwrap();
    }
}

#[test]
fn vfo_stays_in_capture_until_edge_and_file_never_retunes_capture() {
    let mut t = Tuning {
        converter_offset_hz: 0,
        center_hz: 106_950_000,
        station_hz: 106_700_000,
    };
    t.tune(107_103_000, 100_000, false);
    assert_eq!(t.station_hz, 107_100_000);
    assert_eq!(t.center_hz, 106_950_000);
    t.tune(89_900_000, 100_000, false);
    assert_eq!(t.center_hz, 90_150_000);
    t.tune(106_700_000, 100_000, true);
    assert_eq!(t.center_hz, 90_150_000);
    assert!(t.station_hz.abs_diff(t.center_hz) <= 1_000_000);
}

#[test]
fn center_scroll_keeps_vfo_offset_and_clamps_at_band_edges() {
    let mut t = Tuning {
        converter_offset_hz: 0,
        center_hz: 106_950_000,
        station_hz: 106_700_000,
    };
    t.pan(100_000);
    assert_eq!((t.center_hz, t.station_hz), (107_050_000, 106_800_000));
    t.pan(2_000_000_000);
    assert_eq!((t.center_hz, t.station_hz), (1_700_000_000, 1_699_750_000));
    t.pan(-2_000_000_000);
    assert_eq!((t.center_hz, t.station_hz), (29_050_000, 28_800_000));
}

#[test]
fn ssb_fine_tuning_supports_hf_files_but_live_requires_normal_tuner_range() {
    let mut t = Tuning {
        converter_offset_hz: 0,
        center_hz: 14_250_000,
        station_hz: 14_200_000,
    };
    t.tune(14_201_049, 100, true);
    assert_eq!(t.station_hz, 14_201_000);
    assert_eq!(t.center_hz, 14_250_000);
    t.tune(14_201_000, 100, false);
    assert_eq!(t.station_hz, sdr_core::RTL_MIN_HZ);
    assert!(t.center_hz >= sdr_core::RTL_MIN_HZ);
}
