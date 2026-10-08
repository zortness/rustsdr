//! Broadcast service results stay separate from listening controls.
use super::*;
pub(super) struct BroadcastPanel {
    pub open: bool,
    pub enabled: bool,
    pub cw_enabled: bool,
    pub cw_config: sdr_dsp::cw::CwConfig,
    pub cw_reset: u64,
    cw_reset_button: Option<egui::Pos2>,
    pub button: Option<egui::Pos2>,
    toggle: Option<egui::Pos2>,
    ticker_text: String,
    ticker_started: f64,
}
impl Default for BroadcastPanel {
    fn default() -> Self {
        Self {
            open: false,
            enabled: true,
            cw_enabled: true,
            cw_config: sdr_dsp::cw::CwConfig::default(),
            cw_reset: 0,
            cw_reset_button: None,
            button: None,
            toggle: None,
            ticker_text: String::new(),
            ticker_started: 0.0,
        }
    }
}
pub(super) fn available(display: &Display) -> bool {
    display.source != Source::Synthetic
        && !display.source.is_audio_radio()
        && (display.receiver.mode.is_wfm() || display.receiver.mode == Mode::Cw)
}
/// Compact live metadata, independent of whether the details window is open.
pub(super) fn ticker(ui: &mut egui::Ui, display: &mut Display) {
    let current = available(display)
        && (if display.receiver.mode == Mode::Cw {
            display.broadcast.cw_enabled
        } else {
            display.broadcast.enabled
        })
        && display.running
        && display.status.running
        && !display.pending
        && display.status.revision == display.revision
        && display.status.station_hz == (display.station_mhz * 1e6).round() as u32;
    let text = if current && display.receiver.mode == Mode::Cw {
        display
            .status
            .cw
            .as_ref()
            .map(|cw| {
                let text = &cw.text[cw.text.len().saturating_sub(100)..];
                if text.is_empty() {
                    String::new()
                } else {
                    format!("CW  {text}")
                }
            })
            .unwrap_or_default()
    } else if current {
        display
            .status
            .rds
            .as_ref()
            .filter(|rds| rds.locked)
            .map(|rds| {
                [rds.program_service.as_deref(), rds.radio_text.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("   •   ")
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let now = ui.input(|input| input.time);
    if display.broadcast.ticker_text != text {
        display.broadcast.ticker_text = text;
        display.broadcast.ticker_started = now;
    }
    if display.broadcast.ticker_text.is_empty() {
        return;
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 25.0), egui::Sense::hover());
    let text = &display.broadcast.ticker_text;
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), egui::FontId::proportional(16.0), CYAN);
    let overflow = (galley.size().x - rect.width()).max(0.0);
    let elapsed = (now - display.broadcast.ticker_started).max(0.0) as f32;
    // Leave short messages still. Long messages pause at either end, then loop.
    let travel = overflow / 35.0;
    let offset = if overflow > 0.0 && !response.hovered() {
        ((elapsed % (travel + 5.0) - 2.5) * 35.0).clamp(0.0, overflow)
    } else {
        0.0
    };
    ui.painter_at(rect)
        .galley(rect.left_top() + egui::vec2(-offset, 3.0), galley, CYAN);
    response.on_hover_text(text);
    if overflow > 0.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}
pub(super) fn show(context: &egui::Context, display: &mut Display) -> bool {
    if !available(display) {
        display.broadcast.open = false;
        display.broadcast.toggle = None;
        return false;
    }
    let mut open = display.broadcast.open;
    let mut changed = false;
    if display.receiver.mode == Mode::Cw {
        egui::Window::new("CW Signal Data").open(&mut open).default_width(520.0).show(context, |ui| {
            let toggle = ui.checkbox(&mut display.broadcast.cw_enabled, "Decode Morse");
            display.broadcast.toggle = Some(toggle.rect.center());
            changed |= toggle.changed();
            let before = display.broadcast.cw_config;
            let config = &mut display.broadcast.cw_config;
            ui.horizontal(|ui| {
                ui.label("Starting WPM");
                ui.add(egui::DragValue::new(&mut config.seed_wpm).range(5.0..=60.0).speed(1.0));
                ui.checkbox(&mut config.adaptive, "Track speed");
            });
            ui.add(egui::Slider::new(&mut config.threshold_dbfs, -110.0..=-5.0).text("Key threshold · dBFS"));
            ui.add(egui::Slider::new(&mut config.spacing, 1.0..=4.0).text("Word spacing"));
            changed |= before != *config;
            let reset = ui.button("Reset decoder / clear text");
            display.broadcast.cw_reset_button = Some(reset.rect.center());
            if reset.clicked() {
                display.broadcast.cw_reset = display.broadcast.cw_reset.wrapping_add(1);
                changed = true;
            }
            ui.small("Tune to the carrier. Audio pitch: 600 Hz. Set the threshold above background noise and below the keyed signal. Increasing word spacing keeps stretched character gaps from becoming spaces.");
            ui.separator();
            if !display.broadcast.cw_enabled { ui.label("Decoder disabled"); }
            else if display.pending || display.status.revision != display.revision {
                ui.label("Applying settings…");
            } else if let Some(data) = &display.status.cw {
                ui.label(format!("{:.1} WPM estimate · {} · {} rejected marks · {} unknowns", data.wpm,
                    if display.running && display.status.running { if data.key_down { "Key down" } else { "Key up" } } else { "Reception stopped" },
                    data.rejected_marks, data.unknown_characters));
                egui::ScrollArea::vertical().max_height(240.0).stick_to_bottom(true).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&data.text).monospace()).wrap().selectable(true));
                });
                if ui.button("Copy text").clicked() { ui.ctx().copy_text(data.text.clone()); }
            } else { ui.label("Start reception to decode CW"); }
            ui.small("Experimental: irregular timing, fading, and nearby signals can produce wrong letters. ? marks an unrecognized pattern; valid letters are not guaranteed correct. History is limited to 4096 characters and clears when tuning or changing decoder settings.");
        });
        display.broadcast.open = open;
        return changed;
    }
    egui::Window::new("FM broadcast data").open(&mut open).default_width(440.0).resizable(true).show(context, |ui| {
        let toggle = ui.checkbox(&mut display.broadcast.enabled, "Decode RDS / RBDS");
        display.broadcast.toggle = Some(toggle.rect.center());
        changed = toggle.changed();
        ui.small("Station information and RadioText · works with mono or stereo audio");
        ui.separator();
        if !display.broadcast.enabled { ui.label("Decoder disabled"); return; }
        if !display.running || !display.status.running {
            ui.label("Reception stopped"); return;
        }
        if display.pending || display.status.revision != display.revision
            || display.status.station_hz != (display.station_mhz*1e6).round() as u32 {
            ui.label("Tuning…"); return;
        }
        let Some(data) = &display.status.rds else { ui.label("Acquiring RDS…"); return; };
        ui.horizontal(|ui| {
            ui.colored_label(if data.locked { CYAN } else { AMBER }, if data.locked { "Receiving RDS" } else if data.groups > 0 { "Signal lost · reacquiring" } else { "Acquiring RDS…" });
            ui.small(format!("{} checked groups", data.groups));
        });
        if !data.locked {
            ui.small("A clear FM audio signal does not guarantee an RDS service. Try a strong station and allow several seconds."); return;
        }
        ui.heading(data.program_service.as_deref().unwrap_or("Waiting for station name…"));
        ui.label(data.radio_text.as_deref().unwrap_or("Waiting for RadioText…"));
        ui.separator();
        egui::Grid::new("rds-fields").num_columns(2).show(ui, |ui| {
            ui.label("Program ID (PI)"); ui.monospace(data.pi.map_or_else(|| "—".into(), |pi| format!("{pi:04X}"))); ui.end_row();
            ui.label("Program type (PTY)"); ui.label(data.program_type.map_or_else(|| "—".into(), |pty| format!("{pty} (regional code)"))); ui.end_row();
            ui.label("Traffic program"); ui.label(flag(data.traffic_program)); ui.end_row();
            ui.label("Traffic announcement"); ui.label(flag(data.traffic_announcement)); ui.end_row();
        });
        ui.small("PTY meanings differ between RDS and North American RBDS. RadioText is shown as transmitted; title/artist tags are not decoded yet.");
    });
    display.broadcast.open = open;
    changed
}
fn flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "Yes",
        Some(false) => "No",
        None => "Not received",
    }
}

