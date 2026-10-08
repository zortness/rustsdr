//! Memory library editor and scan controls; radio timing lives in the engine.
use super::*;
use sdr_app::{
    memories::{Library, Memory, MemoryGroup},
    scanning::{ScanChannel, ScanParameters, ScanPlan, ScanTargets},
};
use std::{path::PathBuf, sync::Arc, thread::JoinHandle};

pub(super) struct Memories {
    stop_position: Option<egui::Pos2>,
    first_name_position: Option<egui::Pos2>,
    pub open: bool,
    pub memory_mode: bool,
    pub library: Library,
    pub group: usize,
    pub selected: usize,
    path: String,
    transfer_path: String,
    dirty: bool,
    message: String,
    advanced: bool,
    filter: String,
    filter_dirty: bool,
    visible: Vec<usize>,
    reveal_selected: bool,
    pub parameters: ScanParameters,
    start_mhz: f64,
    stop_mhz: f64,
    step_hz: u32,
    task: Option<FileTask>,
    recall_after_load: bool,
    restore_selection: Option<(usize, usize, bool)>,
}
enum FileOperation {
    Load,
    Import,
    Save,
    Export,
}
struct FileTask {
    operation: FileOperation,
    worker: Option<JoinHandle<Result<Library, String>>>,
}
impl Drop for FileTask {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Default for Memories {
    fn default() -> Self {
        Self {
            stop_position: None,
            first_name_position: None,
            open: false,
            memory_mode: false,
            library: Library::default(),
            group: 0,
            selected: 0,
            path: sdr_app::memories::default_path()
                .to_string_lossy()
                .into_owned(),
            transfer_path: String::new(),
            dirty: false,
            message: String::new(),
            advanced: false,
            filter: String::new(),
            filter_dirty: true,
            visible: Vec::new(),
            reveal_selected: false,
            parameters: ScanParameters::default(),
            start_mhz: 88.0,
            stop_mhz: 108.0,
            step_hz: 100_000,
            task: None,
            recall_after_load: false,
            restore_selection: None,
        }
    }
}
impl Memories {
    pub(super) fn restore_preferences(
        &mut self,
        doc: &sdr_app::preferences::Document,
        warnings: &mut Vec<String>,
    ) {
        if let Some(p) = settings::section::<Preferences>(doc, "memories", warnings) {
            self.path = p.path;
            self.transfer_path = p.transfer_path;
            self.open = p.open;
            self.advanced = p.advanced;
            self.filter = p.filter;
            self.restore_selection = Some((p.group, p.selected, p.memory_mode));
        }
        if let Some(p) = settings::section::<ScanPreferences>(doc, "scan", warnings) {
            let parameters = ScanParameters {
                settle_ms: p.settle_ms,
                dwell_ms: p.dwell_ms,
                hold_ms: p.hold_ms,
                threshold_db: p.threshold_db,
            };
            let valid = ScanPlan {
                parameters,
                start_index: 0,
                targets: ScanTargets::Range {
                    start_hz: (p.start_mhz * 1e6) as u32,
                    stop_hz: (p.stop_mhz * 1e6) as u32,
                    step_hz: p.step_hz,
                    receiver: ReceiverConfig::default(),
                },
            }
            .validate(1, 1_700_000_000)
            .is_ok();
            if valid
                && p.start_mhz.is_finite()
                && p.stop_mhz.is_finite()
                && p.start_mhz > 0.0
                && p.stop_mhz <= 1700.0
            {
                self.parameters = parameters;
                self.start_mhz = p.start_mhz;
                self.stop_mhz = p.stop_mhz;
                self.step_hz = p.step_hz;
            } else {
                warnings.push("Invalid scan settings; using defaults".into());
            }
        }
    }
    pub(super) fn save_preferences(&self, doc: &mut sdr_app::preferences::Document) {
        let (group, selected, memory_mode) =
            self.restore_selection
                .unwrap_or((self.group, self.selected, self.memory_mode));
        doc.put(
            "memories",
            &Preferences {
                path: self.path.clone(),
                transfer_path: self.transfer_path.clone(),
                open: self.open,
                advanced: self.advanced,
                filter: self.filter.clone(),
                group,
                selected,
                memory_mode,
            },
        );
        let p = self.parameters;
        doc.put(
            "scan",
            &ScanPreferences {
                settle_ms: p.settle_ms,
                dwell_ms: p.dwell_ms,
                hold_ms: p.hold_ms,
                threshold_db: p.threshold_db,
                start_mhz: self.start_mhz,
                stop_mhz: self.stop_mhz,
                step_hz: self.step_hz,
            },
        );
    }
    pub fn load_startup(&mut self) {
        if PathBuf::from(&self.path).exists() {
            self.file_operation(FileOperation::Load);
        } else {
            self.restore_selection = None;
            self.group = 0;
            self.selected = 0;
            self.memory_mode = false;
        }
    }
    fn file_operation(&mut self, operation: FileOperation) {
        if self.task.is_some() {
            return;
        }
        let path = match operation {
            FileOperation::Import | FileOperation::Export => &self.transfer_path,
            _ => &self.path,
        };
        if path.trim().is_empty() {
            self.message = "Enter a CSV file path first".into();
            return;
        }
        let path = PathBuf::from(path);
        let library = if matches!(operation, FileOperation::Export) {
            Library {
                version: 1,
                groups: self
                    .library
                    .groups
                    .get(self.group)
                    .cloned()
                    .into_iter()
                    .collect(),
            }
        } else {
            self.library.clone()
        };
        let save = matches!(operation, FileOperation::Save | FileOperation::Export);
        match std::thread::Builder::new()
            .name("memory-file-io".into())
            .spawn(move || {
                if save {
                    library.save(&path)?;
                    Ok(library)
                } else {
                    Library::load(&path)
                }
            }) {
            Ok(worker) => {
                self.task = Some(FileTask {
                    operation,
                    worker: Some(worker),
                });
                self.message = "Working…".into();
            }
            Err(error) => self.message = error.to_string(),
        }
    }
    pub fn poll(&mut self) {
        if !self.task.as_ref().is_some_and(|task| {
            task.worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        }) {
            return;
        }
        let mut task = self.task.take().unwrap();
        match task
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap_or_else(|_| Err("Memory file worker failed".into()))
        {
            Err(error) => {
                self.message = error;
                self.restore_selection = None;
            }
            Ok(mut library) => {
                match task.operation {
                    FileOperation::Load => {
                        self.library = library;
                        self.group = 0;
                        self.selected = 0;
                        self.dirty = false;
                        self.memory_mode = false;
                        if let Some((group, selected, memory_mode)) = self.restore_selection.take()
                        {
                            self.group = group.min(self.library.groups.len().saturating_sub(1));
                            self.selected = selected.min(
                                self.library
                                    .groups
                                    .get(self.group)
                                    .map_or(0, |g| g.entries.len().saturating_sub(1)),
                            );
                            self.memory_mode = memory_mode
                                && self
                                    .library
                                    .groups
                                    .get(self.group)
                                    .is_some_and(|g| !g.entries.is_empty());
                        }
                    }
                    FileOperation::Import => {
                        let mut combined = self.library.clone();
                        let selected_group = combined.groups.len();
                        for mut group in library.groups.drain(..) {
                            let base = group.name.clone();
                            let mut suffix = 2;
                            while combined.groups.iter().any(|g| g.name == group.name) {
                                group.name = format!("{base} ({suffix})");
                                suffix += 1;
                            }
                            combined.groups.push(group);
                        }
                        if let Err(error) = combined.validate() {
                            self.message = error;
                            return;
                        }
                        self.library = combined;
                        self.group =
                            selected_group.min(self.library.groups.len().saturating_sub(1));
                        self.selected = 0;
                        self.dirty = true;
                        self.recall_after_load = self.memory_mode;
                    }
                    FileOperation::Save => self.dirty = false,
                    FileOperation::Export => {}
                }
                if self.library.groups.is_empty() {
                    self.library = Library::default();
                }
                self.filter_dirty = true;
                self.visible.clear();
                self.message = "CSV operation completed".into();
            }
        }
    }
    fn invalidate(&mut self) {
        self.dirty = true;
        self.filter_dirty = true;
        self.visible.clear();
    }
    pub fn take_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall_after_load)
    }
    fn refresh_filter(&mut self) {
        if self.filter_dirty {
            let needle = self.filter.to_lowercase();
            self.visible = self
                .library
                .groups
                .get(self.group)
                .map_or_else(Vec::new, |group| {
                    group
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            needle.is_empty()
                                || entry.name.to_lowercase().contains(&needle)
                                || entry.frequency_hz.to_string().contains(&needle)
                                || entry.receiver.mode.label().to_lowercase().contains(&needle)
                        })
                        .map(|(index, _)| index)
                        .collect()
                });
            self.filter_dirty = false;
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    path: String,
    transfer_path: String,
    open: bool,
    advanced: bool,
    filter: String,
    group: usize,
    selected: usize,
    memory_mode: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct ScanPreferences {
    settle_ms: u32,
    dwell_ms: u32,
    hold_ms: u32,
    threshold_db: f32,
    start_mhz: f64,
    stop_mhz: f64,
    step_hz: u32,
}

