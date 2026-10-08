use sdr_app::digital::{Protocol, decode, read_wav};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/digital")
        .join(name)
}
#[test]
fn independent_ft4_reference() {
    let messages = decode(
        Protocol::Ft4,
        &read_wav(&fixture("ft4.wav"), Protocol::Ft4).unwrap(),
    )
    .unwrap();
    for expected in [
        "CQ RU AB5XS EM12",
        "CQ RU N9OY EN43",
        "KB0VHA KA1YQC R 539 MA",
    ] {
        assert!(
            messages.iter().any(|m| m.text == expected),
            "Missing {expected}"
        );
    }
}
#[test]
fn independent_ft8_reference() {
    let messages = decode(
        Protocol::Ft8,
        &read_wav(&fixture("ft8.wav"), Protocol::Ft8).unwrap(),
    )
    .unwrap();
    for (expected, hz) in [
        ("W0RSJ EA3BMU RR73", 400.0),
        ("N1PJT HB9CQK -10", 466.0),
        ("K1JT HA0DU KN07", 590.0),
        ("CQ F5RXL IN94", 1197.0),
    ] {
        let m = messages
            .iter()
            .find(|m| m.text == expected)
            .unwrap_or_else(|| panic!("Missing {expected}"));
        assert!((m.audio_hz - hz).abs() < 4.0);
        if expected == "CQ F5RXL IN94" {
            assert_eq!(m.sender.as_deref(), Some("F5RXL"));
        }
        if expected == "W0RSJ EA3BMU RR73" {
            assert_eq!(m.sender.as_deref(), Some("EA3BMU"));
        }
    }
}
#[test]
fn silence_and_invalid_input() {
    for protocol in [Protocol::Ft8, Protocol::Ft4] {
        assert!(
            decode(protocol, &vec![0; protocol.samples()])
                .unwrap()
                .is_empty()
        );
        assert!(decode(protocol, &[0; 12]).is_err());
    }
    assert!(read_wav(&fixture("ft8.wav"), Protocol::Ft4).is_err());
}

#[test]
fn ft4_reference_through_iq_usb_and_decoder_audio() {
    use sdr_dsp::{DecoderAudio, Mode, Receiver, ReceiverConfig};
    let pcm = read_wav(&fixture("ft4.wav"), Protocol::Ft4).unwrap();
    let mut config = ReceiverConfig::for_mode(Mode::Usb);
    config.bandwidth_hz = 4700;
    config.agc = false;
    let mut receiver = Receiver::new(0.0, config).unwrap();
    let mut resampler = DecoderAudio::default();
    let mut output = Vec::new();
    let mut slot = Vec::new();
    // Independent reference audio forms an AM/double-sideband IQ signal;
    // USB selects its positive sideband. Zero-order hold images lie outside
    // the selected channel. This exercises the actual RTL receiver DSP.
    for chunk in pcm.chunks(60) {
        let iq: Vec<_> = chunk
            .iter()
            .flat_map(|&v| {
                std::iter::repeat_n(
                    sdr_core::Iq {
                        i: v as f32 / 32768.0,
                        q: 0.0,
                    },
                    200,
                )
            })
            .collect();
        output.clear();
        receiver.process(&iq, &mut output);
        for frame in &output {
            if let Some(v) = resampler.push((frame.left + frame.right) * 0.5) {
                slot.push(v);
            }
        }
    }
    assert_eq!(slot.len(), Protocol::Ft4.samples());
    let results = decode(Protocol::Ft4, &slot).unwrap();
    assert!(results.iter().any(|m| m.text == "CQ RU AB5XS EM12"));
    assert!(results.iter().any(|m| m.text == "KB0VHA KA1YQC R 539 MA"));
}
