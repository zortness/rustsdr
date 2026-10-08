# Roadmap

Audience: contributors selecting the next increment. Milestones describe observable outcomes rather than calendar estimates. M01 is in progress; no full milestone is complete yet. See the [implementation status](implementation-status.md).

## M01: File-backed receiver and rendering foundation

Build synthetic and file input, a Bevy spectrum/waterfall, one tunable receiver, mono broadcast-FM demodulation, and speaker audio. Establish bounded buffering, shutdown, diagnostics, and headless DSP tests.

Gate: known signals display and demodulate correctly; playback stays bounded during UI interaction; acceptance evidence in the [first-milestone brief](milestone-01.md) is recorded.

## M02: First live radio

Select equipment and implement one adapter with discovery, tuning, gain, sample-rate selection, and disconnect/reconnect handling. Keep manufacturer-specific libraries inside the adapter. Add audio-clock drift handling and long-run diagnostics.

Gate: sustained live reception, repeated lifecycle transitions, and sample-rate changes work on the named test equipment. Report overflows and audio underruns with measured results. Capture device, driver, OS, and build details.

## M03: Daily-use single receiver

The user completed a 30-minute manual multi-mode stability test. Current priority: modular receiver controls and results panels, followed by RDS/RBDS text decoding; live AM validation awaits suitable signals or converter support. See [the panel and broadcast-service design](receiver-panels.md). See [receiver modes and optional decoders](receiver-modes.md) and the [mode backlog](mode-backlog.md) for remaining work. Moving this work forward does not waive the earlier milestones' outstanding acceptance gates.

Add AM, narrow FM, USB/LSB, CW, appropriate AGC/squelch, configurable filters, bookmarks, settings recovery, and audio recording. Expand format support based on real recordings. Add packaging for the initial platform.

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
