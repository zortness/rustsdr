use sdr_app::memories::{Library, Memory, MemoryGroup};
use sdr_dsp::{Mode, ReceiverConfig};

#[test]
fn large_memory_groups_round_trip_full_receiver_settings() {
    let path = std::env::temp_dir().join(format!("rustsdr-memories-{}.csv", std::process::id()));
    let mut config = ReceiverConfig::for_mode(Mode::Nfm);
    config.squelch_db = Some(-63.0);
    config.deemphasis_us = 300;
    let library = Library {
        version: 1,
        groups: vec![MemoryGroup {
            name: "VHF channels".into(),
            entries: (0..10_000)
                .map(|n| Memory {
                    name: format!("Channel {n}"),
                    frequency_hz: 144_000_000 + n * 100,
                    receiver: config,
                    scan_enabled: n % 2 == 0,
                })
                .collect(),
        }],
    };
    library.save(&path).unwrap();
    let loaded = Library::load(&path).unwrap();
    assert_eq!(loaded, library);
    assert_eq!(loaded.groups[0].step_index(0, -1), Some(9999));
    assert_eq!(loaded.groups[0].step_index(9999, 1), Some(0));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn minimal_csv_uses_filename_group_and_mode_defaults_and_reports_physical_lines() {
    let library = Library::from_csv(
        b"name,frequency_hz,mode\r\n\"Town, FM\",106700000,stereo\r\n",
        "Broadcast",
    )
    .unwrap();
    assert_eq!(library.groups[0].name, "Broadcast");
    assert_eq!(library.groups[0].entries[0].name, "Town, FM");
    assert_eq!(
        library.groups[0].entries[0].receiver,
        ReceiverConfig::for_mode(Mode::WfmStereo)
    );
    let error = Library::from_csv(
        b"name,frequency_hz,mode\n\"multi\nline\",106700000,stereo\nbad,89900000,invalid\n",
        "Test",
    )
    .unwrap_err();
    assert!(error.contains("line 4"), "{error}");
}

#[test]
fn rejected_save_preserves_existing_file_and_duplicate_groups_are_rejected() {
    let path = std::env::temp_dir().join(format!(
        "rustsdr-invalid-memories-{}.csv",
        std::process::id()
    ));
    let library = Library::default();
    library.save(&path).unwrap();
    library.save(&path).unwrap(); // Replacement must work on Windows, too.
    let mut invalid = library.clone();
    invalid.groups.push(invalid.groups[0].clone());
    assert!(invalid.save(&path).is_err());
    assert_eq!(Library::load(&path).unwrap(), library);
    std::fs::remove_file(path).unwrap();
}
