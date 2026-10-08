//! Presentation panels; receiver lifecycle remains behind the engine client.
use super::*;
use sdr_app::engine::Capabilities;
pub(super) fn header(
    context: &egui::Context,
    display: &mut Display,
    capabilities: Capabilities,
) -> bool {
    let mut changed = false;
    let previous_source = display.source;
    if previous_source == Source::Ic705 {
        display.ic705.settings.step_hz = display.tune_step_hz;
    }
    if previous_source == Source::Qmx {
        display.qmx.remembered_step = Some(display.tune_step_hz);
    }
    egui::TopBottomPanel::top("header").show(context, |ui| {
        ui.add_space(5.0);
        ui.horizontal_top(|ui| {
            let controls_width = (ui.available_width() - 112.0).max(220.0);
            ui.allocate_ui_with_layout(
                egui::vec2(controls_width, 30.0),
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| {
                    ui.set_min_width(controls_width);
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.spacing_mut().interact_size.y = 30.0;
                    ui.label("Interface");
                    display.rtl_backend_positions = [None; 2];
                    let source_combo = egui::ComboBox::from_id_salt("source")
                        .selected_text(if display.source == Source::RtlSdr {
                            display.rtl_backend.label()
                        } else if display.source == Source::Ic705 {
                            "IC-705 USB"
                        } else if display.source == Source::Qmx {
                            "QMX USB"
                        } else if display.source == Source::File {
                            "IQ file"
                        } else {
                            "Synthetic tone"
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .add_enabled(capabilities.ic705, egui::Button::new("IC-705 USB"))
                                .clicked()
                            {
                                if !display.source.is_audio_radio() {
                                    display
                                        .mode_settings
                                        .restore(display.receiver, display.tune_step_hz);
                                }
                                display.source = Source::Ic705;
                                display.running = false;
                                display.ic705.settings.command = None;
                                display.ic705.refresh();
                                display.tune_step_hz =
                                    display.ic705.settings.step_hz.clamp(1, 1_000_000);
                                changed = true;
                                ui.close();
                            }
                            if ui
                                .add_enabled(capabilities.qmx, egui::Button::new("QMX USB"))
                                .clicked()
                            {
                                if !display.source.is_audio_radio() {
                                    display
                                        .mode_settings
                                        .restore(display.receiver, display.tune_step_hz);
                                }
                                display.source = Source::Qmx;
                                display.running = false;
                                display.qmx.refresh();
                                display.qmx.settings.command = None;
                                display.tune_step_hz = display.qmx.remembered_step.unwrap_or(100);
                                changed = true;
                            }
                            changed |= ui
                                .selectable_value(
                                    &mut display.source,
                                    Source::Synthetic,
                                    "Synthetic tone",
                                )
                                .changed();
                            for (backend, available) in [
                                (RtlBackend::Librtlsdr, capabilities.rtl),
                                (RtlBackend::Rust, capabilities.rtl_rust),
                            ] {
                                let response = ui.add_enabled(
                                    available,
                                    egui::Button::new(backend.label()).selected(
                                        display.source == Source::RtlSdr
                                            && display.rtl_backend == backend,
                                    ),
                                );
                                display.rtl_backend_positions
                                    [usize::from(backend == RtlBackend::Rust)] =
                                    Some(response.rect.center());
                                if response.clicked()
                                    && (display.source != Source::RtlSdr
                                        || display.rtl_backend != backend)
                                {
                                    display.source = Source::RtlSdr;
                                    display.rtl_backend = backend;
                                    changed = true;
                                    ui.close();
                                }
                            }
                            if ui
                                .selectable_value(&mut display.source, Source::File, "IQ file")
                                .changed()
                            {
                                display.running = false;
                                changed = true;
                            }
                        });

                    display.source_button_position = Some(source_combo.response.rect.center());
                    if previous_source.is_audio_radio() && !display.source.is_audio_radio() {
                        (display.receiver, display.tune_step_hz) = *display
                            .mode_settings
                            .entries()
                            .iter()
                            .find(|(c, _)| c.mode == display.receiver.mode)
                            .unwrap();
                    }

                    ui.separator();
                    if display.source == Source::Ic705 {
                        changed |= ic705::header(ui, display);
                    } else if display.source == Source::Qmx {
                        changed |= qmx::header(ui, display);
                    } else if display.source != Source::Synthetic {
                        let previous_mode = display.receiver.mode;
                        let mut mode = previous_mode;
                        egui::ComboBox::from_id_salt("receiver-mode")
                            .width(110.0)
                            .selected_text(mode.label())
                            .show_ui(ui, |ui| {
                                for choice in Mode::ALL {
                                    ui.selectable_value(&mut mode, choice, choice.label());
                                }
                            });
                        if mode != previous_mode {
                            let (receiver, step) = display.mode_settings.switch(
                                display.receiver,
                                display.tune_step_hz,
                                mode,
                            );
                            display.receiver = receiver;
                            display.tune_step_hz = step;
                            changed = true;
                        }
                        let (low, high) = sdr_app::engine::scan_bounds(&display.settings());
                        let minimum = low as f64 / 1e6;
                        let maximum = high as f64 / 1e6;
                        let speed = display.tune_step_hz as f64 / 1e6;
                        changed |= ui
                            .scope(|ui| {
                                ui.style_mut().override_font_id =
                                    Some(egui::FontId::monospace(20.0));
                                ui.visuals_mut().override_text_color = Some(AMBER);
                                ui.add(
                                    egui::DragValue::new(&mut display.station_mhz)
                                        .speed(speed)
                                        .range(minimum..=maximum)
                                        .fixed_decimals(6)
                                        .suffix(" MHz"),
                                )
                                .changed()
                            })
                            .inner;
                    } else {
                        ui.label("Test signal");
                        ui.monospace(format!(
                            "{:.6} MHz",
                            (display.center_hz() + display.tone_hz) / 1e6
                        ));
                    }
                    ui.separator();
                    if context.content_rect().width() < 760.0 && ui.button("Controls").clicked() {
                        display.controls_open = !display.controls_open;
                    }
                    if !display.source.is_audio_radio() {
                        memories::header(ui, display);
                    }
                    display.broadcast.button = None;
                    changed |= digital::header(ui, display);
                    if broadcast::available(display) {
                        let fm_data = ui.selectable_label(display.broadcast.open, "Signal Data");
                        display.broadcast.button = Some(fm_data.rect.center());
                        if fm_data.clicked() {
                            display.broadcast.open = !display.broadcast.open;
                        }
                    }
                    let button = ui
                        .selectable_label(display.debug_open, "Diagnostics")
                        .on_hover_text("Show performance and radio counters (F12)");
                    display.debug_button_position = Some(button.rect.center());
                    if button.clicked() {
                        display.debug_open = !display.debug_open;
                    }
                    let button = ui.selectable_label(display.practice.open, "CW Practice");
                    display.practice.button = Some(button.rect.center());
                    if button.clicked() {
                        display.practice.open = !display.practice.open;
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(104.0, 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let state = if display.status.error.is_some() || display.command_error.is_some()
                    {
                        "FAULT"
                    } else if display.pending || display.status.revision != display.revision {
                        "APPLYING"
                    } else if display.status.eof {
                        "END OF FILE"
                    } else if display.status.running {
                        if display
                            .status
                            .ic705
                            .as_ref()
                            .is_some_and(|s| s.transmitting)
                        {
                            "TRANSMITTING"
                        } else {
                            "RECEIVING"
                        }
                    } else {
                        "STOPPED"
                    };
                    ui.label(egui::RichText::new(state).color(AMBER));
                },
            );
        });
        ui.add_space(5.0);
        if let Some(error) = &display.status.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(error) = &display.command_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(error) = &display.status.audio_error {
            ui.colored_label(AMBER, error);
        }
    });
    changed
}
pub(super) fn show(context: &egui::Context, display: &mut Display) -> (bool, bool) {
    let mut result = (false, false);
    if context.content_rect().width() >= 760.0 {
        egui::SidePanel::left("controls")
            .default_width(235.0)
            .width_range(220.0..=400.0)
            .resizable(true)
            .show(context, |ui| result = contents(ui, display));
    } else {
        let mut open = display.controls_open;
        egui::Window::new("Controls")
            .open(&mut open)
            .default_width(235.0)
            .max_height((context.content_rect().height() - 140.0).max(150.0))
            .show(context, |ui| result = contents(ui, display));
        display.controls_open = open;
    }
    result
}
fn contents(ui: &mut egui::Ui, display: &mut Display) -> (bool, bool) {
    let mut changed = false;
    let mut recolor = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(16.0);
        ui.heading("Reception");
        ui.add_space(12.0);
        let button = ui.add_sized(
            [210.0, 34.0],
            egui::Button::new(if display.running {
                "Stop reception"
            } else {
                "Start reception"
            }),
        );
        display.button_position = Some(button.rect.center());
        if button.clicked() {
            display.running = !display.running;
            changed = true;
        }
        ui.add_space(8.0);
        if display.source != Source::Synthetic {
            ui.add(egui::Slider::new(&mut display.volume, 0.0..=1.0).text("Volume"));
            ui.checkbox(&mut display.muted, "Mute");
        }
        ui.add_space(12.0);
        if display.source == Source::Ic705 {
            ic705::controls(ui, display);
            return;
        }
        if display.source == Source::Qmx {
            qmx::controls(ui, display);
            return;
        }
        ui.label("2.400 MS/s · complex f32");
        if display.source == Source::RtlSdr {
            ui.label("Converter offset");
            let mut offset = display.converter_offset_hz;
            ui.add(egui::DragValue::new(&mut offset).speed(1000).suffix(" Hz"));
            ui.horizontal(|ui| {
                if ui.small_button("None").clicked() {
                    offset = 0;
                }
                if ui.small_button("Ham It Up (-125 MHz)").clicked() {
                    offset = -125_000_000;
                }
            });
            ui.small("RF = tuner + offset + correction. Upconverter: negative; downconverter: positive.");
            let mut correction = display.converter_correction_hz;
            ui.horizontal(|ui| {
                ui.label("Fine correction");
                ui.add(egui::DragValue::new(&mut correction).speed(1).range(-100_000..=100_000).suffix(" Hz"))
                    .on_hover_text("Added to the converter offset. Positive correction moves a known signal higher on the RF scale; calibrate against a known reference, not the USB dial alone.");
            });
            if ui.small_button("Reset correction").clicked() { correction = 0; }
            if offset != display.converter_offset_hz || correction != display.converter_correction_hz {
                if offset.checked_add(correction as i64).and_then(sdr_app::tuning::converter_bounds).is_some() {
                    display.converter_offset_hz = offset;
                    display.converter_correction_hz = correction;
                    let mut t = display.tuning();
                    t.tune(t.station_hz, 1, false);
                    display.station_mhz = t.station_hz as f64 / 1e6;
                    display.rtl_center_hz = t.center_hz;
                    changed = true;
                } else {
                    ui.colored_label(AMBER, "Offset leaves no supported RF tuning range");
                }
            }
            if let Some(hz) = display.settings().tuner_center_hz() {
                ui.small(format!("RTL tuner center: {:.6} MHz", hz as f64 / 1e6));
            }
            let mut center = display.rtl_center_hz as f64 / 1e6;
            ui.horizontal(|ui| {
                ui.label("Center");
                if ui
                    .add(
                        egui::DragValue::new(&mut center)
                            .speed(0.1)
                            .fixed_decimals(4)
                            .suffix(" MHz"),
                    )
                    .changed()
                {
                    let mut t = display.tuning();
                    t.pan((center * 1e6).round() as i64 - t.center_hz as i64);
                    display.apply_tuning(t);
                }
            });
        }
        ui.add_space(20.0);
        if display.source == Source::File {
            ui.label("File path · float32 LE I/Q");
            if ui.text_edit_singleline(&mut display.file_path).changed() {
                display.running = false;
                changed = true;
            }
            ui.label("File capture center (MHz)");
            if ui
                .add(
                    egui::DragValue::new(&mut display.file_center_mhz)
                        .speed(0.1)
                        .range(0.000001..=1700.0)
                        .fixed_decimals(6),
                )
                .changed()
            {
                display.running = false;
                changed = true;
            }
            ui.small("Rate: 2,400,000 samples/s. Start replays from the beginning.");
            if let Some(total) = display.status.total_samples {
                ui.label(format!(
                    "{:.2} / {:.2} s",
                    display.status.samples as f64 / SAMPLE_RATE as f64,
                    total as f64 / SAMPLE_RATE as f64
                ));
            }
        }
        if display.source != Source::Synthetic {
            changed |= receiver_controls(ui, &mut display.receiver);
            if display.receiver.mode == Mode::WfmStereo {
                ui.small(
                    if display.status.stereo_locked && display.status.revision == display.revision {
                        "19 kHz pilot locked · stereo"
                    } else {
                        "Acquiring pilot · mono fallback"
                    },
                );
            }
            ui.horizontal(|ui| {
                ui.label("Tune step");
                egui::ComboBox::from_id_salt("tune-step")
                    .selected_text(format!("{} kHz", display.tune_step_hz as f64 / 1000.0))
                    .show_ui(ui, |ui| {
                        for step in [
                            10, 100, 500, 1_000, 5_000, 10_000, 12_500, 25_000, 50_000, 100_000,
                            200_000, 1_000_000,
                        ] {
                            ui.selectable_value(
                                &mut display.tune_step_hz,
                                step,
                                format!("{} kHz", step as f64 / 1000.0),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("VFO");
                for (label, sign) in [("- step", -1_i64), ("+ step", 1)] {
                    if ui
                        .add_sized([75.0, 30.0], egui::Button::new(label))
                        .clicked()
                    {
                        let mut tuning = display.tuning();
                        let hz = (tuning.station_hz as i64 + sign * display.tune_step_hz as i64)
                            .clamp(1, 1_700_000_000) as u32;
                        tuning.tune(hz, display.tune_step_hz, display.source == Source::File);
                        display.apply_tuning(tuning);
                    }
                }
            });
            if display.source == Source::RtlSdr && display.receiver.mode.is_wfm() {
                ui.horizontal(|ui| {
                    if ui.button("106.7 MHz").clicked() {
                        display.station_mhz = 106.7;
                        changed = true;
                    }
                    let preset = ui.button("89.9 MHz");
                    display.preset_position = Some(preset.rect.center());
                    if preset.clicked() {
                        display.station_mhz = 89.9;
                        changed = true;
                    }
                });
            }
            if display.source == Source::RtlSdr {
                let (low, high) = sdr_app::engine::scan_bounds(&display.settings());
                ui.small(format!(
                    "RF {:.6}–{:.3} MHz · 28 dB RF gain",
                    low as f64 / 1e6,
                    high as f64 / 1e6
                ));
            } else {
                ui.small("Tune within the recorded capture");
            }
            if display.receiver.mode.is_wfm() {
                ui.horizontal(|ui| {
                    ui.label("De-emphasis");
                    changed |= ui
                        .selectable_value(&mut display.receiver.deemphasis_us, 75, "75 μs")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut display.receiver.deemphasis_us, 50, "50 μs")
                        .changed();
                });
            }
        } else {
            ui.label("Test tone offset");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut display.tone_hz)
                        .speed(100.0)
                        .range(-1_199_000.0..=1_199_000.0)
                        .suffix(" Hz"),
                )
                .changed();
            ui.small("Changing the tone restarts the source and clears history.");
        }
        ui.add_space(20.0);
        ui.separator();
        ui.heading("Display");
        let mut span = display.spectrum_view().span_hz / 1000.0;
        if ui
            .add(
                egui::Slider::new(&mut span, 0.5..=2400.0)
                    .logarithmic(true)
                    .text("Span kHz"),
            )
            .changed()
        {
            display.zoom = (2400.0 / span) as f32;
        }
        ui.horizontal(|ui| {
            let fit = ui.button("Fit receiver");
            display.fit_position = Some(fit.rect.center());
            if fit.clicked() {
                display.fit_receiver();
            }
            if ui.button("Full span").clicked() {
                display.zoom = 1.0;
            }
        });
        let spacing = display.analysis_rate / display.spectrum.len().max(1) as f64;
        ui.small(format!("Auto resolution · {spacing:.2} Hz/bin"));
        ui.small(format!(
            "Hann RBW ≈ {:.2} Hz · {:.0} ms observation",
            spacing * 1.5,
            1000.0 / spacing
        ));
        if display.received_view != Some(display.spectrum_view()) {
            ui.small(if display.running {
                "Waiting for analysis at this span…"
            } else {
                "Start reception for spectrum"
            });
        }
        recolor |= ui
            .add(egui::Slider::new(&mut display.floor, -140.0..=-30.0).text("Floor"))
            .changed();
        recolor |= ui
            .add(egui::Slider::new(&mut display.ceiling, -20.0..=0.0).text("Ceiling"))
            .changed();
        ui.add_space(16.0);
        ui.label(if display.source != Source::Synthetic {
            display.receiver.mode.label()
        } else {
            "Inspection cursor"
        });
        ui.label(
            egui::RichText::new(format!(
                "{:.6} MHz",
                (display.center_hz() + display.cursor_hz) / 1e6
            ))
            .monospace()
            .color(AMBER),
        );
        ui.small(if display.source != Source::Synthetic {
            "Click / drag: VFO · Wheel: center\nShift+wheel: VFO · snaps to tune step"
        } else {
            "Click either plot to inspect a frequency."
        });
        ui.add_space(20.0);
        ui.small(if display.source != Source::Synthetic {
            "48 kHz audio · default output"
        } else {
            "Synthetic spectrum test · no audio"
        });
    });

    (changed, recolor)
}
fn receiver_controls(ui: &mut egui::Ui, config: &mut ReceiverConfig) -> bool {
    let before = *config;
    let widths: &[u32] = match config.mode {
        Mode::Cw => &[200, 500, 1000],
        Mode::Am => &[6_000, 10_000, 12_000],
        Mode::Nfm => &[12_500, 25_000],
        Mode::Usb | Mode::Lsb => &[1_800, 2_400, 2_700, 4_700],
        _ => &[200_000],
    };
    egui::ComboBox::from_id_salt("receiver-bandwidth")
        .selected_text(format!("Bandwidth: {} Hz", config.bandwidth_hz))
        .show_ui(ui, |ui| {
            for &width in widths {
                ui.selectable_value(&mut config.bandwidth_hz, width, format!("{width} Hz"));
            }
        });
    if matches!(config.mode, Mode::Am | Mode::Usb | Mode::Lsb | Mode::Cw) {
        ui.checkbox(&mut config.agc, "Audio AGC");
    }
    if config.mode == Mode::Nfm {
        ui.horizontal(|ui| {
            ui.label("Deviation");
            ui.selectable_value(&mut config.deviation_hz, 2_500, "2.5 kHz");
            ui.selectable_value(&mut config.deviation_hz, 5_000, "5 kHz");
        });
        ui.horizontal(|ui| {
            ui.label("De-emphasis");
            ui.selectable_value(&mut config.deemphasis_us, 0, "Off");
            ui.selectable_value(&mut config.deemphasis_us, 300, "300 μs");
            ui.selectable_value(&mut config.deemphasis_us, 750, "750 μs");
        });
        let mut enabled = config.squelch_db.is_some();
        if ui.checkbox(&mut enabled, "Squelch").changed() {
            config.squelch_db = enabled.then_some(-60.0);
        }
        if let Some(level) = &mut config.squelch_db {
            ui.add(egui::Slider::new(level, -100.0..=-20.0).text("dBFS"));
        }
    }
    before != *config
}
