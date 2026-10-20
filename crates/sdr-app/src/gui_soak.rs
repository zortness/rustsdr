//! Instrumented long-run reception soak supporting the M01 stability and memory gates.
//!
//! Samples engine counters and Bevy system diagnostics on a fixed interval while a
//! scripted schedule exercises window resize, spectrum zoom, VFO tuning, capture-center
//! hardware retune and reception stop/start. Engine counters restart from zero whenever
//! the worker re-enters a live session, so every counter here is accumulated across
//! revisions instead of being read as a final value. The report separates unexplained
//! IQ drops from intentional retune discards, which is the distinction the M01 gate needs.

use super::*;
use std::fs::File;
use std::io::Write;

/// M01 gate: the steady-state scenario must show no unexplained IQ drops.
/// Intentional retune discards are tracked separately and are not a failure.
const MAX_IQ_DROPS: u64 = 0;
/// M01 gate: no audio underruns or overruns during the scenario.
const MAX_AUDIO_FAULTS: u64 = 0;
/// Long-run memory must not trend upward. Least-squares slope limit, MiB per hour.
const MAX_MEM_SLOPE_MIB_PER_HOUR: f64 = 10.0;
/// Median of the final quarter minus median of the first quarter, in MiB.
const MAX_MEM_STEP_MIB: f64 = 16.0;
const MIN_ACCEPTANCE_SECONDS: f64 = 1800.0;

/// One scripted interaction stage followed by an idle observation window.
const PHASES: &[(&str, f32)] = &[
    ("settle", 40.0),
    ("resize", 25.0),
    ("zoom-fit", 25.0),
    ("zoom-full", 25.0),
    ("tune-vfo", 30.0),
    ("center-scroll", 30.0),
    ("restart", 25.0),
];

/// Counter that tolerates the engine restarting a value when a live session re-enters.
#[derive(Default)]
struct Cumulative {
    closed: u64,
    current: u64,
}

impl Cumulative {
    fn observe(&mut self, value: u64) {
        if value < self.current {
            self.closed += self.current;
        }
        self.current = value;
    }

    fn total(&self) -> u64 {
        self.closed + self.current
    }
}

#[derive(Resource)]
pub(super) struct Soak {
    started: std::time::Instant,
    duration_s: f64,
    interval_s: f64,
    next_sample: f64,
    csv: Option<File>,
    output_error: Option<String>,
    report_path: String,
    source_label: String,
    backend_label: String,
    rows: u64,
    phase: usize,
    phase_started: f64,
    action_done: bool,
    pending: Option<(usize, f64)>,
    verified: [bool; 6],
    restart_stage: u8,
    restart_at: f64,
    tune_index: usize,
    unverified_actions: u32,
    release: Option<egui::Pos2>,
    wheel: Option<f32>,
    resize_index: usize,
    events: Vec<String>,
    samples: Cumulative,
    display_drops: Cumulative,
    iq_drops: Cumulative,
    retune_discarded: Cumulative,
    underruns: Cumulative,
    overruns: Cumulative,
    memory_mib: Vec<(f64, f64)>,
    fps: Vec<f64>,
    cpu_pct: Vec<f64>,
    input_rates: Vec<f64>,
    last_rate_sample: Option<(f64, u64, u64)>,
    peak_mem_mib: f64,
    peak_audio_queue_frames: usize,
    peak_image_assets: usize,
    audio_output_rate_hz: Option<u32>,
    audio_output_device: Option<String>,
    audio_errors: u32,
    engine_errors: u32,
    stalled_ticks: u32,
    last_samples: u64,
    finished: bool,
    finish_at: f64,
    passed: bool,
}

const RESOLUTIONS: [(f32, f32); 3] = [(1100.0, 760.0), (900.0, 700.0), (1280.0, 820.0)];