impl Display {
    pub(super) fn recall_memory(&mut self, index: usize) {
        let Some(entry) = self
            .memories
            .library
            .groups
            .get(self.memories.group)
            .and_then(|group| group.entries.get(index))
            .cloned()
        else {
            return;
        };
        let bounds = sdr_app::engine::scan_bounds(&self.settings());
        self.memories.memory_mode = true;
        self.memories.selected = index;
        self.memories.reveal_selected = true;
        if self.source == Source::Synthetic || !(bounds.0..=bounds.1).contains(&entry.frequency_hz)
        {
            self.memories.message = "This memory is outside the selected source's usable range. Select RTL-SDR or a suitable recording.".into();
            self.command_error = Some(self.memories.message.clone());
            if self.scan_plan.is_some() {
                self.stop_scan_requested = true;
            }
            return;
        }
        self.scan_plan = None;
        self.stop_scan_requested = false;
        self.memories.memory_mode = true;
        self.memories.selected = index;
        self.receiver = entry.receiver;
        let revision = self.revision;
        let mut tuning = self.tuning();
        tuning.tune(entry.frequency_hz, 1, self.source == Source::File);
        self.apply_tuning(tuning);
        if self.revision == revision {
            self.revision += 1;
            self.pending = true;
        }
    }
    pub(super) fn memory_step(&mut self, steps: i64) {
        if let Some(index) = self
            .memories
            .library
            .groups
            .get(self.memories.group)
            .and_then(|group| group.step_index(self.memories.selected, steps))
        {
            self.recall_memory(index);
        }
    }
    fn start_scan(&mut self) {
        if self.source == Source::Synthetic {
            self.memories.message = "Select RTL-SDR or an IQ recording before scanning".into();
            return;
        }
        let (targets, start_index) = if self.memories.memory_mode {
            let channels: Vec<_> = self.memories.library.groups[self.memories.group]
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.scan_enabled)
                .map(|(index, entry)| ScanChannel {
                    frequency_hz: entry.frequency_hz,
                    receiver: entry.receiver,
                    memory_index: Some(index),
                })
                .collect();
            let index = channels
                .iter()
                .position(|channel| channel.memory_index == Some(self.memories.selected))
                .unwrap_or(0);
            (ScanTargets::Memories(channels.into()), index)
        } else {
            let start_hz = (self.memories.start_mhz * 1e6).round() as u32;
            let stop_hz = (self.memories.stop_mhz * 1e6).round() as u32;
            let station = (self.station_mhz * 1e6).round() as u32;
            let step_hz = self.memories.step_hz;
            let index = if (start_hz..=stop_hz).contains(&station) && step_hz > 0 {
                ((station - start_hz) / step_hz) as usize
            } else {
                0
            };
            (
                ScanTargets::Range {
                    start_hz,
                    stop_hz,
                    step_hz,
                    receiver: self.receiver,
                },
                index,
            )
        };
        let plan = Arc::new(ScanPlan {
            targets,
            parameters: self.memories.parameters,
            start_index,
        });
        let (min, max) = sdr_app::engine::scan_bounds(&self.settings());
        if let Err(error) = plan.validate(min, max) {
            self.memories.message = error;
            return;
        }
        self.scan_plan = Some(plan);
        self.stop_scan_requested = false;
        if !self.running {
            self.reset();
        }
        self.running = true;
        self.revision += 1;
        self.pending = true;
        self.memories.message.clear();
    }
}

