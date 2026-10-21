# M03: Daily-use single receiver

Status: in progress. This checklist uses the Windows 11 desktop application with one active receiver and the documented Direct3D 12 renderer configuration as its initial acceptance target. M01 and M02 established file/live acquisition, audio output, device lifecycle, and a 30-minute instrumented RTL-SDR baseline. M03 is complete when a new user can install the application, use the supported listening modes, save decoded audio, and recover from ordinary file, device, and configuration failures without developer tooling.

## Scope

M03 covers WFM mono/stereo, AM, NFM, USB, LSB, and CW listening through one receiver; their filters, AGC/squelch and tuning controls; station memories; persistent settings; decoded audio recording; and initial Windows distribution. The supported IQ-file workflow remains explicit about its sample format, rate, and capture center. Additional input formats should be driven by real recordings rather than assumed in advance.

Multiple simultaneous listening VFOs and independent audio routes belong to M04. IQ recording, additional platforms and network sources belong to M05. New digital protocols, comprehensive RDS extensions, and RF transmission are separate work. The existing RDS/RBDS, FT8/FT4, CW text, scanning, QMX, and IC-705 features may be included in a distribution, but their expanded capabilities are not prerequisites for the single-receiver M03 gate.

## Acceptance checklist

### Listening and tuning

- [x] The seven supported listening modes have selectable, validated receiver settings and deterministic signal tests. WFM stereo, AM, NFM, USB/LSB, and CW behavior, including filter choices, AGC or squelch where applicable, are covered by the [receiver-mode record](receiver-modes.md), [CW record](cw-decoding.md), and DSP fixtures. These tests establish expected processing on known signals, not reception quality on every live signal.
- [x] Switching mode or tuning the VFO keeps the active capture and audio endpoint where possible, resets incompatible decoder/audio state, and leaves display labels consistent with the actual center and passband. Controller and GUI checks cover file and live workflows.
- [x] CSV memories store and recall the full receiver configuration; scan inclusion and explicit save/load errors are visible. The [memory workflow](memories-and-scanning.md) includes large-library and failed-save tests.
- [x] Per-mode preferences and workspace settings restore after restart without automatically starting reception. Invalid or future settings are preserved or rejected with a visible warning. The [settings record](settings.md) includes close/reopen and corruption checks.
- [x] Modular controls and the optional RDS/RBDS results panel are implemented. The [broadcast data record](fm-broadcast-data.md) includes generated-IQ and live station-text evidence; extended RDS services remain outside this gate.
- [x] The user completed a 30-minute manual multi-mode session. The exact modes, release build, loss counters, and memory trace were not recorded, so this is listening evidence rather than the final measured M03 mode-coverage result.

### Remaining integrated checks

- [ ] Validate live AM on a named suitable signal and RF setup. Record the antenna/converter configuration, RF and tuner frequencies, mode/filter/AGC settings, audible result, and limitations. An independently sourced recording can help diagnose reception, but the generated AM fixture alone does not close this live check.
- [ ] Record a representative single-receiver session that exercises each supported mode with a known signal or recording, including mode changes and at least one source or output interruption. Name the build, input and output devices, renderer, run length, and IQ/audio loss counters; distinguish deliberate retune gaps from unexplained loss. Existing M01/M02 soaks cover sustained WFM reception, not this full mode matrix.
- [ ] Save decoded monitor audio to a declared, widely playable audio file with correct rate and channel layout. Verify start/stop, retune or mode change, mute behavior, short recordings, and playback in an independent player. Define the recorded signal point so the saved audio cannot silently differ from what the user expects to capture.
- [ ] Make recording failures recoverable: invalid destination, permission or disk-write failure, and shutdown must report an actionable error without losing reception or corrupting a previously completed file. Verify the partial-file policy and that no recording worker remains stuck.
- [ ] Resolve the initial Windows distribution format and package the application and required runtime files with a versioned build and concise device/audio setup instructions. A new user must be able to launch, select a supported source, receive audio, and recover from a missing device without Cargo or a Rust toolchain. Test the artifact in a clean user environment, including its bundled native RTL library path.
- [ ] Decide whether a real user capture requires an IQ input format beyond the current headerless float32 I/Q file. If so, add explicit metadata and a replay/failure fixture for the selected format. If not, record the sample file and reason the current format meets M03's daily-use playback need. Do not guess rate or center frequency from raw data.
- [ ] Publish a completion record with the exact build revision, artifact, supported hardware/OS configuration, test commands or scenarios, measured counters, known limitations, and links to the evidence. Mark M03 complete only after every required check above has evidence.

## Current evidence and limits

The [implementation status](implementation-status.md) records the mode, settings, memory, and RDS work already delivered. The [M01](milestone-01.md) and [M02](milestone-02.md) acceptance records supply the Direct3D 12 resource baseline and physical RTL lifecycle results. The initial IQ-file format is headerless little-endian complex float32 with explicit 2.4 MS/s rate and capture center. Decoded WAV recording now has a receiver-to-file integration test and external `ffprobe` format check; GUI and live checks remain open. A Windows ZIP packaging script stages a local candidate with the native RTL runtime; its [local test record](evidence/m03-local-candidate-2026-10-20.md) covers Direct3D 12 synthetic, file, and bundled-library live smokes. Clean-environment installation and redistribution review remain open. Live AM validation is not yet recorded.

The default Vulkan renderer still fails the M01/M02 memory trend on the named host. M03's initial distribution must use the documented Direct3D 12 configuration or provide fresh passing resource evidence for another default.