pub(super) fn cw_smoke(
    mut smoke: ResMut<RdsSmoke>,
    mut display: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let ready = display
        .status
        .cw
        .as_ref()
        .is_some_and(|cw| cw.text.contains("CQ RUST"));
    let mut click = None;
    match smoke.stage {
        0 if ready && display.broadcast.button.is_some() => {
            assert!(!display.broadcast.ticker_text.is_empty());
            click = display.broadcast.button.map(|p| (p, true));
            smoke.stage = 1;
        }
        1 => {
            click = display.broadcast.button.map(|p| (p, false));
            smoke.stage = 2;
        }
        2 if display.broadcast.open => {
            smoke.marked = std::time::Instant::now();
            smoke.stage = 3;
        }
        3 if smoke.marked.elapsed().as_millis() > 500 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk("target/cw-smoke.png"));
            smoke.marked = std::time::Instant::now();
            smoke.stage = 4;
        }
        4 if smoke.marked.elapsed().as_millis() > 500 => {
            click = display.broadcast.cw_reset_button.map(|p| (p, true));
            smoke.stage = 5;
        }
        5 => {
            click = display.broadcast.cw_reset_button.map(|p| (p, false));
            smoke.stage = 6;
        }
        6 if display.broadcast.cw_reset == 1 && display.status.revision == display.revision => {
            assert!(
                display
                    .status
                    .cw
                    .as_ref()
                    .is_none_or(|cw| cw.text.is_empty())
            );
            assert_eq!(display.status.capture_revision, 1);
            click = display.broadcast.toggle.map(|p| (p, true));
            smoke.stage = 7;
        }
        7 => {
            click = display.broadcast.toggle.map(|p| (p, false));
            smoke.stage = 8;
        }
        8 if !display.broadcast.cw_enabled && display.status.revision == display.revision => {
            assert!(display.status.cw.is_none());
            click = display.broadcast.toggle.map(|p| (p, true));
            smoke.stage = 9;
        }
        9 => {
            click = display.broadcast.toggle.map(|p| (p, false));
            smoke.stage = 10;
        }
        10 if display.broadcast.cw_enabled
            && ready
            && display.status.revision == display.revision =>
        {
            assert_eq!(display.status.capture_revision, 1);
            display.receiver = ReceiverConfig::for_mode(Mode::Am);
            assert!(!available(&display));
            info!(
                "CW smoke passed: IQ text, inline ticker, Signal Data, reset, disable/re-enable, unchanged capture, mode gating"
            );
            smoke.stage = 11;
            exit.write(AppExit::Success);
        }
        _ => {}
    }
    if let Some((pos, pressed)) = click {
        for mut input in &mut inputs {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    if smoke.stage != 11 && smoke.started.elapsed().as_secs() > 35 {
        let detail = format!(
            "CW smoke timed out at stage {}: {:?}",
            smoke.stage, display.status.cw
        );
        error!("{detail}");
        exit.write(AppExit::error());
    }
}

#[derive(Resource)]
pub(super) struct RdsSmoke {
    stage: u8,
    started: std::time::Instant,
    marked: std::time::Instant,
}
impl Default for RdsSmoke {
    fn default() -> Self {
        Self {
            stage: 0,
            started: std::time::Instant::now(),
            marked: std::time::Instant::now(),
        }
    }
}
pub(super) fn smoke(
    mut smoke: ResMut<RdsSmoke>,
    display: Res<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let ready = display.status.rds.as_ref().is_some_and(|d| {
        d.program_service.as_deref() == Some("RUST FM")
            && d.radio_text.as_deref() == Some("Hello radio!")
    });
    let mut click = None;
    match smoke.stage {
        0 if ready && display.broadcast.button.is_some() => {
            assert!(
                display.plot_rect.is_some_and(|rect| rect.top() < 180.0),
                "header should not consume plot space"
            );
            assert!(
                !display.broadcast.ticker_text.is_empty(),
                "decoded text should appear outside the details window"
            );
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk("target/signal-ticker-smoke.png"));
            smoke.marked = std::time::Instant::now();
            smoke.stage = 10;
        }
        10 if smoke.marked.elapsed().as_millis() > 500 => {
            click = display.broadcast.button.map(|p| (p, true));
            smoke.stage = 1;
        }
        1 => {
            click = display.broadcast.button.map(|p| (p, false));
            smoke.stage = 2;
        }
        2 if display.broadcast.open => {
            smoke.marked = std::time::Instant::now();
            smoke.stage = 3;
        }
        3 if smoke.marked.elapsed().as_millis() > 500 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk("target/rds-smoke.png"));
            smoke.marked = std::time::Instant::now();
            smoke.stage = 4;
        }
        4 if smoke.marked.elapsed().as_millis() > 500 && display.broadcast.toggle.is_some() => {
            click = display.broadcast.toggle.map(|p| (p, true));
            smoke.stage = 5;
        }
        5 => {
            click = display.broadcast.toggle.map(|p| (p, false));
            smoke.stage = 6;
        }
        6 if !display.broadcast.enabled
            && display.status.revision == display.revision
            && display.status.rds.is_none() =>
        {
            assert_eq!(display.status.capture_revision, 1);
            click = display.broadcast.toggle.map(|p| (p, true));
            smoke.stage = 7;
        }
        7 => {
            click = display.broadcast.toggle.map(|p| (p, false));
            smoke.stage = 8;
        }
        8 if display.broadcast.enabled && ready && display.status.revision == display.revision => {
            assert_eq!(display.status.capture_revision, 1);
            info!(
                "RDS smoke passed: decoded PS/RT, opened FM data, disabled/re-enabled via checkbox and reacquired without capture restart"
            );
            exit.write(AppExit::Success);
            smoke.stage = 9;
        }
        _ => {}
    }
    if let Some((pos, pressed)) = click {
        for mut input in &mut inputs {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    if smoke.stage != 9 && smoke.started.elapsed().as_secs() > 30 {
        let detail = format!(
            "RDS smoke timed out at stage {}: {:?}",
            smoke.stage, display.status.rds
        );
        error!("{detail}");
        exit.write(AppExit::error());
    }
}
