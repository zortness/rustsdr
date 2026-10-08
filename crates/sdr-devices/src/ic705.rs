//! Icom IC-705 CI-V control over a bounded serial transport.
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub struct Radio<T: Read + Write> {
    transport: T,
    address: u8,
    tx_owned: bool,
    saved_power: Option<u16>,
}
pub const MODES: &[(u8, &str)] = &[
    (0, "LSB"),
    (1, "USB"),
    (2, "AM"),
    (3, "CW"),
    (5, "FM"),
    (6, "WFM"),
    (7, "CW-R"),
    (0x17, "DV"),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Tune {
        frequency_hz: u32,
        mode: u8,
        data: bool,
    },
    TestTone {
        frequency_hz: u32,
        power_percent: u8,
    },
    StopTx,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub frequency_hz: u32,
    pub mode: u8,
    pub filter: u8,
    pub transmitting: bool,
    pub power: u16,
    pub data_mode: bool,
    pub usb_if: bool,
}
impl<T: Read + Write> Radio<T> {
    pub fn connect(transport: T, address: u8) -> io::Result<Self> {
        if address == 0 || address >= 0xe0 {
            return Err(io::Error::other("Invalid CI-V radio address"));
        }
        let mut radio = Self {
            transport,
            address,
            tx_owned: false,
            saved_power: None,
        };
        if radio.query(&[0x19, 0])? != [0xa4] {
            return Err(io::Error::other("Selected radio is not an IC-705"));
        }
        Ok(radio)
    }
    pub fn frequency(&mut self) -> io::Result<u32> {
        decode_frequency(&self.query(&[3])?)
    }
    pub fn state(&mut self) -> io::Result<State> {
        let mode = self.query(&[4])?;
        if mode.len() != 2 || !(1..=3).contains(&mode[1]) {
            return Err(io::Error::other("Invalid IC-705 mode/filter"));
        }
        let data = self.query(&[0x1a, 6])?;
        if !matches!(data.as_slice(), [0, 0] | [1, 1..=3]) {
            return Err(io::Error::other("Invalid IC-705 DATA mode"));
        }
        let usb_if = match self.setting(0x0109)?.as_slice() {
            [0] => false,
            [1] => true,
            _ => return Err(io::Error::other("Invalid USB AF/IF setting")),
        };
        Ok(State {
            frequency_hz: self.frequency()?,
            mode: mode[0],
            filter: mode[1],
            transmitting: self.is_transmitting()?,
            power: decode_level(&self.query(&[0x14, 0x0a])?)?,
            data_mode: data[0] == 1,
            usb_if,
        })
    }
    pub fn is_transmitting(&mut self) -> io::Result<bool> {
        match self.query(&[0x1c, 0])?.as_slice() {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err(io::Error::other("Invalid TX status")),
        }
    }
    /// Read-only connector configuration, BCD menu number.
    pub fn setting(&mut self, number: u16) -> io::Result<Vec<u8>> {
        self.query(&[0x1a, 5, (number >> 8) as u8, number as u8])
    }
    pub fn tune(&mut self, hz: u32, mode: u8, data: bool) -> io::Result<()> {
        let before = self.state()?;
        if before.transmitting {
            return Err(io::Error::other("Stop transmitting before tuning"));
        }
        if !(30_000..=470_000_000).contains(&hz)
            || !MODES.iter().any(|v| v.0 == mode)
            || (data && !matches!(mode, 0 | 1 | 2 | 5))
        {
            return Err(io::Error::other("Unsupported IC-705 tuning request"));
        }
        let mut command = vec![5];
        let mut value = hz;
        for _ in 0..5 {
            let pair = value % 100;
            command.push((((pair / 10) << 4) | (pair % 10)) as u8);
            value /= 100;
        }
        self.exchange(&command, true)?;
        if mode != before.mode {
            self.exchange(&[6, mode], true)?;
        }
        if matches!(mode, 0 | 1 | 2 | 5) && (mode != before.mode || data != before.data_mode) {
            self.exchange(
                &[
                    0x1a,
                    6,
                    u8::from(data),
                    if data { before.filter } else { 0 },
                ],
                true,
            )?;
        }
        let actual = self.state()?;
        if actual.frequency_hz != hz
            || actual.mode != mode
            || (matches!(mode, 0 | 1 | 2 | 5) && actual.data_mode != data)
        {
            return Err(io::Error::other("IC-705 tuning readback mismatch"));
        }
        Ok(())
    }
    /// Finite test lifecycle is owned by the caller's deadline. No persistent TX arm.
    /// Requires USB data mode, USB-only modulation, simplex, and an exact TX dial.
    pub fn begin_test(&mut self, hz: u32, power_percent: u8) -> io::Result<()> {
        if power_percent > 10 {
            return Err(io::Error::other("Test power is limited to 10%"));
        }
        let state = self.state()?;
        self.check_test_state(&state, hz)?;
        self.saved_power = Some(state.power);
        self.set_power(u16::from(power_percent) * 255 / 100)?;
        self.tx_owned = true; // Before write: even a lost ACK requires an RX attempt.
        self.exchange(&[0x1c, 0, 1], true)?;
        if !self.is_transmitting()? {
            return Err(io::Error::other("Radio did not enter TX"));
        }
        Ok(())
    }
    /// Check the test configuration without changing power, routing or PTT.
    pub fn check_test(&mut self, hz: u32) -> io::Result<()> {
        let state = self.state()?;
        self.check_test_state(&state, hz)
    }
    fn check_test_state(&mut self, state: &State, hz: u32) -> io::Result<()> {
        if state.transmitting || self.tx_owned {
            return Err(io::Error::other(
                "TX test blocked: radio is already transmitting",
            ));
        }
        if !matches!(state.mode, 0 | 1) {
            return Err(io::Error::other(
                "TX test requires USB or LSB; select an SSB mode",
            ));
        }
        if !state.data_mode {
            return Err(io::Error::other(
                "TX test requires DATA enabled for the selected SSB mode",
            ));
        }
        if state.frequency_hz != hz {
            return Err(io::Error::other(format!(
                "TX test dial changed: requested {hz} Hz, radio is at {} Hz; retry on the current dial",
                state.frequency_hz
            )));
        }
        let route = self.setting(0x0119)?;
        if route != [1] {
            let name = match route.as_slice() {
                [0] => "MIC",
                [2] => "MIC + USB",
                [3] => "WLAN",
                _ => "unknown",
            };
            return Err(io::Error::other(format!(
                "TX audio source: DATA MOD is {name}. On IC-705 set MENU > SET > Connectors > MOD Input > DATA MOD to USB. SSB DATA mode alone does not select USB audio."
            )));
        }
        let split = self.query(&[0x0f])?;
        if split != [0] {
            return Err(io::Error::other(format!(
                "TX test requires simplex: disable split/duplex on the radio (CI-V reply {split:02X?})"
            )));
        }
        let tx_hz = decode_frequency(&self.query(&[0x1c, 3])?)?;
        if tx_hz != hz {
            return Err(io::Error::other(format!(
                "TX frequency is {tx_hz} Hz; expected {hz} Hz. Check split, duplex and transmit offset settings."
            )));
        }
        Ok(())
    }
    pub fn end_test(&mut self) -> io::Result<()> {
        if self.tx_owned {
            self.exchange(&[0x1c, 0, 0], true)?;
            // An ACK accepts PTT-off; the hardware can still be transitioning.
            // Do not restore RF power until a subsequent status confirms RX.
            let deadline = Instant::now() + Duration::from_millis(750);
            while self.is_transmitting()? {
                if Instant::now() >= deadline {
                    return Err(io::Error::other(
                        "Radio still transmitting after waiting for RX confirmation",
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            self.tx_owned = false;
        }
        if let Some(power) = self.saved_power {
            self.set_power(power)?;
            self.saved_power = None;
        }
        Ok(())
    }
    fn set_power(&mut self, level: u16) -> io::Result<()> {
        self.exchange(
            &[
                0x14,
                0x0a,
                (level / 100) as u8,
                ((((level % 100) / 10) << 4) | (level % 10)) as u8,
            ],
            true,
        )?;
        if decode_level(&self.query(&[0x14, 0x0a])?)? != level {
            return Err(io::Error::other("RF power readback mismatch"));
        }
        Ok(())
    }
    fn query(&mut self, command: &[u8]) -> io::Result<Vec<u8>> {
        let payload = self.exchange(command, false)?;
        Ok(payload[command.len()..].to_vec())
    }
    fn exchange(&mut self, command: &[u8], ack: bool) -> io::Result<Vec<u8>> {
        self.exchange_inner(command, ack).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("CI-V command {command:02X?}: {error}"),
            )
        })
    }
    fn exchange_inner(&mut self, command: &[u8], ack: bool) -> io::Result<Vec<u8>> {
        let mut request = vec![0xfe, 0xfe, self.address, 0xe0];
        request.extend(command);
        request.push(0xfd);
        self.transport.write_all(&request)?;
        let deadline = Instant::now() + Duration::from_millis(900);
        let mut frame = Vec::new();
        while Instant::now() < deadline {
            let mut byte = [0];
            match self.transport.read_exact(&mut byte) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                result => result?,
            }
            frame.push(byte[0]);
            if frame.len() > 512 {
                return Err(io::Error::other("Oversized CI-V frame"));
            }
            if byte[0] != 0xfd {
                continue;
            }
            if let Some(start) = frame
                .windows(4)
                .position(|v| v == [0xfe, 0xfe, 0xe0, self.address])
            {
                let payload = &frame[start + 4..frame.len() - 1];
                if payload == [0xfa] {
                    return Err(io::Error::other("IC-705 rejected command"));
                }
                if (ack && payload == [0xfb]) || (!ack && payload.starts_with(command)) {
                    return Ok(payload.to_vec());
                }
            }
            frame.clear(); // Ignore echo, other controllers and unsolicited broadcasts.
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "IC-705 CI-V reply timed out",
        ))
    }
}
impl<T: Read + Write> Drop for Radio<T> {
    fn drop(&mut self) {
        let _ = self.end_test();
    }
}
fn decode_level(data: &[u8]) -> io::Result<u16> {
    if data.len() != 2 || data.iter().any(|b| b & 15 > 9 || b >> 4 > 9) {
        return Err(io::Error::other("Invalid CI-V level"));
    }
    let n = u16::from((data[0] >> 4) * 10 + (data[0] & 15)) * 100
        + u16::from((data[1] >> 4) * 10 + (data[1] & 15));
    if n > 255 {
        return Err(io::Error::other("CI-V level out of range"));
    }
    Ok(n)
}
#[cfg(feature = "cat")]
pub fn open(
    port: &str,
    baud: u32,
    address: u8,
) -> io::Result<Radio<Box<dyn serialport::SerialPort>>> {
    let mut port = serialport::new(port, baud)
        .flow_control(serialport::FlowControl::None)
        .dtr_on_open(false)
        .timeout(Duration::from_millis(100))
        .open()?;
    port.write_data_terminal_ready(false)?;
    port.write_request_to_send(false)?;
    Radio::connect(port, address)
}
fn decode_frequency(data: &[u8]) -> io::Result<u32> {
    if data.len() != 5 {
        return Err(io::Error::other("Invalid frequency length"));
    }
    let mut hz = 0u64;
    for &b in data.iter().rev() {
        if b & 15 > 9 || b >> 4 > 9 {
            return Err(io::Error::other("Invalid BCD frequency"));
        }
        hz = hz * 100 + u64::from((b >> 4) * 10 + (b & 15));
    }
    hz.try_into().map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    #[derive(Default)]
    struct Wire {
        incoming: VecDeque<u8>,
        sent: Vec<Vec<u8>>,
        tx: bool,
        data_off: bool,
        power: u16,
        bad_mod: bool,
        lose_tx_ack: bool,
        rx_delay_polls: usize,
        rx_pending: bool,
        stuck_tx: bool,
    }
    impl Read for Wire {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if bytes.is_empty() {
                return Ok(0);
            }
            bytes[0] = self
                .incoming
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "no reply"))?;
            Ok(1)
        }
    }
    impl Write for Wire {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.sent.push(bytes.to_vec());
            self.incoming.extend(bytes); // USB echo must not count as a reply.
            let reply: Vec<u8> = match &bytes[4..bytes.len() - 1] {
                [0x19, 0] => vec![0x19, 0, 0xa4],
                [3] => vec![3, 0x00, 0x40, 0x07, 0x07, 0x00],
                [4] => vec![4, 1, 1],
                [0x1c, 0] => {
                    if self.rx_pending && !self.stuck_tx {
                        if self.rx_delay_polls == 0 {
                            self.tx = false;
                            self.rx_pending = false;
                        } else {
                            self.rx_delay_polls -= 1;
                        }
                    }
                    vec![0x1c, 0, u8::from(self.tx)]
                }
                [0x1c, 3] => vec![0x1c, 3, 0, 0x40, 7, 7, 0],
                [0x1c, 0, value] => {
                    if *value == 1 {
                        self.tx = true;
                    } else if self.tx && (self.rx_delay_polls > 0 || self.stuck_tx) {
                        self.rx_pending = true;
                    } else {
                        self.tx = false;
                    }
                    if self.tx && self.lose_tx_ack {
                        return Err(io::Error::other("Lost TX ACK"));
                    }
                    vec![0xfb]
                }
                [0x0f] => vec![0x0f, 0],
                [0x1a, 6] => {
                    if self.data_off {
                        vec![0x1a, 6, 0, 0]
                    } else {
                        vec![0x1a, 6, 1, 1]
                    }
                }
                [0x1a, 5, 1, 9] => vec![0x1a, 5, 1, 9, 0],
                [0x1a, 5, 1, 0x19] => vec![0x1a, 5, 1, 0x19, u8::from(!self.bad_mod)],
                [0x14, 0x0a] => vec![
                    0x14,
                    0x0a,
                    (self.power / 100) as u8,
                    ((((self.power % 100) / 10) << 4) | (self.power % 10)) as u8,
                ],
                [0x14, 0x0a, hi, lo] => {
                    assert!(
                        !self.tx,
                        "Power must not be restored before RX confirmation"
                    );
                    self.power = decode_level(&[*hi, *lo])?;
                    vec![0xfb]
                }
                _ => vec![0xfa],
            };
            self.incoming.extend([0xfe, 0xfe, 0xe0, 0xa4]);
            self.incoming.extend(reply);
            self.incoming.push_back(0xfd);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn identifies_radio_and_reads_bcd_frequency_ignoring_echo() {
        let mut radio = Radio::connect(Wire::default(), 0xa4).unwrap();
        assert_eq!(radio.frequency().unwrap(), 7_074_000);
        assert_eq!(radio.transport.sent.len(), 2);
    }
    #[test]
    fn dropping_an_active_session_returns_to_receive_and_restores_power() {
        let mut wire = Wire {
            power: 200,
            ..Default::default()
        };
        let mut radio = Radio::connect(&mut wire, 0xa4).unwrap();
        radio.begin_test(7_074_000, 0).unwrap();
        drop(radio);
        assert!(!wire.tx);
        assert_eq!(wire.power, 200);
    }
    #[test]
    fn test_tx_checks_route_and_restores_power_after_rx_confirmation() {
        let mut radio = Radio::connect(
            Wire {
                power: 200,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        radio.begin_test(7_074_000, 5).unwrap();
        assert!(radio.is_transmitting().unwrap());
        assert_eq!(radio.transport.power, 12);
        radio.end_test().unwrap();
        assert!(!radio.is_transmitting().unwrap());
        assert_eq!(radio.transport.power, 200);
        let commands: Vec<_> = radio
            .transport
            .sent
            .iter()
            .map(|v| &v[4..v.len() - 1])
            .collect();
        let off = commands.iter().position(|v| *v == [0x1c, 0, 0]).unwrap();
        let restore = commands
            .iter()
            .rposition(|v| *v == [0x14, 0x0a, 2, 0])
            .unwrap();
        assert!(off < restore);
    }
    #[test]
    fn ptt_off_ack_can_precede_rx_status_without_failing_the_session() {
        let mut radio = Radio::connect(
            Wire {
                power: 200,
                rx_delay_polls: 3,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        radio.begin_test(7_074_000, 0).unwrap();
        radio.end_test().unwrap();
        assert!(!radio.state().unwrap().transmitting);
        assert!(!radio.tx_owned);
        assert_eq!(radio.transport.power, 200);
    }
    #[test]
    fn missing_rx_confirmation_keeps_reduced_power_and_cleanup_ownership() {
        let mut radio = Radio::connect(
            Wire {
                power: 200,
                stuck_tx: true,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        radio.begin_test(7_074_000, 0).unwrap();
        assert!(
            radio
                .end_test()
                .unwrap_err()
                .to_string()
                .contains("RX confirmation")
        );
        assert!(radio.tx_owned);
        assert_eq!(radio.transport.power, 0);
        assert_eq!(radio.saved_power, Some(200));
        radio.transport.stuck_tx = false;
        radio.end_test().unwrap();
        assert_eq!(radio.transport.power, 200);
    }
    #[test]
    fn refuses_mismatched_frequency_route_and_excess_power_without_keying() {
        for (hz, power, bad_mod) in [
            (7_075_000, 5, false),
            (7_074_000, 11, false),
            (7_074_000, 5, true),
        ] {
            let mut radio = Radio::connect(
                Wire {
                    bad_mod,
                    ..Default::default()
                },
                0xa4,
            )
            .unwrap();
            assert!(radio.begin_test(hz, power).is_err());
            assert!(
                !radio
                    .transport
                    .sent
                    .iter()
                    .any(|v| v[4..v.len() - 1] == [0x1c, 0, 1])
            );
        }
    }
    #[test]
    fn valid_ssb_data_with_microphone_route_reports_the_actual_blocker() {
        let mut radio = Radio::connect(
            Wire {
                bad_mod: true,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        let error = radio.begin_test(7_074_000, 0).unwrap_err().to_string();
        assert!(error.contains("DATA MOD is MIC"), "{error}");
        assert!(error.contains("Connectors > MOD Input > DATA MOD"));
        assert!(
            !radio
                .transport
                .sent
                .iter()
                .any(|v| matches!(&v[4..v.len() - 1], [0x1c, 0, 1] | [0x14, 0x0a, _, _]))
        );
    }
    #[test]
    fn lost_key_ack_still_requires_unkey_and_power_restore() {
        let mut radio = Radio::connect(
            Wire {
                power: 200,
                lose_tx_ack: true,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        assert!(radio.begin_test(7_074_000, 0).is_err());
        assert!(radio.transport.tx);
        radio.end_test().unwrap();
        assert!(!radio.transport.tx);
        assert_eq!(radio.transport.power, 200);
    }
    #[test]
    fn malformed_numbers_and_ng_replies_are_errors() {
        for bytes in [&[0xfa, 0, 0, 0, 0][..], &[0, 0][..], &[0x99; 5][..]] {
            assert!(decode_frequency(bytes).is_err());
        }
        let mut radio = Radio::connect(Wire::default(), 0xa4).unwrap();
        let error = radio.setting(0x9999).unwrap_err().to_string();
        assert!(error.contains("CI-V command [1A, 05, 99, 99]"), "{error}");
        assert!(error.contains("rejected command"), "{error}");
    }
    #[test]
    fn data_off_has_zero_filter_byte() {
        let mut radio = Radio::connect(
            Wire {
                data_off: true,
                ..Default::default()
            },
            0xa4,
        )
        .unwrap();
        assert!(!radio.state().unwrap().data_mode);
    }
}
