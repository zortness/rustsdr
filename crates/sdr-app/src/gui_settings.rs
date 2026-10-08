//! Each panel owns its preferences; runtime state never enters the document.
use super::*;
use sdr_app::preferences::{Document, Store};
use serde::{Deserialize, Serialize};

#[derive(Resource)]
pub(super) struct Persistence {
    store: Store,
    warnings: Vec<String>,
}
pub(super) fn section<T: serde::de::DeserializeOwned>(
    doc: &Document,
    key: &str,
    warnings: &mut Vec<String>,
) -> Option<T> {
    match doc.get(key) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(error);
            None
        }
    }
}
fn mode_key(mode: Mode) -> &'static str {
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
#[derive(Serialize, Deserialize)]
struct ReceiverPreferences {
    bandwidth_hz: u32,
    deviation_hz: u32,
    deemphasis_us: u32,
    agc: bool,
    squelch_db: Option<f32>,
    step_hz: u32,
}
impl ReceiverPreferences {
    fn from(config: ReceiverConfig, step_hz: u32) -> Self {
        Self {
            bandwidth_hz: config.bandwidth_hz,
            deviation_hz: config.deviation_hz,
            deemphasis_us: config.deemphasis_us,
            agc: config.agc,
            squelch_db: config.squelch_db,
            step_hz,
        }
    }
    fn config(&self, mode: Mode) -> ReceiverConfig {
        ReceiverConfig {
            mode,
            bandwidth_hz: self.bandwidth_hz,
            deviation_hz: self.deviation_hz,
            deemphasis_us: self.deemphasis_us,
            agc: self.agc,
            squelch_db: self.squelch_db,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Workspace {
    source: String,
    mode: String,
    station_mhz: f64,
    volume: f32,
    muted: bool,
    floor: f32,
    ceiling: f32,
    zoom: f32,
    controls_open: bool,
    debug_open: bool,
    signal_data_open: bool,
}
#[derive(Serialize, Deserialize)]
struct FilePreferences {
    path: String,
    center_mhz: f64,
}
#[derive(Serialize, Deserialize)]
struct QmxPreferences {
    port: String,
    input: String,
    output: String,
    audio_gain_db: u8,
    step_hz: u32,
}
#[derive(Serialize, Deserialize)]
struct CwPreferences {
    enabled: bool,
    seed_wpm: f32,
    adaptive: bool,
    threshold_dbfs: f32,
    spacing: f32,
}
#[derive(Serialize, Deserialize)]
struct WindowPreferences {
    width: u32,
    height: u32,
}

impl Persistence {
    pub fn qmx_step(&self) -> u32 {
        self.store
            .document
            .get::<QmxPreferences>("devices.qmx")
            .ok()
            .flatten()
            .map_or(100, |p| p.step_hz.clamp(1, 10_000_000))
    }
    pub fn window_size(&self) -> (u32, u32) {
        self.store
            .document
            .get::<WindowPreferences>("window")
            .ok()
            .flatten()
            .filter(|p| (480..=7680).contains(&p.width) && (400..=4320).contains(&p.height))
            .map_or((1280, 820), |p| (p.width, p.height))
    }
    pub fn load(display: &mut Display) -> Option<Self> {
        let path = sdr_app::preferences::default_path()?;
        let store = Store::open(path);
        let mut warnings = Vec::new();
        restore(display, &store.document, &mut warnings);
        display.running = false;
        Some(Self { store, warnings })
    }
    pub fn warning(&self) -> Option<String> {
        self.store
            .error()
            .or_else(|| (!self.warnings.is_empty()).then(|| self.warnings.join("; ")))
    }
}
fn restore(d: &mut Display, doc: &Document, warnings: &mut Vec<String>) {
    if let Some(p) = section::<sdr_app::engine::Ic705Settings>(doc, "devices.ic705", warnings) {
        d.ic705.settings = p;
    }
    d.running = false;
    for mode in Mode::ALL {
        let key = format!("modes.{}", mode_key(mode));
        if let Some(p) = section::<ReceiverPreferences>(doc, &key, warnings) {
            let config = p.config(mode);
            if config.validate().is_ok() && (1..=10_000_000).contains(&p.step_hz) {
                d.mode_settings.restore(config, p.step_hz);
            } else {
                warnings.push(format!("Invalid {key}; using defaults"));
            }
        }
    }
    if let Some(p) = section::<Workspace>(doc, "workspace", warnings) {
        if (0.000001..=1700.0).contains(&p.station_mhz)
            && (0.0..=1.0).contains(&p.volume)
            && (-200.0..=20.0).contains(&p.floor)
            && (p.floor + 1.0..=40.0).contains(&p.ceiling)
            && (1.0..=4800.0).contains(&p.zoom)
        {
            d.source = match p.source.as_str() {
                "rtl" => Source::RtlSdr,
                "file" => Source::File,
                "qmx" => Source::Qmx,
                "ic705" => Source::Ic705,
                _ => Source::Synthetic,
            };
            let mode = Mode::ALL
                .into_iter()
                .find(|m| mode_key(*m) == p.mode)
                .unwrap_or_default();
            let (config, step) = *d
                .mode_settings
                .entries()
                .iter()
                .find(|(c, _)| c.mode == mode)
                .unwrap();
            d.receiver = config;
            d.tune_step_hz = step;
            d.station_mhz = p.station_mhz;
            d.volume = p.volume;
            d.muted = p.muted;
            d.floor = p.floor;
            d.ceiling = p.ceiling;
            d.zoom = p.zoom;
            d.controls_open = p.controls_open;
            d.debug_open = p.debug_open;
            d.broadcast.open = p.signal_data_open;
        } else {
            warnings.push("Invalid workspace settings; using defaults".into());
        }
    }
    if let Some(key) = section::<String>(doc, "devices.rtl.backend", warnings) {
        match RtlBackend::from_key(&key).filter(|b| b.available()) {
            Some(backend) => d.rtl_backend = backend,
            None => {
                d.rtl_backend = RtlBackend::default();
                warnings.push(format!(
                    "Saved RTL backend unavailable; using {}",
                    d.rtl_backend.label()
                ));
            }
        }
    }
    if let Some(offset) = section::<i64>(doc, "devices.rtl.converter_offset_hz", warnings) {
        if sdr_app::tuning::converter_bounds(offset).is_some() {
            d.converter_offset_hz = offset;
        } else {
            warnings.push("Invalid converter offset; using zero".into());
        }
    }
    if let Some(correction) = section::<i32>(doc, "devices.rtl.correction_hz", warnings) {
        if (-100_000..=100_000).contains(&correction)
            && d.converter_offset_hz
                .checked_add(correction as i64)
                .and_then(sdr_app::tuning::converter_bounds)
                .is_some()
        {
            d.converter_correction_hz = correction;
        } else {
            warnings.push("Invalid RTL fine correction; using zero".into());
        }
    }
    if let Some(center) = section::<u32>(doc, "devices.rtl.center_hz", warnings)
        && sdr_app::tuning::tuner_frequency(center as u64, d.effective_converter_offset()).is_some()
    {
        d.rtl_center_hz = center;
    }
    if let Some(p) = section::<FilePreferences>(doc, "devices.file", warnings)
        && (0.000001..=1700.0).contains(&p.center_mhz)
    {
        d.file_path = p.path;
        d.file_center_mhz = p.center_mhz;
    }
    if let Some(tone) = section::<f64>(doc, "devices.synthetic.tone_hz", warnings)
        && (-1_199_000.0..=1_199_000.0).contains(&tone)
    {
        d.tone_hz = tone;
    }
    if d.source == Source::Ic705 {
        d.tune_step_hz = d.ic705.settings.step_hz.clamp(1, 1_000_000);
    }
    if let Some(p) = section::<QmxPreferences>(doc, "devices.qmx", warnings) {
        d.qmx.settings.port = p.port;
        d.qmx.settings.input = p.input;
        d.qmx.settings.output = p.output;
        d.qmx.settings.audio_gain_db = p.audio_gain_db.min(60);
        d.qmx.remembered_step = Some(p.step_hz.clamp(1, 10_000_000));
        if d.source == Source::Qmx && (1..=10_000_000).contains(&p.step_hz) {
            d.tune_step_hz = p.step_hz;
        }
    }
    if let Some(enabled) = section::<bool>(doc, "decoders.rds", warnings) {
        d.broadcast.enabled = enabled;
    }
    if let Some(p) = section::<CwPreferences>(doc, "decoders.cw", warnings) {
        let config = sdr_dsp::cw::CwConfig {
            seed_wpm: p.seed_wpm,
            adaptive: p.adaptive,
            threshold_dbfs: p.threshold_dbfs,
            spacing: p.spacing,
        };
        if config.validate().is_ok() {
            d.broadcast.cw_enabled = p.enabled;
            d.broadcast.cw_config = config;
        } else {
            warnings.push("Invalid CW decoder settings; using defaults".into());
        }
    }
    d.memories.restore_preferences(doc, warnings);
    if let Some(p) = section(doc, "decoders.ft", warnings) {
        d.digital.preferences = p;
    }
    d.practice.restore_preferences(doc, warnings);
    if (d.source == Source::Ic705 && !cfg!(feature = "ic705"))
        || (d.source == Source::Qmx && !cfg!(feature = "qmx"))
        || (d.source == Source::RtlSdr && !cfg!(feature = "rtl"))
    {
        warnings
            .push("Saved interface is unavailable in this build; selected synthetic input".into());
        d.source = Source::Synthetic;
    }
    if d.source == Source::RtlSdr {
        let mut tuning = d.tuning();
        tuning.tune(tuning.station_hz, 1, false);
        d.station_mhz = tuning.station_hz as f64 / 1e6;
        d.rtl_center_hz = tuning.center_hz;
    }
    d.cursor_hz = if d.source == Source::Synthetic {
        d.tone_hz
    } else {
        d.station_mhz * 1e6 - d.center_hz()
    };
}
fn snapshot(d: &Display, doc: &mut Document) {
    doc.put("devices.ic705", &d.ic705.settings);
    doc.put(
        "workspace",
        &Workspace {
            source: match d.source {
                Source::RtlSdr => "rtl",
                Source::File => "file",
                Source::Qmx => "qmx",
                Source::Ic705 => "ic705",
                Source::Synthetic => "synthetic",
            }
            .into(),
            mode: mode_key(d.receiver.mode).into(),
            station_mhz: d.station_mhz,
            volume: d.volume,
            muted: d.muted,
            floor: d.floor,
            ceiling: d.ceiling,
            zoom: d.zoom,
            controls_open: d.controls_open,
            debug_open: d.debug_open,
            signal_data_open: d.broadcast.open,
        },
    );
    for &(mut config, mut step) in d.mode_settings.entries() {
        if !d.source.is_audio_radio() && config.mode == d.receiver.mode {
            config = d.receiver;
            step = d.tune_step_hz;
        }
        doc.put(
            &format!("modes.{}", mode_key(config.mode)),
            &ReceiverPreferences::from(config, step),
        );
    }
    doc.put("devices.rtl.backend", &d.rtl_backend.key());
    doc.put("devices.rtl.center_hz", &d.rtl_center_hz);
    doc.put("devices.rtl.converter_offset_hz", &d.converter_offset_hz);
    doc.put("devices.rtl.correction_hz", &d.converter_correction_hz);
    doc.put(
        "devices.file",
        &FilePreferences {
            path: d.file_path.clone(),
            center_mhz: d.file_center_mhz,
        },
    );
    doc.put("devices.synthetic.tone_hz", &d.tone_hz);
    // Preserve the QMX tuning step when another interface is selected.
    let step = if d.source == Source::Qmx {
        d.tune_step_hz
    } else {
        d.qmx.remembered_step.unwrap_or(100)
    };
    doc.put(
        "devices.qmx",
        &QmxPreferences {
            port: d.qmx.settings.port.clone(),
            input: d.qmx.settings.input.clone(),
            output: d.qmx.settings.output.clone(),
            audio_gain_db: d.qmx.settings.audio_gain_db,
            step_hz: step,
        },
    );
    doc.put("decoders.rds", &d.broadcast.enabled);
    let c = d.broadcast.cw_config;
    doc.put(
        "decoders.cw",
        &CwPreferences {
            enabled: d.broadcast.cw_enabled,
            seed_wpm: c.seed_wpm,
            adaptive: c.adaptive,
            threshold_dbfs: c.threshold_dbfs,
            spacing: c.spacing,
        },
    );
    d.memories.save_preferences(doc);
    doc.put("decoders.ft", &d.digital.preferences);
    d.practice.save_preferences(doc);
}
pub(super) fn save(
    display: Res<Display>,
    windows: Query<&Window>,
    persistence: Option<ResMut<Persistence>>,
) {
    if let Some(mut p) = persistence {
        let mut doc = p.store.document.clone();
        snapshot(&display, &mut doc);
        if let Ok(window) = windows.single()
            && window.width() >= 480.0
            && window.height() >= 400.0
        {
            doc.put(
                "window",
                &WindowPreferences {
                    width: window.width().round() as u32,
                    height: window.height().round() as u32,
                },
            );
        }
        p.store.save(doc);
    }
}

#[derive(Resource)]
pub(super) struct Smoke {
    write: bool,
    stage: u8,
    started: std::time::Instant,
}
impl Smoke {
    pub fn new(write: bool) -> Self {
        Self {
            write,
            stage: 0,
            started: std::time::Instant::now(),
        }
    }
}
pub(super) fn smoke(
    mut smoke: ResMut<Smoke>,
    mut display: ResMut<Display>,
    mut windows: Query<&mut Window>,
    persistence: Res<Persistence>,
    mut exit: MessageWriter<AppExit>,
) {
    assert!(
        persistence.warning().is_none(),
        "{:?}",
        persistence.warning()
    );
    if smoke.stage == 0 {
        assert!(!display.running, "Restoration must not start reception");
        if smoke.write {
            display.source = Source::Synthetic;
            display.tone_hz = -125_000.0;
            display.receiver = ReceiverConfig::for_mode(Mode::Cw);
            display.receiver.bandwidth_hz = 200;
            display.tune_step_hz = 25;
            display.volume = 0.31;
            display.muted = true;
            display.floor = -117.0;
            display.zoom = 4.0;
            display.memories.parameters.hold_ms = 12_000;
            display.broadcast.cw_config.seed_wpm = 31.0;
            windows.single_mut().unwrap().resolution.set(1100.0, 760.0);
        } else {
            assert_eq!(display.source, Source::Synthetic);
            assert_eq!(display.tone_hz, -125_000.0);
            assert_eq!(display.receiver.mode, Mode::Cw);
            assert_eq!(display.receiver.bandwidth_hz, 200);
            assert_eq!(display.tune_step_hz, 25);
            assert_eq!(display.volume, 0.31);
            assert!(display.muted);
            assert_eq!(display.floor, -117.0);
            assert_eq!(display.zoom, 4.0);
            assert_eq!(display.memories.parameters.hold_ms, 12_000);
            assert_eq!(display.broadcast.cw_config.seed_wpm, 31.0);
            assert_eq!(windows.single().unwrap().width(), 1100.0);
            assert_eq!(windows.single().unwrap().height(), 760.0);
            info!("Settings smoke: restarted workspace restored; reception stopped");
        }
        smoke.stage = 1;
    }
    if smoke.started.elapsed().as_secs_f32() > 3.0 {
        info!("Settings smoke complete (write={})", smoke.write);
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn ic705_settings_restore_without_transmit_or_tuning_actions() {
        let mut d = Display {
            source: Source::Ic705,
            ..Default::default()
        };
        d.ic705.settings.port = "COM19".into();
        d.ic705.settings.tx_output = "Speakers (USB Audio CODEC )".into();
        d.ic705.settings.command = Some((
            1,
            sdr_devices::ic705::Command::TestTone {
                frequency_hz: 14_074_000,
                power_percent: 0,
            },
        ));
        let mut doc = Document::default();
        snapshot(&d, &mut doc);
        let mut next = Display::default();
        restore(&mut next, &doc, &mut Vec::new());
        assert_eq!(next.ic705.settings.port, "COM19");
        assert_eq!(next.ic705.settings.tx_output, d.ic705.settings.tx_output);
        assert!(next.ic705.settings.command.is_none());
        assert!(!next.running);
    }
    #[cfg(feature = "rtl-rust")]
    #[test]
    fn rtl_backend_round_trip_keeps_converter_and_receiver_settings() {
        let d = Display {
            rtl_backend: RtlBackend::Rust,
            source: Source::RtlSdr,
            converter_offset_hz: -125_000_000,
            converter_correction_hz: 2700,
            station_mhz: 7.074,
            rtl_center_hz: 7_324_000,
            receiver: ReceiverConfig::for_mode(Mode::Usb),
            ..Default::default()
        };
        let mut doc = Document::default();
        snapshot(&d, &mut doc);
        let mut restored = Display::default();
        let mut warnings = Vec::new();
        restore(&mut restored, &doc, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(restored.rtl_backend, RtlBackend::Rust);
        assert_eq!(restored.settings().rtl_backend, RtlBackend::Rust);
        assert_eq!(restored.settings().tuner_center_hz(), Some(132_321_300));
        assert_eq!(restored.receiver.mode, Mode::Usb);
        doc.put("devices.rtl.backend", &"unknown-driver");
        let mut fallback = Display::default();
        restore(&mut fallback, &doc, &mut warnings);
        assert_eq!(fallback.rtl_backend, RtlBackend::Rust);
        assert!(!warnings.is_empty());
        doc.put("devices.rtl.backend", &"librtlsdr");
        restore(&mut fallback, &doc, &mut Vec::new());
        assert_eq!(
            fallback.rtl_backend,
            RtlBackend::Librtlsdr,
            "Preserve explicit saved choice"
        );
        let mut fresh = Display::default();
        restore(&mut fresh, &Document::default(), &mut Vec::new());
        assert_eq!(
            fresh.rtl_backend,
            RtlBackend::Rust,
            "Older settings without a backend use the new default"
        );
    }
    #[cfg(feature = "rtl")]
    #[test]
    fn restores_hf_converter_before_validating_rf_tuning() {
        let d = Display {
            source: Source::RtlSdr,
            converter_offset_hz: -125_000_000,
            converter_correction_hz: 1900,
            station_mhz: 7.1,
            rtl_center_hz: 7_350_000,
            ..Default::default()
        };
        let mut doc = Document::default();
        snapshot(&d, &mut doc);
        let mut restored = Display::default();
        let mut warnings = Vec::new();
        restore(&mut restored, &doc, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(restored.converter_offset_hz, -125_000_000);
        assert_eq!(restored.converter_correction_hz, 1900);
        assert_eq!(restored.station_mhz, 7.1);
        assert_eq!(restored.rtl_center_hz, 7_350_000);
        assert_eq!(restored.settings().tuner_center_hz(), Some(132_348_100));
    }
    #[test]
    fn memory_selection_waits_for_csv_load_and_is_clamped() {
        let path =
            std::env::temp_dir().join(format!("rustsdr-memory-restore-{}.csv", std::process::id()));
        let library = sdr_app::memories::Library {
            version: 1,
            groups: vec![sdr_app::memories::MemoryGroup {
                name: "Test".into(),
                entries: vec![sdr_app::memories::Memory {
                    name: "Test channel".into(),
                    frequency_hz: 89_900_000,
                    receiver: ReceiverConfig::default(),
                    scan_enabled: true,
                }],
            }],
        };
        library.save(&path).unwrap();
        let mut doc = Document::default();
        doc.put("memories",&json!({"path":path.to_string_lossy(),"transfer_path":"","open":true,"advanced":false,"filter":"Test","group":999,"selected":999,"memory_mode":true}));
        let mut display = Display::default();
        restore(&mut display, &doc, &mut Vec::new());
        assert_eq!(display.memories.group, 0);
        assert!(!display.memories.memory_mode);
        display.memories.load_startup();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !display.memories.memory_mode && std::time::Instant::now() < deadline {
            display.memories.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(display.memories.memory_mode);
        assert_eq!(display.memories.group, 0);
        assert_eq!(display.memories.selected, 0);
        assert_eq!(display.memories.library, library);
        assert!(!display.memories.take_recall());
        assert!(!display.running);
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(feature = "rtl")]
    #[test]
    fn session_round_trip_keeps_independent_modes_and_panels_but_not_actions() {
        let mut d = Display {
            source: Source::RtlSdr,
            station_mhz: 89.9,
            rtl_center_hz: 90_150_000,
            volume: 0.37,
            muted: true,
            floor: -110.0,
            ceiling: -8.0,
            zoom: 4.0,
            debug_open: true,
            ..Default::default()
        };
        for (index, mode) in Mode::ALL.into_iter().enumerate() {
            let mut config = ReceiverConfig::for_mode(mode);
            config.squelch_db = Some(-50.0 - index as f32);
            d.mode_settings.restore(config, 100 + index as u32);
        }
        d.receiver = ReceiverConfig::for_mode(Mode::Nfm);
        d.receiver.squelch_db = Some(-67.0);
        d.tune_step_hz = 6250;
        d.broadcast.cw_config.seed_wpm = 27.0;
        d.broadcast.enabled = false;
        d.qmx.settings.port = "COM18".into();
        d.qmx.settings.input = "QMX".into();
        d.qmx.settings.output = "Speakers".into();
        d.qmx.settings.audio_gain_db = 23;
        d.qmx.settings.command = Some((
            4,
            sdr_devices::qmx::Command::Tune {
                frequency_hz: 14_055_000,
                mode: 3,
            },
        ));
        let mut saved = Document::default();
        snapshot(&d, &mut saved);
        saved.put("practice",&json!({"open":true,"mode":"iambic_b","wpm":29.0,"pitch_hz":750.0,"volume":0.25,"midi_port_id":"device-123"}));
        saved.put("practice.bindings",&json!([{"Key":"Space"},{"Mouse":"Middle"},{"Gamepad":[{"name":"Test paddle","vendor":123,"product":456},"South"]}]));
        let mut warnings = Vec::new();
        let mut next = Display::default();
        restore(&mut next, &saved, &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!next.running);
        assert!(next.scan_plan.is_none());
        assert!(next.qmx.settings.command.is_none());
        assert_eq!(next.receiver, d.receiver);
        assert_eq!(next.tune_step_hz, 6250);
        let mut roundtrip = Document::default();
        snapshot(&next, &mut roundtrip);
        assert_eq!(roundtrip, saved);
        for mode in Mode::ALL {
            if mode != Mode::Nfm {
                assert_eq!(
                    next.mode_settings
                        .entries()
                        .iter()
                        .find(|(c, _)| c.mode == mode),
                    d.mode_settings
                        .entries()
                        .iter()
                        .find(|(c, _)| c.mode == mode)
                );
            }
        }
    }
    #[test]
    fn malformed_mode_does_not_discard_other_sections() {
        let mut doc = Document::default();
        snapshot(&Display::default(), &mut doc);
        doc.put("modes.am", &json!({"bandwidth_hz":3}));
        doc.put("decoders.rds", &false);
        let mut d = Display::default();
        let mut warnings = Vec::new();
        restore(&mut d, &doc, &mut warnings);
        assert!(!d.broadcast.enabled);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("modes.am"));
        assert_eq!(
            d.mode_settings.entries()[2].0,
            ReceiverConfig::for_mode(Mode::Am)
        );
    }
    #[cfg(feature = "qmx")]
    #[test]
    fn qmx_display_state_does_not_overwrite_software_receiver_preferences() {
        let mut d = Display {
            source: Source::Qmx,
            receiver: ReceiverConfig::for_mode(Mode::Cw),
            tune_step_hz: 50,
            ..Default::default()
        };
        let mut cw = ReceiverConfig::for_mode(Mode::Cw);
        cw.bandwidth_hz = 200;
        d.mode_settings.restore(cw, 10);
        let mut doc = Document::default();
        snapshot(&d, &mut doc);
        assert_eq!(
            doc.get::<Value>("modes.cw").unwrap().unwrap()["bandwidth_hz"],
            200
        );
        let mut next = Display::default();
        restore(&mut next, &doc, &mut Vec::new());
        assert_eq!(next.tune_step_hz, 50);
        assert_eq!(next.source, Source::Qmx);
        assert!(!next.running);
    }
}
