# Decisions and open questions

## Future platform exploration

The user wants to retain the portability findings and native-server/browser-renderer WebSDR topology as potential future work. Windows remains the only validated platform. Favor incremental native feature isolation, an engine/client boundary, independent monitor/decoder outputs, and modular responsive UI because they also support present desktop work. Defer network protocols, browser execution adapters, mobile shells, and public multi-user control until a scoped prototype. See [platform and WebSDR explorations](platform-explorations.md).

Recorded: 2026-09-23. Audience: implementers deciding scope and dependencies. Use this record to distinguish requirements from provisional implementation choices.

## Confirmed requirements

| Decision | Consequence |
| --- | --- |
| Build an SDR application similar in purpose to SDR++ | Reuse its subsystem concepts as references; develop an independent Rust architecture |
| Write as much of the software we create as practical in Rust | Native libraries, SDKs, and tools may be integrated through adapters |
| Use Bevy or another suitable rendering engine | Evaluate Bevy first; keep DSP independent so presentation can change |
| Support future transmitters and transceivers | Design device capabilities, control, resource coordination, and output boundaries now |
| Do not implement transmission yet | No live PTT, microphone-to-radio routing, modulation for transmission, or power control in the receive milestones |

## Initial design choices

These are defaults to implement and validate, rather than additional user requirements.

| Choice | Reason and revisit condition |
| --- | --- |
| Bevy with egui controls and custom spectrum/waterfall rendering | Fits the requested engine direction; revisit if the first milestone exposes unacceptable integration or resource costs |
| Windows first | Matches the development environment; avoid Windows dependencies in DSP and domain interfaces |
| CPU DSP and FFT initially | Keeps processing testable and avoids GPU transfer/scheduling complexity; revisit after profiling |
| Dedicated streaming workers with bounded queues | Decouples reception from rendering and bounds latency/memory; choose worker partitioning from measurements |
| Built-in modules selected at build time | Provides extensibility without defining a binary plugin contract prematurely |
| One active device session initially | Simplifies lifecycle and tuning; multiple VFOs can later share the same captured bandwidth |
| Synthetic tests plus early RTL-SDR integration | User supplied a dongle; IQ file playback now shares the live FM processing path |

Bevy ECS may represent UI and presentation state. Sample processing must not depend on ECS update cadence. Avoid a generic graph editor, a thread for every DSP stage, or an external plugin ABI in the first implementation.

## SDR++ inspection basis

The reference was the local checkout at `C:\Users\zortn\projects\github\SDRPlusPlus`, inspected on 2026-09-23. This was a source inspection, not a build or runtime comparison; the inspected commit was not recorded. Capture a revision before any detailed parity work.

Decision-scoped reference locations:

- `core/src/signal_path/iq_frontend.h`: preprocessing, FFT branch, and receiver branches.
- `core/src/dsp/stream.h` and `core/src/dsp/block.h`: synchronized buffers and worker lifecycle.
- `core/src/signal_path/vfo_manager.h`: receiver management coupled to waterfall UI objects.
- `decoder_modules/radio/src/radio_module.h`: receiver processing and audio routing.
- `core/src/gui/widgets/waterfall.cpp`: spectrum/waterfall presentation and interaction.
- `core/src/module.h`: runtime module lifecycle.
- `source_modules/rtl_sdr_source/src/main.cpp`: native hardware-library integration.

Preserve the separation of sources, shared preprocessing, receivers, and sinks. Introduce stronger boundaries between device state, DSP, and presentation. The local checkout includes a GPLv3 license; the project's own license and any proposed source reuse remain decisions to resolve before importing reference code. No reference source has been copied into this project.

## Dependency candidates

The implementation pins Bevy 0.18.1 and bevy_egui 0.39.1, compatible with Rust 1.93.1. RustFFT resolves to 6.4.1. CPAL 0.17.3 and ringbuf 0.4.8 provide audio output; libloading 0.8.9 provides the native RTL boundary. Exact transitive versions are in the workspace lockfile.

- [Bevy custom rendering](https://bevy.org/examples/shaders/custom-post-processing/) and [bevy_egui](https://docs.rs/bevy_egui/latest/bevy_egui/) for presentation.
- [RustFFT](https://docs.rs/rustfft/latest/rustfft/) for CPU FFTs.
- [CPAL](https://docs.rs/cpal/latest/cpal/) for audio devices.
- [egui/eframe](https://github.com/emilk/egui) with wgpu as the presentation fallback.

Only required Bevy rendering/windowing features are enabled, behind the optional `gui` feature. The headless application does not depend on Bevy. The frontend has passed a Windows Vulkan smoke test; other graphics backends and platforms remain unverified.

## Open questions and when they matter

| Question | Resolve by | Default until resolved |
| --- | --- | --- |
| First physical radio and connection method? | Selected: RTL-SDR v3 over USB | User has connected the dongle with a small VHF antenna; first live test targets broadcast FM |
| Target machine and sustainable sample rate? | First performance evaluation | Trial target of 2.4 MS/s; record actual machine and results |
| Bevy retained after prototype? | First milestone review | Proceed with Bevy |
| First external IQ recording format? | Implemented initial format | Headerless interleaved little-endian complex float32; explicit center and 2.4 MS/s rate supplied through CLI/GUI; other formats/rates deferred |
| Project license and reference-code reuse policy? | Before importing code or distributing | Write original implementation; retain dependency provenance |
| First transmit topology: IQ SDR or conventional transceiver? | Transmit design milestone | Support both conceptually; implement neither |
| Linux/macOS release support? | Packaging planning | Keep core portable, validate Windows first |

Update this record when a default becomes a measured decision or a requirement changes.

## First hardware target

The user supplied an RTL-SDR v3 USB dongle with a small VHF antenna. Windows enumerated the device as VID 0BDA / PID 2838. Interface 0 initially had no driver reported; a subsequent device-property check reported the WinUSB service and problem code 0. This verifies enumeration and driver binding, not successful IQ reception.

The RTL-SDR adapter and mono FM/audio path are now implemented. The user suggested 106.7 MHz and 89.9 MHz in the 88–108 MHz band. Both completed short live reception/audio probes at 2.4 MS/s with zero reported host IQ drops or audio underruns/overruns. On 2026-09-24, the user confirmed clear audio on multiple stations, working controls, correct-looking FFT/waterfall displays, and working waterfall display-limit adjustments. Sustained performance remains unverified.

Defaults are device index 0, manual 28 dB RF gain, nominal 200 kHz channel, capture center 250 kHz above the station, and 75 microsecond de-emphasis (50 selectable). The supplied DLL is x86; the x64 app uses a separately installed official RTL-SDR Blog V1.4.0 runtime, preserving the original. See [runtime provenance](../third-party/README.md). General device capabilities, selectable gains/rates, and distribution packaging remain later work.

## Next receive modes

The user confirmed that the improved tuning UX is much better and explicitly deferred the 30-minute stability test. They subsequently authorized implementing the voice modes including SSB, and designing other modes with a maintained backlog. WFM mono/stereo, AM, NFM, USB, and LSB now use a shared Rust receiver configuration and stereo-frame transport. The [receiver design](receiver-modes.md) separates listening modes from optional decoders and preserves hot tuning. The [mode backlog](mode-backlog.md) includes CW, FT8/FT4, digital voice, and other candidates. No decoder dependency or TX functionality was added.

