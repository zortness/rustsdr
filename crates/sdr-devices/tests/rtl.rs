#![cfg(feature = "rtl")]
use sdr_core::Iq;
use sdr_devices::rtl::convert_u8;

#[test]
fn backend_selection_is_explicit_and_invalid_rf_never_opens_usb() {
    use sdr_devices::{RtlBackend, rtl::RtlSession};
    assert_eq!(
        RtlBackend::default(),
        if cfg!(feature = "rtl-rust") {
            RtlBackend::Rust
        } else {
            RtlBackend::Librtlsdr
        }
    );
    for backend in [RtlBackend::Librtlsdr, RtlBackend::Rust] {
        assert_eq!(RtlBackend::from_key(backend.key()), Some(backend));
        assert!(RtlSession::open_with(backend, 0, 7_074_000, 2_400_000).is_err());
        assert!(RtlSession::open_with(backend, 0, 132_324_000, 0).is_err());
    }
    assert_eq!(RtlBackend::from_key("unknown"), None);
}

#[test]
fn unsigned_iq_conversion_preserves_order_and_rejects_partial_pairs() {
    let mut output = [Iq::default(); 2];
    convert_u8(&[0, 255, 127, 128], &mut output).unwrap();
    assert_eq!(
        output[0],
        Iq {
            i: -127.5 / 128.0,
            q: 127.5 / 128.0
        }
    );
    assert_eq!(
        output[1],
        Iq {
            i: -0.5 / 128.0,
            q: 0.5 / 128.0
        }
    );
    assert!(convert_u8(&[1, 2, 3], &mut output).is_err());
}
