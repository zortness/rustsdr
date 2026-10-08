//! QMX receive control. There is deliberately no transmit or arbitrary-command API.
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub const RECEIVE_MODES: &[(u8, &str)] = &[
    (1, "LSB"),
    (2, "USB"),
    (3, "CW"),
    (7, "CW-R"),
    (6, "Digi USB"),
    (9, "Digi LSB"),
];
pub fn receive_mode_supported(mode: u8) -> bool {
    RECEIVE_MODES.iter().any(|&(code, _)| code == mode)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Band {
    pub index: u8,
    pub meters: u32,
    pub min_hz: u32,
    pub start_hz: u32,
    pub max_hz: u32,
}
impl Band {
    pub fn contains(&self, hz: u32) -> bool {
        (self.min_hz..=self.max_hz).contains(&hz)
    }
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub frequency_hz: u32,
    pub mode: u8,
    pub vfo_b: bool,
    pub firmware: String,
    pub bands: Vec<Band>,
    pub band_command: bool,
}
impl State {
    pub fn band(&self) -> Option<&Band> {
        self.bands
            .iter()
            .filter(|b| b.contains(self.frequency_hz))
            .min_by_key(|b| b.start_hz.abs_diff(self.frequency_hz))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Tune { frequency_hz: u32, mode: u8 },
    Band(u8),
}

/// Transport must provide bounded reads/writes and keep hardware keying lines inactive.
pub struct Radio<T: Read + Write> {
    transport: T,
    pub state: State,
    remembered: Vec<(u8, u32, u8)>,
}
impl<T: Read + Write> Radio<T> {
    pub fn connect(transport: T) -> io::Result<Self> {
        let mut radio = Self {
            transport,
            state: State::default(),
            remembered: Vec::new(),
        };
        if radio.query("OM;")? != "OMQC" {
            return Err(io::Error::other("Selected port is not a QMX"));
        }
        radio.state.firmware = radio.query("VN;")?.trim_start_matches("VN").to_owned();
        radio.require_receive()?;
        if radio.query("Q9;")? != "Q90" {
            return Err(io::Error::other(
                "QMX is in raw IQ mode; select normal USB audio on the radio first",
            ));
        }
        radio.state.band_command = radio.query("BN;")? != "?";
        for index in 0..16 {
            let meters = radio.menu_number("Band name (m)", index)?;
            if meters == 0 {
                continue;
            }
            let band = Band {
                index,
                meters,
                min_hz: radio.menu_number("Frequency min.", index)?,
                start_hz: radio.menu_number("Frequency center", index)?,
                max_hz: radio.menu_number("Frequency max.", index)?,
            };
            if band.min_hz == 0 || band.min_hz >= band.max_hz || !band.contains(band.start_hz) {
                return Err(io::Error::other("Invalid QMX band table"));
            }
            radio.state.bands.push(band);
        }
        if radio.state.bands.is_empty() {
            return Err(io::Error::other("QMX has no configured receive bands"));
        }
        radio.refresh()?;
        Ok(radio)
    }
    fn menu_number(&mut self, name: &str, index: u8) -> io::Result<u32> {
        let reply = self.query(&format!("MMBand config.|{name}[{index}];"))?;
        number(&reply, "MM")
    }
    fn require_receive(&mut self) -> io::Result<()> {
        if self.query("TQ;")? != "TQ0" {
            return Err(io::Error::other(
                "QMX is transmitting; receive session stopped",
            ));
        }
        Ok(())
    }
    pub fn refresh(&mut self) -> io::Result<()> {
        self.require_receive()?;
        if self.query("Q9;")? != "Q90" {
            return Err(io::Error::other(
                "QMX stream changed to IQ; restart after restoring normal audio",
            ));
        }
        let fr = self.query("FR;")?;
        self.state.vfo_b = match fr.as_str() {
            "FR0" => false,
            "FR1" => true,
            _ => return Err(io::Error::other("Invalid receive VFO")),
        };
        let prefix = if self.state.vfo_b { "FB" } else { "FA" };
        let reply = self.query(&format!("{prefix};"))?;
        self.state.frequency_hz = number(&reply, prefix)?;
        self.state.mode = number(&self.query("MD;")?, "MD")?
            .try_into()
            .map_err(io::Error::other)?;
        if !receive_mode_supported(self.state.mode) {
            return Err(io::Error::other(
                "Select USB, LSB, CW, CW-R or Digi on QMX for this receive adapter",
            ));
        }
        Ok(())
    }
    pub fn apply(&mut self, command: &Command) -> io::Result<()> {
        self.refresh()?;
        if let Some(band) = self.state.band() {
            let entry = (band.index, self.state.frequency_hz, self.state.mode);
            self.remembered.retain(|v| v.0 != entry.0);
            self.remembered.push(entry);
        }
        match *command {
            Command::Tune { frequency_hz, mode } => self.tune(frequency_hz, mode)?,
            Command::Band(index) => {
                let band = self
                    .state
                    .bands
                    .iter()
                    .find(|b| b.index == index)
                    .ok_or_else(|| io::Error::other("Unknown QMX band"))?
                    .clone();
                if let Some((_, frequency, mode)) =
                    self.remembered.iter().find(|v| v.0 == index).copied()
                {
                    self.tune(frequency, mode)?;
                } else if self.state.band_command {
                    self.transport.write_all(format!("BN{index};").as_bytes())?;
                    self.refresh()?;
                    if !band.contains(self.state.frequency_hz) {
                        return Err(io::Error::other("Band readback outside requested range"));
                    }
                } else {
                    // Older firmware: tuning the band center selects its filters.
                    // Read back the mode selected/retained by the radio; do not invent one.
                    self.set_frequency(band.start_hz)?;
                    self.refresh()?;
                }
            }
        }
        self.refresh()
    }
    fn tune(&mut self, frequency: u32, mode: u8) -> io::Result<()> {
        if !receive_mode_supported(mode) {
            return Err(io::Error::other("Unsupported receive mode"));
        }
        if !self.state.bands.iter().any(|b| b.contains(frequency)) {
            return Err(io::Error::other(
                "Frequency is outside configured QMX receive ranges",
            ));
        }
        self.set_frequency(frequency)?;
        self.transport.write_all(format!("MD{mode};").as_bytes())?;
        if number(&self.query("MD;")?, "MD")? != u32::from(mode) {
            return Err(io::Error::other("QMX mode readback mismatch"));
        }
        Ok(())
    }
    fn set_frequency(&mut self, hz: u32) -> io::Result<()> {
        let prefix = if self.state.vfo_b { "FB" } else { "FA" };
        self.transport
            .write_all(format!("{prefix}{hz:011};").as_bytes())?;
        if number(&self.query(&format!("{prefix};"))?, prefix)? != hz {
            return Err(io::Error::other("QMX frequency readback mismatch"));
        }
        Ok(())
    }
    fn query(&mut self, command: &str) -> io::Result<String> {
        self.transport.write_all(command.as_bytes())?;
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
            self.transport.read_exact(&mut byte)?;
            if byte[0] == b';' {
                let text =
                    String::from_utf8(std::mem::take(&mut frame)).map_err(io::Error::other)?;
                if text == "?" || text.starts_with(&command[..2]) {
                    return Ok(text);
                }
            } else {
                if frame.len() >= 512 {
                    return Err(io::Error::other("CAT frame too long"));
                }
                frame.push(byte[0]);
            }
        }
    }
}
fn number(reply: &str, prefix: &str) -> io::Result<u32> {
    let value = reply
        .strip_prefix(prefix)
        .ok_or_else(|| io::Error::other("Unexpected CAT reply"))?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::other("Malformed CAT number"));
    }
    value.parse().map_err(io::Error::other)
}
#[cfg(feature = "cat")]
pub fn open(port: &str) -> io::Result<Radio<Box<dyn serialport::SerialPort>>> {
    if !cfg!(windows) {
        return Err(io::Error::other(
            "QMX serial keying-line behavior is currently validated only on Windows",
        ));
    }
    let mut port = serialport::new(port, 38400)
        .flow_control(serialport::FlowControl::None)
        .dtr_on_open(false)
        .timeout(Duration::from_millis(250))
        .open()?;
    port.write_data_terminal_ready(false)?;
    port.write_request_to_send(false)?;
    Radio::connect(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake {
        replies: VecDeque<u8>,
        sent: Vec<String>,
        hz: u32,
        mode: u8,
        tx: bool,
        b: bool,
    }
    impl Read for Fake {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if bytes.is_empty() {
                return Ok(0);
            }
            bytes[0] = self
                .replies
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "no reply"))?;
            Ok(1)
        }
    }
    impl Write for Fake {
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let command = std::str::from_utf8(bytes).unwrap().to_owned();
            self.sent.push(command.clone());
            let reply = match command.as_str() {
                "TQ;" => format!("TQ{};", u8::from(self.tx)),
                "Q9;" => "Q90;".into(),
                "FR;" => format!("FR{};", u8::from(self.b)),
                "FA;" | "FB;" => format!("{}{hz:011};", &command[..2], hz = self.hz),
                "MD;" => format!("MD{};", self.mode),
                _ if command.starts_with("FA") || command.starts_with("FB") => {
                    self.hz = command[2..command.len() - 1].parse().unwrap();
                    String::new()
                }
                _ if command.starts_with("MD") => {
                    self.mode = command[2..command.len() - 1].parse().unwrap();
                    String::new()
                }
                _ => "?;".into(),
            };
            self.replies.extend(reply.bytes());
            Ok(bytes.len())
        }
    }
    fn radio() -> Radio<Fake> {
        Radio {
            transport: Fake {
                replies: VecDeque::new(),
                sent: Vec::new(),
                hz: 14_055_000,
                mode: 3,
                tx: false,
                b: false,
            },
            state: State {
                bands: vec![
                    Band {
                        index: 0,
                        meters: 20,
                        min_hz: 13_900_000,
                        start_hz: 14_074_000,
                        max_hz: 14_500_000,
                    },
                    Band {
                        index: 1,
                        meters: 17,
                        min_hz: 18_000_000,
                        start_hz: 18_104_600,
                        max_hz: 18_200_000,
                    },
                ],
                ..Default::default()
            },
            remembered: Vec::new(),
        }
    }
    #[test]
    fn band_roundtrip_restores_mode_and_tuning() {
        let mut r = radio();
        r.apply(&Command::Band(1)).unwrap();
        assert_eq!(r.state.frequency_hz, 18_104_600);
        r.apply(&Command::Tune {
            frequency_hz: 18_110_000,
            mode: 2,
        })
        .unwrap();
        r.apply(&Command::Band(0)).unwrap();
        assert_eq!((r.state.frequency_hz, r.state.mode), (14_055_000, 3));
        r.apply(&Command::Band(1)).unwrap();
        assert_eq!((r.state.frequency_hz, r.state.mode), (18_110_000, 2));
    }
    #[test]
    fn gap_and_transmit_mode_are_never_written() {
        for (hz, mode) in [(16_000_000, 2), (14_055_000, 8)] {
            let mut r = radio();
            assert!(
                r.apply(&Command::Tune {
                    frequency_hz: hz,
                    mode
                })
                .is_err()
            );
            assert!(
                !r.transport
                    .sent
                    .iter()
                    .any(|s| s.starts_with("FA0") || s.starts_with("MD8"))
            );
        }
        let mut r = radio();
        r.transport.tx = true;
        assert!(r.apply(&Command::Band(1)).is_err());
        assert_eq!(r.transport.sent, vec!["TQ;"]);
    }
    #[test]
    fn active_vfo_b_and_external_changes_are_read_back() {
        let mut r = radio();
        r.transport.b = true;
        r.refresh().unwrap();
        r.apply(&Command::Tune {
            frequency_hz: 14_060_000,
            mode: 2,
        })
        .unwrap();
        assert!(r.transport.sent.iter().any(|s| s == "FB00014060000;"));
        r.transport.hz = 14_065_000;
        r.transport.mode = 7;
        r.refresh().unwrap();
        assert_eq!((r.state.frequency_hz, r.state.mode), (14_065_000, 7));
    }
    #[test]
    fn malformed_cat_numbers_fail_closed() {
        for value in ["?", "FA-1", "FA12x", "FA4294967296", "FB123"] {
            assert!(number(value, "FA").is_err());
        }
    }
    #[test]
    fn digi_front_panel_changes_keep_control_and_can_cycle_out() {
        let mut r = radio();
        for digi in [6, 9] {
            r.transport.mode = digi;
            r.refresh().unwrap();
            assert_eq!(r.state.mode, digi);
            for &(mode, _) in RECEIVE_MODES {
                r.apply(&Command::Tune {
                    frequency_hz: 14_055_000,
                    mode,
                })
                .unwrap();
                assert_eq!(r.state.mode, mode);
            }
        }
        assert!(!receive_mode_supported(8)); // SWR tune must never become a receive option.
    }
    #[test]
    fn band_memory_preserves_digi_sideband() {
        let mut r = radio();
        r.transport.mode = 9;
        r.apply(&Command::Band(1)).unwrap();
        r.apply(&Command::Tune {
            frequency_hz: 18_110_000,
            mode: 6,
        })
        .unwrap();
        r.apply(&Command::Band(0)).unwrap();
        assert_eq!((r.state.frequency_hz, r.state.mode), (14_055_000, 9));
    }
}
