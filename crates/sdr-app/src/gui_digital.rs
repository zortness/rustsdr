use super::*;
use sdr_app::digital::{Batch, Config, Handle, Protocol};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Resource)]
pub(super) struct Service(pub Handle, pub sdr_app::vor::Handle);
#[derive(Serialize, Deserialize)]
pub(super) struct Preferences {
    pub protocol: Option<Protocol>,
    pub open: bool,
    #[serde(default = "labels_default")]
    pub waterfall_labels: bool,
    #[serde(default)]
    pub vor_open: bool,
}
fn labels_default() -> bool {
    true
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            protocol: None,
            open: false,
            waterfall_labels: true,
            vor_open: false,
        }
    }
}
struct DecodeRow {
    time: String,
    protocol: Protocol,
    dial_hz: Option<u64>,
    message: sdr_app::digital::Message,
}
#[derive(Default)]
pub(super) struct Panel {
    pub preferences: Preferences,
    vor_status: Option<sdr_app::vor::Status>,
    vor_arrival: Option<std::time::Instant>,
    text_tab: bool,
    path: String,
    rows: VecDeque<DecodeRow>,
    new_rows: bool,
    status: String,
    pub count: usize,
    decode_button: Option<egui::Pos2>,
    text_button: Option<egui::Pos2>,
    waterfall_button: Option<egui::Pos2>,
}
fn compatible(d: &Display) -> bool {
    if d.source == Source::Ic705 {
        return d
            .status
            .ic705
            .as_ref()
            .is_some_and(|s| s.mode == 1 && !s.transmitting);
    }
    if d.source == Source::Qmx {
        d.status
            .qmx
            .as_ref()
            .is_some_and(|s| matches!(s.mode, 2 | 6))
    } else {
        d.source != Source::Synthetic && d.receiver.mode == Mode::Usb
    }
}
fn config(d: &Display) -> Config {
    Config {
        protocol: d
            .digital
            .preferences
            .protocol
            .filter(|_| compatible(d) && d.running && d.status.running && !d.pending),
        revision: d.revision,
        dial_hz: if d.source.is_audio_radio() {
            d.status.station_hz as u64
        } else {
            (d.station_mhz * 1e6).round() as u64
        },
        file_clock: d.source == Source::File,
    }
}
pub(super) fn header(ui: &mut egui::Ui, d: &mut Display) -> bool {
    if !d.source.is_audio_radio() && d.source != Source::Synthetic {
        ui.toggle_value(&mut d.digital.preferences.vor_open, "VOR")
            .on_hover_text(
                "Experimental VOR radial from the VFO IQ channel; independent of listening mode",
            );
    }
    let previous = d.digital.preferences.protocol;
    egui::ComboBox::from_id_salt("digital-protocol")
        .selected_text(previous.map_or("Decoder off", Protocol::label))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut d.digital.preferences.protocol, None, "Decoder off");
            for p in [Protocol::Ft8, Protocol::Ft4] {
                ui.selectable_value(&mut d.digital.preferences.protocol, Some(p), p.label());
            }
        });
    let selected =
        previous != d.digital.preferences.protocol && d.digital.preferences.protocol.is_some();
    if selected {
        d.digital.preferences.open = true;
        d.digital.text_tab = false;
        if !d.source.is_audio_radio() && d.source != Source::Synthetic {
            d.mode_settings.restore(d.receiver, d.tune_step_hz);
            d.receiver = ReceiverConfig::for_mode(Mode::Usb);
            d.receiver.bandwidth_hz = 4700;
            d.receiver.agc = false;
            d.tune_step_hz = 100;
            d.zoom = SAMPLE_RATE as f32 / 6000.0;
        }
    }
    if d.digital.preferences.protocol.is_some()
        && ui
            .selectable_label(d.digital.preferences.open, "Digital text")
            .clicked()
    {
        d.digital.preferences.open = !d.digital.preferences.open;
        d.digital.text_tab = true;
        if ui.ctx().content_rect().width() < 760.0 {
            d.controls_open = false;
        }
    }
    selected && !d.source.is_audio_radio() && d.source != Source::Synthetic
}
pub(super) fn update(
    context: &egui::Context,
    d: &mut Display,
    service: &Handle,
    vor: &sdr_app::vor::Handle,
) {
    let enabled = d.digital.preferences.vor_open
        && d.running
        && d.status.running
        && !d.pending
        && !d.source.is_audio_radio()
        && d.source != Source::Synthetic;
    vor.enable(enabled);
    while let Some(status) = vor.results.next() {
        if enabled && (status.revision == d.revision || status.revision == 0) {
            d.digital.vor_status = Some(status);
            d.digital.vor_arrival = Some(std::time::Instant::now());
        }
    }
    if d.digital.preferences.vor_open {
        egui::Window::new("VOR signal data (experimental)")
            .open(&mut d.digital.preferences.vor_open)
            .default_width(350.0).show(context, |ui| {
                let fresh = enabled && d.digital.vor_arrival.is_some_and(|t| t.elapsed().as_secs_f32() < 2.5);
                if let Some(status) = d.digital.vor_status.as_ref().filter(|s| fresh && s.revision == d.revision) {
                    ui.label(format!("{:.6} MHz", status.frequency_hz as f64 / 1e6));
                    if let Some(m) = &status.measurement {
                        ui.colored_label(AMBER, egui::RichText::new(format!("{:05.1}\u{00b0} FROM", m.radial_deg)).size(30.0));
                        ui.label(format!("Reference coherence {:.0}% | deviation {:.0} Hz", m.coherence * 100.0, m.reference_deviation_hz));
                    }
                    ui.label(&status.message);
                } else {
                    ui.label(if enabled { "Waiting for a reliable VOR signal..." } else { "Start an IQ source to decode VOR." });
                }
                ui.separator();
                ui.label("Signal analysis only. Not a navigation instrument.");
                ui.small("Tune a VOR carrier; AM listening is optional. Station-ident decoding is not yet included.");
            });
    }
    let cfg = config(d);
    service.configure(cfg);
    let mut expected = cfg;
    // A file can reach EOF while its final complete slot is decoding.
    // Keep those results, but still reject another tuning/revision/protocol.
    expected.protocol = d.digital.preferences.protocol.filter(|_| compatible(d));
    while let Some(batch) = service.results.next() {
        if batch.imported || (batch.config == expected && expected.protocol.is_some()) {
            d.labels.add(&batch);
            receive(&mut d.digital, batch);
        }
    }
    if d.digital.preferences.open && context.content_rect().width() >= 760.0 {
        let mut open = true;
        egui::Window::new("Digital signal data")
            .open(&mut open)
            .default_size([780.0, 420.0])
            .show(context, |ui| {
                contents(ui, d, service);
            });
        d.digital.preferences.open = open;
    }
}
fn receive(panel: &mut Panel, batch: Batch) {
    match batch.result {
        Ok(messages) => {
            panel.status = format!(
                "{} messages in {:.2} s",
                messages.len(),
                batch.elapsed.as_secs_f32()
            );
            let prefix = if batch.imported {
                "WAV".into()
            } else if batch.config.file_clock {
                format!("+{:.1}", batch.slot_ms as f64 / 1000.0)
            } else {
                let seconds = batch.slot_ms / 1000 % 86400;
                format!(
                    "{:02}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            };
            for m in messages.into_iter().rev() {
                panel.count += 1;
                panel.new_rows = true;
                panel.rows.push_front(DecodeRow {
                    time: prefix.clone(),
                    protocol: batch.config.protocol.unwrap(),
                    dial_hz: (!batch.imported).then_some(batch.config.dial_hz),
                    message: m,
                });
                if panel.rows.len() > 1000 {
                    panel.rows.pop_back();
                }
            }
        }
        Err(e) => panel.status = e,
    }
}
pub(super) fn compact_text(context: &egui::Context, d: &mut Display, service: &Handle) -> bool {
    if context.content_rect().width() >= 760.0 || d.digital.preferences.protocol.is_none() {
        return false;
    }
    let was_text = d.digital.text_tab;
    egui::TopBottomPanel::top("digital-tabs").show(context, |ui| {
        ui.horizontal(|ui| {
            d.digital.waterfall_button = Some(
                ui.selectable_value(&mut d.digital.text_tab, false, "Waterfall")
                    .rect
                    .center(),
            );
            d.digital.text_button = Some(
                ui.selectable_value(
                    &mut d.digital.text_tab,
                    true,
                    format!("Text ({})", d.digital.count),
                )
                .rect
                .center(),
            );
        });
    });
    if d.digital.text_tab && !was_text {
        d.controls_open = false;
    }
    if d.digital.text_tab {
        egui::CentralPanel::default().show(context, |ui| contents(ui, d, service));
    }
    d.digital.text_tab
}
fn contents(ui: &mut egui::Ui, d: &mut Display, service: &Handle) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            d.digital
                .preferences
                .protocol
                .map_or("Decoder off", Protocol::label),
        );
        if ui.button("Clear").clicked() {
            d.digital.rows.clear();
            d.digital.count = 0;
        }
        ui.label(&d.digital.status);
        ui.checkbox(
            &mut d.digital.preferences.waterfall_labels,
            "Waterfall callsigns",
        );
    });
    ui.small("Receive only • 300–5000 Hz audio • UTC-aligned slots • keep the computer clock synchronized");
    if !compatible(d) {
        ui.colored_label(
            AMBER,
            "Live decoding needs USB audio (QMX: USB or Digi USB). WAV import is available below.",
        );
    } else if !d.running {
        ui.label("Start reception to collect live slots.");
    } else {
        ui.label("Collecting complete slots: FT8 15 s / FT4 7.5 s. First partial slot is skipped.");
    }
    ui.small(format!(
        "Skipped busy slots: {} · audio gaps: {} · dropped results: {}",
        service.skipped.load(std::sync::atomic::Ordering::Relaxed),
        service.dropped_audio(),
        service.results.dropped()
    ));
    ui.horizontal_wrapped(|ui| {
        ui.label("Slot WAV");
        ui.add(
            egui::TextEdit::singleline(&mut d.digital.path)
                .desired_width(320.0)
                .hint_text("Mono PCM16, 12000 Hz"),
        );
        let button = ui.add_enabled(
            d.digital.preferences.protocol.is_some(),
            egui::Button::new("Decode WAV"),
        );
        d.digital.decode_button = Some(button.rect.center());
        if button.clicked() {
            let mut cfg = config(d);
            cfg.protocol = d.digital.preferences.protocol;
            d.digital.status = match service.import(d.digital.path.clone().into(), cfg) {
                Ok(()) => "Decoding WAV…".into(),
                Err(e) => e,
            };
        }
    });
    ui.separator();
    let compact = ui.available_width() < 650.0;
    egui::ScrollArea::horizontal()
        .id_salt("digital-table-x")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_min_width(if compact { 440.0 } else { 720.0 });
            table_row(ui, None, compact, 0);
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt("digital-table-y")
                .auto_shrink([false, false]);
            if d.digital.new_rows {
                scroll = scroll.vertical_scroll_offset(0.0);
                d.digital.new_rows = false;
            }
            scroll.show_rows(ui, 19.0, d.digital.rows.len(), |ui, range| {
                for index in range {
                    table_row(ui, Some(&d.digital.rows[index]), compact, index);
                }
            });
        });
}
fn table_row(ui: &mut egui::Ui, row: Option<&DecodeRow>, compact: bool, index: usize) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 19.0), egui::Sense::hover());
    if index.is_multiple_of(2) {
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_white_alpha(9));
    }
    let mut cells = if let Some(r) = row {
        vec![
            r.time.clone(),
            format!("{:+.0}", r.message.snr),
            format!("{:+.1}", r.message.dt),
            format!("{:.0}", r.message.audio_hz),
        ]
    } else {
        vec!["UTC".into(), "dB".into(), "DT".into(), "Hz".into()]
    };
    let mut widths = vec![70.0, 32.0, 40.0, 52.0];
    if !compact {
        widths.extend([44.0, 96.0]);
        cells.extend(if let Some(r) = row {
            vec![
                r.protocol.label().into(),
                r.dial_hz
                    .map_or_else(|| "WAV".into(), |hz| format!("{:.6}", hz as f64 / 1e6)),
            ]
        } else {
            vec!["Mode".into(), "Dial MHz".into()]
        });
    }
    cells.push(row.map_or_else(
        || "Message (newest first)".into(),
        |r| r.message.text.clone(),
    ));
    widths.push((rect.width() - widths.iter().sum::<f32>()).max(220.0));
    let mut x = rect.left();
    let len = cells.len();
    for (i, (text, width)) in cells.into_iter().zip(widths).enumerate() {
        let color = if i + 1 == len && row.is_some() {
            if text.starts_with("CQ ") {
                egui::Color32::LIGHT_GREEN
            } else {
                CYAN
            }
        } else {
            egui::Color32::LIGHT_GRAY
        };
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height()));
        let response = ui.put(
            cell,
            egui::Label::new(
                egui::RichText::new(text)
                    .monospace()
                    .size(12.0)
                    .color(color),
            )
            .selectable(true)
            .truncate(),
        );
        if let Some(r) = row {
            response.on_hover_text(format!(
                "{} {} | dial {} | {}",
                r.time,
                r.protocol.label(),
                r.dial_hz
                    .map_or_else(|| "WAV".into(), |hz| format!("{hz} Hz")),
                r.message.text
            ));
        }
        x += width;
    }
}

