//! CSV memory libraries. Disk operations belong on an I/O worker, not the radio worker.
use sdr_dsp::{Mode, ReceiverConfig};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]

pub struct Memory {
    pub name: String,
    pub frequency_hz: u32,
    pub receiver: ReceiverConfig,
    pub scan_enabled: bool,
}

impl Memory {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 256 {
            return Err("Memory names must contain 1–256 bytes".into());
        }
        if !(sdr_core::FILE_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&self.frequency_hz) {
            return Err("Memory frequency is outside 1 Hz–1700 MHz".into());
        }
        self.receiver.validate().map_err(str::to_owned)
    }
}
#[derive(Clone, Debug, PartialEq)]

pub struct MemoryGroup {
    pub name: String,
    pub entries: Vec<Memory>,
}
impl MemoryGroup {
    /// Manual recall visits all memories, including entries excluded from scanning.
    pub fn step_index(&self, current: usize, delta: i64) -> Option<usize> {
        (!self.entries.is_empty()).then(|| {
            let length = self.entries.len() as i64;
            ((current % self.entries.len()) as i64 + delta.rem_euclid(length)).rem_euclid(length)
                as usize
        })
    }
}
#[derive(Clone, Debug, PartialEq)]

pub struct Library {
    pub version: u32,
    pub groups: Vec<MemoryGroup>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            groups: vec![MemoryGroup {
                name: "My memories".into(),
                entries: Vec::new(),
            }],
        }
    }
}
impl Library {
    pub fn from_csv(bytes: &[u8], default_group: &str) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Memory file exceeds 64 MiB".into());
        }
        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(bytes);
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        for required in ["name", "frequency_hz", "mode"] {
            if !headers.iter().any(|h| h == required) {
                return Err(format!("Missing CSV column: {required}"));
            }
        }
        let mut names = std::collections::HashSet::new();
        for header in &headers {
            if ![
                "group",
                "name",
                "frequency_hz",
                "mode",
                "bandwidth_hz",
                "deviation_hz",
                "deemphasis_us",
                "agc",
                "squelch_db",
                "scan_enabled",
                "schema_version",
            ]
            .contains(&header)
                || !names.insert(header)
            {
                return Err(format!("Unknown or duplicate CSV column: {header}"));
            }
        }
        let mut library = Self {
            version: 1,
            groups: Vec::new(),
        };
        let mut groups = std::collections::HashMap::new();
        let mut count = 0;
        for result in reader.records() {
            let record = result.map_err(|e| format!("Invalid CSV row: {e}"))?;
            let line = record
                .position()
                .map_or(count as u64 + 2, |position| position.line());
            let row: CsvMemory = record
                .deserialize(Some(&headers))
                .map_err(|e| format!("CSV line {line}: {e}"))?;
            count += 1;
            if count > MAX_ENTRIES + 1024 {
                return Err("Too many CSV rows".into());
            }
            if row.schema_version.is_some_and(|v| v != 1) {
                return Err(format!("CSV line {line}: unsupported schema version"));
            }
            let name = if row.group.is_empty() {
                default_group.to_owned()
            } else {
                row.group.clone()
            };
            let index = *groups.entry(name.clone()).or_insert_with(|| {
                library.groups.push(MemoryGroup {
                    name,
                    entries: Vec::new(),
                });
                library.groups.len() - 1
            });
            if row.frequency_hz.is_none() && row.name.is_empty() && row.mode.is_empty() {
                continue;
            }
            let entry = row
                .into_memory()
                .map_err(|e| format!("CSV line {line}: {e}"))?;
            library.groups[index].entries.push(entry);
        }
        library.validate()?;
        Ok(library)
    }
    pub fn to_csv(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(Vec::new());
        writer
            .write_record([
                "group",
                "name",
                "frequency_hz",
                "mode",
                "bandwidth_hz",
                "deviation_hz",
                "deemphasis_us",
                "agc",
                "squelch_db",
                "scan_enabled",
                "schema_version",
            ])
            .map_err(|e| e.to_string())?;
        for group in &self.groups {
            if group.entries.is_empty() {
                writer
                    .serialize(CsvMemory {
                        group: group.name.clone(),
                        schema_version: Some(1),
                        ..Default::default()
                    })
                    .map_err(|e| e.to_string())?;
            }
            for memory in &group.entries {
                let config = memory.receiver;
                writer
                    .serialize(CsvMemory {
                        group: group.name.clone(),
                        name: memory.name.clone(),
                        frequency_hz: Some(memory.frequency_hz),
                        mode: mode_name(config.mode).into(),
                        bandwidth_hz: Some(config.bandwidth_hz),
                        deviation_hz: Some(config.deviation_hz),
                        deemphasis_us: Some(config.deemphasis_us),
                        agc: Some(config.agc),
                        squelch_db: config.squelch_db,
                        scan_enabled: Some(memory.scan_enabled),
                        schema_version: Some(1),
                    })
                    .map_err(|e| e.to_string())?;
            }
        }
        writer.into_inner().map_err(|e| e.to_string())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "Unsupported memory library version {}",
                self.version
            ));
        }
        if self.groups.len() > 1024
            || self.groups.iter().map(|g| g.entries.len()).sum::<usize>() > MAX_ENTRIES
        {
            return Err("Library limit: 1024 groups and 100,000 memories".into());
        }
        for group in &self.groups {
            if group.name.trim().is_empty() || group.name.len() > 256 {
                return Err("Group names must contain 1–256 bytes".into());
            }
            if self.groups.iter().filter(|g| g.name == group.name).count() != 1 {
                return Err("Group names must be unique".into());
            }
            for entry in &group.entries {
                entry.validate()?;
            }
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Memory file exceeds 64 MiB".into());
        }
        let library = Self::from_csv(
            &bytes,
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Imported"),
        )?;
        library.validate()?;
        Ok(library)
    }
    /// Validate first; write and sync a sibling temporary file, then replace the destination.
    /// A failure leaves the previous destination intact. No delete-then-rename window.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = self.to_csv()?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Memory file exceeds 64 MiB".into());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let suffix = format!(
            ".{}.{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        );
        let mut temp = path.as_os_str().to_os_string();
        temp.push(suffix);
        let temp = PathBuf::from(temp);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|e| e.to_string())
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct CsvMemory {
    group: String,
    name: String,
    frequency_hz: Option<u32>,
    mode: String,
    bandwidth_hz: Option<u32>,
    deviation_hz: Option<u32>,
    deemphasis_us: Option<u32>,
    agc: Option<bool>,
    squelch_db: Option<f32>,
    scan_enabled: Option<bool>,
    schema_version: Option<u32>,
}
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::WfmMono => "wfm",
        Mode::WfmStereo => "stereo",
        Mode::Am => "am",
        Mode::Nfm => "nfm",
        Mode::Usb => "usb",
        Mode::Lsb => "lsb",
        Mode::Cw => "cw",
    }
}
impl CsvMemory {
    fn into_memory(self) -> Result<Memory, String> {
        let mode = Mode::ALL
            .into_iter()
            .find(|&mode| mode_name(mode).eq_ignore_ascii_case(&self.mode))
            .ok_or("Mode must be wfm, stereo, am, nfm, usb, lsb or cw")?;
        let mut receiver = ReceiverConfig::for_mode(mode);
        if let Some(value) = self.bandwidth_hz {
            receiver.bandwidth_hz = value;
        }
        if let Some(value) = self.deviation_hz {
            receiver.deviation_hz = value;
        }
        if let Some(value) = self.deemphasis_us {
            receiver.deemphasis_us = value;
        }
        if let Some(value) = self.agc {
            receiver.agc = value;
        }
        receiver.squelch_db = self.squelch_db;
        let memory = Memory {
            name: self.name,
            frequency_hz: self.frequency_hz.ok_or("Missing frequency_hz")?,
            receiver,
            scan_enabled: self.scan_enabled.unwrap_or(true),
        };
        memory.validate()?;
        Ok(memory)
    }
}
pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("RUSTSDR_MEMORY_FILE") {
        return path.into();
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("APPDATA") {
        return PathBuf::from(root).join("RustSDR/memories.csv");
    }
    #[cfg(target_os = "macos")]
    if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root).join("Library/Application Support/RustSDR/memories.csv");
    }
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("rustsdr/memories.csv");
    }
    if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root).join(".config/rustsdr/memories.csv");
    }
    PathBuf::from("memories.csv")
}