impl Soak {
    pub(super) fn new(duration_s: f64, interval_s: f64, source_label: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let directory = format!("target/soak/{stamp}-{source_label}");
        if let Err(error) = std::fs::create_dir_all(&directory) {
            warn!("Soak: cannot create {directory}: {error}");
        }
        let source_label = source_label.to_owned();
        let csv_path = format!("{directory}/soak.csv");
        let mut output_error = None;
        let mut csv = match File::create(&csv_path) {
            Ok(file) => Some(file),
            Err(error) => {
                warn!("Soak: cannot create {csv_path}: {error}");
                output_error = Some(format!("cannot create {csv_path}: {error}"));
                None
            }
        };
        if let Some(file) = csv.as_mut()
            && let Err(error) = writeln!(
                file,
                "elapsed_s,phase,rtl_tuner_center_hz,samples,display_drops,iq_drops,retune_discarded,\
                 audio_underruns,audio_overruns,audio_queue_frames,audio_output_rate_hz,audio_output_device,bevy_image_assets,process_mem_mib,app_cpu_pct,fps,frame_time_ms,\
                 capture_center_hz,station_hz,revision,capture_revision,stereo_locked,channel_power_dbfs,\
                 audio_rms,audio_error,engine_error"
            )
        {
            output_error = Some(format!("cannot write {csv_path}: {error}"));
        }
        info!("Soak: writing {csv_path}");
        Self {
            started: std::time::Instant::now(),
            duration_s,
            interval_s,
            next_sample: 1.0,
            csv,
            output_error,
            report_path: format!("{directory}/soak-summary.json"),
            source_label,
            backend_label: "n/a".into(),
            rows: 0,
            phase: 0,
            phase_started: 0.0,
            action_done: false,
            pending: None,
            verified: [false; 6],
            restart_stage: 0,
            restart_at: 0.0,
            tune_index: 0,
            unverified_actions: 0,
            release: None,
            wheel: None,
            resize_index: 0,
            events: Vec::new(),
            samples: Cumulative::default(),
            display_drops: Cumulative::default(),
            iq_drops: Cumulative::default(),
            retune_discarded: Cumulative::default(),
            underruns: Cumulative::default(),
            overruns: Cumulative::default(),
            memory_mib: Vec::new(),
            fps: Vec::new(),
            cpu_pct: Vec::new(),
            input_rates: Vec::new(),
            last_rate_sample: None,
            peak_mem_mib: 0.0,
            peak_audio_queue_frames: 0,
            peak_image_assets: 0,
            audio_output_rate_hz: None,
            audio_output_device: None,
            audio_errors: 0,
            engine_errors: 0,
            stalled_ticks: 0,
            last_samples: 0,
            finished: false,
            finish_at: 0.0,
            passed: false,
        }
    }

    pub(super) fn start(&mut self) {
        self.started = std::time::Instant::now();
    }

