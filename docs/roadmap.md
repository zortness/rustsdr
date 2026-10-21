# Roadmap

Audience: contributors selecting the next increment. Milestones describe observable outcomes rather than calendar estimates. M01 and M02 are complete for their documented Direct3D 12 acceptance configuration on the named Windows host. See the [implementation status](implementation-status.md).

## M01: File-backed receiver and rendering foundation

Build synthetic and file input, a Bevy spectrum/waterfall, one tunable receiver, mono broadcast-FM demodulation, and speaker audio. Establish bounded buffering, shutdown, diagnostics, and headless DSP tests.

Gate: known signals display and demodulate correctly; playback stays bounded during UI interaction; acceptance evidence in the [first-milestone brief](milestone-01.md) is recorded.

Status: accepted on 2026-10-19 with the Direct3D 12 backend and a separate live audio-output recovery check. Default Vulkan still fails the memory trend on the test host; that limitation is recorded in the [soak results](soak-results.md).

## M02: First live radio

Select equipment and implement one adapter with discovery, tuning, gain, sample-rate selection, and disconnect/reconnect handling. Keep manufacturer-specific libraries inside the adapter. Add audio-clock drift handling and long-run diagnostics.

Status: accepted on 2026-10-20. A Direct3D 12 30-minute run passed the resource and host-loss gates on the named AMD/Windows host. Device discovery/selection, manual gain and 1.92/2.4 MS/s controls passed software and live checks; three physical reconnect cycles resumed reception in one process. Two physical dongles were not available for a simultaneous selection test. Three earlier Vulkan runs failed the memory gate. See the [M02 brief](milestone-02.md) and [soak record](soak-results.md).

Gate: sustained live reception, repeated lifecycle transitions, and sample-rate changes work on the named test equipment. Report overflows and audio underruns with measured results. Capture device, driver, OS, and build details.

## M03: Daily-use single receiver

Status: in progress. The seven listening modes, modular controls, RDS/RBDS results, memories/scanning, and persistent settings are implemented. Decoded WAV recording and a local Windows package candidate are implemented but still need the acceptance checks in the [M03 brief](milestone-03.md). A 30-minute manual multi-mode session is recorded; live AM validation and a measured mode-coverage session remain open. See the [receiver modes](receiver-modes.md) and [mode backlog](mode-backlog.md) for related work.

Outcome: one receiver supports AM, narrow FM, USB/LSB, CW and broadcast FM with suitable AGC/squelch and filters, reusable memories and settings, decoded audio recording, and an installable package for the initial platform. Expand input formats when real recordings require them.

Gate: each supported mode has signal fixtures and documented behavior; recording and configuration failures are recoverable; a new user can install and receive without developer tooling.

## M04: Multiple receivers

Allow multiple VFOs inside one captured band with independent modes, bandwidths, and output routing. Define hardware-retune behavior when another VFO would move outside the usable band. Mix or route audio explicitly.

Gate: measured receiver-count limits, fair processing, isolated receiver removal, and predictable behavior during retuning and output-device changes.

## M05: Broader integration

Potential exploration: a native Rust radio service with a browser renderer/audio client, alongside Linux/macOS and mobile portability. This is recorded interest, not a committed milestone. See [platform and WebSDR explorations](platform-explorations.md) for useful current refactors and deferred transport/platform work.

Prioritize additional hardware, network sources, IQ recording, scanning, selected decoders, and additional platforms based on actual needs. Each feature should have its own small acceptance brief.

Gate: feature-specific end-to-end evidence and resource limits. External binary plugins and a generic DSP graph editor remain separate proposals.

## Future milestone: Transceiver control and transmission

Schedule separately when equipment and use cases are selected; it need not wait for every M05 integration. First prove read/control behavior and reported state, then conventional audio/PTT or IQ transmission for one selected device.

Prerequisites: a device-specific control model, separate RX/TX configuration, resource/duplex coordination, transmit lifecycle and failure policy, explicit audio routing, and a controlled validation setup appropriate to the equipment and authorized operation.

Gate: review the transmit design before enabling RF output; validate stream/PTT ordering, startup state, external control changes, disconnects, underruns, and shutdown on actual equipment. Receive-only adapters must remain usable without transmit features.

## Evidence and maintenance

After each milestone, record what was implemented, the commands/scenarios tested, platform/device details, measurements, and remaining limitations. Update the decisions record when evidence changes a default. Do not equate a smooth display with uninterrupted IQ or audio processing.
