use sdr_dsp::rds::RdsDecoder;
#[test]
fn real_fip_broadcast_matches_independent_decoder_reference() {
    let data = include_bytes!("fixtures/desperado/fip-mpx.f32");
    let samples: Vec<_> = data
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut decoder = RdsDecoder::new();
    for chunk in samples.chunks(997) {
        decoder.process(chunk);
    }
    let state = decoder.status();
    assert_eq!(state.pi, Some(0xF204), "{state:?}");
    assert_eq!(
        state.program_service.as_deref().map(str::trim),
        Some("F I P"),
        "{state:?}"
    );
    assert_eq!(
        state.radio_text.as_deref(),
        Some("MOON RIVER - MELODY GARDOT (2020) - FIP"),
        "{state:?}"
    );
}
