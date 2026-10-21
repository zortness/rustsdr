use bevy::diagnostic::{
    DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInfo, SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use sdr_app::engine::{
    Client, Command, Engine, FFT_SIZE, FileSettings, RTL_OFFSET_HZ, SAMPLE_RATE, Settings, Source,
    SubmitError, Update as RadioUpdate,
};
use sdr_app::recording::Recorder;
use sdr_app::tuning::Tuning;
use sdr_devices::RtlBackend;
use sdr_dsp::{Mode, ReceiverConfig};
#[path = "gui_broadcast.rs"]
mod broadcast;
#[path = "gui_controls.rs"]
mod controls;
#[path = "gui_diagnostics.rs"]
mod diagnostics;
#[path = "gui_digital.rs"]
mod digital;
#[path = "gui_ic705.rs"]
mod ic705;
#[path = "gui_memories.rs"]
mod memories;
#[path = "gui_practice.rs"]
mod practice;
#[path = "gui_qmx.rs"]
mod qmx;
#[path = "gui_rtl_smoke.rs"]
mod rtl_smoke;
#[path = "gui_settings.rs"]
mod settings;
#[path = "gui_soak.rs"]
mod soak;
#[path = "gui_zoom_smoke.rs"]
mod zoom_smoke;
#[derive(Resource)]
struct RadioClient(Client);

const HISTORY: usize = 256;
const WATERFALL_WIDTH: usize = 1024;
const CYAN: egui::Color32 = egui::Color32::from_rgb(68, 218, 218);
const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 192, 89);

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(2).collect();
    let mut qmx_port = None;
    let mut ic705_port = None;
    let mut radio_input = String::new();
    let mut monitor_output = String::new();
    let mut file_path = None;
    let mut file_center = None;
    let mut file_rate = None;
    let mut mode = Mode::WfmMono;
    let mut rtl_backend = None;
    let mut soak_seconds: Option<f64> = None;
    let mut soak_interval: f64 = 10.0;
    let mut args = arguments.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rtl-backend" => rtl_backend = Some(RtlBackend::from_key(args.next().ok_or("--rtl-backend requires librtlsdr or rs-rtl")?).ok_or("Unknown RTL backend")?),
            "--ic705" => ic705_port=Some(args.next().ok_or("--ic705 requires a port")?.clone()),
            "--qmx" => qmx_port = Some(args.next().ok_or("--qmx requires a port")?.clone()),
            "--radio-input" => radio_input = args.next().ok_or("--radio-input requires a name")?.clone(),
            "--monitor-output" => monitor_output = args.next().ok_or("--monitor-output requires a name")?.clone(),
            "--qmx-smoke" | "--settings-smoke-write" | "--settings-smoke-read" | "--converter-smoke" | "--zoom-smoke" | "--digital-smoke" | "--digital-compact-smoke" | "--vor-smoke" | "--rtl-backend-smoke" | "--ic705-smoke" => {},
            "--mode" => mode = match args.next().map(String::as_str) {
                Some("wfm") => Mode::WfmMono,
                Some("stereo") => Mode::WfmStereo,
                Some("am") => Mode::Am,
                Some("nfm") => Mode::Nfm,
                Some("usb") => Mode::Usb,
                Some("lsb") => Mode::Lsb, Some("cw") => Mode::Cw,
                _ => return Err("--mode requires wfm, stereo, am, nfm, usb, lsb, or cw".into()),
            },
            "--rtl" | "--smoke-test" | "--compact-smoke" | "--memory-smoke" | "--rds-smoke" | "--cw-smoke" | "--practice-smoke" => {},
            "--file" => file_path = Some(args.next().ok_or("--file requires a path")?.clone()),
            "--center-hz" => file_center = Some(args.next().ok_or("--center-hz requires a value")?.parse::<u64>()?),
            "--sample-rate" => file_rate = Some(args.next().ok_or("--sample-rate requires a value")?.parse::<u32>()?),
            "--soak" => soak_seconds = Some(args.next().ok_or("--soak requires seconds")?.parse::<f64>()?),
            "--soak-interval" => soak_interval = args.next().ok_or("--soak-interval requires seconds")?.parse::<f64>()?,
            _ => return Err("usage: --gui [--rtl | --file PATH --center-hz HZ --sample-rate 2400000] [--smoke-test | --soak SECONDS --soak-interval SECONDS]".into()),
        }
    }
    let compact = arguments.iter().any(|arg| arg == "--compact-smoke");
    let memory_smoke = arguments.iter().any(|arg| arg == "--memory-smoke");
    let rds_smoke = arguments.iter().any(|arg| arg == "--rds-smoke");
    let cw_smoke = arguments.iter().any(|arg| arg == "--cw-smoke");
    let practice_smoke = arguments.iter().any(|arg| arg == "--practice-smoke");
    let smoke = compact || arguments.iter().any(|arg| arg == "--smoke-test");
    let rtl = arguments.iter().any(|arg| arg == "--rtl");
    if file_path.is_some() && (rtl || file_center.is_none() || file_rate != Some(SAMPLE_RATE)) {
        return Err(
            "File playback requires --center-hz and --sample-rate 2400000, without --rtl".into(),
        );
    }
    if file_path.is_none() && (file_center.is_some() || file_rate.is_some()) {
        return Err("File metadata requires --file".into());
    }
    if ic705_port.is_some()
        && (!cfg!(feature = "ic705")
            || qmx_port.is_some()
            || rtl
            || file_path.is_some()
            || radio_input.is_empty()
            || monitor_output.is_empty())
    {
        return Err(
            "IC-705 requires ic705 feature, radio input and monitor output; no other source flags"
                .into(),
        );
    }
    let qmx_enabled = qmx_port.is_some();
    if qmx_enabled
        && (!cfg!(feature = "qmx")
            || rtl
            || file_path.is_some()
            || radio_input.is_empty()
            || monitor_output.is_empty())
    {
        return Err(
            "QMX needs --features qmx, --radio-input and --monitor-output, without RTL/file flags"
                .into(),
        );
    }
    let playback = file_path.is_some();
    if soak_seconds.is_none() && arguments.iter().any(|arg| arg == "--soak-interval") {
        return Err("--soak-interval requires --soak".into());
    }
    if let Some(seconds) = soak_seconds {
        if !(rtl || playback) {
            return Err("--soak needs --rtl or --file so it measures a real source".into());
        }
        if !seconds.is_finite() || seconds < 60.0 {
            return Err("--soak needs at least 60 seconds to be a meaningful trend".into());
        }
        if !soak_interval.is_finite() || !(1.0..=60.0).contains(&soak_interval) {
            return Err("--soak-interval must be between 1 and 60 seconds".into());
        }
        if arguments.iter().any(|a| a.contains("smoke")) {
            return Err("--soak cannot be combined with a smoke test".into());
        }
    }
    let center_mhz = file_center.unwrap_or(106_950_000) as f64 / 1e6;
    let mut initial = Display::default();
    let settings_smoke = arguments
        .iter()
        .any(|arg| arg.starts_with("--settings-smoke-"));
    if settings_smoke && std::env::var_os("RUSTSDR_CONFIG_DIR").is_none() {
        return Err("Settings smoke requires an isolated RUSTSDR_CONFIG_DIR".into());
    }
    let testing = !settings_smoke
        && (arguments.iter().any(|arg| arg == "--soak")
            || arguments
                .iter()
                .any(|arg| arg.starts_with("--") && arg.contains("smoke")));
    let persistence = if testing {
        None
    } else {
        settings::Persistence::load(&mut initial)
    };
    if let Some(backend) = rtl_backend {
        if !rtl || !backend.available() {
            return Err("--rtl-backend requires --rtl and a compiled backend".into());
        }
        initial.rtl_backend = backend;
    }
    // A --rtl launch after a saved HF radio session needs a tuner-safe VFO.
    if rtl {
        let mut tuning = Tuning {
            converter_offset_hz: initial.effective_converter_offset(),
            center_hz: initial.rtl_center_hz,
            station_hz: (initial.station_mhz * 1e6).round() as u32,
        };
        tuning.tune_with_limit(
            tuning.station_hz,
            1,
            false,
            initial.rtl_sample_rate / 2 - 200_000,
        );
        initial.station_mhz = tuning.station_hz as f64 / 1e6;
        initial.rtl_center_hz = tuning.center_hz;
    }
    if arguments.iter().any(|arg| arg == "--mode") {
        (initial.receiver, initial.tune_step_hz) = *initial
            .mode_settings
            .entries()
            .iter()
            .find(|(c, _)| c.mode == mode)
            .unwrap();
    }
    if rtl || playback {
        initial.source = if rtl { Source::RtlSdr } else { Source::File };
        initial.running = true;
        initial.cursor_hz = RTL_OFFSET_HZ;
    }
    if playback {
        initial.file_path = file_path.unwrap_or_default();
        initial.file_center_mhz = center_mhz;
        initial.station_mhz = (center_mhz - 0.25).clamp(0.000001, 1700.0);
    }
    if let Some(port) = ic705_port {
        initial.source = Source::Ic705;
        initial.running = true;
        initial.ic705.settings.port = port;
        initial.ic705.settings.input = radio_input.clone();
        initial.ic705.settings.output = monitor_output.clone();
        initial.ic705.settings.command = None;
        initial.ic705.refresh();
        initial.tune_step_hz = 100;
    }
    if let Some(port) = qmx_port {
        initial.source = Source::Qmx;
        initial.running = true;
        initial.tune_step_hz = persistence
            .as_ref()
            .map_or(100, settings::Persistence::qmx_step);
        initial.qmx.settings.port = port;
        initial.qmx.settings.input = radio_input;
        initial.qmx.settings.output = monitor_output;
        initial.qmx.refresh();
        if testing {
            initial.tune_step_hz = 100;
            initial.qmx.settings.audio_gain_db = 20;
        }
    }
    let digital_compact = arguments.iter().any(|a| a == "--digital-compact-smoke");
    let window_size = if compact || digital_compact {
        (640, 820)
    } else {
        persistence
            .as_ref()
            .map_or((1280, 820), settings::Persistence::window_size)
    };
    let mut app = App::new();
    if arguments.iter().any(|a| a == "--rtl-backend-smoke") {
        if !cfg!(feature = "rtl-rust") {
            return Err("Backend smoke requires rtl-rust".into());
        }
        initial.source = Source::RtlSdr;
        initial.running = false;
        initial.rtl_backend = RtlBackend::Librtlsdr;
        app.init_resource::<rtl_smoke::Smoke>().add_systems(
            PreUpdate,
            rtl_smoke::run
                .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                .before(bevy_egui::EguiPreUpdateSet::BeginPass),
        );
    }
    if arguments.iter().any(|a| a == "--vor-smoke") {
        if initial.source != Source::File {
            return Err("--vor-smoke requires the generated IQ file".into());
        }
        initial.station_mhz = 113.0;
        initial.cursor_hz = -250_000.0;
        initial.receiver = ReceiverConfig::for_mode(Mode::Am);
        initial.digital.preferences.vor_open = true;
        app.add_systems(Update, digital::vor_smoke);
    }
    if digital_compact || arguments.iter().any(|a| a == "--digital-smoke") {
        app.insert_resource(digital::setup(&mut initial, digital_compact))
            .add_systems(
                PreUpdate,
                digital::smoke
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    if arguments.iter().any(|a| a == "--zoom-smoke") {
        initial.receiver = ReceiverConfig::for_mode(Mode::Usb);
        app.insert_resource(zoom_smoke::Smoke::default())
            .add_systems(
                PreUpdate,
                zoom_smoke::run
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    if arguments.iter().any(|a| a == "--converter-smoke") {
        initial.source = Source::RtlSdr;
        initial.running = false;
        initial.converter_offset_hz = -125_000_000;
        initial.station_mhz = 7.1;
        initial.rtl_center_hz = 7_350_000;
        initial.cursor_hz = -250_000.0;
        initial.receiver = ReceiverConfig::for_mode(Mode::Lsb);
        initial.tune_step_hz = 100;
        app.add_systems(Update, converter_smoke);
    }
    if let Some(persistence) = persistence {
        app.insert_resource(persistence)
            .add_systems(Last, settings::save);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "RustSDR".into(),
            resolution: window_size.into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .add_plugins((
        FrameTimeDiagnosticsPlugin::default(),
        SystemInformationDiagnosticsPlugin,
        bevy::render::diagnostic::RenderDiagnosticsPlugin,
    ))
    .insert_resource(initial)
    .add_systems(Startup, setup_view)
    .add_systems(
        PreUpdate,
        practice::input
            .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
            .before(bevy_egui::EguiPreUpdateSet::BeginPass),
    )
    .add_systems(EguiPrimaryContextPass, draw);
    if arguments.iter().any(|a| a == "--ic705-smoke") {
        app.add_systems(Update, ic705::smoke);
    }
    if settings_smoke {
        app.insert_resource(settings::Smoke::new(
            arguments.iter().any(|a| a == "--settings-smoke-write"),
        ))
        .add_systems(Update, settings::smoke);
    }
    if arguments.iter().any(|a| a == "--qmx-smoke") {
        if !qmx_enabled {
            return Err("--qmx-smoke requires --qmx".into());
        }
        app.insert_resource(qmx::Smoke::default()).add_systems(
            PreUpdate,
            qmx::smoke
                .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                .before(bevy_egui::EguiPreUpdateSet::BeginPass),
        );
    }
    if practice_smoke {
        app.insert_resource(practice::PracticeSmoke::default())
            .add_systems(
                PreUpdate,
                practice::smoke
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(practice::input),
            );
    }
    if cw_smoke {
        app.insert_resource(broadcast::RdsSmoke::default())
            .add_systems(
                PreUpdate,
                broadcast::cw_smoke
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    if rds_smoke {
        app.insert_resource(broadcast::RdsSmoke::default())
            .add_systems(
                PreUpdate,
                broadcast::smoke
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    if memory_smoke {
        app.insert_resource(memories::MemorySmoke::default())
            .add_systems(
                PreUpdate,
                memories::smoke
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    if smoke {
        std::fs::create_dir_all("target")?;
        app.insert_resource(SmokeTest {
            compact,
            started: std::time::Instant::now(),
            captured: false,
            stage: 0,
            baseline_samples: 0,
            baseline_rows: 0,
            debug_stage: 0,
            debug_captured: false,
            debug_capture_time: 0.0,
        })
        .add_systems(Update, smoke_test)
        .add_systems(
            PreUpdate,
            smoke_input
                .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                .before(bevy_egui::EguiPreUpdateSet::BeginPass),
        );
    }
    if let Some(seconds) = soak_seconds {
        let source_label = if rtl { "rtl-sdr" } else { "file" };
        app.insert_resource(soak::Soak::new(seconds, soak_interval, source_label))
            .add_systems(
                PreUpdate,
                soak::run
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            );
    }
    let outcome = app.run();
    if outcome.is_success() {
        Ok(())
    } else {
        Err("GUI exited with an error".into())
    }
}

fn setup_view(world: &mut World) -> bevy::prelude::Result {
    // Start acquisition after renderer initialization so short files are not
    // consumed while the graphics device and pipelines are being prepared.
    let settings = world.resource::<Display>().settings();
    let engine = Engine::with_settings(settings)?;
    world.insert_resource(digital::Service(
        engine.client().digital(),
        engine.client().vor(),
    ));
    world.insert_resource(RadioClient(engine.client()));
    world.insert_non_send_resource(engine);
    world.resource_mut::<Display>().memories.load_startup();
    world.spawn(Camera2d);
    if let Some(mut smoke) = world.get_resource_mut::<SmokeTest>() {
        smoke.started = std::time::Instant::now();
    }
    if let Some(mut soak) = world.get_resource_mut::<soak::Soak>() {
        soak.start();
    }
    Ok(())
}

fn converter_smoke(
    mut commands: Commands,
    display: Res<Display>,
    mut started: Local<Option<std::time::Instant>>,
    mut captured: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = started
        .get_or_insert_with(std::time::Instant::now)
        .elapsed()
        .as_secs_f32();
    assert!(!display.running);
    assert_eq!(display.station_mhz, 7.1);
    assert_eq!(display.settings().tuner_center_hz(), Some(132_350_000));
    if elapsed > 1.0 && !*captured {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("target/converter-smoke.png"));
        *captured = true;
    }
    if elapsed > 3.0 {
        info!("Converter smoke: 7.1 MHz RF / 132.35 MHz tuner center; stopped");
        exit.write(AppExit::Success);
    }
}

#[derive(Resource)]
struct SmokeTest {
    compact: bool,
    debug_capture_time: f32,
    debug_stage: u8,
    debug_captured: bool,
    started: std::time::Instant,
    captured: bool,
    stage: u8,
    baseline_samples: u64,
    baseline_rows: usize,
}

fn smoke_input(
    mut smoke: ResMut<SmokeTest>,
    mut display: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut windows: Query<&mut Window>,
) {
    let elapsed = smoke.started.elapsed().as_secs_f32();
    // Compact controls float over the left plot; close them before testing a
    // plot click so this input targets the spectrum rather than the window.
    if smoke.compact && smoke.stage == 4 && display.status.revision == 3 && display.controls_open {
        display.controls_open = false;
        return;
    }
    if smoke.captured {
        let pressed = match smoke.debug_stage {
            0 => Some(true),
            1 => Some(false),
            2 if smoke.debug_captured && elapsed > smoke.debug_capture_time + 0.75 => Some(true),
            3 => Some(false),
            _ => None,
        };
        if let Some(pressed) = pressed
            && let Some(pos) = display.debug_button_position
        {
            for mut input in &mut inputs {
                input.events.push(egui::Event::PointerMoved(pos));
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            smoke.debug_stage += 1;
            let debug_open = display.debug_open;
            info!(
                "Diagnostics smoke input stage {}, open {}",
                smoke.debug_stage, debug_open
            );
        }
        return;
    }
    let action = match smoke.stage {
        0 if elapsed > 1.0 && display.status.running && display.last_sequence > 5 => {
            display.button_position.map(|p| (p, true))
        }
        1 => display.button_position.map(|p| (p, false)),
        2 if !display.status.running && display.status.revision == 2 => {
            display.button_position.map(|p| (p, true))
        }
        3 => display.button_position.map(|p| (p, false)),
        4 if display.source == Source::File
            && display.status.revision == 3
            && display.status.eof =>
        {
            for mut window in &mut windows {
                window
                    .resolution
                    .set(if smoke.compact { 640.0 } else { 1100.0 }, 760.0);
            }
            smoke.stage = 7;
            None
        }
        4 if display.source != Source::File
            && display.status.running
            && display.status.revision == 3
            && display.last_sequence > 10 =>
        {
            if display.source == Source::RtlSdr {
                display.preset_position.map(|p| (p, true))
            } else {
                display
                    .plot_rect
                    .map(|r| (egui::pos2(r.left() + r.width() * 0.25, r.center().y), true))
            }
        }
        5 => {
            if display.source == Source::RtlSdr {
                display.preset_position.map(|p| (p, false))
            } else {
                display
                    .plot_rect
                    .map(|r| (egui::pos2(r.left() + r.width() * 0.25, r.center().y), false))
            }
        }
        6 if display.source == Source::RtlSdr && display.status.revision == 4 => {
            smoke.baseline_samples = display.status.samples;
            smoke.baseline_rows = display.row_centers.iter().filter(|c| **c != 0.0).count();
            display
                .waterfall_rect
                .map(|r| (egui::pos2(r.left() + r.width() * 0.35, r.center().y), true))
        }
        7 if display.source == Source::RtlSdr => display
            .waterfall_rect
            .map(|r| (egui::pos2(r.left() + r.width() * 0.35, r.center().y), false)),
        8 if display.source == Source::RtlSdr
            && display.status.revision == 5
            && display.status.capture_revision == 3
            && display.status.samples >= smoke.baseline_samples =>
        {
            if let Some(rect) = display.waterfall_rect {
                for mut input in &mut inputs {
                    input.events.push(egui::Event::PointerMoved(rect.center()));
                    input.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(0.0, 1.0),
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                smoke.stage = 9;
            }
            None
        }
        9 if display.source == Source::RtlSdr
            && display.status.revision == 6
            && display.status.capture_revision == 3
            && display.status.center_hz == 90_250_000
            && display.row_centers.iter().filter(|c| **c != 0.0).count() >= smoke.baseline_rows =>
        {
            for mut window in &mut windows {
                window
                    .resolution
                    .set(if smoke.compact { 640.0 } else { 1100.0 }, 760.0);
            }
            smoke.stage = 10;
            None
        }
        6 if display.source != Source::RtlSdr && display.cursor_hz < 0.0 => {
            for mut window in &mut windows {
                window
                    .resolution
                    .set(if smoke.compact { 640.0 } else { 1100.0 }, 760.0);
            }
            smoke.stage = 7;
            None
        }
        _ => None,
    };
    if let Some((pos, pressed)) = action {
        for mut input in &mut inputs {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        smoke.stage += 1;
        info!("GUI smoke: input stage {}", smoke.stage);
    }
}

fn smoke_test(
    mut commands: Commands,
    mut smoke: ResMut<SmokeTest>,
    display: Res<Display>,
    metrics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    if smoke.captured
        && !smoke.debug_captured
        && display.debug_open
        && smoke.started.elapsed().as_secs_f32() > 6.0
        && metrics
            .get(&FrameTimeDiagnosticsPlugin::FPS)
            .and_then(|d| d.smoothed())
            .is_some_and(|v| v > 0.0)
        && metrics
            .get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)
            .and_then(|d| d.value())
            .is_some_and(|v| v.is_finite())
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(if smoke.compact {
                "target/compact-diagnostics-smoke.png"
            } else {
                "target/diagnostics-smoke.png"
            }));
        smoke.debug_captured = true;
        smoke.debug_capture_time = smoke.started.elapsed().as_secs_f32();
        info!("GUI smoke: diagnostics opened with live FPS and CPU measurements");
    }
    if !smoke.captured
        && smoke.stage
            == if display.source == Source::RtlSdr {
                10
            } else {
                7
            }
        && smoke.started.elapsed().as_secs_f32() > 5.0
        && display.last_sequence > 20
        && display.cursor_hz < 0.0
        && (display.source != Source::RtlSdr
            || (display.status.revision == 6 && (display.station_mhz - 89.9).abs() < 0.001))
        && (display.source != Source::File || (display.status.revision == 3 && display.status.eof))
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(if smoke.compact {
                "target/compact-smoke.png"
            } else if display.source == Source::RtlSdr {
                "target/rtl-smoke.png"
            } else if display.source == Source::File {
                "target/file-smoke.png"
            } else {
                "target/gui-smoke.png"
            }));
        smoke.captured = true;
        let row = display.last_sequence;
        let revision = display.status.revision;
        info!(
            "GUI smoke: received spectrum row {}, revision {}",
            row, revision
        );
    }
    if smoke.started.elapsed().as_secs_f32()
        > if display.source != Source::Synthetic && !display.source.is_audio_radio() {
            15.0
        } else {
            9.0
        }
    {
        if smoke.captured && smoke.debug_captured && smoke.debug_stage == 4 && !display.debug_open {
            exit.write(AppExit::Success);
        } else {
            let debug_open = display.debug_open;
            info!(
                "Diagnostics smoke state: stage {}, open {}, FPS {:?}, CPU {:?}",
                smoke.debug_stage,
                debug_open,
                metrics
                    .get(&FrameTimeDiagnosticsPlugin::FPS)
                    .and_then(|d| d.smoothed()),
                metrics
                    .get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)
                    .and_then(|d| d.value())
            );
            let revision = display.status.revision;
            let running = display.status.running;
            let row = display.last_sequence;
            let failure = &display.status.error;
            error!(
                "GUI smoke timed out: stage {}, revision {}, running {}, row {}, error {:?}",
                smoke.stage, revision, running, row, failure
            );
            exit.write(AppExit::error());
        }
    }
}

#[derive(Resource)]
struct Display {
    converter_offset_hz: i64,
    converter_correction_hz: i32,
    qmx: qmx::Panel,
    ic705: ic705::Panel,
    practice: practice::PracticePanel,
    broadcast: broadcast::BroadcastPanel,
    digital: digital::Panel,
    memories: memories::Memories,
    scan_plan: Option<std::sync::Arc<sdr_app::scanning::ScanPlan>>,
    stop_scan_requested: bool,
    command_error: Option<String>,
    mode_settings: sdr_app::mode_settings::ModeSettings,
    controls_open: bool,
    debug_open: bool,
    debug_button_position: Option<egui::Pos2>,
    source_button_position: Option<egui::Pos2>,
    rtl_backend_positions: [Option<egui::Pos2>; 2],
    rtl_backend: RtlBackend,
    rtl_device_index: u32,
    rtl_device_serial: Option<String>,
    rtl_devices: Vec<sdr_devices::RtlDevice>,
    rtl_devices_scanned: bool,
    rtl_device_error: Option<String>,
    rtl_sample_rate: u32,
    rtl_gain_tenth_db: i32,
    rtl_center_hz: u32,
    tune_step_hz: u32,
    wheel_remainder: f32,
    row_centers: Vec<f64>,
    row_spans: Vec<f64>,
    labels: sdr_app::waterfall_labels::History,
    analysis_center: f64,
    analysis_rate: f64,
    received_view: Option<sdr_dsp::zoom_spectrum::SpectrumView>,
    texture_axis: Option<(f64, f64)>,
    drag_axis: Option<(f64, f64)>,
    fit_position: Option<egui::Pos2>,
    full_span_position: Option<egui::Pos2>,
    file_path: String,
    file_center_mhz: f64,
    running: bool,
    tone_hz: f64,
    source: Source,
    station_mhz: f64,
    receiver: ReceiverConfig,
    volume: f32,
    muted: bool,
    record_path: String,
    recorder: Option<Recorder>,
    record_message: Option<String>,
    revision: u64,
    pending: bool,
    status: RadioUpdate,
    spectrum: Vec<f32>,
    history: Vec<f32>,
    next_row: usize,
    last_sequence: u64,
    texture: Option<egui::TextureHandle>,
    floor: f32,
    ceiling: f32,
    zoom: f32,
    cursor_hz: f64,
    button_position: Option<egui::Pos2>,
    preset_position: Option<egui::Pos2>,
    plot_rect: Option<egui::Rect>,
    waterfall_rect: Option<egui::Rect>,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            converter_offset_hz: 0,
            converter_correction_hz: 0,
            qmx: qmx::Panel::default(),
            ic705: ic705::Panel::default(),
            practice: practice::PracticePanel::default(),
            memories: memories::Memories::default(),
            broadcast: broadcast::BroadcastPanel::default(),
            digital: digital::Panel::default(),
            scan_plan: None,
            stop_scan_requested: false,
            command_error: None,
            mode_settings: sdr_app::mode_settings::ModeSettings::default(),
            controls_open: true,
            debug_open: false,
            debug_button_position: None,
            source_button_position: None,
            rtl_backend_positions: [None; 2],
            rtl_backend: RtlBackend::default(),
            rtl_device_index: 0,
            rtl_device_serial: None,
            rtl_devices: Vec::new(),
            rtl_devices_scanned: false,
            rtl_device_error: None,
            rtl_sample_rate: SAMPLE_RATE,
            rtl_gain_tenth_db: 280,
            rtl_center_hz: 106_950_000,
            tune_step_hz: 100_000,
            wheel_remainder: 0.0,
            row_centers: vec![0.0; HISTORY],
            row_spans: vec![SAMPLE_RATE as f64; HISTORY],
            labels: sdr_app::waterfall_labels::History::new(HISTORY),
            analysis_center: 100_000_000.0,
            analysis_rate: SAMPLE_RATE as f64,
            received_view: None,
            texture_axis: None,
            drag_axis: None,
            fit_position: None,
            full_span_position: None,
            file_path: String::new(),
            file_center_mhz: 106.95,
            running: true,
            tone_hz: 125_000.0,
            source: Source::Synthetic,
            station_mhz: 106.7,
            receiver: ReceiverConfig::default(),
            volume: 0.15,
            muted: false,
            record_path: "recording.wav".into(),
            recorder: None,
            record_message: None,
            revision: 1,
            pending: false,
            status: RadioUpdate::default(),
            spectrum: vec![-140.0; FFT_SIZE],
            history: vec![-140.0; WATERFALL_WIDTH * HISTORY],
            next_row: 0,
            last_sequence: 0,
            texture: None,
            floor: -100.0,
            ceiling: 0.0,
            zoom: 1.0,
            cursor_hz: 125_000.0,
            button_position: None,
            preset_position: None,
            plot_rect: None,
            waterfall_rect: None,
        }
    }
}

impl Display {
    fn finish_recording(&mut self) {
        if let Some(recorder) = self.recorder.take() {
            let path = recorder.destination().display().to_string();
            self.record_message = Some(match recorder.finish() {
                Ok(frames) => format!("Saved {:.2} s to {path}", frames as f64 / 48_000.0),
                Err(error) => format!("Recording failed: {error}"),
            });
        }
    }

    fn capture_rate(&self) -> u32 {
        if self.source == Source::RtlSdr {
            self.rtl_sample_rate
        } else {
            SAMPLE_RATE
        }
    }
    fn vfo_limit(&self) -> u32 {
        self.capture_rate() / 2 - 200_000
    }
    fn refresh_rtl_devices(&mut self) {
        self.rtl_devices_scanned = true;
        #[cfg(feature = "rtl")]
        match sdr_devices::rtl::discover(self.rtl_backend) {
            Ok(devices) => {
                self.rtl_devices = devices;
                self.rtl_device_error = None;
            }
            Err(error) => {
                self.rtl_devices.clear();
                self.rtl_device_error = Some(error);
            }
        }
        #[cfg(not(feature = "rtl"))]
        {
            self.rtl_devices.clear();
            self.rtl_device_error = Some("RTL support was not compiled in".into());
        }
    }
    fn spectrum_view(&self) -> sdr_dsp::zoom_spectrum::SpectrumView {
        use sdr_dsp::zoom_spectrum::SpectrumView;
        if self.source.is_audio_radio() {
            return SpectrumView::full(SAMPLE_RATE as f64);
        }
        let rate = self.capture_rate() as f64;
        let span = (rate / self.zoom as f64).clamp(500.0, rate);
        let (lo, hi) = self.receiver.passband();
        let desired = if self.source == Source::Synthetic {
            self.tone_hz
        } else {
            self.station_mhz * 1e6 - self.center_hz() + (lo + hi) / 2.0
        };
        let margin = (rate - span) / 2.0;
        SpectrumView {
            center_offset_hz: desired.clamp(-margin, margin),
            span_hz: span,
        }
    }
    fn view_axis(&self) -> (f64, f64) {
        let view = self.spectrum_view();
        (self.center_hz() + view.center_offset_hz, view.span_hz)
    }
    fn fit_receiver(&mut self) {
        let (low, high) = self.receiver.passband();
        self.zoom = (self.capture_rate() as f64
            / ((high - low) * 1.4).clamp(500.0, self.capture_rate() as f64))
            as f32;
    }
    fn settings(&self) -> Settings {
        Settings {
            spectrum_view: self.spectrum_view(),
            converter_offset_hz: self.effective_converter_offset(),
            qmx: self.qmx.settings.clone(),
            ic705: self.ic705.settings.clone(),
            rds_enabled: self.broadcast.enabled,
            cw_decoder: self
                .broadcast
                .cw_enabled
                .then_some(self.broadcast.cw_config),
            cw_reset: self.broadcast.cw_reset,
            scan: self.scan_plan.clone(),
            revision: self.revision,
            running: self.running,
            tone_hz: self.tone_hz,
            source: self.source,
            station_hz: (self.station_mhz * 1e6).round() as u32,
            rtl_backend: self.rtl_backend,
            rtl_device_index: self.rtl_device_index,
            rtl_device_serial: self.rtl_device_serial.clone(),
            rtl_sample_rate: self.rtl_sample_rate,
            rtl_gain_tenth_db: self.rtl_gain_tenth_db,
            rtl_center_hz: Some(self.rtl_center_hz),
            receiver: self.receiver,
            file: (self.source == Source::File).then(|| {
                std::sync::Arc::new(FileSettings {
                    path: self.file_path.clone().into(),
                    sample_rate: SAMPLE_RATE,
                    center_hz: (self.file_center_mhz * 1e6).round() as u64,
                })
            }),
        }
    }
    fn effective_converter_offset(&self) -> i64 {
        self.converter_offset_hz
            .saturating_add(self.converter_correction_hz as i64)
    }
    fn center_hz(&self) -> f64 {
        match self.source {
            Source::Synthetic => sdr_app::controller::CENTER_HZ as f64,
            Source::RtlSdr => self.rtl_center_hz as f64,
            Source::File => self.file_center_mhz * 1e6,
            Source::Qmx | Source::Ic705 => self.station_mhz * 1e6,
        }
    }
    fn reset(&mut self) {
        self.spectrum.resize(FFT_SIZE, -140.0);
        self.spectrum.fill(-140.0);
        self.analysis_center = self.center_hz();
        self.analysis_rate = self.capture_rate() as f64;
        self.received_view = None;
        self.texture_axis = None;
        self.drag_axis = None;
        self.history.fill(-140.0);
        self.next_row = 0;
        self.last_sequence = 0;
        self.row_centers.fill(0.0);
        let capture_rate = self.capture_rate() as f64;
        self.row_spans.fill(capture_rate);
        self.labels.clear();
    }

    fn tuning(&self) -> Tuning {
        Tuning {
            converter_offset_hz: self.effective_converter_offset(),
            center_hz: self.center_hz().round() as u32,
            station_hz: (self.station_mhz * 1e6).round() as u32,
        }
    }

    fn apply_tuning(&mut self, tuning: Tuning) {
        let stopped_scan = self.scan_plan.take().is_some();
        self.stop_scan_requested = false;
        if self.tuning() == tuning {
            if stopped_scan {
                self.revision += 1;
                self.pending = true;
            }
            return;
        }
        let old_center = self.center_hz();
        self.station_mhz = tuning.station_hz as f64 / 1e6;
        if self.source == Source::RtlSdr {
            self.rtl_center_hz = tuning.center_hz;
        }
        self.cursor_hz = tuning.station_hz as f64 - self.center_hz();
        if self.center_hz() != old_center {
            self.spectrum.fill(-140.0);
            self.texture_axis = None; // Reproject retained rows onto the new frequency axis.
        }
        self.revision += 1;
        self.pending = true;
    }

    fn row(&mut self, values: &[f32]) {
        #[cfg(feature = "perfetto")]
        let _span = info_span!("sdr.ui.waterfall_row").entered();
        let (center, span) = self.view_axis();
        let values = if self.source.is_audio_radio() {
            sdr_app::spectrum_view::project(
                values,
                center,
                SAMPLE_RATE as f64,
                center,
                SAMPLE_RATE as f64,
                WATERFALL_WIDTH,
            )
        } else {
            sdr_app::spectrum_view::project(
                values,
                self.analysis_center,
                self.analysis_rate,
                center,
                span,
                WATERFALL_WIDTH,
            )
        };
        let start = self.next_row * WATERFALL_WIDTH;
        self.history[start..start + WATERFALL_WIDTH].copy_from_slice(&values);
        self.row_centers[self.next_row] = center;
        self.row_spans[self.next_row] = span;
        if let Some(texture) = &mut self.texture {
            let colors = values
                .iter()
                .map(|value| palette(*value, self.floor, self.ceiling))
                .collect();
            texture.set_partial(
                [0, self.next_row],
                egui::ColorImage::new([WATERFALL_WIDTH, 1], colors),
                egui::TextureOptions::NEAREST,
            );
        }
        self.next_row = (self.next_row + 1) % HISTORY;
    }

    fn upload(&mut self, context: &egui::Context) {
        #[cfg(feature = "perfetto")]
        let _span = info_span!("sdr.ui.waterfall_recolor_and_texture_enqueue").entered();
        let mut colors = Vec::with_capacity(self.history.len());
        let (center, span) = self.view_axis();
        for (row, values) in self.history.chunks_exact(WATERFALL_WIDTH).enumerate() {
            let values = if self.row_centers[row] == 0.0 {
                vec![-140.0; WATERFALL_WIDTH]
            } else {
                sdr_app::spectrum_view::project(
                    values,
                    if self.source.is_audio_radio() {
                        center
                    } else {
                        self.row_centers[row]
                    },
                    self.row_spans[row],
                    center,
                    span,
                    WATERFALL_WIDTH,
                )
            };
            colors.extend(
                values
                    .into_iter()
                    .map(|v| palette(v, self.floor, self.ceiling)),
            );
        }
        self.texture_axis = Some((center, span));
        let image = egui::ColorImage::new([WATERFALL_WIDTH, HISTORY], colors);
        if let Some(texture) = &mut self.texture {
            // Keep the Bevy image asset instead of creating a new one for every
            // zoom, retune or restart. The waterfall dimensions stay fixed.
            texture.set_partial([0, 0], image, egui::TextureOptions::NEAREST);
        } else {
            self.texture =
                Some(context.load_texture("waterfall", image, egui::TextureOptions::NEAREST));
        }
    }
}

fn palette(db: f32, floor: f32, ceiling: f32) -> egui::Color32 {
    if !db.is_finite() {
        return egui::Color32::from_rgb(55, 28, 35);
    }
    let t = ((db - floor) / (ceiling - floor)).clamp(0.0, 1.0);
    let stops = [
        [7.0, 14.0, 23.0],
        [16.0, 52.0, 94.0],
        [28.0, 150.0, 165.0],
        [141.0, 224.0, 176.0],
        [255.0, 220.0, 115.0],
    ];
    let position = t * 4.0;
    let index = (position as usize).min(3);
    let mix = position - index as f32;
    let color: [u8; 3] = std::array::from_fn(|channel| {
        (stops[index][channel] * (1.0 - mix) + stops[index + 1][channel] * mix) as u8
    });
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

#[allow(
    clippy::too_many_arguments,
    reason = "Bevy injects these independent system resources"
)]
fn draw(
    mut contexts: EguiContexts,
    mut display: ResMut<Display>,
    client: Res<RadioClient>,
    metrics: Res<DiagnosticsStore>,
    adapter: Option<Res<RenderAdapterInfo>>,
    system: Option<Res<SystemInfo>>,
    persistence: Option<Res<settings::Persistence>>,
    digital_service: Res<digital::Service>,
) -> bevy::prelude::Result {
    let context = contexts.ctx_mut()?;
    #[cfg(feature = "perfetto")]
    let _span = info_span!("sdr.ui.frame").entered();
    context.set_visuals(egui::Visuals::dark());
    if let Some(warning) = persistence.as_ref().and_then(|p| p.warning()) {
        egui::TopBottomPanel::bottom("settings_warning").show(context, |ui| {
            ui.colored_label(AMBER, warning);
        });
    }
    if context.input(|input| input.key_pressed(egui::Key::F12)) {
        display.debug_open = !display.debug_open;
    }
    let controller = &client.0;
    display.memories.poll();
    if !display.source.is_audio_radio() && display.memories.take_recall() {
        let index = display.memories.selected;
        display.recall_memory(index);
    }
    if let Some(mut update) = controller.poll() {
        if display.source.is_audio_radio()
            && !display.pending
            && update.revision == display.revision
            && (update.qmx.is_some() || update.ic705.is_some())
        {
            display.station_mhz = update.station_hz as f64 / 1e6;
            display.receiver = update.receiver;
        }
        if !display.pending && update.revision == display.revision && display.scan_plan.is_some() {
            if update.center_hz != display.center_hz().round() as u64 {
                display.spectrum.fill(-140.0);
                display.texture_axis = None;
            }
            display.station_mhz = update.station_hz as f64 / 1e6;
            if display.source == Source::RtlSdr {
                display.rtl_center_hz = update.center_hz as u32;
            }
            display.receiver = update.receiver;
            display.cursor_hz = display.station_mhz * 1e6 - display.center_hz();
            if let Some(index) = update.scan.memory_index {
                display.memories.selected = index;
            }
            if update.scan.phase == sdr_app::scanning::ScanPhase::Off {
                display.scan_plan = None;
                display.stop_scan_requested = false;
            }
        }
        if let Some(frame) = update.frame.take()
            && frame.revision == display.revision
            && frame.center_hz == display.center_hz().round() as u64
            && (display.source.is_audio_radio() || frame.view == display.spectrum_view())
        {
            let missing = frame
                .row
                .saturating_sub(display.last_sequence + 1)
                .min(HISTORY as u64);
            let blank = vec![f32::NAN; FFT_SIZE];
            for _ in 0..missing {
                display.row(&blank);
                display.labels.push(None);
            }
            display.analysis_center = frame.center_hz as f64 + frame.view.center_offset_hz;
            display.analysis_rate = frame.analysis_rate_hz;
            display.received_view = Some(frame.view);
            display.row(&frame.dbfs);
            let seconds = if display.source == Source::File {
                frame.stream_seconds
            } else {
                frame.end_time.as_secs_f64()
            };
            display.labels.push(Some(sdr_app::waterfall_labels::Stamp {
                seconds,
                revision: frame.revision,
                capture: frame.capture_revision,
            }));
            display.spectrum = frame.dbfs;
            display.last_sequence = frame.row;
        }
        if update.revision == display.revision && (update.eof || update.error.is_some()) {
            display.running = false;
        }
        display.status = update;
    }
    if display.recorder.as_ref().is_some_and(Recorder::is_finished)
        || (display.recorder.is_some() && !display.status.running)
    {
        display.finish_recording();
    }
    let mut changed = false;
    if display.source.is_audio_radio() && !context.wants_keyboard_input() {
        let steps = context.input(|i| {
            i64::from(i.key_pressed(egui::Key::ArrowRight))
                - i64::from(i.key_pressed(egui::Key::ArrowLeft))
        });
        if steps != 0 {
            qmx::step(&mut display, steps);
        }
    }
    if !display.source.is_audio_radio()
        && display.memories.memory_mode
        && !context.wants_keyboard_input()
    {
        let step = context.input(|input| {
            i64::from(input.key_pressed(egui::Key::ArrowRight))
                - i64::from(input.key_pressed(egui::Key::ArrowLeft))
        });
        if step != 0 {
            display.memory_step(step);
        }
    }
    let before = display.settings();
    let mut recolor = false;
    changed |= controls::header(context, &mut display, controller.capabilities());
    let view = &mut *display;
    diagnostics::show(
        context,
        &mut view.debug_open,
        &view.status,
        &metrics,
        adapter.as_deref(),
        system.as_deref(),
    );
    let panel = controls::show(context, &mut display, controller);
    changed |= panel.0;
    recolor |= panel.1;
    if !display.source.is_audio_radio() {
        memories::show(context, &mut display);
    }
    practice::show(context, &mut display.practice);
    // Decoder-only edits submit a revision without canceling scans, rebuilding
    // the audio receiver or resetting waterfall history.
    if broadcast::show(context, &mut display) {
        display.revision += 1;
        display.pending = true;
    }
    if changed {
        display.scan_plan = None;
        display.stop_scan_requested = false;
        if display.source != Source::Synthetic && !display.source.is_audio_radio() {
            let mut t = display.tuning();
            t.tune_with_limit(
                t.station_hz,
                1,
                display.source == Source::File,
                display.vfo_limit(),
            );
            display.station_mhz = t.station_hz as f64 / 1e6;
            if display.source == Source::RtlSdr {
                display.rtl_center_hz = t.center_hz;
            }
            display.cursor_hz = t.station_hz as f64 - display.center_hz();
        }
        display.revision += 1;
        display.pending = true;
        let after = display.settings();
        if before.source != after.source {
            display.finish_recording();
        }
        if before.source != after.source
            || before.rtl_backend != after.rtl_backend
            || before.rtl_device_index != after.rtl_device_index
            || before.rtl_device_serial != after.rtl_device_serial
            || before.rtl_sample_rate != after.rtl_sample_rate
            || before.rtl_gain_tenth_db != after.rtl_gain_tenth_db
            || before.running != after.running
            || before.file != after.file
            || before.tone_hz != after.tone_hz
            || before.converter_offset_hz != after.converter_offset_hz
        {
            display.reset();
        } else if before.center_hz() != after.center_hz() {
            display.spectrum.fill(-140.0);
            display.texture_axis = None;
        }
    }
    if display.pending {
        match controller.submit(Command::Configure(Box::new(display.settings()))) {
            Ok(()) => {
                display.pending = false;
                display.command_error = None;
            }
            Err(SubmitError::Busy) => {}
            Err(error) => {
                display.pending = false;
                display.command_error = Some(format!("Radio command rejected: {error:?}"));
            }
        }
    }
    if display.stop_scan_requested {
        match controller.submit(Command::StopScan {
            revision: display.revision,
        }) {
            Ok(()) | Err(SubmitError::Busy) => {}
            Err(error) => {
                display.stop_scan_requested = false;
                display.command_error = Some(format!("Cannot stop scan: {error:?}"));
            }
        }
    }
    controller.set_volume(if display.muted { 0.0 } else { display.volume });
    digital::update(
        context,
        &mut display,
        &digital_service.0,
        &digital_service.1,
    );
    let _ = controller.submit(Command::SpectrumView(display.spectrum_view()));
    if display.texture.is_none() || recolor || display.texture_axis != Some(display.view_axis()) {
        display.upload(context);
    }
    if digital::compact_text(context, &mut display, &digital_service.0) {
        return Ok(());
    }
    if display.source.is_audio_radio() {
        qmx::plot(context, &mut display);
        return Ok(());
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(9, 16, 24))
                .inner_margin(18.0),
        )
        .show(context, |ui| {
            broadcast::ticker(ui, &mut display);
            let width = ui.available_width().max(1.0);
            let height = (ui.available_height() * 0.40).max(100.0);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
            display.plot_rect = Some(rect);
            inspect(&response, rect, &mut display);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(7, 13, 20));
            for line in 0..=5 {
                let y = rect.top() + rect.height() * line as f32 / 5.0;
                painter.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    egui::Stroke::new(1.0, egui::Color32::from_gray(35)),
                );
                let db = display.ceiling - (display.ceiling - display.floor) * line as f32 / 5.0;
                painter.text(
                    egui::pos2(rect.left() + 6.0, y + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{db:.0}"),
                    egui::FontId::monospace(11.0),
                    egui::Color32::GRAY,
                );
            }
            let pixels = (width as usize).max(1);
            let (view_center, view_span) = display.view_axis();
            let values = sdr_app::spectrum_view::project(
                &display.spectrum,
                display.analysis_center,
                display.analysis_rate,
                view_center,
                view_span,
                pixels,
            );
            let points: Vec<_> = values
                .into_iter()
                .enumerate()
                .map(|(x, db)| {
                    let db = if db.is_finite() { db } else { display.floor };
                    let level =
                        ((db - display.floor) / (display.ceiling - display.floor)).clamp(0.0, 1.0);
                    egui::pos2(
                        rect.left() + x as f32,
                        rect.bottom() - level * rect.height(),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.3, CYAN)));
            cursor(&painter, rect, &display);
            let (labels, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
            let half = display.view_axis().1 / 2.0;
            for (x, align, hz, color) in [
                (
                    labels.left(),
                    egui::Align2::LEFT_CENTER,
                    display.view_axis().0 - half,
                    egui::Color32::GRAY,
                ),
                (
                    labels.center().x,
                    egui::Align2::CENTER_CENTER,
                    display.view_axis().0,
                    CYAN,
                ),
                (
                    labels.right(),
                    egui::Align2::RIGHT_CENTER,
                    display.view_axis().0 + half,
                    egui::Color32::GRAY,
                ),
            ] {
                ui.painter_at(labels).text(
                    egui::pos2(x, labels.center().y),
                    align,
                    format!("{:.4} MHz", hz / 1e6),
                    egui::FontId::monospace(12.0),
                    color,
                );
            }
            ui.add_space(6.0);
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(width, ui.available_height().max(40.0)),
                egui::Sense::click_and_drag(),
            );
            display.waterfall_rect = Some(rect);
            inspect(&response, rect, &mut display);
            if let Some(texture) = &display.texture {
                let split = display.next_row as f32 / HISTORY as f32;
                let middle = rect.top() + rect.height() * split;
                let u0 = 0.0;
                let u1 = 1.0;
                // Reverse row order and wrap using two textured rectangles. Only a
                // new row is uploaded; history is never shifted or copied per frame.
                if split > 0.0 {
                    ui.painter().image(
                        texture.id(),
                        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), middle)),
                        egui::Rect::from_min_max(egui::pos2(u0, split), egui::pos2(u1, 0.0)),
                        egui::Color32::WHITE,
                    );
                }
                if split < 1.0 {
                    ui.painter().image(
                        texture.id(),
                        egui::Rect::from_min_max(egui::pos2(rect.left(), middle), rect.max),
                        egui::Rect::from_min_max(egui::pos2(u0, 1.0), egui::pos2(u1, split)),
                        egui::Color32::WHITE,
                    );
                }
            }
            cursor(ui.painter(), rect, &display);
            digital::paint_labels(ui, rect, &display);
        });
    Ok(())
}