pub(super) fn header(ui: &mut egui::Ui, display: &mut Display) {
    if ui
        .add_enabled(
            display.source != Source::Synthetic && display.memories.task.is_none(),
            egui::Button::new(if display.memories.memory_mode {
                "Memory"
            } else {
                "VFO"
            })
            .selected(display.memories.memory_mode),
        )
        .on_hover_text("Toggle VFO / Memory tuning")
        .clicked()
    {
        if display.memories.memory_mode {
            display.memories.memory_mode = false;
            display.stop_scan_requested = display.scan_plan.is_some();
        } else if display.memories.library.groups[display.memories.group]
            .entries
            .is_empty()
        {
            display.memories.open = true;
            display.memories.message = "Add or load memories before entering Memory mode".into();
        } else {
            display.recall_memory(display.memories.selected);
        }
    }
    let scanning = display.scan_plan.is_some();
    let scan = ui
        .add_enabled(
            display.source != Source::Synthetic && (scanning || display.memories.task.is_none()),
            egui::Button::new("Scan").selected(scanning),
        )
        .on_hover_text(if scanning {
            "Stop scanning and stay on this channel"
        } else {
            "Start scanning using the editor settings"
        });
    display.memories.stop_position = scanning.then_some(scan.rect.center());
    if scan.clicked() {
        if scanning {
            display.stop_scan_requested = true;
        } else {
            display.start_scan();
            if display.scan_plan.is_none() {
                display.memories.open = true;
            }
        }
    }
    if ui
        .selectable_label(display.memories.open, "Memories…")
        .clicked()
    {
        display.memories.open = !display.memories.open;
    }
    if display.scan_plan.is_some() {
        ui.small(format!(
            "{:?} · {} / {}",
            display.status.scan.phase,
            display.status.scan.index + 1,
            display.status.scan.count
        ));
    } else if display.memories.memory_mode {
        ui.label(format!("MEM {}", display.memories.selected + 1));
    }
}

