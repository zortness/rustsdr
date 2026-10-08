# M01: First working receiver

Status: acceptance evidence still in progress. Synthetic/file/live acquisition, receiver tuning, voice demodulation/audio, and the Bevy spectrum/waterfall are implemented. The user completed a 30-minute manual multi-mode stability test and validated most modes; AM remains outstanding. Instrumented memory, queue, and loss measurements were not supplied for that run. See the [implementation status](implementation-status.md). The original scope below is retained as the milestone baseline; later voice/live features were brought forward.

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

- [ ] A new contributor can build and run from documented commands.
- [ ] Synthetic input and a generated IQ fixture run without connected radio hardware.
- [ ] FFT peaks are within one bin of known tone frequencies; normalization expectations and numeric tolerances are documented.
- [ ] Mono FM is verified offline and audible through the selected output device.
- [ ] Tuning and bandwidth handling reject unsupported values and keep labels consistent with actual processing state.
- [ ] Queues and waterfall history have fixed limits; long-run memory does not grow with elapsed reception time.
- [ ] Artificial display stalls do not block reception; display gaps and sample/audio losses are distinguishable in diagnostics.
- [ ] Stop/start, EOF, invalid input, unavailable audio output, and window close terminate or recover without hanging.
- [ ] The steady-state target scenario has no unexplained IQ drops or audio underruns; any revised target and remaining limitations are recorded.
- [ ] Device capability design covers the three future device profiles without operational transmit controls.
- [ ] The Bevy decision and measured baseline are recorded for the next milestone.

## Completion record to produce

Record the build revision, dependency versions, machine/OS, commands, fixture parameters, test output, performance measurements, and limitations. Include UI evidence of spectrum/waterfall interaction. This milestone is not complete solely because the application compiles or displays a moving waterfall.
