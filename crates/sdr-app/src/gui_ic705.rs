use super::*;
use sdr_devices::ic705::Command;
#[derive(Default)]
pub(super) struct Panel {
    pub settings: sdr_app::engine::Ic705Settings,
    inputs: Vec<String>,
    outputs: Vec<String>,
    error: Option<String>,
    sequence: u64,
    test_confirmed: bool,
    power_percent: u8,
}
impl Panel {
    pub fn refresh(&mut self) {
        #[cfg(feature = "audio")]
        match (
            sdr_app::audio::device_names(true),
            sdr_app::audio::device_names(false),
        ) {
            (Ok(inputs), Ok(outputs)) => {
                self.inputs = inputs;
                self.outputs = outputs;
                self.error = None;
            }
            (Err(e), _) | (_, Err(e)) => self.error = Some(e),
        }
    }
    fn command(&mut self, command: Command) {
        self.sequence += 1;
        self.settings.command = Some((self.sequence, command));
    }
}
pub(super) fn header(ui: &mut egui::Ui, d: &mut Display) -> bool {
    let Some(state) = d.status.ic705.clone() else {
        ui.label("Connect to read radio tuning");
        return false;
    };
    let mut changed = false;
    ui.add_enabled_ui(
        d.running && d.status.running && !d.pending && !state.transmitting,
        |ui| {
            let mut hz = state.frequency_hz;
            let mut mode = state.mode;
            let mut data = state.data_mode;
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(AMBER);
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut hz)
                            .range(30_000..=470_000_000)
                            .speed(d.tune_step_hz)
                            .suffix(" Hz"),
                    )
                    .changed();
            });
            egui::ComboBox::from_id_salt("ic705-mode")
                .selected_text(
                    sdr_devices::ic705::MODES
                        .iter()
                        .find(|v| v.0 == mode)
                        .map_or("Other", |v| v.1),
                )
                .show_ui(ui, |ui| {
                    for &(code, name) in sdr_devices::ic705::MODES {
                        changed |= ui.selectable_value(&mut mode, code, name).changed();
                    }
                });
            if matches!(mode, 0 | 1 | 2 | 5) {
                changed |= ui.checkbox(&mut data, "DATA").changed();
            } else {
                data = false;
            }
            if changed {
                d.ic705.command(Command::Tune {
                    frequency_hz: hz,
                    mode,
                    data,
                });
            }
        },
    );
    changed
}
pub(super) fn controls(ui: &mut egui::Ui, d: &mut Display) {
    ui.label("IC-705 USB audio / CI-V");
    if d.status.running && d.status.ic705.is_some() {
        ui.small(format!(
            "USB input: {:.1} dBFS",
            20.0 * d.status.audio_rms.max(1e-9).log10()
        ));
        if cfg!(windows) {
            ui.small("Unprocessed radio audio");
        }
    }
    ui.add_enabled_ui(!d.running, |ui| {
        ui.label("CI-V port (USB A)");
        ui.text_edit_singleline(&mut d.ic705.settings.port);
        ui.horizontal(|ui| {
            ui.label("Baud");
            ui.add(egui::DragValue::new(&mut d.ic705.settings.baud).range(4800..=115200));
        });
        ui.horizontal(|ui| {
            ui.label("CI-V address (decimal; A4 = 164)");
            ui.add(egui::DragValue::new(&mut d.ic705.settings.address).range(1..=223));
        });
        if ui.button("Refresh audio devices").clicked() {
            d.ic705.refresh();
        }
        for (id, label, selected, options) in [
            (
                "705-in",
                "Radio input",
                &mut d.ic705.settings.input,
                &d.ic705.inputs,
            ),
            (
                "705-monitor",
                "Monitor speakers",
                &mut d.ic705.settings.output,
                &d.ic705.outputs,
            ),
            (
                "705-tx",
                "Radio transmit output",
                &mut d.ic705.settings.tx_output,
                &d.ic705.outputs,
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
    if let Some(error) = &d.ic705.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    egui::ComboBox::from_id_salt("705-step")
        .selected_text(format!("{} Hz step", d.tune_step_hz))
        .show_ui(ui, |ui| {
            for step in [1, 10, 100, 500, 1000, 5000, 12500, 25000] {
                ui.selectable_value(&mut d.tune_step_hz, step, format!("{step} Hz"));
            }
        });
    ui.small("Radio performs demodulation. Select AF USB output. Wheel / arrows tune the VFO.");
    ui.separator();
    ui.label("USB transmit test");
    ui.small("3-second 1 kHz tone. Requires USB/LSB DATA, DATA MOD=USB and simplex. Power is a radio percentage, not calibrated watts.");
    ui.add(egui::Slider::new(&mut d.ic705.power_percent, 0..=10).text("RF power %"));
    ui.checkbox(
        &mut d.ic705.test_confirmed,
        "Dummy load connected; ready to test",
    );
    let ready = d.running && d.status.running && !d.pending;
    if ui
        .add_enabled(
            ready
                && d.ic705.test_confirmed
                && d.status.ic705.as_ref().is_some_and(|s| !s.transmitting),
            egui::Button::new("Send 3-second test tone"),
        )
        .clicked()
    {
        let hz = d.status.station_hz;
        d.ic705.command(Command::TestTone {
            frequency_hz: hz,
            power_percent: d.ic705.power_percent,
        });
        d.ic705.test_confirmed = false;
        d.revision += 1;
        d.pending = true;
    }
    if ui
        .add_enabled(d.running, egui::Button::new("Stop test TX"))
        .clicked()
    {
        d.ic705.command(Command::StopTx);
        d.ic705.test_confirmed = false;
        d.revision += 1;
        d.pending = true;
    }
    ui.small(
        "TX never resumes on reconnect. If USB control is lost during TX, release TX on the radio.",
    );
    ui.add(egui::Slider::new(&mut d.floor, -140.0..=-30.0).text("Plot floor"));
    ui.add(egui::Slider::new(&mut d.ceiling, -20.0..=0.0).text("Plot ceiling"));
}
pub(super) fn step(d: &mut Display, steps: i64) {
    let Some(state) = d.status.ic705.clone() else {
        return;
    };
    if !d.running || !d.status.running || d.pending || state.transmitting {
        return;
    }
    let hz = (i64::from(state.frequency_hz) + steps * i64::from(d.tune_step_hz))
        .clamp(30_000, 470_000_000) as u32;
    d.ic705.command(Command::Tune {
        frequency_hz: hz,
        mode: state.mode,
        data: state.data_mode,
    });
    d.revision += 1;
    d.pending = true;
}

/// Live receive-only smoke; startup parser disables preferences for smoke runs.
pub(super) fn smoke(
    d: Res<Display>,
    time: Res<Time>,
    mut captured: Local<Option<f32>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if captured.is_none()
        && time.elapsed_secs() > 4.0
        && d.source == Source::Ic705
        && d.status.running
        && d.status.samples > 48000
    {
        assert!(d.status.ic705.as_ref().is_some_and(|s| !s.transmitting));
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("target/ic705-receive.png"));
        *captured = Some(time.elapsed_secs());
        info!("IC-705 GUI received {} audio frames", d.status.samples);
    }
    if captured.is_some_and(|t| time.elapsed_secs() > t + 1.0) {
        exit.write(AppExit::Success);
    }
    assert!(
        time.elapsed_secs() < 20.0,
        "IC-705 receive smoke failed: {:?}",
        d.status.error
    );
}