#[derive(Resource)]
pub(super) struct MemorySmoke {
    started: std::time::Instant,
    stage: u8,
    held_hz: u32,
    marked: std::time::Instant,
}
impl Default for MemorySmoke {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            stage: 0,
            held_hz: 0,
            marked: std::time::Instant::now(),
        }
    }
}
pub(super) fn smoke(
    mut smoke: ResMut<MemorySmoke>,
    mut display: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use sdr_app::scanning::ScanPhase;
    let mut event = None;
    let stage = smoke.stage;
    match stage {
        0 if display.memories.task.is_none() && display.status.samples > 0 => {
            if display.source != Source::File
                || display.memories.library.groups[0].entries.len() < 3
            {
                error!("Memory smoke needs a recording and the three-entry CSV fixture");
                exit.write(AppExit::error());
                return;
            }
            display.memories.parameters = ScanParameters {
                settle_ms: 20,
                dwell_ms: 25,
                hold_ms: 500,
                threshold_db: -45.0,
            };
            display.recall_memory(0);
            smoke.stage = 1;
        }
        1 if display.status.station_hz == 106_700_000
            && display.status.mode == Mode::WfmStereo
            && display.status.revision == display.revision =>
        {
            event = Some(egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            smoke.stage = 2;
        }
        2 if display.status.station_hz == 106_800_000 && display.status.mode == Mode::Am => {
            if let Some(rect) = display.plot_rect {
                for mut input in &mut inputs {
                    input.events.push(egui::Event::PointerMoved(rect.center()));
                }
                event = Some(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, 1.0),
                    modifiers: egui::Modifiers::NONE,
                });
                smoke.stage = 3;
            }
        }
        3 if display.status.station_hz == 106_750_000 && display.status.mode == Mode::Usb => {
            // Release before another press so input does not depend on key repeat timing.
            event = Some(egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            smoke.stage = 4;
        }
        4 => {
            event = Some(egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            smoke.stage = 5;
        }
        5 if display.status.station_hz == 106_700_000 && display.status.mode == Mode::WfmStereo => {
            event = Some(egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            display.start_scan();
            smoke.stage = 6;
        }
        6 if display.status.scan.phase == ScanPhase::Holding => {
            smoke.held_hz = display.status.station_hz;
            if let Some(pos) = display.memories.stop_position {
                for mut input in &mut inputs {
                    input.events.push(egui::Event::PointerMoved(pos));
                }
                event = Some(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                smoke.stage = 7;
            }
        }
        7 => {
            if let Some(pos) = display.memories.stop_position {
                event = Some(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                smoke.stage = 8;
                smoke.marked = std::time::Instant::now();
            }
        }
        8 if display.scan_plan.is_none() && smoke.marked.elapsed().as_millis() > 700 => {
            assert_eq!(
                display.status.station_hz, smoke.held_hz,
                "stop scan did not hold the channel"
            );
            display.start_scan();
            smoke.stage = 9;
        }
        9 if display.status.scan.visit >= 2 => {
            assert_eq!(
                display.status.scan.count, 2,
                "unchecked memory was not excluded from scanning"
            );
            display.stop_scan_requested = true;
            smoke.stage = 10;
        }
        10 if display.scan_plan.is_none() => {
            display.memories.memory_mode = false;
            display.memories.start_mhz = 106.7;
            display.memories.stop_mhz = 106.9;
            display.memories.step_hz = 100_000;
            display.start_scan();
            smoke.stage = 11;
        }
        11 if display.status.scan.visit >= 3 => {
            display.memories.open = true;
            smoke.marked = std::time::Instant::now();
            smoke.stage = 12;
        }
        12 if smoke.marked.elapsed().as_millis() > 300 => {
            display.stop_scan_requested = true;
            smoke.marked = std::time::Instant::now();
            smoke.stage = 13;
        }
        13 if display.scan_plan.is_none() && smoke.marked.elapsed().as_millis() > 300 => {
            if let Some(pos) = display.memories.first_name_position {
                smoke.held_hz = display.status.station_hz;
                event = Some(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                smoke.stage = 14;
            }
        }
        14 => {
            event = display
                .memories
                .first_name_position
                .map(|pos| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            smoke.stage = 15;
        }
        15 => {
            event = Some(egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            });
            smoke.stage = 16;
        }
        16 => {
            event = Some(egui::Event::Text("Edited in table".into()));
            smoke.stage = 17;
        }
        17 => {
            event = Some(egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            smoke.stage = 18;
        }
        18 => {
            event = Some(egui::Event::Text("106.725000".into()));
            smoke.stage = 19;
        }
        19 => {
            event = Some(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            smoke.stage = 20;
        }
        20 => {
            let entry = &display.memories.library.groups[display.memories.group].entries[0];
            assert_eq!(entry.name, "Edited in table");
            assert_eq!(
                entry.frequency_hz, 106_725_000,
                "Tab should focus the frequency cell for editing"
            );
            assert_eq!(
                display.status.station_hz, smoke.held_hz,
                "cell editing must not retune"
            );
            display.memories.path = "target/memory-editor-result.csv".into();
            display.memories.file_operation(FileOperation::Save);
            smoke.stage = 21;
        }
        21 if display.memories.task.is_none() => {
            assert!(!display.memories.dirty, "{:?}", display.memories.message);
            let saved =
                Library::load(std::path::Path::new("target/memory-editor-result.csv")).unwrap();
            assert_eq!(
                saved.groups[display.memories.group].entries[0].frequency_hz,
                106_725_000
            );
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk("target/memories-scan-smoke.png"));
            smoke.marked = std::time::Instant::now();
            smoke.stage = 22;
        }
        22 if smoke.marked.elapsed().as_millis() > 700 => {
            info!(
                "Memory smoke passed: CSV load, arrow/wheel recall, scan pause/stop/restart, VFO scan, click/Tab cell editing without retuning, and CSV save round trip"
            );
            exit.write(AppExit::Success);
            smoke.stage = 23;
        }
        _ => {}
    }
    if smoke.stage != stage {
        info!("Memory smoke stage {}", smoke.stage);
    }
    if let Some(event) = event {
        for mut input in &mut inputs {
            input.events.push(event.clone());
        }
    }
    if smoke.stage < 23 && smoke.started.elapsed().as_secs() > 30 {
        let detail = format!(
            "Memory smoke timed out at stage {}, station {}, revision {}, scan {:?}, error {:?}",
            smoke.stage,
            display.status.station_hz,
            display.status.revision,
            display.status.scan,
            display.status.error
        );
        error!("{detail}");
        exit.write(AppExit::error());
    }
}

enum Action {
    None,
    Recall(usize),
    Store(bool),
    Add,
    Delete,
    Start,
    Stop,
}

pub(super) fn show(context: &egui::Context, display: &mut Display) {
    let mut open = display.memories.open;
    let scanning = display.scan_plan.is_some();
    let mut action = Action::None;
    egui::Window::new("Memories and scanning")
        .open(&mut open).default_width(1040.0).default_height(560.0)
        .default_pos(egui::pos2(230.0, 65.0))
        .max_height((context.content_rect().height() - 130.0).max(280.0))
        .min_width(520.0).resizable(true).show(context, |ui| {
        let memory = &mut display.memories;
        ui.horizontal(|ui| {
            ui.strong(if memory.memory_mode { "Memory scan" } else { "VFO range scan" });
            if ui.add_enabled(scanning || memory.task.is_none(), egui::Button::new(if scanning { "Stop scan · stay here" } else { "Start scan" }).selected(scanning)).clicked() {
                action = if scanning { Action::Stop } else { Action::Start };
            }
            ui.small(format!("{}  ·  {:?}  ·  {:.1} s", display.status.channel_power_dbfs.map_or("No signal level".into(), |p| format!("{p:.1} dBFS")), display.status.scan.phase, display.status.scan.remaining_ms as f64 / 1000.0));
        });
        ui.add_enabled_ui(!scanning, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut memory.parameters.threshold_db).range(-120.0..=0.0).prefix("Threshold ").suffix(" dBFS"));
                let mut hold = memory.parameters.hold_ms as f64 / 1000.0;
                if ui.add(egui::DragValue::new(&mut hold).range(0.1..=300.0).speed(0.1).prefix("Pause ").suffix(" s")).changed() { memory.parameters.hold_ms = (hold * 1000.0).round() as u32; }
                ui.add(egui::DragValue::new(&mut memory.parameters.settle_ms).range(20..=5000).prefix("Settle ").suffix(" ms"));
                ui.add(egui::DragValue::new(&mut memory.parameters.dwell_ms).range(25..=10000).prefix("Listen ").suffix(" ms"));
            });
            if !memory.memory_mode { ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut memory.start_mhz).range(0.000001..=1700.0).speed(0.1).fixed_decimals(6).prefix("Start ").suffix(" MHz"));
                ui.add(egui::DragValue::new(&mut memory.stop_mhz).range(0.000001..=1700.0).speed(0.1).fixed_decimals(6).prefix("Stop ").suffix(" MHz"));
                ui.add(egui::DragValue::new(&mut memory.step_hz).range(1..=1_700_000_000).prefix("Step ").suffix(" Hz"));
            }); }
        });
        ui.separator();
        ui.add_enabled_ui(memory.task.is_none() && !scanning, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Memories");
                if ui.add_enabled(!memory.dirty, egui::Button::new("New CSV")).on_hover_text("Save unsaved edits before starting a new library").clicked() {
                    memory.library = Library::default(); memory.group = 0; memory.selected = 0;
                    memory.path.clear(); memory.filter.clear(); memory.memory_mode = false; memory.invalidate();
                    memory.message = "New library: enter a CSV path, add rows, then Save".into();
                }
                ui.label("CSV path");
                ui.add(egui::TextEdit::singleline(&mut memory.path).desired_width(320.0).hint_text("C:\\path\\memories.csv"));
                if ui.add_enabled(!memory.dirty && !memory.path.trim().is_empty(), egui::Button::new("Load")).on_hover_text("Save unsaved edits before replacing the library").clicked() { memory.file_operation(FileOperation::Load); }
                if ui.add_enabled(!memory.path.trim().is_empty(), egui::Button::new("Save")).clicked() { memory.file_operation(FileOperation::Save); }
                if memory.dirty { ui.colored_label(AMBER, "Unsaved"); }
            });
            ui.collapsing("Import / export", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("CSV path"); ui.add(egui::TextEdit::singleline(&mut memory.transfer_path).desired_width(320.0));
                    if ui.add_enabled(!memory.transfer_path.trim().is_empty(), egui::Button::new("Import groups")).clicked() { memory.file_operation(FileOperation::Import); }
                    if ui.add_enabled(!memory.transfer_path.trim().is_empty(), egui::Button::new("Export group")).clicked() { memory.file_operation(FileOperation::Export); }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Group");
                let previous = memory.group;
                egui::ComboBox::from_id_salt("memory-group").selected_text(&memory.library.groups[memory.group].name).show_ui(ui, |ui| {
                    for (index, group) in memory.library.groups.iter().enumerate() { ui.selectable_value(&mut memory.group, index, &group.name); }
                });
                if previous != memory.group { memory.selected = 0; memory.filter_dirty = true; memory.visible.clear(); if memory.memory_mode { action = Action::Recall(0); } }
                if ui.add(egui::TextEdit::singleline(&mut memory.library.groups[memory.group].name).desired_width(180.0).hint_text("Group name")).changed() { memory.invalidate(); }
                if ui.add_enabled(memory.library.groups.len() < 1024, egui::Button::new("New group")).clicked() {
                    let mut n = memory.library.groups.len() + 1;
                    while memory.library.groups.iter().any(|group| group.name == format!("Group {n}")) { n += 1; }
                    memory.library.groups.push(MemoryGroup { name: format!("Group {n}"), entries: Vec::new() });
                    memory.group = memory.library.groups.len()-1; memory.selected = 0; memory.memory_mode = false; memory.invalidate();
                }
                ui.label(format!("{} rows", memory.library.groups[memory.group].entries.len()));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Add row").clicked() { action = Action::Add; }
                if ui.add_enabled(display.source != Source::Synthetic, egui::Button::new("Store current")).clicked() { action = Action::Store(false); }
                let has_row = !memory.library.groups[memory.group].entries.is_empty();
                if ui.add_enabled(has_row && display.source != Source::Synthetic, egui::Button::new("Update from VFO")).clicked() { action = Action::Store(true); }
                if ui.add_enabled(has_row, egui::Button::new("Delete row")).clicked() { action = Action::Delete; }
                if ui.add_enabled(has_row && display.source != Source::Synthetic, egui::Button::new("Tune selected")).clicked() { action = Action::Recall(memory.selected); }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Filter"); if ui.add(egui::TextEdit::singleline(&mut memory.filter).desired_width(220.0)).changed() { memory.filter_dirty = true; memory.refresh_filter(); }
            ui.checkbox(&mut memory.advanced, "More receiver columns");
            ui.small("Click cells to edit · Tab / Shift+Tab to move · double-click a row number to tune");
        });
        if !memory.message.is_empty() { ui.colored_label(AMBER, &memory.message); }
        memory_table(ui, memory, !scanning && memory.task.is_none(), &mut action);
        if scanning { ui.small("Stop scanning to edit memories. Scan uses a snapshot of the selected group."); }
    });
    display.memories.open = open;
    match action {
        Action::None => {}
        Action::Recall(index) => display.recall_memory(index),
        Action::Start => display.start_scan(),
        Action::Stop => {
            if scanning {
                display.stop_scan_requested = true;
            }
        }
        Action::Store(replace) => store_row(display, replace, false),
        Action::Add => store_row(display, false, true),
        Action::Delete => {
            let memory = &mut display.memories;
            let entries = &mut memory.library.groups[memory.group].entries;
            if memory.selected < entries.len() {
                entries.remove(memory.selected);
                memory.selected = memory.selected.min(entries.len().saturating_sub(1));
                memory.invalidate();
            }
        }
    }
}

