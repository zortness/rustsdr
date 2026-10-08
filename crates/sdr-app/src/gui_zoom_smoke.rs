//! Exercises the actual Fit receiver button without hardware or persisted settings.
use super::*;
#[derive(Resource)]
pub(super) struct Smoke {
    started: std::time::Instant,
    stage: u8,
    capture: u64,
    samples: u64,
    captured_at: f32,
}
impl Default for Smoke {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            stage: 0,
            capture: 0,
            samples: 0,
            captured_at: 0.0,
        }
    }
}
pub(super) fn run(
    mut smoke: ResMut<Smoke>,
    mut display: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = smoke.started.elapsed().as_secs_f32();
    let mut click = None;
    match smoke.stage {
        0 if display.last_sequence > 5 && display.fit_position.is_some() => {
            smoke.capture = display.status.capture_revision;
            smoke.samples = display.status.samples;
            click = Some(true);
            smoke.stage = 1;
        }
        1 => {
            click = Some(false);
            smoke.stage = 2;
        }
        2 if display.zoom > 100.0
            && display.received_view == Some(display.spectrum_view())
            && display.analysis_rate < 20_000.0 =>
        {
            assert_eq!(display.revision, 1);
            assert_eq!(display.status.capture_revision, smoke.capture);
            assert!(display.status.samples > smoke.samples);
            assert!(display.analysis_rate / (display.spectrum.len() as f64) < 3.0);
            assert!(
                display
                    .row_spans
                    .iter()
                    .zip(&display.row_centers)
                    .any(|(span, center)| *span == SAMPLE_RATE as f64 && *center != 0.0),
                "wide history was discarded"
            );
            assert!(display.row_spans.iter().any(|span| *span < 5000.0));
            if elapsed > 3.0 {
                use bevy::render::view::screenshot::{Screenshot, save_to_disk};
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk("target/zoom-smoke.png"));
                smoke.captured_at = elapsed;
                smoke.stage = 3;
                let spacing = display.analysis_rate / display.spectrum.len() as f64;
                info!(
                    "Zoom smoke: Fit receiver delivered {:.3} Hz/bin without capture/revision change",
                    spacing
                );
            }
        }
        3 if elapsed > smoke.captured_at + 0.7 => {
            display.zoom = 1.0;
            smoke.stage = 4;
        }
        4 if display.received_view == Some(display.spectrum_view())
            && display.analysis_rate == SAMPLE_RATE as f64 =>
        {
            assert_eq!(display.status.capture_revision, smoke.capture);
            assert_eq!(display.revision, 1);
            info!("Zoom smoke: full span restored; history retained");
            exit.write(AppExit::Success);
        }
        _ => {}
    }
    if let Some(pressed) = click {
        let pos = display.fit_position.unwrap();
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
    if elapsed > 15.0 {
        panic!("Zoom smoke timed out at stage {}", smoke.stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_reset_restores_qmx_compatible_spectrum_size() {
        let mut d = Display {
            source: Source::Qmx,
            spectrum: vec![-40.0; 1024],
            ..Default::default()
        };
        d.reset();
        assert_eq!(d.spectrum.len(), FFT_SIZE);
        assert!(d.received_view.is_none());
    }
    #[test]
    fn fit_follows_sideband_and_stays_inside_capture() {
        for mode in [Mode::Usb, Mode::Lsb, Mode::Cw] {
            let mut d = Display {
                source: Source::RtlSdr,
                station_mhz: 7.074,
                rtl_center_hz: 7_324_000,
                receiver: ReceiverConfig::for_mode(mode),
                ..Default::default()
            };
            d.fit_receiver();
            let (low, high) = d.receiver.passband();
            let (center, span) = d.view_axis();
            assert!((center - (7_074_000.0 + (low + high) / 2.0)).abs() < 0.001);
            assert!(
                center - span / 2.0 <= 7_074_000.0 + low
                    && center + span / 2.0 >= 7_074_000.0 + high
            );
            assert!(d.spectrum_view().validate(SAMPLE_RATE as f64));
            d.station_mhz = 8.5;
            assert!(d.spectrum_view().validate(SAMPLE_RATE as f64));
        }
    }
}
