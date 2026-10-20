//! Optional diagnostics presentation, independent of receiver controls.
use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin as Frame, SystemInfo,
    SystemInformationDiagnosticsPlugin as System,
};
use bevy::render::renderer::RenderAdapterInfo;
use bevy_egui::egui;
use sdr_app::controller::Update;

pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    status: &Update,
    metrics: &DiagnosticsStore,
    adapter: Option<&RenderAdapterInfo>,
    system: Option<&SystemInfo>,
) {
    egui::Window::new("Diagnostics").open(open).default_pos([480.0, 90.0]).default_width(470.0).resizable(true).vscroll(true).show(ctx, |ui| {
        ui.heading("Performance");
        egui::Grid::new("performance-metrics").num_columns(2).striped(true).show(ui, |ui| {
            metric(ui, metrics, "Render loop FPS", &Frame::FPS, "fps");
            metric(ui, metrics, "Frame interval", &Frame::FRAME_TIME, "ms");
            metric(ui, metrics, "App CPU / total capacity", &System::PROCESS_CPU_USAGE, "%");
            metric(ui, metrics, "System CPU", &System::SYSTEM_CPU_USAGE, "%");
            metric(ui, metrics, "App resident memory", &System::PROCESS_MEM_USAGE, "GiB");
            metric(ui, metrics, "System memory used", &System::SYSTEM_MEM_USAGE, "%");
        });
        ui.small("Smoothed frame rate includes frame pacing; it is not DSP throughput. CPU sampling runs in the background.");
        if let Some(system) = system {
            ui.label(format!("CPU: {}", system.cpu));
            ui.small(format!("{} physical cores · {} RAM", system.core_count, system.memory));
        }
        ui.separator();
        ui.heading("GPU");
        if let Some(adapter) = adapter {
            ui.label(&adapter.name);
            ui.small(format!("{:?} · {} {}", adapter.backend, adapter.driver, adapter.driver_info));
        } else { ui.label("Adapter information unavailable"); }
        let mut timings: Vec<_> = metrics.iter().filter(|d| d.path().as_str().ends_with("elapsed_gpu")).collect();
        timings.sort_by(|a,b| a.path().as_str().cmp(b.path().as_str()));
        if timings.is_empty() {
            ui.small("GPU pass timings unavailable on this renderer/backend.");
        } else {
            ui.collapsing("Instrumented GPU pass times", |ui| {
                for timing in timings {
                    ui.horizontal(|ui| {
                        ui.label(timing.path().as_str());
                        ui.monospace(timing.smoothed().map_or("Waiting…".into(), |v| format!("{v:.3} ms")));
                    });
                }
            });
        }
        ui.small("Pass times cover instrumented work only. GPU utilization, VRAM usage, and temperature are not exposed.");
        ui.separator();
        ui.heading("Reception and audio");
        if let Some(rtl) = &status.rtl {
            ui.label(rtl.backend.label());
            ui.monospace(format!("Driver configuration: {} Hz | {} samples/s | gain readback {}", rtl.center_hz, rtl.sample_rate, rtl.gain_tenth_db.map_or("unavailable".into(), |gain| format!("{:.1} dB", gain as f32 / 10.0))));
            ui.small("Requested gain: 28 dB. Frequency/rate readback is not an RF calibration.");
        }
        egui::Grid::new("radio-metrics").num_columns(2).striped(true).show(ui, |ui| {
            for (label,value) in [
                ("IQ samples", status.samples.to_string()),
                ("Skipped display frames", status.display_drops.to_string()),
                ("Dropped IQ samples", status.iq_drops.to_string()),
                ("Intentional retune discards", status.retune_discarded.to_string()),
                ("Audio underruns / overruns", format!("{} / {}",status.audio_underruns,status.audio_overruns)),
                ("Audio queue / output rate", format!("{} frames / {} Hz", status.audio_queue_frames.map_or("n/a".into(), |n| n.to_string()), status.audio_output_rate_hz.map_or("n/a".into(), |n| n.to_string()))),
                ("Audio output device", status.audio_output_device.clone().unwrap_or_else(|| "n/a".into())),
                ("Audio RMS",format!("{:.3}",status.audio_rms)),
                ("Receiver / capture revision",format!("{} / {}",status.revision,status.capture_revision)),
            ] { ui.label(label); ui.monospace(value); ui.end_row(); }
        });
        ui.small("Hann · 4096 FFT bins · dBFS (tone reference)");
    });
}

fn metric(
    ui: &mut egui::Ui,
    metrics: &DiagnosticsStore,
    label: &str,
    path: &DiagnosticPath,
    unit: &str,
) {
    ui.label(label);
    ui.monospace(
        metrics
            .get(path)
            .and_then(|d| d.smoothed())
            .filter(|v| v.is_finite())
            .map_or("Waiting / unavailable".into(), |v| format!("{v:.2} {unit}")),
    );
    ui.end_row();
}
