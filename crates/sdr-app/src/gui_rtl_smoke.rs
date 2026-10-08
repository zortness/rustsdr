//! Actual interface-selector round trip; stopped receiver, no USB or preferences.
use super::*;
#[derive(Resource)]
pub(super) struct Smoke {
    started: std::time::Instant,
    stage: u8,
    release: Option<egui::Pos2>,
    captured: f32,
    last_click: f32,
}
impl Default for Smoke {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            stage: 0,
            release: None,
            captured: 0.0,
            last_click: 0.0,
        }
    }
}
pub(super) fn run(
    mut smoke: ResMut<Smoke>,
    d: Res<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = smoke.started.elapsed().as_secs_f32();
    if elapsed > 5.0 && smoke.stage == 2 && smoke.captured == 0.0 {
        info!(
            "Selector state: {:?}, pending={}, running={}, rust_button={:?}",
            d.rtl_backend, d.pending, d.running, d.rtl_backend_positions[1]
        );
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("target/rtl-selector-debug.png"));
        smoke.captured = elapsed;
    }
    let mut event = smoke.release.take().map(|p| (p, false));
    if event.is_none() {
        let pos = match smoke.stage {
            0 if elapsed > 1.5 => d.source_button_position,
            1 if elapsed > smoke.last_click + 0.3 => d.rtl_backend_positions[1],
            2 if d.rtl_backend == RtlBackend::Rust && !d.pending => d.source_button_position,
            3 if elapsed > smoke.last_click + 0.3
                && d.rtl_backend_positions.iter().all(Option::is_some) =>
            {
                assert!(!d.running);
                use bevy::render::view::screenshot::{Screenshot, save_to_disk};
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk("target/rtl-backends.png"));
                smoke.captured = elapsed;
                smoke.stage = 4;
                None
            }
            4 if elapsed > smoke.captured + 0.5 => d.rtl_backend_positions[0],
            5 if d.rtl_backend == RtlBackend::Librtlsdr && !d.pending => {
                assert!(!d.running);
                info!(
                    "RTL backend selector smoke: librtlsdr -> Rust -> librtlsdr; no hardware opened"
                );
                exit.write(AppExit::Success);
                None
            }
            _ => None,
        };
        if let Some(pos) = pos {
            event = Some((pos, true));
            smoke.release = Some(pos);
            smoke.stage += 1;
            smoke.last_click = elapsed;
        }
    }
    if let Some((pos, pressed)) = event {
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
        "Backend selector smoke stalled at {}",
        smoke.stage
    );
}
