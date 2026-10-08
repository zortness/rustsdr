//! Experimental receive-only QMX CAT probe; no arbitrary command option.
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

fn query(port: &mut impl ReadWrite, command: &str) -> io::Result<String> {
    port.write_all(command.as_bytes())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut frame = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "CAT response deadline",
            ));
        }
        let mut byte = [0];
        port.read_exact(&mut byte)?;
        if byte[0] == b';' {
            if frame == b"?" {
                return Err(io::Error::other("Radio rejected CAT command"));
            }
            let text = String::from_utf8(std::mem::take(&mut frame)).map_err(io::Error::other)?;
            if text.starts_with(&command[..2]) {
                return Ok(text);
            }
            // Ignore bounded unsolicited status frames, e.g. AI/IF updates.
        } else {
            if frame.len() >= 512 {
                return Err(io::Error::other("Oversized CAT frame"));
            }
            frame.push(byte[0]);
        }
    }
}
trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn frequency(reply: &str) -> io::Result<u64> {
    let digits = reply
        .strip_prefix("FA")
        .ok_or_else(|| io::Error::other("Expected FA"))?;
    if digits.len() != 11 || !digits.bytes().all(|v| v.is_ascii_digit()) {
        return Err(io::Error::other("Malformed VFO frequency"));
    }
    digits.parse().map_err(io::Error::other)
}

fn tune_test(port: &mut impl ReadWrite) -> io::Result<()> {
    if query(port, "TQ;")? != "TQ0" || query(port, "FR;")? != "FR0" || query(port, "FT;")? != "FT0"
    {
        return Err(io::Error::other(
            "Tune test requires receiving on VFO A, without split",
        ));
    }
    let original = frequency(&query(port, "FA;")?)?;
    // Stay inside a high-band amateur allocation; do not change bands in this probe.
    let bands = [
        (14_000_000, 14_350_000),
        (18_068_000, 18_168_000),
        (21_000_000, 21_450_000),
        (24_890_000, 24_990_000),
        (28_000_000, 29_700_000),
    ];
    let test = original + 100;
    if !bands
        .iter()
        .any(|&(low, high)| original >= low && test < high)
    {
        return Err(io::Error::other(
            "No safe in-band +100 Hz test for this frequency",
        ));
    }
    // Restore even if the initial write partially succeeds or verification fails.
    let result = (|| {
        port.write_all(format!("FA{test:011};").as_bytes())?;
        let actual = frequency(&query(port, "FA;")?)?;
        println!("Tune: {original} -> requested {test}, reported {actual} Hz");
        if actual != test {
            return Err(io::Error::other("Tune readback mismatch"));
        }
        if query(port, "TQ;")? != "TQ0" {
            return Err(io::Error::other("Radio no longer reports receive"));
        }
        Ok(())
    })();
    let restored = (|| {
        port.write_all(format!("FA{original:011};").as_bytes())?;
        if frequency(&query(port, "FA;")?)? != original {
            return Err(io::Error::other("Restore readback mismatch"));
        }
        println!("Restored {original} Hz; {}", query(port, "TQ;")?);
        Ok(())
    })();
    if let Err(error) = restored {
        return Err(io::Error::other(format!(
            "RESTORATION FAILED: {error}; manually restore {original} Hz. Test result: {result:?}"
        )));
    }
    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        for port in serialport::available_ports()? {
            println!("{} {:?}", port.port_name, port.port_type);
        }
        println!("qmx_probe <port> [--tune-test] (read-only by default)");
        return Ok(());
    }
    if args.len() > 2 || (args.len() == 2 && args[1] != "--tune-test") {
        return Err("Invalid arguments".into());
    }
    // Linux may briefly assert DTR on open even with dtr_on_open(false).
    // Validate a platform-specific transport before enabling this hardware probe there.
    if !cfg!(windows) {
        return Err("Hardware opening is currently validated/enabled only on Windows".into());
    }
    let mut port = serialport::new(&args[0], 38400)
        .flow_control(serialport::FlowControl::None)
        .dtr_on_open(false)
        .timeout(Duration::from_millis(1500))
        .open()?;
    port.write_data_terminal_ready(false)?;
    port.write_request_to_send(false)?;
    let model = query(&mut port, "OM;")?;
    if model != "OMQC" {
        return Err(format!("Expected QMX model QC; got {model}").into());
    }
    println!("OM; -> {model};");
    for command in [
        "ID;", "VN;", "TQ;", "FA;", "FB;", "FR;", "FT;", "MD;", "Q9;", "PL;", "SM;", "LC;",
    ] {
        println!("{command} -> {};", query(&mut port, command)?);
    }
    if args.len() == 2 {
        tune_test(&mut port)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        input: io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Fake {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }
    impl Write for Fake {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn fake(input: &str) -> Fake {
        Fake {
            input: io::Cursor::new(input.as_bytes().to_vec()),
            output: Vec::new(),
        }
    }
    #[test]
    fn framing_and_invalid_responses() {
        assert_eq!(
            query(&mut fake("IFstatus;FA00024924600;"), "FA;").unwrap(),
            "FA00024924600"
        );
        for reply in ["?;".to_owned(), "X".repeat(513), "FA00024924600".to_owned()] {
            assert!(query(&mut fake(&reply), "FA;").is_err());
        }
        for reply in ["FA-0000000001", "FA123", "FB00024924600"] {
            assert!(frequency(reply).is_err());
        }
    }
    #[test]
    fn restores_after_failed_tune_verification() {
        let mut port = fake("TQ0;FR0;FT0;FA00024924600;FA00024924601;FA00024924600;TQ0;");
        assert!(tune_test(&mut port).is_err());
        let sent = String::from_utf8(port.output).unwrap();
        assert!(sent.contains("FA00024924700;FA;FA00024924600;FA;"));
    }
    #[test]
    fn refuses_tuning_while_transmitting() {
        let mut port = fake("TQ1;");
        assert!(tune_test(&mut port).is_err());
        assert_eq!(port.output, b"TQ;");
    }
}