fn inspect(response: &egui::Response, rect: egui::Rect, display: &mut Display) {
    if response.drag_started() {
        display.drag_axis = Some(display.view_axis());
    }
    if (response.clicked() || response.dragged())
        && let Some(position) = response.interact_pointer_pos()
    {
        let axis = display.drag_axis.unwrap_or_else(|| display.view_axis());
        display.cursor_hz = axis.0 - display.center_hz()
            + ((position.x - rect.left()) / rect.width() - 0.5) as f64 * axis.1;
        if display.source != Source::Synthetic && !display.source.is_audio_radio() {
            let mut t = display.tuning();
            t.tune_with_limit(
                (display.center_hz() + display.cursor_hz).round() as u32,
                display.tune_step_hz,
                display.source == Source::File,
                display.vfo_limit(),
            );
            display.apply_tuning(t);
            display.cursor_hz = display.station_mhz * 1e6 - display.center_hz();
        }
    }
    if response.drag_stopped() {
        display.drag_axis = None;
    }
    if response.hovered() && display.source != Source::Synthetic {
        let (delta, shift) = response.ctx.input(|input| {
            let mut delta = 0.0;
            for event in &input.events {
                if let egui::Event::MouseWheel {
                    unit,
                    delta: movement,
                    ..
                } = event
                {
                    delta += match unit {
                        egui::MouseWheelUnit::Line => movement.y,
                        egui::MouseWheelUnit::Point => movement.y / 40.0,
                        egui::MouseWheelUnit::Page => movement.y,
                    };
                }
            }
            (delta, input.modifiers.shift)
        });
        display.wheel_remainder += delta;
        let steps = display.wheel_remainder.trunc() as i64;
        display.wheel_remainder -= steps as f32;
        if steps != 0 {
            if display.memories.memory_mode {
                display.memory_step(steps);
                return;
            }
            let mut t = display.tuning();
            let delta = steps * display.tune_step_hz as i64;
            if shift || display.source == Source::File {
                t.tune_with_limit(
                    (t.station_hz as i64 + delta).clamp(1, sdr_core::RX_MAX_HZ as i64) as u32,
                    display.tune_step_hz,
                    display.source == Source::File,
                    display.vfo_limit(),
                );
            } else {
                t.pan(delta);
            }
            display.apply_tuning(t);
        }
    }
}

fn cursor(painter: &egui::Painter, rect: egui::Rect, display: &Display) {
    let fraction = (display.center_hz() + display.cursor_hz - display.view_axis().0)
        / display.view_axis().1
        + 0.5;
    if (0.0..=1.0).contains(&fraction) {
        let x = rect.left() + fraction as f32 * rect.width();
        if display.source != Source::Synthetic && !display.source.is_audio_radio() {
            let (low, high) = display.receiver.passband();
            let scale = display.zoom / display.capture_rate() as f32 * rect.width();
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(
                        (x + low as f32 * scale).clamp(rect.left(), rect.right()),
                        rect.top(),
                    ),
                    egui::pos2(
                        (x + high as f32 * scale).clamp(rect.left(), rect.right()),
                        rect.bottom(),
                    ),
                ),
                0.0,
                AMBER.linear_multiply(0.08),
            );
        }
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, AMBER),
        );
    }
}