fn store_row(display: &mut Display, replace: bool, blank: bool) {
    let memory = &mut display.memories;
    if !replace
        && memory
            .library
            .groups
            .iter()
            .map(|g| g.entries.len())
            .sum::<usize>()
            >= sdr_app::memories::MAX_ENTRIES
    {
        memory.message = "Library limit: 100,000 memories".into();
        return;
    }
    let entry = Memory {
        name: if replace {
            memory.library.groups[memory.group].entries[memory.selected]
                .name
                .clone()
        } else if blank {
            format!(
                "Channel {}",
                memory.library.groups[memory.group].entries.len() + 1
            )
        } else {
            format!("{:.6} MHz", display.station_mhz)
        },
        frequency_hz: (display.station_mhz * 1e6).round() as u32,
        receiver: display.receiver,
        scan_enabled: if replace {
            memory.library.groups[memory.group].entries[memory.selected].scan_enabled
        } else {
            true
        },
    };
    if let Err(error) = entry.validate() {
        memory.message = error;
        return;
    }
    let entries = &mut memory.library.groups[memory.group].entries;
    if replace {
        entries[memory.selected] = entry;
    } else {
        entries.push(entry);
        memory.selected = entries.len() - 1;
        memory.filter.clear();
        memory.reveal_selected = true;
    }
    memory.invalidate();
}

