# Capture a Perfetto trace

RustSDR's optional `perfetto` Cargo feature includes the GUI and enables Bevy's Chrome JSON trace exporter. Normal `gui` builds do not enable this exporter or the custom SDR spans. No Perfetto SDK or installed viewer is required. This workflow is for the GUI application; the standalone headless CLI does not install Bevy's tracing subscriber.

## Windows capture

Run these commands in PowerShell from the project root. Build an optimized executable first so compilation is outside the capture:

```powershell
cargo build -p sdr-app --release --features perfetto
```

Then create a uniquely named trace and launch the application:

```powershell
New-Item -ItemType Directory -Force target/traces | Out-Null
$sdrTracePath = Join-Path (Resolve-Path target/traces) ("rustsdr-" + (Get-Date -Format "yyyyMMdd-HHmmss-fff") + ".json")
$sdrPreviousTrace = $env:TRACE_CHROME
$sdrPreviousLog = $env:RUST_LOG
try {
    $env:TRACE_CHROME = $sdrTracePath
    $env:RUST_LOG = "info,wgpu=error,naga=warn"
    & .\target\release\sdr-app.exe --gui --rtl --mode stereo
    if ($LASTEXITCODE -ne 0) { throw "RustSDR exited with an error; the trace may be incomplete." }
    Write-Host "Trace saved to $sdrTracePath"
} finally {
    $env:TRACE_CHROME = $sdrPreviousTrace
    $env:RUST_LOG = $sdrPreviousLog
}
```

Reproduce the operation of interest for about 10–20 seconds, then **close the application window normally**. The exporter finalizes its JSON on shutdown. Do not use Task Manager to terminate the process or inspect a still-being-written file. Trace files can grow quickly; a short focused capture is preferable to a 30-minute recording. The destination directory must exist. Reusing the same trace filename can overwrite it.

To capture without hardware, substitute this invocation inside the same `try` block (the stereo fixture command is documented in the README):

```powershell
& .\target\release\sdr-app.exe --gui --file target/stereo-demo.cf32 --center-hz 106950000 --sample-rate 2400000 --mode stereo
```

The two-second fixture reaches EOF quickly; press Start reception again for additional passes, or use a longer recording. `--gui` alone uses a synthetic tone and exercises FFT/UI processing without audio demodulation. Choose a distinct filename for each comparison and note mode, source, frequency, build profile, display size/zoom, and whether Diagnostics was open.

## Inspect in Perfetto

1. Open [ui.perfetto.dev](https://ui.perfetto.dev/).
2. Choose **Open trace file** and select the JSON after RustSDR exits.
3. Expand the application thread tracks. Search for `sdr.ui.frame` or `sdr.dsp.receiver`, select a slice, and zoom into a slow interval.
4. Compare the main/UI thread with the radio controller and Bevy worker/render threads. Engine spans show schedules and systems; SDR spans identify application work. The radio worker processes sample blocks independently of render frames.

| Span prefix | What its duration includes |
| --- | --- |
| `sdr.ui.frame` | GUI update, spectrum preparation, controls, diagnostics presentation, and egui drawing submission |
| `sdr.ui.waterfall_row` | History-row conversion and partial texture-update submission |
| `sdr.ui.waterfall_recolor_and_texture_enqueue` | Full history reprojection/recoloring and texture-update submission |
| `sdr.dsp.receiver` | One IQ block's complete channel filtering and demodulation; mode and sample count are recorded |
| `sdr.dsp.fft` | Spectrum analysis for one displayed FFT block |
| `sdr.audio.resample_and_queue` | Producer-side audio rate conversion and queue insertion |
| `sdr.receiver.reconfigure` | Receiver configuration changes, audio invalidation, and any nested hardware retune |
| `sdr.device.retune` | USB reader stop/restart and native frequency change; includes waiting |
| `sdr.iq.receive_wait_and_copy` | Waiting for an RTL IQ block and copying it; this is not all active CPU time |
| `sdr.file.pacing_wait` / `sdr.file.read` | File sample-clock wait versus reading/validating IQ |

These slices measure elapsed wall time. A long wait or OS scheduling delay is not necessarily a CPU hotspot. Nested durations overlap; do not sum parent and child times. Texture submission is CPU-side work, not completed GPU upload time. Chrome/Perfetto tracing here primarily covers CPU spans; use the Diagnostics GPU pass timings for available GPU measurements. This does not implement GPU utilization sampling or a full GPU timeline.

No per-sample trace events or audio-callback spans are added. Receiver internals are initially one block-level span to limit overhead; add targeted sub-stage instrumentation only when a capture identifies that area as a bottleneck. Enabled tracing still affects timing and file I/O, so compare ordinary release behavior before treating a measured regression as an application defect.

## Troubleshooting

Full engine traces are large: a 15-second development-build playback check produced roughly 272 MiB. For a smaller application-only capture, set `$env:RUST_LOG = "warn,sdr_app=info"` in the capture script. This retains the custom `sdr.*` spans but omits most engine internals, so use the broader filter when investigating Bevy scheduling/rendering. This filter can also suppress renderer diagnostics' explanatory logs; it does not disable the diagnostics window.

- **Missing custom spans:** build with `--features perfetto`, launch with `--gui`, and keep the log filter at `info` or more verbose. Plain `gui` builds have these spans compiled out. Bevy's exporter is installed by its logging plugin; do not install a second global tracing subscriber.
- **Missing receiver/audio spans:** synthetic mode has no audio receiver. Use live RTL or an IQ file while playback is active.
- **Missing or invalid file:** verify the output directory and `TRACE_CHROME` path, then close normally and wait for the process to exit. A killed/crashed process can leave an incomplete capture. Without `TRACE_CHROME`, Bevy chooses its default trace filename in the working directory.
- **Unexpectedly slow capture:** use the release build, shorten the recording, and avoid excessively verbose logging. Debug builds are useful to verify trace structure, not to establish performance baselines.

Backend reference: [Bevy 0.18.1 profiling guide](https://github.com/bevyengine/bevy/blob/v0.18.1/docs/profiling.md#chrome-tracing-format).

## Architecture regression comparison

See [the recorded before/after comparison](architecture-trace-comparison.md) for the matched playback scenario and results. Use the "trace_summary" example for streaming SDR-span statistics. The native engine/audio use the independent "trace" feature; "perfetto" adds Bevy's exporter and the desktop composition. Span names and the "warn,sdr_app=info" filter are retained across the module extraction.