    fn observe(
        &mut self,
        display: &Display,
        metrics: &DiagnosticsStore,
        elapsed: f64,
        image_assets: usize,
    ) {
        let status = &display.status;
        self.peak_image_assets = self.peak_image_assets.max(image_assets);
        if let Some(rtl) = &status.rtl {
            self.backend_label = rtl.backend.label().to_owned();
        }
        if status.running && status.samples > 0 {
            if let Some((prior_time, prior_samples, prior_capture)) = self.last_rate_sample
                && prior_capture == status.capture_revision
                && status.samples > prior_samples
            {
                self.input_rates
                    .push((status.samples - prior_samples) as f64 / (elapsed - prior_time));
            }
            self.last_rate_sample = Some((elapsed, status.samples, status.capture_revision));
        } else {
            self.last_rate_sample = None;
        }
        if let Some(queued) = status.audio_queue_frames {
            self.peak_audio_queue_frames = self.peak_audio_queue_frames.max(queued);
        }
        if let Some(rate) = status.audio_output_rate_hz {
            self.audio_output_rate_hz = Some(rate);
        }
        if let Some(device) = &status.audio_output_device {
            self.audio_output_device = Some(device.clone());
        }
        if status.audio_error.is_some() {
            self.audio_errors += 1;
        }
        if status.error.is_some() {
            self.engine_errors += 1;
        }
        // A live session that is running must keep delivering samples.
        if self.rows > 0
            && status.running
            && status.samples > 0
            && status.samples == self.last_samples
        {
            self.stalled_ticks += 1;
        }
        self.last_samples = status.samples;

        let mem_gib = value(
            metrics,
            &SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE,
        );
        let cpu = value(
            metrics,
            &SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE,
        );
        let fps = smoothed(metrics, &FrameTimeDiagnosticsPlugin::FPS);
        let frame_time = smoothed(metrics, &FrameTimeDiagnosticsPlugin::FRAME_TIME);
        if let Some(gib) = mem_gib {
            // Bevy reports process memory in GiB as f64; keep full precision in MiB.
            let mib = gib * 1024.0;
            self.peak_mem_mib = self.peak_mem_mib.max(mib);
            if elapsed >= PHASES[0].1 as f64 && status.running && status.samples > 0 {
                self.memory_mib.push((elapsed, mib));
            }
        }
        if let Some(cpu) = cpu {
            self.cpu_pct.push(cpu);
        }
        if let Some(fps) = fps {
            self.fps.push(fps);
        }
        if let Some(csv) = &mut self.csv {
            let row: Vec<String> = vec![
                format!("{elapsed:.1}"),
                PHASES[self.phase].0.to_owned(),
                status
                    .rtl
                    .as_ref()
                    .map(|r| r.center_hz.to_string())
                    .unwrap_or_default(),
                status.samples.to_string(),
                status.display_drops.to_string(),
                status.iq_drops.to_string(),
                status.retune_discarded.to_string(),
                status.audio_underruns.to_string(),
                status.audio_overruns.to_string(),
                status
                    .audio_queue_frames
                    .map_or(String::new(), |v| v.to_string()),
                status
                    .audio_output_rate_hz
                    .map_or(String::new(), |v| v.to_string()),
                status.audio_output_device.clone().unwrap_or_default(),
                image_assets.to_string(),
                mem_gib.map_or(String::new(), |v| format!("{:.4}", v * 1024.0)),
                cpu.map_or(String::new(), |v| format!("{:.2}", v)),
                fps.map_or(String::new(), |v| format!("{:.2}", v)),
                frame_time.map_or(String::new(), |v| format!("{:.3}", v)),
                status.center_hz.to_string(),
                status.station_hz.to_string(),
                status.revision.to_string(),
                status.capture_revision.to_string(),
                status.stereo_locked.to_string(),
                status
                    .channel_power_dbfs
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_default(),
                format!("{:.4}", status.audio_rms),
                status.audio_error.clone().unwrap_or_default(),
                status.error.clone().unwrap_or_default(),
            ];
            let encoded = row
                .iter()
                .map(|cell| csv_cell(cell))
                .collect::<Vec<_>>()
                .join(",");
            if let Err(error) = writeln!(csv, "{encoded}") {
                self.output_error = Some(format!("cannot write soak CSV: {error}"));
            }
        }
        self.rows += 1;
        info!(
            "Soak t={elapsed:.0}s samples={} iqDrops={} retuneDiscarded={} \
             displayDrops={} audioFaults={} memMiB={:.1} fps={:.1}",
            self.samples.total(),
            self.iq_drops.total(),
            self.retune_discarded.total(),
            self.display_drops.total(),
            self.underruns.total() + self.overruns.total(),
            self.peak_mem_mib,
            fps.unwrap_or(0.0),
        );
    }

    fn observe_counters(&mut self, status: &sdr_app::controller::Update) {
        // Watch every GUI frame so a stop/start cannot reset a counter between
        // the slower CSV samples. This is still a host-side observation.
        self.samples.observe(status.samples);
        self.display_drops.observe(status.display_drops);
        self.iq_drops.observe(status.iq_drops);
        self.retune_discarded.observe(status.retune_discarded);
        self.underruns.observe(status.audio_underruns);
        self.overruns.observe(status.audio_overruns);
    }

    /// Least-squares slope of the memory series in MiB per hour.
    fn memory_slope(&self) -> Option<f64> {
        let n = self.memory_mib.len();
        if n < 4 {
            return None;
        }
        let mean_x = self.memory_mib.iter().map(|(x, _)| *x).sum::<f64>() / n as f64;
        let mean_y = self.memory_mib.iter().map(|(_, y)| *y).sum::<f64>() / n as f64;
        let numerator = self
            .memory_mib
            .iter()
            .map(|(x, y)| (x - mean_x) * (y - mean_y))
            .sum::<f64>();
        let denominator = self
            .memory_mib
            .iter()
            .map(|(x, _)| (x - mean_x) * (x - mean_x))
            .sum::<f64>();
        if denominator == 0.0 {
            return None;
        }
        Some(numerator / denominator * 3600.0)
    }

