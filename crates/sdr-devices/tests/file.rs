use sdr_core::Iq;
use sdr_devices::file::IqFile;

#[test]
fn rejects_invalid_metadata_empty_truncated_and_nonfinite_input() {
    let path = std::env::temp_dir().join(format!("rustsdr-invalid-{}.cf32", std::process::id()));
    std::fs::write(&path, [0_u8; 8]).unwrap();
    assert!(IqFile::open(&path, 0, 100_000_000).is_err());
    assert!(IqFile::open(&path, 48_000, 100_000_000).is_err());
    assert!(IqFile::open(&path, 2_400_000, 0).is_err());
    for length in [0, 7, 9] {
        std::fs::write(&path, vec![0; length]).unwrap();
        assert!(IqFile::open(&path, 2_400_000, 100_000_000).is_err());
    }
    std::fs::write(
        &path,
        [f32::NAN.to_le_bytes(), 0_f32.to_le_bytes()].concat(),
    )
    .unwrap();
    let mut file = IqFile::open(&path, 2_400_000, 100_000_000).unwrap();
    assert!(file.read(&mut [Iq { i: 0.0, q: 0.0 }]).is_err());
    drop(file);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reads_little_endian_pairs_and_partial_final_block_then_eof() {
    let path = std::env::temp_dir().join(format!("rustsdr-file-{}.cf32", std::process::id()));
    let values = [0.25_f32, -0.5, 0.75, 1.0, -1.0, 0.0];
    std::fs::write(
        &path,
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut file = IqFile::open(&path, 2_400_000, 100_000_000).unwrap();
    assert_eq!(file.total_samples(), 3);
    let mut out = [Iq { i: 0.0, q: 0.0 }; 2];
    assert_eq!(file.read(&mut out).unwrap(), 2);
    assert_eq!(
        (out[0].i, out[0].q, out[1].i, out[1].q),
        (0.25, -0.5, 0.75, 1.0)
    );
    assert_eq!(file.read(&mut out).unwrap(), 1);
    assert_eq!((out[0].i, out[0].q), (-1.0, 0.0));
    assert_eq!(file.read(&mut out).unwrap(), 0);
    drop(file);
    std::fs::remove_file(path).unwrap();
}