/// Paint labels as part of the historical waterfall: age is measured in rows,
/// so pausing reception freezes them and changing render FPS cannot move them.
pub(super) fn paint_labels(ui: &egui::Ui, rect: egui::Rect, d: &Display) {
    if !d.digital.preferences.waterfall_labels {
        return;
    }
    let (center, span) = if d.source.is_audio_radio() {
        (2000.0, 4000.0)
    } else {
        d.view_axis()
    };
    let mut labels: Vec<_> = d.labels.visible().collect();
    labels.sort_by(|a, b| b.0.snr.total_cmp(&a.0.snr));
    let painter = ui.painter_at(rect);
    let mut occupied: Vec<egui::Rect> = Vec::new();
    for (label, age) in labels {
        let frequency = if d.source.is_audio_radio() {
            label.audio_hz
        } else {
            label.rf_hz
        };
        let fraction = (frequency - center) / span + 0.5;
        if !(0.0..=1.0).contains(&fraction) {
            continue;
        }
        let x = rect.left() + fraction as f32 * rect.width();
        let y = rect.top() + (age + 0.5) / HISTORY as f32 * rect.height();
        painter.line_segment(
            [egui::pos2(x - 2.0, y), egui::pos2(x + 3.0, y)],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
        let galley = painter.layout_no_wrap(
            label.call.clone(),
            egui::FontId::monospace(12.0),
            egui::Color32::WHITE,
        );
        let mut text =
            egui::epaint::TextShape::new(egui::pos2(x + 3.0, y), galley, egui::Color32::WHITE);
        text.angle = -std::f32::consts::FRAC_PI_2;
        let bounds = text.visual_bounding_rect().expand(2.0);
        if !bounds.intersects(rect) || occupied.iter().any(|r| r.intersects(bounds)) {
            continue;
        }
        painter.rect_filled(bounds, 2.0, egui::Color32::from_black_alpha(190));
        painter.add(text);
        occupied.push(bounds);
    }
}
#[derive(Resource)]
pub(super) struct Smoke {
    started: std::time::Instant,
    stage: u8,
    click: Option<egui::Pos2>,
    deadline: f32,
    compact: bool,
}
pub(super) fn setup(d: &mut Display, compact: bool) -> Smoke {
    d.source = Source::RtlSdr;
    d.running = false;
    d.digital.preferences = Preferences {
        protocol: Some(Protocol::Ft8),
        open: true,
        waterfall_labels: true,
        vor_open: false,
    };
    d.digital.path = "crates/sdr-app/tests/fixtures/digital/ft8.wav".into();
    d.receiver = ReceiverConfig::for_mode(Mode::Usb);
    d.receiver.bandwidth_hz = 4700;
    d.receiver.agc = false;
    d.tune_step_hz = 100;
    d.converter_offset_hz = -125_000_000;
    d.converter_correction_hz = 1900;
    d.station_mhz = 7.074;
    d.rtl_center_hz = 7_324_000;
    d.cursor_hz = -250_000.0;
    d.zoom = SAMPLE_RATE as f32 / 6000.0;
    Smoke {
        started: std::time::Instant::now(),
        stage: 0,
        click: None,
        deadline: 0.0,
        compact,
    }
}
pub(super) fn smoke(
    mut s: ResMut<Smoke>,
    mut d: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = s.started.elapsed().as_secs_f32();
    let mut press = None;
    if let Some(pos) = s.click.take() {
        press = Some((pos, false));
    } else {
        match s.stage {
            0 if elapsed > 2.0 => {
                if s.compact {
                    if let Some(pos) = d.digital.text_button {
                        press = Some((pos, true));
                        s.stage = 1;
                    }
                } else {
                    s.stage = 1;
                }
            }
            1 if d.digital.decode_button.is_some() => {
                press = Some((d.digital.decode_button.unwrap(), true));
                s.stage = 2;
            }
            2 if d
                .digital
                .rows
                .iter()
                .any(|r| r.message.text == "K1JT HA0DU KN07") =>
            {
                assert_eq!(d.spectrum_view().span_hz, 6000.0);
                info!("Digital smoke: FT8 reference decoded through actual WAV button");
                d.digital.preferences.protocol = Some(Protocol::Ft4);
                d.digital.path = "crates/sdr-app/tests/fixtures/digital/ft4.wav".into();
                press = Some((d.digital.decode_button.unwrap(), true));
                s.stage = 3;
            }
            3 if d
                .digital
                .rows
                .iter()
                .any(|r| r.message.text == "CQ RU AB5XS EM12") =>
            {
                assert_eq!(d.digital.rows.front().unwrap().protocol, Protocol::Ft4);
                assert_eq!(d.settings().tuner_center_hz(), Some(132_322_100));
                info!(
                    "Digital smoke: FT4 reference decoded, {} messages displayed",
                    d.digital.count
                );
                use bevy::render::view::screenshot::{Screenshot, save_to_disk};
                let file = if s.compact {
                    "target/digital-compact.png"
                } else {
                    "target/digital-desktop.png"
                };
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(file));
                s.deadline = elapsed + 0.8;
                s.stage = 4;
            }
            4 if elapsed > s.deadline => {
                if s.compact {
                    press = Some((d.digital.waterfall_button.unwrap(), true));
                    s.stage = 5;
                } else {
                    seed_label_fixture(&mut d);
                    s.deadline = elapsed + 0.5;
                    s.stage = 6;
                }
            }
            5 if !d.digital.text_tab => {
                info!("Digital smoke: compact text/waterfall tab round trip passed");
                seed_label_fixture(&mut d);
                s.deadline = elapsed + 0.5;
                s.stage = 6;
            }
            6 if elapsed > s.deadline => {
                assert!(d.labels.visible().count() > 5);
                use bevy::render::view::screenshot::{Screenshot, save_to_disk};
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(if s.compact {
                        "target/digital-labels-compact.png"
                    } else {
                        "target/digital-labels.png"
                    }));
                s.deadline = elapsed + 0.8;
                s.stage = 7;
            }
            7 if elapsed > s.deadline => {
                let before = d.labels.visible().next().unwrap().1;
                for _ in 0..10 {
                    d.row(&vec![-110.0; FFT_SIZE]);
                    d.labels.push(None);
                }
                assert_eq!(d.labels.visible().next().unwrap().1, before + 10.0);
                info!(
                    "Digital smoke: fine correction applied; newest FT4 rows first; callsign overlays age with history"
                );
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
    if let Some((pos, pressed)) = press {
        if pressed {
            s.click = Some(pos);
        }
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
    assert!(
        elapsed < 20.0,
        "Digital smoke timed out at stage {}",
        s.stage
    );
}

// Stopped, deterministic waterfall scene from actual decoded reference messages.
// This is deliberately a GUI test fixture, not an imported-WAV live annotation.
fn seed_label_fixture(d: &mut Display) {
    let messages: Vec<_> = d
        .digital
        .rows
        .iter()
        .filter(|r| r.protocol == Protocol::Ft8)
        .map(|r| r.message.clone())
        .collect();
    d.digital.preferences.open = false;
    d.digital.text_tab = false;
    d.controls_open = false;
    d.reset();
    let (center, span) = d.view_axis();
    d.analysis_center = center;
    d.analysis_rate = span;
    for row in 0..HISTORY {
        let seconds = 6.0 + row as f64 * 0.04;
        let mut values = vec![-105.0; FFT_SIZE];
        for m in &messages {
            if seconds > 13.14 + m.dt as f64 {
                continue;
            }
            let hz = d.station_mhz * 1e6 + m.audio_hz as f64 + (row % 8) as f64 * 6.25;
            let bin = ((hz - center) / span + 0.5) * FFT_SIZE as f64;
            for delta in -2..=2 {
                let i = bin as isize + delta;
                if (0..FFT_SIZE as isize).contains(&i) {
                    values[i as usize] = -30.0;
                }
            }
        }
        d.row(&values);
        d.labels.push(Some(sdr_app::waterfall_labels::Stamp {
            seconds,
            revision: d.revision,
            capture: 3,
        }));
    }
    d.labels.add(&Batch {
        config: Config {
            protocol: Some(Protocol::Ft8),
            revision: d.revision,
            dial_hz: 7_074_000,
            file_clock: true,
        },
        capture_revision: 3,
        slot_ms: 0,
        imported: false,
        elapsed: Default::default(),
        result: Ok(messages),
    });
}

pub(super) fn vor_smoke(
    d: Res<Display>,
    mut state: Local<(u32, bool)>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    state.0 += 1;
    if !state.1
        && let Some(status) = &d.digital.vor_status
        && let Some(m) = &status.measurement
    {
        assert!((m.radial_deg - 120.0).abs() < 2.0);
        assert_eq!(status.frequency_hz, 113_000_000);
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("target/vor-smoke.png"));
        info!(
            "VOR smoke: live IQ file decoded at {:.1} degrees",
            m.radial_deg
        );
        state.0 = 0;
        state.1 = true;
    }
    if state.1 && state.0 > 40 {
        exit.write(AppExit::Success);
    }
    assert!(state.0 < 2000, "VOR GUI did not acquire");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch(slot_ms: u64) -> Batch {
        Batch {
            config: Config {
                protocol: Some(Protocol::Ft8),
                revision: 1,
                dial_hz: 7_074_000,
                file_clock: false,
            },
            capture_revision: 1,
            slot_ms,
            imported: false,
            elapsed: Default::default(),
            result: Ok(vec![sdr_app::digital::Message {
                text: "CQ K1ABC FN42".into(),
                sender: Some("K1ABC".into()),
                audio_hz: 1000.0,
                dt: 0.0,
                snr: -5.0,
            }]),
        }
    }
    #[test]
    fn newest_slots_first_bounded_history_and_legacy_preferences() {
        let mut panel = Panel::default();
        for n in 0..1005 {
            receive(&mut panel, batch(n * 15000));
        }
        assert_eq!(panel.rows.len(), 1000);
        assert_eq!(panel.rows.front().unwrap().time, "04:11:00");
        assert_eq!(panel.rows.back().unwrap().time, "00:01:15");
        assert!(
            serde_json::from_str::<Preferences>(r#"{"protocol":"Ft8","open":true}"#)
                .unwrap()
                .waterfall_labels
        );
        assert!(Preferences::default().waterfall_labels);
    }
}
