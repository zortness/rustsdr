# M01: First working receiver

Status: in progress. Synthetic/file/live acquisition, receiver tuning, voice demodulation/audio, and the Bevy spectrum/waterfall are implemented. The user completed a 30-minute manual multi-mode stability test and validated most modes; AM remains outstanding. Three [instrumented 30-minute RTL-SDR runs](soak-results.md) on Vulkan found zero sampled host IQ/audio faults but failed the memory trend gate. A fourth run with Direct3D 12, accumulating counters every GUI frame across restarts, passed the 30-minute resource and host-loss gates on the named AMD/Windows host. The original scope below is retained as the milestone baseline; later voice/live and IC-705 experimental transmit features were brought forward.

Audience: the contributor beginning implementation. The outcome is a local application that visualizes synthetic or recorded IQ and plays mono broadcast-FM audio through one receiver, without any physical radio requirement.

## Scope

- Minimal Rust workspace with core, DSP, device-adapter, and application boundaries.
- Bevy frontend with egui controls, spectrum trace, and GPU waterfall.
- Deterministic synthetic input and paced IQ file playback.
- One VFO with frequency translation, anti-alias filtering/decimation, mono broadcast-FM demodulation, selectable de-emphasis, audio resampling, and volume/mute.
- Source start/stop, frequency entry, click-to-tune, bandwidth display, spectrum zoom, level range, and basic diagnostics.
- Headless verification of sample processing and a reproducible performance scenario.

Exclude physical device integration, multiple VFOs, stereo/RDS decoding, other demodulation modes, recording, transmit implementation, dynamic plugins, and a general graph editor.

## Implementation sequence

### 1. Establish contracts and a headless sample path

Choose compatible dependencies, scaffold the workspace, and document build commands. Define block metadata, source lifecycle, controller commands, and receiver state. Implement a deterministic tone generator and reusable bounded buffers.

Validate the design against receive-only, IQ-transceiver, and control-plus-audio device profiles on paper. Reserve capability boundaries without implementing unused TX traits in exhaustive detail.

Evidence: generated sample count, tone frequency, lifecycle behavior, bounded memory ownership, and explicit unsupported-capability behavior.

### 2. Display known signals

Implement windowed FFT analysis, normalized power values, correct frequency ordering, and a GPU waterfall history. Wire source controls and diagnostics through the controller. Add zoom and tuning interactions with correct panel input ownership.

Evidence: tones appear at expected frequencies, scaling remains consistent across supported FFT sizes, history timing is defined, and UI stalls cannot block sample processing.

### 3. Produce audio from synthetic FM

Generate a known mono FM signal, then translate/filter/decimate, demodulate, apply de-emphasis as configured, and resample to the selected audio output rate. Use a bounded output ring buffer and a minimal audio callback.

Evidence: an offline recovered tone matches expected frequency and amplitude within explicit test tolerances; output sample counts match the rate ratio after accounted-for filter delay. Manual listening supplements these checks.

### 4. Add file playback and lifecycle handling

Initially support interleaved little-endian float32 I/Q pairs. Require sample rate and center frequency at open time; do not guess missing metadata. Document the format and create a deterministic fixture generator. Reject truncated sample pairs and invalid metadata with actionable errors.

Pace file input according to sample time. At EOF, drain valid output and transition visibly to stopped; looping is optional and must mark a discontinuity if implemented. File frequency metadata describes recorded RF center; VFO tuning cannot recover signals outside the recording's bandwidth.

Evidence: fixture playback, EOF, invalid files, repeated stop/start, source replacement, and stale-data rejection after configuration changes.

### 5. Measure and decide whether to retain the frontend

Profile a release build on a named Windows machine. Trial target: 2.4 MS/s complex input, 4096-point FFT, 30-60 display updates per second, and one receiver. Choose a sustainable audio-buffer latency and report it. These are evaluation targets, not existing guarantees.

Run for at least 30 minutes while periodically resizing, zooming, tuning within the captured band, and stopping/restarting. Record throughput, queue occupancy, drops, underruns, memory trend, and render responsiveness. Define the steady-state measurement window separately from startup/retune transitions.

If the target is missed, identify whether DSP, rendering, allocation, or scheduling is responsible. Optimize the measured bottleneck or revise the target with recorded reasoning. Compare an egui/eframe frontend only if Bevy integration is the material obstacle.

## Acceptance checklist

- [x] A new contributor can build and run from documented commands.
- [x] Synthetic input and a generated IQ fixture run without connected radio hardware.
- [x] FFT peaks are within one bin of known tone frequencies; normalization expectations and numeric tolerances are documented.
- [x] Mono FM is verified offline and audible through the selected output device.
- [x] Tuning and bandwidth handling reject unsupported values and keep labels consistent with actual processing state.
- [x] Queues and waterfall history have fixed limits; the 30-minute memory trend meets the recorded limits on the named host using Direct3D 12. Default Vulkan still fails this gate on that host.
- [x] Artificial display stalls do not block reception; display gaps and sample/audio losses are distinguishable in diagnostics.
- [ ] Stop/start, EOF, invalid input, unavailable audio output, and window close terminate or recover without hanging.
- [x] The steady-state target scenario has no unexplained IQ drops or audio underruns; any revised target and remaining limitations are recorded.
- [x] Device capability design covers the three future device profiles; the M01 receive path requires no transmit control.
- [x] The Bevy decision and measured baseline are recorded for the next milestone.

Checked items are backed by the 2026-10-19 all-feature workspace run, the documented synthetic/file/live GUI checks, the [soak record](soak-results.md), and earlier user confirmation of audible FM. The FFT test covers 256- and 4096-point transforms with exact-bin tones and a -6.0206 dBFS expectation within 0.001 dB. The display-stall integration test checks continued acquisition and a separate skipped-frame counter. The full Direct3D 12 soak observed counters every GUI frame across nine restarts and reported zero unexplained host IQ drops and zero audio under/overruns. Its median input rate was 2.399 MS/s with about 60 FPS; host counters cannot rule out unreported tuner/USB loss or prove audible quality throughout the run. That run met the memory gate with a 2.31 MiB/hour fitted trend, while all three Vulkan runs failed it. Bevy is retained provisionally with an explicit Direct3D 12 choice on this host. The lifecycle item remains open for unavailable audio-output recovery; EOF, invalid file recovery and stop/start have automated coverage, and an isolated synthetic GUI exited cleanly after normal Windows window close. The later IC-705 test-tone transmit experiment is outside this M01 receive-path check.

## Completion record to produce

Record the build revision, dependency versions, machine/OS, commands, fixture parameters, test output, performance measurements, and limitations. Include UI evidence of spectrum/waterfall interaction. This milestone is not complete solely because the application compiles or displays a moving waterfall.
