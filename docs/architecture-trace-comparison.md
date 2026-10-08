# Architecture refactor: trace comparison

Measured on the same Windows machine, 2026-09-24 local time. The original user capture `trace-1790306534437827.json` is preserved. It contains RTL receive spans and one unfinished SDR span. The user subsequently confirmed that it was captured with a release build, live RTL-SDR input and WFM stereo using:

```powershell
cargo run -p sdr-app --release --features perfetto -- --gui --rtl --mode stereo
```

This establishes the original capture's build profile and source/mode. Its aggregate statistics are not compared directly with the development-build file-playback runs below because both profile and workload differ. The command alone does not establish the trace filter, window state or interactions during reception. Preserve it as a release/live reference for a later comparable capture.

## Matched scenario

The pre-refactor executable is retained at `target/architecture-baseline/sdr-app.exe`. Before and after used Rust's development profile with `perfetto`, identical `RUST_LOG=warn,sdr_app=info`, the same two-second stereo fixture, 2.4 MS/s IQ, 106.95 MHz capture center, 106.7 MHz VFO and WFM stereo. Each run used the existing 15-second GUI smoke script: stop/start, complete playback, resize, open/close Diagnostics and exit normally. The GUI starts at 1280x820 and resizes to 1100x760. No build or test workload ran alongside the captures.

Two before/after pairs were recorded. This is a short regression check, not a release benchmark or another 30-minute stability test. The script uses timed input, so stop/start timing can shift the amount of processed data slightly: 442/442 receiver blocks in pair one, 444/442 in pair two. FFT counts were 88/88 and 89/88. All four matched traces had zero unmatched/unclosed SDR spans.

## Results

All values below are p95 elapsed microseconds. They include scheduling delays and any nested spans, not exclusively CPU execution.

| Span | Before 1 | After 1 | Before 2 | After 2 |
| --- | ---: | ---: | ---: | ---: |
| Receiver block | 962.6 | 955.2 | 959.5 | 949.3 |
| FFT | 56.4 | 55.0 | 55.5 | 55.1 |
| Monitor resample/queue | 85.3 | 84.4 | 85.4 | 84.0 |
| UI drawing/submission | 1411.4 | 1442.8 | 1366.9 | 1416.3 |
| Waterfall row | 215.0 | 182.9 | 176.7 | 251.1 |

Receiver, FFT and monitor costs are essentially unchanged in this sample. UI p95 increased by approximately 2.2% and 3.6% (31–49 microseconds). Waterfall-row timings vary in both directions; the largest p95 increase is 74 microseconds. This provides no evidence of a large steady-work regression, but is insufficient to establish statistical equivalence or release throughput.

Full waterfall recolor/texture submission remains an existing development-build spike: roughly 21–25 ms, only three events per run. Maximum UI spans remain around 30–33 ms. The refactor does not solve that cost; inspect it separately if large redraws feel slow. `sdr.ui.frame` measures UI work, not the full render/GPU frame.

The after-run Diagnostics screenshot reported zero skipped display frames, dropped IQ samples, audio underruns and audio overruns. Desktop and 640-pixel compact file smoke runs exited successfully; screenshots were inspected. The live RTL tuning probe also preserved the source generation through VFO and center changes: approximately 23 ms VFO acknowledgment and 218–219 ms hardware retunes, with zero reported IQ/audio losses. These are short-run observations, not guarantees under sustained overload.

## Reproduce or extend

Raw captures, logs and CSV summaries are in `target/architecture-baseline/` (`before`, `after`, `before-2`, `after-2`). These local artifacts are not source-controlled and should be copied elsewhere before cleaning `target`. They contain application spans only; use the broader profiling filter for Bevy scheduling analysis.

```powershell
cargo build -p sdr-app --features perfetto
$env:RUST_LOG = 'warn,sdr_app=info'
$env:TRACE_CHROME = 'target/architecture-baseline/new-comparison.json'
& target/debug/sdr-app.exe --gui --file target/stereo-demo.cf32 --center-hz 106950000 --sample-rate 2400000 --mode stereo --smoke-test
cargo run -p sdr-app --no-default-features --example trace_summary -- target/architecture-baseline/new-comparison.json
```

`trace_summary` streams tracing-chrome's newline JSON array and reports count, mean, p50, p95 and maximum for each `sdr.*` span family. It rejects unmatched/unclosed spans after reporting partial statistics. It is not a general Perfetto trace parser. Open the raw JSON in Perfetto to inspect temporal correlations and separate active playback from EOF/idle frames.

For a stronger performance decision, capture multiple release-build pairs with an identical longer recording, fixed window/diagnostics state and interactions, unchanged trace filters, and no concurrent build load. Compare active playback intervals, frame pacing, loss counters and memory alongside duration distributions. Do not compare these development-build numbers directly with the user's original capture or a later release build.
