use super::*;
use sdr_devices::qmx::Command as CatCommand;

#[derive(Default)]
pub(super) struct Panel {
    pub settings: sdr_app::engine::QmxSettings,
    pub remembered_step: Option<u32>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    error: Option<String>,
    sequence: u64,
}
impl Panel {
    pub fn refresh(&mut self) {
        #[cfg(feature = "audio")]
        {
            match (
                sdr_app::audio::device_names(true),
                sdr_app::audio::device_names(false),
            ) {
                (Ok(inputs), Ok(outputs)) => {
                    self.inputs = inputs;
                    self.outputs = outputs
                        .into_iter()
                        .filter(|n| !sdr_app::audio::is_radio_output(n))
                        .collect();
                    if self.settings.input.is_empty() {
                        self.settings.input = self
                            .inputs
                            .iter()
                            .find(|n| n.to_ascii_lowercase().contains("qmx"))
                            .cloned()
                            .unwrap_or_default();
                    }
                    self.error = None;
                }
                (Err(e), _) | (_, Err(e)) => self.error = Some(e),
            }
        }
    }
    fn command(&mut self, command: CatCommand) {
        self.sequence += 1;
        self.settings.command = Some((self.sequence, command));
    }
}
pub(super) fn header(ui: &mut egui::Ui, display: &mut Display) -> bool {
    let Some(state) = display.status.qmx.clone() else {
        ui.label("Connect to read tuning and bands");
        return false;
    };
    let ready = display.running
        && display.status.running
        && !display.pending
        && display.status.revision == display.revision;
    let mut changed = false;
    ui.add_enabled_ui(ready, |ui| {
        ui.label(if state.vfo_b { "VFO B" } else { "VFO A" });
        let mut band = state.band().map(|b| b.index);
        egui::ComboBox::from_id_salt("qmx-band")
            .selected_text(
                state
                    .band()
                    .map_or("Band".into(), |b| format!("{} m", b.meters)),
            )
            .show_ui(ui, |ui| {
                for choice in &state.bands {
                    ui.selectable_value(
                        &mut band,
                        Some(choice.index),
                        format!("{} m", choice.meters),
                    );
                }
            });
        if band != state.band().map(|b| b.index)
            && let Some(index) = band
        {
            display.qmx.command(CatCommand::Band(index));
            changed = true;
        }
        let mut mode = state.mode;
        egui::ComboBox::from_id_salt("qmx-mode")
            .selected_text(mode_name(mode))
            .show_ui(ui, |ui| {
                for &(choice, _) in sdr_devices::qmx::RECEIVE_MODES {
                    ui.selectable_value(&mut mode, choice, mode_name(choice));
                }
            });
        let mut frequency = state.frequency_hz;
        let edited = ui
            .scope(|ui| {
                ui.visuals_mut().override_text_color = Some(AMBER);
                ui.style_mut().override_font_id = Some(egui::FontId::monospace(20.0));
                ui.add(
                    egui::DragValue::new(&mut frequency)
                        .speed(display.tune_step_hz)
                        .range(1..=60_000_000)
                        .suffix(" Hz"),
                )
                .changed()
            })
            .inner;
        if edited || mode != state.mode {
            if state.bands.iter().any(|b| b.contains(frequency)) {
                display.qmx.command(CatCommand::Tune {
                    frequency_hz: frequency,
                    mode,
                });
                changed = true;
                display.command_error = None;
            } else {
                display.command_error =
                    Some("Outside the radio's configured receive ranges".into());
            }
        }
    });
    changed
}
fn mode_name(mode: u8) -> &'static str {
    sdr_devices::qmx::RECEIVE_MODES
        .iter()
        .find(|&&(code, _)| code == mode)
        .map_or("Unknown", |&(_, label)| label)
}
pub(super) fn controls(ui: &mut egui::Ui, display: &mut Display) {
    ui.label("48 kHz USB receive audio");
    ui.add_enabled_ui(!display.running, |ui| {
        ui.label("CAT port (for example COM18)");
        ui.text_edit_singleline(&mut display.qmx.settings.port);
        if ui.button("Refresh audio devices").clicked() {
            display.qmx.refresh();
        }
        for (id, label, selected, options) in [
            (
                "qmx-input",
                "Radio input",
                &mut display.qmx.settings.input,
                &display.qmx.inputs,
            ),
            (
                "qmx-output",
                "Monitor output",
                &mut display.qmx.settings.output,
                &display.qmx.outputs,
            ),
        ] {
            ui.label(label);
            egui::ComboBox::from_id_salt(id)
                .width(205.0)
                .selected_text(if selected.is_empty() {
                    "Select device"
                } else {
                    selected.as_str()
                })
                .show_ui(ui, |ui| {
                    for option in options {
                        ui.selectable_value(selected, option.clone(), option);
                    }
                });
        }
    });
    if let Some(error) = &display.qmx.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    ui.add_space(12.0);
    if ui
        .add(
            egui::Slider::new(&mut display.qmx.settings.audio_gain_db, 0..=60)
                .text("Audio boost dB"),
        )
        .changed()
    {
        display.revision += 1;
        display.pending = true;
    }
    egui::ComboBox::from_id_salt("qmx-step")
        .selected_text(format!("{} Hz step", display.tune_step_hz))
        .show_ui(ui, |ui| {
            for step in [1, 10, 50, 100, 500, 1000, 5000] {
                ui.selectable_value(&mut display.tune_step_hz, step, format!("{step} Hz"));
            }
        });
    ui.horizontal(|ui| {
        for (label, sign) in [("- step", -1), ("+ step", 1)] {
            if ui
                .add_enabled(
                    display.running && display.status.running,
                    egui::Button::new(label),
                )
                .clicked()
            {
                step(display, sign);
            }
        }
    });
    if let Some(state) = &display.status.qmx {
        ui.small(format!("Firmware {}", state.firmware));
        if let Some(band) = state.band() {
            ui.small(format!(
                "Configured RX: {:.3}–{:.3} MHz",
                band.min_hz as f64 / 1e6,
                band.max_hz as f64 / 1e6
            ));
        }
    }
    ui.small("Band selection recalls this session's frequency and mode; first visit uses the radio's band setting.\nWheel or Left/Right: tune VFO.\nRadio knobs control receive filters and RF/AF gain.");
    if ui
        .add(egui::Slider::new(&mut display.floor, -140.0..=-30.0).text("Plot floor"))
        .changed()
    {
        display.texture = None;
    }
    if ui
        .add(egui::Slider::new(&mut display.ceiling, -20.0..=0.0).text("Plot ceiling"))
        .changed()
    {
        display.texture = None;
    }
}
pub(super) fn step(display: &mut Display, steps: i64) {
    if display.source == Source::Ic705 {
        ic705::step(display, steps);
        return;
    }
    let Some(state) = &display.status.qmx else {
        return;
    };
    if !display.running || !display.status.running || display.pending {
        return;
    }
    let hz = (state.frequency_hz as i64 + steps * display.tune_step_hz as i64)
        .clamp(1, u32::MAX as i64) as u32;
    let Some(band) = state.band() else {
        return;
    };
    // Wheel/step stops at a gap. Use the band selector for deliberate band changes.
    if !band.contains(hz) {
        return;
    }
    display.qmx.command(CatCommand::Tune {
        frequency_hz: hz,
        mode: state.mode,
    });
    display.revision += 1;
    display.pending = true;
}
pub(super) fn plot(context: &egui::Context, display: &mut Display) {
    egui::CentralPanel::default().show(context, |ui| {
        ui.label("Receiver audio • 0–4 kHz • radio performs demodulation");
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), (ui.available_height() - 24.0) * 0.45),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(7, 13, 20));
        for k in 0..=4 {
            let x = rect.left() + rect.width() * k as f32 / 4.0;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, egui::Color32::from_gray(35)),
            );
            painter.text(
                egui::pos2(x, rect.bottom()),
                egui::Align2::LEFT_BOTTOM,
                format!("{k} kHz"),
                egui::FontId::monospace(12.0),
                egui::Color32::GRAY,
            );
        }
        let bins = FFT_SIZE * 4000 / 48000;
        let points = (0..bins)
            .map(|i| {
                let db = display.spectrum[FFT_SIZE / 2 + i];
                let level = ((db - display.floor) / (display.ceiling - display.floor).max(1.0))
                    .clamp(0.0, 1.0);
                egui::pos2(
                    rect.left() + rect.width() * i as f32 / bins as f32,
                    rect.bottom() - level * rect.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.3, CYAN)));
        ui.add_space(8.0);
        let (waterfall, waterfall_response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                (ui.available_height() - 30.0).max(40.0),
            ),
            egui::Sense::hover(),
        );
        if let Some(texture) = &display.texture {
            let split = display.next_row as f32 / HISTORY as f32;
            let middle = waterfall.top() + waterfall.height() * split;
            for (top, bottom, vtop, vbottom) in [
                (waterfall.top(), middle, split, 0.0),
                (middle, waterfall.bottom(), 1.0, split),
            ] {
                if bottom > top {
                    ui.painter().image(
                        texture.id(),
                        egui::Rect::from_min_max(
                            egui::pos2(waterfall.left(), top),
                            egui::pos2(waterfall.right(), bottom),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(0.5, vtop),
                            egui::pos2(0.5 + 4000.0 / 48000.0, vbottom),
                        ),
                        egui::Color32::WHITE,
                    );
                }
            }
        }
        digital::paint_labels(ui, waterfall, display);
        if response.hovered() || waterfall_response.hovered() {
            let scroll = context.input(|i| i.raw_scroll_delta.y);
            display.wheel_remainder += scroll / 40.0;
            let steps = display.wheel_remainder.trunc() as i64;
            display.wheel_remainder -= steps as f32;
            if steps != 0 {
                step(display, steps);
            }
        }
        ui.small("Audio spectrum, not a wideband RF view. Radio performs demodulation.");
    });
}