fn memory_table(ui: &mut egui::Ui, memory: &mut Memories, editable: bool, action: &mut Action) {
    // Keep the focused row stable while its text is being edited under a filter.
    if !ui.ctx().wants_keyboard_input() || memory.visible.is_empty() {
        memory.refresh_filter();
    }
    let advanced = memory.advanced;
    let mut widths = if advanced {
        vec![40., 40., 210., 130., 110., 90., 100., 55., 100., 100.]
    } else {
        vec![40., 40., 210., 130., 110., 90.]
    };
    let titles = [
        "Row",
        "Scan",
        "Name",
        "Frequency (MHz)",
        "Mode",
        "Width (Hz)",
        "Squelch (dBFS)",
        "AGC",
        "Deviation (Hz)",
        "De-emphasis (µs)",
    ];
    let base_width = widths.iter().sum::<f32>() + (widths.len() - 1) as f32 * 6.0;
    widths[2] += (ui.available_width() - base_width - 8.0).max(0.0);
    let table_width = widths.iter().sum::<f32>() + (widths.len() - 1) as f32 * 6.0;
    egui::ScrollArea::horizontal()
        .id_salt("memory-table-horizontal")
        .show(ui, |ui| {
            ui.set_min_width(table_width);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                for (title, width) in titles.iter().zip(&widths) {
                    ui.add_sized(
                        [*width, 25.0],
                        egui::Label::new(egui::RichText::new(*title).strong()),
                    );
                }
            });
            ui.separator();
            let height = ui.available_height().clamp(140.0, 350.0);
            let mut rows_scroll = egui::ScrollArea::vertical()
                .id_salt("memory-table-rows")
                .max_height(height)
                .auto_shrink([false, false]);
            if memory.reveal_selected {
                if let Some(row) = memory
                    .visible
                    .iter()
                    .position(|&index| index == memory.selected)
                {
                    rows_scroll = rows_scroll
                        .vertical_scroll_offset(row as f32 * (28.0 + ui.spacing().item_spacing.y));
                }
                memory.reveal_selected = false;
            }
            rows_scroll.show_rows(ui, 28.0, memory.visible.len(), |ui, rows| {
                for row in rows {
                    let index = memory.visible[row];
                    let entry = &mut memory.library.groups[memory.group].entries[index];
                    let mut edited = false;
                    ui.push_id((memory.group, index), |ui| {
                        let fill = if memory.selected == index {
                            egui::Color32::from_rgb(22, 49, 61)
                        } else if row % 2 == 0 {
                            egui::Color32::from_gray(28)
                        } else {
                            egui::Color32::from_gray(22)
                        };
                        egui::Frame::new().fill(fill).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let number = ui.add_sized(
                                    [widths[0], 28.0],
                                    egui::Button::new(format!("{}", index + 1))
                                        .selected(memory.selected == index),
                                );
                                if number.clicked() {
                                    memory.selected = index;
                                }
                                if number.double_clicked() {
                                    *action = Action::Recall(index);
                                }
                                let cells = ui.add_enabled_ui(editable, |ui| {
                                    edited |= ui
                                        .add_sized(
                                            [widths[1], 24.0],
                                            egui::Checkbox::without_text(&mut entry.scan_enabled),
                                        )
                                        .changed();
                                    let name_cell = ui.add_sized(
                                        [widths[2], 24.0],
                                        egui::TextEdit::singleline(&mut entry.name).id_salt("name"),
                                    );
                                    if row == 0 {
                                        memory.first_name_position = Some(name_cell.rect.center());
                                    }
                                    if name_cell.has_focus() || name_cell.clicked() {
                                        memory.selected = index;
                                    }
                                    edited |= name_cell.changed();
                                    let mut mhz = entry.frequency_hz as f64 / 1e6;
                                    if ui
                                        .add_sized(
                                            [widths[3], 24.0],
                                            egui::DragValue::new(&mut mhz)
                                                .range(0.000001..=1700.0)
                                                .speed(0.001)
                                                .fixed_decimals(6),
                                        )
                                        .changed()
                                    {
                                        entry.frequency_hz = (mhz * 1e6).round() as u32;
                                        edited = true;
                                    }
                                    let previous = entry.receiver.mode;
                                    ui.allocate_ui(egui::vec2(widths[4], 24.0), |ui| {
                                        egui::ComboBox::from_id_salt("mode")
                                            .width(widths[4])
                                            .selected_text(previous.label())
                                            .show_ui(ui, |ui| {
                                                for mode in Mode::ALL {
                                                    ui.selectable_value(
                                                        &mut entry.receiver.mode,
                                                        mode,
                                                        mode.label(),
                                                    );
                                                }
                                            });
                                    });
                                    if previous != entry.receiver.mode {
                                        entry.receiver =
                                            ReceiverConfig::for_mode(entry.receiver.mode);
                                        edited = true;
                                    }
                                    let choices: &[u32] = match entry.receiver.mode {
                                        Mode::Am => &[6000, 10000, 12000],
                                        Mode::Nfm => &[12500, 25000],
                                        Mode::Cw => &[200, 500, 1000],
                                        Mode::Usb | Mode::Lsb => &[1800, 2400, 2700],
                                        _ => &[200000],
                                    };
                                    ui.allocate_ui(egui::vec2(widths[5], 24.0), |ui| {
                                        egui::ComboBox::from_id_salt("width")
                                            .width(widths[5])
                                            .selected_text(entry.receiver.bandwidth_hz.to_string())
                                            .show_ui(ui, |ui| {
                                                for &width in choices {
                                                    edited |= ui
                                                        .selectable_value(
                                                            &mut entry.receiver.bandwidth_hz,
                                                            width,
                                                            width.to_string(),
                                                        )
                                                        .changed();
                                                }
                                            });
                                    });
                                    if advanced {
                                        ui.allocate_ui(egui::vec2(widths[6], 24.0), |ui| {
                                            let mut on = entry.receiver.squelch_db.is_some();
                                            if ui.checkbox(&mut on, "").changed() {
                                                entry.receiver.squelch_db = on.then_some(-60.0);
                                                edited = true;
                                            }
                                            if let Some(db) = &mut entry.receiver.squelch_db {
                                                edited |= ui
                                                    .add(
                                                        egui::DragValue::new(db)
                                                            .range(-120.0..=0.0),
                                                    )
                                                    .changed();
                                            }
                                        });
                                        edited |= ui
                                            .add_sized(
                                                [widths[7], 24.0],
                                                egui::Checkbox::without_text(
                                                    &mut entry.receiver.agc,
                                                ),
                                            )
                                            .changed();
                                        ui.allocate_ui(egui::vec2(widths[8], 24.0), |ui| {
                                            egui::ComboBox::from_id_salt("deviation")
                                                .width(widths[8])
                                                .selected_text(
                                                    entry.receiver.deviation_hz.to_string(),
                                                )
                                                .show_ui(ui, |ui| {
                                                    let choices: &[u32] =
                                                        if entry.receiver.mode == Mode::Nfm {
                                                            &[2500, 5000]
                                                        } else {
                                                            &[75000]
                                                        };
                                                    for &v in choices {
                                                        edited |= ui
                                                            .selectable_value(
                                                                &mut entry.receiver.deviation_hz,
                                                                v,
                                                                v.to_string(),
                                                            )
                                                            .changed();
                                                    }
                                                });
                                        });
                                        ui.allocate_ui(egui::vec2(widths[9], 24.0), |ui| {
                                            egui::ComboBox::from_id_salt("deemphasis")
                                                .width(widths[9])
                                                .selected_text(
                                                    entry.receiver.deemphasis_us.to_string(),
                                                )
                                                .show_ui(ui, |ui| {
                                                    for v in [0, 50, 75, 300, 750] {
                                                        edited |= ui
                                                            .selectable_value(
                                                                &mut entry.receiver.deemphasis_us,
                                                                v,
                                                                v.to_string(),
                                                            )
                                                            .changed();
                                                    }
                                                });
                                        });
                                    }
                                });
                                if cells.response.contains_pointer()
                                    && ui.input(|input| input.pointer.any_pressed())
                                {
                                    memory.selected = index;
                                }
                            });
                        });
                    });
                    if edited {
                        memory.dirty = true;
                        memory.filter_dirty = true;
                    }
                }
            });
        });
    if memory.library.groups[memory.group].entries.is_empty() {
        ui.label("No memories yet. Add a row, store the current VFO, or load a CSV.");
    }
}
