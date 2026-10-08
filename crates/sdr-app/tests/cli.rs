use std::process::Command;

#[test]
fn headless_run_reports_expected_signal_and_rejects_invalid_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_sdr-app"))
        .args(["--blocks", "4", "--tone-hz", "-125000"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Samples: 16384"), "{text}");
    assert!(text.contains("Tone offset: -125000.00 Hz"), "{text}");
    assert!(text.contains("Mean power: 0.250000"), "{text}");
    for args in [
        vec!["--unknown"],
        vec!["--blocks", "0"],
        vec!["--tone-hz", "NaN"],
        vec!["--sample-rate", "0"],
        vec!["--blocks"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_sdr-app"))
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!result.stderr.is_empty());
    }
}
