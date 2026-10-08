# Engine boundary and platform preparation

Implemented 2026-09-24, before further decoder work. The desktop application still runs in one process. No network service, browser runtime, mobile shell, protocol decoder, or transmit operation is implemented.

## Ownership

`sdr-app::engine::Engine` owns the native worker and joins it on drop. `Engine::client()` returns a cloneable `Client` for commands and outputs. Keeping a client alive does not keep reception alive. Bevy holds the engine at the composition root; drawing systems receive only a client. The former `controller::Controller` name is a compatibility alias for existing probes/tests.

`engine/runtime.rs` owns source lifecycle, threads, cancellation, pacing and clocks. `sdr-dsp::Receiver::process` remains a synchronous block processor with no device, renderer or scheduling dependency. `engine/types.rs` contains local stream/configuration contracts. These are not a versioned network schema.

`Command::Configure(Settings)` submits a complete desired configuration with a monotonically increasing revision. Successful submission means accepted into the coalescing slot, not yet applied. The worker acknowledges applied revisions in state and ordered `EngineEvent` messages. Intermediate edits can be superseded before application. Source-open failures appear as faults, not successful applied acknowledgments. A later revision can retry a source fault.

Submission distinguishes `Busy` (retry later), `Invalid`, `Stale`, and `Closed`. The GUI retries only busy submissions and retains command rejection messages until a later successful submission. Compile-time capabilities report RTL and monitor support; transmit remains false. They do not imply an attached/available device. Device errors remain runtime state.

`Command::SpectrumView` coalesces display-only center-offset/span changes without advancing the radio revision. It does not restart audio, acquisition or scanning. FFT frames include the requested view and actual analysis sample rate so consumers can reject stale analysis and project frequency coordinates correctly. See [automatic spectrum resolution](spectrum-resolution.md).

## Output contracts

| Output | Policy | Consumer responsibility |
| --- | --- | --- |
| State and spectrum | One coherent latest-value slot; spectrum is consumed once and replaced under load | Poll without blocking; compare revision and capture generation; inspect display loss counters |
| Engine events | Ordered queue, 64 entries, incoming item dropped on overflow/contention | Drain regularly; inspect `dropped()`; recover authoritative state from the latest snapshot after loss |
| Optional receiver audio | Ordered queue, eight blocks, incoming block dropped on overflow/contention | Inspect loss and sample positions; reset downstream processing at revision/generation/discontinuity changes |
| Native monitor | Existing CPAL sink, bounded 500 ms sample queue, retune epochs, prefill and clock correction | Endpoint failure does not stop acquisition, receiver processing, or optional audio delivery |

`EventStream<T>` is the bounded ordered queue primitive available for later decoder results. Current events are lifecycle/configuration events; no decoded messages are fabricated. The in-process boundary has one consuming reader per output stream. Cloned handles share consumption rather than providing broadcast subscriptions or independent VFOs.

`Client::subscribe_audio()` creates one optional receiver-output subscription; a new subscription replaces the previous one. No sample frames are copied for this subscription when it has no live consumer. Eight normal 16,384-IQ blocks represent about 55 ms at 2.4 MS/s. A slow consumer loses blocks rather than stalling the radio. Speaker mute/gain and device-rate resampling happen downstream of the tap.

Audio metadata includes the applied revision, source-open `capture_revision`, input sample start, 48 kHz output sample start, and a discontinuity counter. Output positions reset when receiver state resets; revision/discontinuity distinguish those timelines. Source-open generation stays constant through hot VFO/mode/center changes. IQ gaps also reset the receiver and invalidate monitor audio. Spectrum frames now include capture generation and input sample position.

These are software sample positions, not hardware timestamps or UTC. FT8 time alignment remains future work. The audio tap includes mode AGC, squelch and de-emphasis. RDS needs a separate pre-de-emphasis FM multiplex tap; HD Radio needs an IQ path. Add those explicit taps with their decoders rather than treating speaker audio as a universal decoder input.

## Feature composition

| `sdr-app` feature | Includes |
| --- | --- |
| default | RTL adapter; no GUI/audio |
| `rtl` | Optional native `libloading` adapter through `sdr-devices/rtl` |
| `audio` | CPAL and bounded monitor sample queue |
| `gui` | Bevy/egui rendering, independently of monitor audio |
| `desktop` | GUI + audio + RTL; use this for the usual interactive application |
| `trace` | Application tracing spans, without Bevy or a global subscriber |
| `perfetto` | Desktop + application spans + Bevy's Chrome trace exporter |

The normal launch command is now `cargo run -p sdr-app --features desktop -- --gui --rtl`. Previous `--features gui` commands still render but no longer imply audio. File-only rendering can use `--no-default-features --features gui,audio`. Headless integrations can install their own tracing subscriber; the existing headless CLI does not install one.

RTL library discovery accepts `RUSTSDR_RTLSDR_LIBRARY` (preferred) or the older `RUSTSDR_RTLSDR_DLL`. Windows retains the bundled development runtime. Linux/macOS defaults use the platform loader for `librtlsdr.so.0` / `librtlsdr.dylib`; an absolute override is useful for custom installations. Native libraries, permissions, signing and packaging still require platform validation.

## Presentation

`gui_controls.rs` owns the header, source/tuning controls and receiver settings panel; diagnostics remains separate. Mode settings and tuning step survive mode changes within a session through renderer-independent `ModeSettings`. Desktop controls are resizable. Below 760 logical pixels they move into a closable scrolling window, reopened through the header's Controls button. VFO step buttons complement wheel/drag input. This is compact-window support, not validated mobile touch integration.

## Verification and remaining platform work

New behavioral tests cover invalid/stale/closed commands, stopped state for clients surviving engine shutdown, bounded ordered event loss, nonzero receiver audio despite monitor mute, stalled-consumer EOF, audio generation metadata across hot tuning, and mode preference retention. Existing DSP, lifecycle, file playback, and tuning tests remain in place. The full-feature suite has 33 integration tests; the feature-minimal suite has 32 (excluding the optional RTL conversion test).

Core/DSP `wasm32-unknown-unknown` compilation was verified locally. Windows desktop and headless builds are verified locally; Linux/macOS are not yet built or hardware-tested. `.github/workflows/platforms.yml` adds headless tests, RTL boundary tests and desktop checks on Windows/Linux/macOS, plus a WASM core/DSP check. It has not run on hosted runners. Workflow syntax and the checkout action were checked against [GitHub's workflow reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax) and [the action definition](https://github.com/actions/checkout/blob/v4/action.yml). The optional skill's `ci_monitor.cjs` helper was not installed locally.

This does not make the native engine WASM-compatible: threads, blocking waits, native files, and platform audio remain native adapters. Browser recordings, a native server/browser client, and mobile lifecycle work remain separate experiments. See [platform explorations](platform-explorations.md) and [trace comparison](architecture-trace-comparison.md).
