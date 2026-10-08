//! Read-only by default; --tune checks a reversible frequency/mode roundtrip. Never keys.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let port = args
        .first()
        .ok_or("Usage: ic705_probe <CI-V port> [baud=19200]")?;
    let baud = args.get(1).map(|v| v.parse()).transpose()?.unwrap_or(19200);
    let mut radio = sdr_devices::ic705::open(port, baud, 0xa4)?;
    println!("IC-705 {port}: {:?}", radio.state()?);
    for (number, name) in [
        (0x0110, "USB AF level"),
        (0x0116, "USB MOD level"),
        (0x0118, "DATA OFF MOD"),
        (0x0119, "DATA MOD"),
        (0x0125, "USB SEND"),
        (0x0126, "USB CW keying"),
        (0x0127, "USB RTTY keying"),
    ] {
        println!("{name}: {:02X?}", radio.setting(number)?);
    }
    let dial = radio.frequency()?;
    match radio.check_test(dial) {
        Ok(()) => println!("TX configuration ready (read-only; radio NOT keyed)"),
        Err(e) => println!("TX configuration blocked: {e}"),
    }
    if args.get(2).is_some_and(|v| v == "--tune") {
        let original = radio.state()?;
        if original.transmitting {
            return Err("Radio must be receiving".into());
        }
        let result = (|| -> Result<(), std::io::Error> {
            radio.tune(
                original.frequency_hz + 100,
                original.mode,
                original.data_mode,
            )?;
            println!("Frequency +100 Hz readback passed");
            radio.tune(original.frequency_hz, 1, true)?;
            println!("USB DATA readback passed");
            Ok(())
        })();
        radio.tune(original.frequency_hz, original.mode, original.data_mode)?;
        println!(
            "Restored {} Hz mode {}",
            original.frequency_hz, original.mode
        );
        result?;
    }
    Ok(())
}