    fn quartile_step(&self) -> Option<f64> {
        if self.memory_mib.len() < 8 {
            return None;
        }
        let quarter = self.memory_mib.len() / 4;
        let median = |slice: &[f64]| -> f64 {
            let mut values = slice.to_vec();
            values.sort_by(|a, b| a.total_cmp(b));
            let mid = slice.len() / 2;
            if slice.len() % 2 == 1 {
                values[mid]
            } else {
                (values[mid - 1] + values[mid]) * 0.5
            }
        };
        let values: Vec<f64> = self.memory_mib.iter().map(|(_, y)| *y).collect();
        Some(median(&values[values.len() - quarter..]) - median(&values[..quarter]))
    }

    fn finish(
        &mut self,
        display: &Display,
        system: Option<&SystemInfo>,
        adapter: Option<&RenderAdapterInfo>,
    ) -> bool {
        let elapsed = self.started.elapsed().as_secs_f64();
        let samples = self.samples.total();
        let iq_drops = self.iq_drops.total();
        let retune_discarded = self.retune_discarded.total();
        let display_drops = self.display_drops.total();
        let audio_faults = self.underruns.total() + self.overruns.total();
        let slope = self.memory_slope();
        let step = self.quartile_step();

        let mut failures: Vec<String> = Vec::new();
        if let Some(error) = &self.output_error {
            failures.push(error.clone());
        }
        if iq_drops > MAX_IQ_DROPS {
            failures.push(format!("unexplained IQ drops {iq_drops} > {MAX_IQ_DROPS}"));
        }
        if audio_faults > MAX_AUDIO_FAULTS {
            failures.push(format!(
                "audio underruns/overruns {audio_faults} > {MAX_AUDIO_FAULTS}"
            ));
        }
        if self.audio_errors > 0 {
            failures.push(format!(
                "audio output failed on {} samples",
                self.audio_errors
            ));
        }
        if self.engine_errors > 0 {
            failures.push(format!("engine fault on {} samples", self.engine_errors));
        }
        if self.stalled_ticks > 0 {
            failures.push(format!(
                "sample stream made no progress on {} consecutive samples",
                self.stalled_ticks
            ));
        }
        if self.unverified_actions > 0 {
            failures.push(format!(
                "{} scripted interactions had no observed effect",
                self.unverified_actions
            ));
        }
        for (index, name) in [
            "resize",
            "zoom-fit",
            "zoom-full",
            "tune-vfo",
            "center-scroll",
            "restart",
        ]
        .iter()
        .enumerate()
        {
            if *name == "center-scroll" && self.source_label == "file" {
                continue;
            }
            if !self.verified[index] {
                failures.push(format!("interaction {name} was not verified"));
            }
        }
        if self.duration_s < MIN_ACCEPTANCE_SECONDS {
            failures.push(format!(
                "diagnostic run was shorter than the {MIN_ACCEPTANCE_SECONDS:.0}s M01 duration gate"
            ));
        } else if let Some(slope) = slope {
            if slope > MAX_MEM_SLOPE_MIB_PER_HOUR {
                failures.push(format!(
                    "memory slope {slope:.2} MiB/h > {MAX_MEM_SLOPE_MIB_PER_HOUR}"
                ));
            }
        } else {
            failures.push("memory series too short to evaluate a trend".into());
        }
        if self.duration_s >= MIN_ACCEPTANCE_SECONDS
            && let Some(step) = step
            && step > MAX_MEM_STEP_MIB
        {
            failures.push(format!(
                "memory quartile step {step:.2} MiB > {MAX_MEM_STEP_MIB}"
            ));
        }
        if elapsed < self.duration_s - self.interval_s * 2.0 {
            failures.push(format!("soak ended early at {elapsed:.1}s"));
        }

        let median = |mut values: Vec<f64>| -> Option<f64> {
            if values.is_empty() {
                return None;
            }
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = values.len() / 2;
            Some(if values.len() % 2 == 1 {
                values[mid]
            } else {
                (values[mid - 1] + values[mid]) * 0.5
            })
        };

        let passed = failures.is_empty();
        let quote = |text: &str| {
            let encoded = serde_json::to_string(text).expect("string is JSON serializable");
            encoded[1..encoded.len() - 1].to_owned()
        };
        let summary = format!(
            "{{\n  \
             \"passed\": {passed},\n  \
             \"elapsed_s\": {elapsed:.3},\n  \
             \"requested_duration_s\": {},\n  \
             \"sample_rows\": {},\n  \
             \"source\": \"{}\",\n  \
             \"sample_rate_hz\": {SAMPLE_RATE},\n  \
             \"fft_size\": {FFT_SIZE},\n  \
             \"mode\": \"{}\",\n  \
             \"rtl_backend\": \"{}\",\n  \
             \"delivered_samples\": {samples},\n  \
             \"median_input_rate_samples_per_second\": {},\n  \
             \"unexplained_iq_drops\": {iq_drops},\n  \
             \"intentional_retune_discards\": {retune_discarded},\n  \
             \"skipped_display_frames\": {display_drops},\n  \
             \"audio_underruns\": {},\n  \
             \"audio_overruns\": {},\n  \
             \"peak_audio_queue_frames\": {},\n  \
             \"audio_output_rate_hz\": {},\n  \
             \"audio_output_device\": \"{}\",\n  \
             \"peak_bevy_image_assets\": {},\n  \
             \"audio_error_samples\": {},\n  \
             \"engine_fault_samples\": {},\n  \
             \"stalled_sample_ticks\": {},\n  \
             \"unverified_interactions\": {},\n  \
             \"memory_first_mib\": {},\n  \
             \"memory_last_mib\": {},\n  \
             \"memory_peak_mib\": {:.2},\n  \
             \"memory_slope_mib_per_hour\": {},\n  \
             \"memory_quartile_step_mib\": {},\n  \
             \"median_fps\": {},\n  \
             \"median_app_cpu_pct_of_total_capacity\": {},\n  \
             \"interaction_events\": [{}],\n  \
             \"failures\": [{}],\n  \
             \"host\": {{\n    \
             \"os\": \"{}\",\n    \"cpu\": \"{}\",\n    \
             \"cores\": \"{}\",\n    \"ram\": \"{}\"\n  }},\n  \
             \"gpu\": {{\n    \"adapter\": \"{}\",\n    \"driver\": \"{}\"\n  }}\n}}\n",
            self.duration_s,
            self.rows,
            quote(&self.source_label),
            display.receiver.mode.label(),
            quote(&self.backend_label),
            median(self.input_rates.clone()).map_or("null".into(), |v| format!("{v:.0}")),
            self.underruns.total(),
            self.overruns.total(),
            self.peak_audio_queue_frames,
            self.audio_output_rate_hz
                .map_or("null".into(), |v| v.to_string()),
            quote(self.audio_output_device.as_deref().unwrap_or("n/a")),
            self.peak_image_assets,
            self.audio_errors,
            self.engine_errors,
            self.stalled_ticks,
            self.unverified_actions,
            self.memory_mib
                .first()
                .map_or("null".into(), |(_, y)| format!("{y:.2}")),
            self.memory_mib
                .last()
                .map_or("null".into(), |(_, y)| format!("{y:.2}")),
            self.peak_mem_mib,
            slope.map_or("null".into(), |v| format!("{v:.3}")),
            step.map_or("null".into(), |v| format!("{v:.2}")),
            median(self.fps.clone()).map_or("null".into(), |v| format!("{v:.2}")),
            median(self.cpu_pct.clone()).map_or("null".into(), |v| format!("{v:.2}")),
            self.events
                .iter()
                .map(|e| format!("\"{}\"", quote(e)))
                .collect::<Vec<_>>()
                .join(", "),
            failures
                .iter()
                .map(|f| format!("\"{}\"", quote(f)))
                .collect::<Vec<_>>()
                .join(", "),
            system.map_or("unavailable".into(), |s| quote(&s.os)),
            system.map_or("unavailable".into(), |s| quote(&s.cpu)),
            system.map_or("unavailable".into(), |s| quote(&s.core_count)),
            system.map_or("unavailable".into(), |s| quote(&s.memory)),
            adapter.map_or("unavailable".into(), |a| quote(&a.name)),
            adapter.map_or("unavailable".into(), |a| quote(&format!(
                "{} {}",
                a.driver, a.driver_info
            ))),
        );
        if let Err(error) = std::fs::write(&self.report_path, &summary) {
            error!("Soak: cannot write {}: {error}", self.report_path);
            return false;
        } else {
            info!("Soak: wrote {}", self.report_path);
        }
        if passed {
            info!(
                "Soak PASSED: {:.0} s live, {} samples, 0 unexplained IQ drops, \
                 {} intentional retune discards, {} skipped display frames, \
                 memory {:.1} -> {:.1} MiB (slope {} MiB/h)",
                elapsed,
                samples,
                retune_discarded,
                display_drops,
                self.memory_mib.first().map_or(0.0, |(_, y)| *y),
                self.memory_mib.last().map_or(0.0, |(_, y)| *y),
                slope.map_or("n/a".into(), |v| format!("{v:.2}")),
            );
        } else {
            error!("Soak FAILED: {}", failures.join("; "));
        }
        passed
    }
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn value(metrics: &DiagnosticsStore, path: &bevy::diagnostic::DiagnosticPath) -> Option<f64> {
    metrics
        .get(path)
        .and_then(|d| d.value())
        .filter(|v| v.is_finite())
}

fn smoothed(metrics: &DiagnosticsStore, path: &bevy::diagnostic::DiagnosticPath) -> Option<f64> {
    metrics
        .get(path)
        .and_then(|d| d.smoothed())
        .filter(|v| v.is_finite())
}

/// Sampling and scripted interaction. Runs in PreUpdate so injected egui events are
/// consumed by the same frame's pass, matching the other GUI smoke drivers.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy injects these independent system resources"
)]
pub(super) fn run(
    mut soak: ResMut<Soak>,
    mut display: ResMut<Display>,
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut windows: Query<&mut Window>,
    metrics: Res<DiagnosticsStore>,
    image_assets: Res<Assets<Image>>,
    system: Option<Res<SystemInfo>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if soak.finished {
        if soak.started.elapsed().as_secs_f64() >= soak.finish_at + 1.0 {
            exit.write(if soak.passed {
                AppExit::Success
            } else {
                AppExit::error()
            });
        }
        return;
    }
    let elapsed = soak.started.elapsed().as_secs_f64();
    soak.observe_counters(&display.status);
    if elapsed >= soak.next_sample {
        soak.observe(&display, &metrics, elapsed, image_assets.len());
        soak.next_sample += soak.interval_s;
    }

    // A release event must follow the press that produced it.
    if let Some(pos) = soak.release.take() {
        click(&mut inputs, pos, false);
    } else if let Some(delta) = soak.wheel.take()
        && let Some(rect) = display.waterfall_rect
    {
        for mut input in &mut inputs {
            input.events.push(egui::Event::PointerMoved(rect.center()));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, delta),
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    if elapsed >= soak.duration_s {
        soak.finished = true;
        soak.finish_at = elapsed;
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        let screenshot_path = soak
            .report_path
            .replace("soak-summary.json", "soak-final.png");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(screenshot_path));
        soak.passed = soak.finish(&display, system.as_deref(), adapter.as_deref());
        return;
    }

    if let Some((phase, before)) = soak.pending {
        let changed = match phase {
            1 => windows
                .iter()
                .any(|w| (w.resolution.width() as f64 - before).abs() > 1.0),
            2 => display.zoom as f64 > before + 0.01,
            3 => (display.zoom - 1.0).abs() < 0.01,
            4 => (display.station_mhz - before).abs() > 0.000001,
            5 => (display.rtl_center_hz as f64 - before).abs() > 1.0,
            _ => false,
        };
        if changed {
            soak.verified[phase - 1] = true;
            soak.events
                .push(format!("{elapsed:.1}s {} verified", PHASES[phase].0));
            soak.pending = None;
        }
    }

    // Advance the scripted schedule.
    if elapsed - soak.phase_started >= PHASES[soak.phase].1 as f64 {
        if soak.phase != 0
            && !(soak.phase == 5 && soak.source_label == "file")
            && (soak.pending.is_some() || !soak.action_done)
        {
            soak.unverified_actions += 1;
            let name = PHASES[soak.phase].0;
            soak.events
                .push(format!("{elapsed:.1}s {} had no observed effect", name));
        }
        soak.phase = (soak.phase + 1) % PHASES.len();
        soak.phase_started = elapsed;
        soak.action_done = false;
        soak.pending = None;
        soak.restart_stage = 0;
    }
    if soak.action_done {
        return;
    }
    let name = PHASES[soak.phase].0;
    let before_station = display.station_mhz;
    let before_center = display.rtl_center_hz;
    let before_span = display.received_view.map_or(0.0, |v| v.span_hz);
    let before_running = display.running;
    match name {
        "resize" => {
            let (w, h) = RESOLUTIONS[soak.resize_index % RESOLUTIONS.len()];
            soak.resize_index += 1;
            let old_width = windows
                .iter()
                .next()
                .map_or(0.0, |window| window.resolution.width()) as f64;
            for mut window in &mut windows {
                window.resolution.set(w, h);
            }
            soak.pending = Some((soak.phase, old_width));
            soak.events
                .push(format!("{elapsed:.1}s resize {w:.0}x{h:.0}"));
            soak.action_done = true;
        }
        "zoom-fit" => {
            if let Some(pos) = display.fit_position {
                soak.pending = Some((soak.phase, display.zoom as f64));
                click(&mut inputs, pos, true);
                soak.release = Some(pos);
                soak.events.push(format!(
                    "{elapsed:.1}s zoom-fit span {before_span:.0} Hz -> fit"
                ));
                soak.action_done = true;
            }
        }
        "zoom-full" => {
            if let Some(pos) = display.full_span_position {
                soak.pending = Some((soak.phase, display.zoom as f64));
                click(&mut inputs, pos, true);
                soak.release = Some(pos);
                soak.events.push(format!(
                    "{elapsed:.1}s zoom-full from span {before_span:.0} Hz"
                ));
                soak.action_done = true;
            }
        }
        "tune-vfo" => {
            if let Some(rect) = display.plot_rect {
                soak.pending = Some((soak.phase, display.station_mhz));
                let fraction = if soak.tune_index.is_multiple_of(2) {
                    0.62
                } else {
                    0.38
                };
                soak.tune_index += 1;
                let pos = egui::pos2(rect.left() + rect.width() * fraction, rect.center().y);
                click(&mut inputs, pos, true);
                soak.release = Some(pos);
                soak.events.push(format!(
                    "{elapsed:.1}s tune-vfo from {before_station:.6} MHz"
                ));
                soak.action_done = true;
            }
        }
        "center-scroll" => {
            if display.waterfall_rect.is_some() {
                if soak.source_label == "file" {
                    soak.action_done = true;
                    return;
                }
                soak.pending = Some((soak.phase, display.rtl_center_hz as f64));
                soak.wheel = Some(1.0);
                soak.events.push(format!(
                    "{elapsed:.1}s center-scroll from {:.6} MHz",
                    before_center as f64 / 1e6
                ));
                soak.action_done = true;
            }
        }
        "restart" => match soak.restart_stage {
            0 if before_running => {
                display.running = false;
                display.revision += 1;
                display.pending = true;
                display.reset();
                soak.restart_at = elapsed;
                soak.restart_stage = 1;
                soak.events.push(format!("{elapsed:.1}s stop requested"));
            }
            1 if !before_running && !display.status.running && elapsed >= soak.restart_at + 3.0 => {
                display.running = true;
                display.revision += 1;
                display.pending = true;
                display.reset();
                soak.restart_stage = 2;
                soak.events.push(format!("{elapsed:.1}s start requested"));
            }
            2 if before_running
                && display.status.running
                && display.status.revision == display.revision
                && display.status.samples > 0 =>
            {
                soak.verified[5] = true;
                soak.action_done = true;
                soak.events.push(format!("{elapsed:.1}s restart verified"));
            }
            _ => {}
        },
        _ => soak.action_done = true,
    }
}

fn click(inputs: &mut Query<&mut bevy_egui::EguiInput>, pos: egui::Pos2, pressed: bool) {
    for mut input in inputs.iter_mut() {
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
}