#[derive(Resource)]
pub(super) struct Smoke {
    started: std::time::Instant,
    stage: u8,
    original: u32,
}
impl Default for Smoke {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            stage: 0,
            original: 0,
        }
    }
}
pub(super) fn smoke(
    mut state: ResMut<Smoke>,
    display: Res<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let mut key = None;
    match state.stage {
        0 if display.status.running && display.status.samples > 48_000 => {
            state.original = display.status.station_hz;
            key = Some((egui::Key::ArrowRight, true));
            state.stage = 1;
        }
        1 => {
            key = Some((egui::Key::ArrowRight, false));
            state.stage = 2;
        }
        2 if display.status.station_hz == state.original + 100 => {
            key = Some((egui::Key::ArrowLeft, true));
            state.stage = 3;
        }
        3 => {
            key = Some((egui::Key::ArrowLeft, false));
            state.stage = 4;
        }
        4 if display.status.station_hz == state.original && display.last_sequence > 5 => {
            assert!(
                display
                    .status
                    .qmx
                    .as_ref()
                    .is_some_and(|s| !s.bands.is_empty())
            );
            assert_eq!(display.status.iq_drops, 0);
            commands
                .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                .observe(bevy::render::view::screenshot::save_to_disk(
                    "target/qmx-gui-smoke.png",
                ));
            state.stage = 5;
            state.started = std::time::Instant::now();
        }
        5 if state.started.elapsed().as_secs_f32() > 1.5 => {
            info!(
                "QMX GUI smoke passed: audio, band metadata, VFO arrow-key roundtrip and spectrum"
            );
            exit.write(AppExit::Success);
            state.stage = 6;
        }
        _ => {}
    }
    if let Some((key, pressed)) = key {
        for mut input in &mut inputs {
            input.events.push(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    if state.stage < 5 && state.started.elapsed().as_secs() > 35 {
        let failure = display.status.error.clone();
        error!(
            "QMX GUI smoke failed, stage {}, error {:?}",
            state.stage, failure
        );
        exit.write(AppExit::error());
    }
}
