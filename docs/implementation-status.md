# Implementation status

Updated: 2026-10-13. M01 remains in progress; live RTL-SDR integration was brought forward after the user connected a dongle.

## SSTV service exploration (2026-10-13)

The user accepted the SSTV plan. Added an opt-in `sstv` codec/service prototype,
pinned MIT-licensed sstv 0.3.1, headless probes and bounded cancellable PCM input.
All four initial modes decode independent PySSTV fixtures completely. Nine
targeted tests (including the opt-in fixture test), workspace tests, strict
Clippy, formatting, desktop-plus-SSTV build and hardware-free checks passed.
Reverse encoder validation remains open: the independent decoder has similar
image errors for our audio and PySSTV controls, and does not support PD120.
The service is not wired into the engine/GUI or any transmit path. See the
[spike report and next integration boundary](sstv-spike.md).

## IC-705 USB interface (2026-10-03)

Audio follow-up: the user reported audible radio reception but no app audio.
Windows processed USB capture measured about -110 dBFS versus -31 dBFS with
WASAPI RAW. Radio AF capture now requests per-stream RAW on Windows; this changes
no system-wide settings. The corrected IC-705 engine receives usable audio with
zero input drops. QMX shares the adapter, while non-Windows retains CPAL.
Added USB level display and an optional minimum-level receive probe check.
Validation: final live probe passed its -80 dBFS minimum with -32.0 dBFS input,
201,120 subscribed frames and zero input drops; the GUI screenshot shows the
recovered spectrum/waterfall and USB level. Workspace tests, strict Clippy,
desktop build and hardware-free check passed. Windows system settings were not
modified. QMX's shared path compiled but was not hardware-tested in this follow-up.

Added native CI-V control, USB AF reception, separate monitor/TX audio routes and
a bounded three-second USB transmit-tone test. Settings persist without actions.
Live identification, tune/mode roundtrip and receive engine/GUI checks passed on
COM19; the radio was restored to 146.520 MHz FM. No live TX has been performed;
frequency, power limit and dummy-load rating are awaiting user specification.
Microphone and encoded transmissions remain follow-ups. See [IC-705 details](ic705.md).

Verification: all-feature workspace tests, strict all-target/all-feature Clippy,
formatting and desktop build passed. Hardware-free and GUI-only configurations
compiled. Final live GUI smoke received 173,280 audio frames, exited normally,
and its screenshot was visually inspected. No RF output claim is made.

## Rust RTL default (2026-10-03)

The user validated USB unplug/replug recovery and extended runtime with the Rust
backend. Rust is now the default in normal builds and the experimental label is
removed. Explicit saved backend choices remain intact; librtlsdr stays selectable.
Native-only builds still default to librtlsdr. Gain and sensitivity comparisons
remain uncalibrated. See [backend details](rtl-backends.md).

Verification: both RTL backend tests passed in default and native-only builds;
all six GUI settings tests passed, covering new defaults, saved selections and
fallback. Formatting, strict all-feature/all-target workspace Clippy and the
hardware-free app check passed. No additional live hardware test was run.

## Selectable RTL backends (2026-09-30)

User follow-up confirms weak-signal FT8 decoding and broadcast-FM reception with
the Rust backend. The user reports lower signal/noise-floor levels and subjectively
better FM SNR than the DLL backend, without an apparent reception issue. Gain and
sensitivity comparisons remain uncalibrated. Reconnect and extended-runtime
validation followed on 2026-10-03, enabling the default change above.

Added pinned rs-rtl 0.5.0 as an optional experimental adapter alongside librtlsdr.
Desktop builds expose both choices in the Interface selector and persist selection
independently of shared RF/converter/receiver settings. The engine tears down the
old session before backend handoff. Diagnostics reports the active adapter and
driver configuration; Rust gain readback is unavailable because the upstream getter
returns an encoded register value. Both request 28 dB gain and 2.4 MS/s.

Live V3/Ham It Up probes exercised acquisition, synchronous retuning and shutdown
after a forced consumer stall. A headless engine switched DLL -> Rust -> DLL at
7.074 MHz using the saved +2700 Hz fine correction. Each 35-second period reported
zero host IQ drops; Rust decoded four FT8 messages, while the DLL periods decoded
none. These are different transmission slots, not a sensitivity benchmark. Input
level differences at identical requested gain need further comparison.

The actual selector GUI smoke passed both directions without opening hardware or
writing preferences. Regression coverage includes backend persistence/fallback and
shared buffer-loss/sample-index behavior. Physical reconnect and extended manual
testing were subsequently confirmed by the user. See [evaluation instructions](rtl-backends.md).

## Decoder subscriptions, VOR prototype and Desperado review (2026-09-30)

Implemented engine-owned FT/VOR services, independent shared audio subscriptions,
and bounded off-thread channel-IQ extraction with capture provenance and gap/retune
resets. VOR has an optional persisted panel showing the radial FROM the station,
reference coherence and deviation. Tests cover synthetic bearings, missing-modulation
rejection, recorded KLO/ARL reference ranges, and headless IQ-file decoding alongside
a second channel subscriber. Station identification and live VOR validation remain.

The existing FM/RDS decoder reads the independently documented PI, station name
and full RadioText from a derived real FIP recording. Fixture provenance and license
notices are included. No production FM decoder or hardware driver was replaced.

Verification: all 113 all-feature workspace tests passed; formatting, strict
all-target/all-feature Clippy and desktop build passed. The VOR GUI smoke displayed
120.0 degrees from generated IQ; screenshot inspection passed. The FT8/FT4 GUI
smoke still decoded both WAVs and exercised message display and waterfall labels.
DAB discovery remains deferred; hardware adapter opportunities and test procedures
are recorded in [decoder extensions](decoder-extensions.md).

## FT8 / FT4 decoding (2026-09-30)

UX follow-up: the user confirmed live messages decode. Added a separately persisted
fine RTL/converter correction in Hz, a newest-first compact table with narrow-screen
columns, and optional vertical sender callsigns anchored to original waterfall
rows. Structured sender extraction avoids guessing from free text. Tests cover
correction restoration, history ordering/limits, annotation timing, wrap/expiry,
capture identity and imported/ambiguous exclusions. All 104 tests, strict Clippy,
desktop build, and desktop/compact GUI smokes passed; screenshots confirmed table
density and callsign overlays. Actual converter calibration and off-air annotation
timing remain for user validation. The test's +1900 Hz correction is isolated from
user preferences.

Integrated pinned pure-Rust mfsk-core 0.11.0 behind a receive-only service, with separate bounded audio collection and decode threads, anti-aliased 12 kHz audio, UTC slot assembly, gap/revision resets and static WAV import. The decoder selector applies a 300–5000 Hz USB passband and 6 kHz view to RTL/file sources; QMX uses existing USB/Digi USB audio. Added a bounded message history window, compact Waterfall/Text tabs and independent decoder preferences. Reference tests decoded 14 FT8 and 16 FT4 messages; additional tests exercise IQ-to-USB-to-decoder processing, aliases, slots, discontinuities and invalid input. Desktop and compact GUI smoke tests exercised actual WAV buttons and tab switching. Live RTL/QMX sensitivity and clock/latency behavior remain for validation. See [usage, backend/license decision, fixtures and follow-ups](ft-decoding.md). JS8 and encoding remain future work.

## Automatic spectrum resolution (2026-09-29)

Implemented filtered/decimated zoom FFT analysis for RTL, file and synthetic sources, with a logarithmic Span control, Fit receiver / Full span buttons and bin/RBW/observation readouts. Display-only commands preserve radio revisions, capture, scanning and audio state. Historical waterfall rows retain original frequency coverage and resolution. Tests cover close tones, alias suppression, chunk invariance, uninterrupted audio during zoom, passband fitting and source reset after a smaller FFT. All 91 workspace tests, strict all-feature Clippy, formatting, desktop build and feature-minimal GUI check passed. The actual Fit receiver GUI smoke reached 2.289 Hz/bin and returned to full span without changing capture/revision; the existing synthetic interaction/diagnostics smoke also passed. Live FT8 display remains for user validation. See [usage and analysis design](spectrum-resolution.md).

## External frequency converters (2026-09-29)

Added a signed RTL converter offset with a Ham It Up -125 MHz preset. RF coordinates are shared by the VFO, spectrum, memories and scans; only hardware tuning is translated. Offset changes stop scans and restart acquisition, and settings restore the offset before validating HF frequencies. All 85 workspace tests, all-target/all-feature Clippy, formatting, desktop build and stopped 40 m GUI smoke passed. Live reception through the user's converter remains to be checked. See [setup and limitations](frequency-converters.md).

## Persistent preferences (2026-09-28)

Added OS-user configuration storage with independent versioned receiver, decoder, device, memory/scan, practice and workspace sections. Background autosave uses atomic replacement and a single-writer file lock; damaged/future documents remain read-only. Normal restoration leaves reception/scanning/practice stopped. The actual GUI close/reopen check restored its workspace successfully. All 82 workspace tests and all-target/all-feature Clippy passed. See [settings behavior, extension contract and verification](settings.md).

## QMX receive/control exploration (2026-09-28)

Digi follow-up: CAT modes 6/9 now support startup, live polling, mode selection and band recall without terminating reception. Live mode transitions preserved capture generation and continued audio without reported input drops. Restored the user's 21,094,600 Hz CW state. Six protocol tests, all-target/all-feature Clippy and desktop build passed. See [Digi mode correction](qmx-reception.md#digi-mode-correction).

QMX is now an optional engine source (`qmx`, included in `desktop`), with CAT VFO/mode control, band discovery/recall and CPAL receive audio. The GUI exposes USB/LSB/CW/CW-R, band/step controls, explicit input/monitor selection, audio gain, and an audio spectrum/waterfall. The hardware engine test verified tuning and band round trips; the GUI smoke verified receive display and keyboard tuning/restoration. Protocol tests cover gaps, mode rejection, band memory and VFO B/front-panel readback. See [setup and current limitations](qmx-reception.md). The earlier exploration history follows.

The connected QMX (firmware 1_03_002, COM18) supplied five seconds of 48 kHz two-channel USB audio without reported drops, stream errors or clipping. A Rust CAT probe verified a +100 Hz change and restoration to 24,924,600 Hz in receive. Added optional `sdr-devices/cat` and two reusable command-line probes; three CAT tests and scoped Clippy checks passed. This remains outside the GUI/engine; raw IQ sample delivery was subsequently verified with a second five-second capture and restoration of normal audio mode; IQ orientation and off-air reception remain untested. The user changed default playback from QMX to the Behringer speakers, verified through CPAL. Explicit monitor-device routing remains the first integration priority. See [QMX findings, Hamlib/Android evaluation and next slices](qmx-integration.md).

QMX follow-up: a user-provided radiated 14.055 MHz VNA reference was detected in raw I/Q and USB audio. Two dial settings established positive frequency orientation for channel 0 + j*channel 1 and the approximately 12 kHz USB IF offset. Normal audio showed approximately 998 Hz when the dial was 1 kHz below the generator. All three captures had no reported drops/errors/clipping; original 14.055 MHz USB/audio/receive settings were restored. See the QMX integration document for measurements and limitations.


## CW prosigns (2026-09-28)

The user-reported joined BK failure was reproduced and fixed in the shared practice/live decoder. Nine-element groups and explicit prosign labels now cover BK, AR, AS, BT, SK, KN, KA, AA, VE, HH and SOS. Tests distinguish joined and spaced groups and reject invalid overlong sequences; transcript trimming preserves complete labels. All 66 workspace tests, all-feature Clippy, and desktop build passed. See [CW decoder conventions](cw-decoding.md).

## Local CW practice (2026-09-27)

The header's **CW Practice** window supports straight key, iambic A/B, paddle WPM, adjustable sidetone, practice text, and learnable keyboard/mouse/controller/MIDI contacts. The keyer runs on a separate sample-timed worker with no radio-command access. Escape, focus loss, controller removal, input overflow and stalled heartbeat cancel practice. Desktop includes optional midir input and Bevy's gilrs controller backend. All 63 tests, all-feature Clippy, desktop and GUI-without-MIDI builds passed locally; core/DSP WASM compilation remains supported. The practice GUI check covers learned bindings, straight-key and Mode B text, and cancellation using injected input states. An Arturia MicroLab input was enumerated and successfully opened; the user subsequently validated MIDI and gamepad input and the 600 Hz sidetone. Quantitative end-to-end latency remains unmeasured. See [CW practice setup and timing limits](cw-practice.md).

## CW listening and text (2026-09-27)

CW now has 200/500/1000 Hz filtering, a fixed 600 Hz listening tone, optional AGC, CSV memories, and an experimental Rust Morse decoder. Signal Data exposes seeded adaptive WPM, manual threshold, stretched word spacing, reset, and copyable text; recent text appears above the spectrum. Decoder-only edits preserve audio/capture; retunes and discontinuities invalidate text. Library research, the generated IQ fixture, and current limitations are documented in [CW decoding](cw-decoding.md). All 55 workspace tests, all-feature Clippy, core/DSP WASM compilation, and the CW GUI smoke passed locally. Off-air hand-keyed accuracy is not yet validated; automatic speed/threshold acquisition and adjustable pitch remain follow-ups.

## Architecture preparation (2026-09-24)

Native RTL/audio/rendering features are separated; the UI uses an engine client; optional bounded sample-timed receiver audio and ordered events are available. Controls retain mode preferences and adapt to compact windows. Core/DSP compiled for WASM locally; native Windows tests and before/after traces were checked. Linux/macOS CI is configured but unexecuted. See [engine boundary](engine-boundary.md) and [trace results](architecture-trace-comparison.md). Server and TX operations remain unimplemented.

## Broadcast FM data (2026-09-25)

The Rust RDS/RBDS decoder and **Signal Data** window receive station name, RadioText, PI, numeric PTY and traffic flags. A pre-audio multiplex tap works with mono/stereo and mute; decoder toggles preserve listening and scan state. Generated IQ tests and GUI interactions pass; the live 89.9 MHz probe decoded KCRW text with zero IQ drops. Extended characters, weak-signal optimization and additional RDS services remain follow-ups. See [usage, limitations and verification](fm-broadcast-data.md).

## Memories and scanning (2026-09-25)

CSV memory groups, full receiver-setting recall, wheel/arrow navigation, per-entry scan inclusion, and worker-owned memory/VFO range scans are implemented. Scans use filtered channel-power thresholds and fixed pauses; stopping holds the actual channel. See [usage, storage format and verification](memories-and-scanning.md). Additional demodulation modes remain deferred.

## Receiver UX and memory editor (2026-09-25)

Signal Data remains mode-aware, and fresh RDS station/RadioText also appears above the spectrum. Long text scrolls; plot headings are removed. The header highlights the VFO in amber and places receive status at the right. Volume/Mute sits below Reception; the capture center is a cyan label on the spectrum axis with the same font size as its bounds. Its editable input remains in the controls.

VFO/Memory and Scan are independent header toggles. The separate memory editor puts scan settings and CSV/group operations above an editable, virtualized table. Cells support mouse and Tab navigation; editing does not retune. Optional columns expose the complete receiver settings. GUI checks cover click/Tab editing, CSV save/reload, navigation/scanning, inline RDS with details closed, and the compact layout. See [memory workflow](memories-and-scanning.md).
## Implemented

- Compact operational header with interface selection, mode selection, and a prominent editable VFO; decorative branding and the permanent debug footer are removed. Diagnostics are an optional resizable, scrollable window toggled from the header or F12. Source/audio faults remain visible in the header even with Diagnostics closed.
- Bevy frame-rate and background system diagnostics show smoothed render-loop FPS/frame interval, app/system CPU, resident memory, and total memory use. CPU percentages use total machine capacity; app memory is GiB. GPU adapter/backend/driver details and available instrumented GPU pass timings are shown separately; these are not whole-GPU utilization, VRAM, or temperature measurements. The GUI feature now enables Bevy's sysinfo integration.
- Acquisition begins in application startup after renderer initialization, preventing short recordings from playing through during graphics setup. GUI smoke coverage includes diagnostics open/close and requires populated FPS/CPU metrics.

- Four Rust crates: engine-independent domain types, devices, DSP, and application/controller/frontend. Bevy/egui are optional; the headless synthetic CLI remains available.
- Phase-continuous synthetic IQ with fixed reusable buffer pools, bounded channels, explicit lifecycle commands, generation isolation, and block recycling.
- Periodic-Hann 4096-bin shifted FFT. A bin-centered unit complex tone peaks at 0 dBFS; silence is clamped to -140 dBFS. This is not calibrated RF power or noise spectral density.
- Controller worker with coalesced settings and replaceable spectrum slots. UI submission/polling does not wait on locks; obsolete revisions are rejected. FFT display updates are approximately 29.3 Hz at 2.4 MS/s.
- Bevy spectrum and circular 1024-by-256 GPU waterfall, zoom/levels, source selection, start/stop, and tuning. Four FFT bins reduce by maximum per waterfall column. Missing display rows are marked; level changes recolor bounded history on the CPU.
- Dynamic librtlsdr boundary with explicit library override, configuration readback, unsigned-byte IQ conversion, eight reusable host buffers, bounded delivery, and host-drop accounting. USB reads and cancellation run off the UI thread. Cancellation retries during reader startup and reports a two-second timeout; a stuck native reader retains its resources until it exits.
- Rust receiver DSP: WFM mono/stereo, AM, NFM, USB/LSB; multistage channel filtering, 48 kHz stereo frames, pilot recovery/blend, audio AGC, optional NFM power squelch, de-emphasis, and DC removal. IQ discontinuities reset receiver state.
- CPAL audio with bounded SPSC ring, producer-side linear rate conversion and bounded clock-drift feedback, volume/mute, and underrun/overrun counters. The callback consumes prepared samples without allocation or logging. Audio errors are shown while spectrum processing can continue.
- 106.7/89.9 MHz presets, station entry from 28.8 to 1700 MHz in normal RTL mode, and plot tuning. Initial capture center is station plus 250 kHz; VFO and center then tune independently. Fixed gain and selected bandwidth are labeled. No TX controls were introduced.

## Verification evidence

### Optional Perfetto tracing

The `perfetto` feature enables Bevy Chrome JSON export and custom block-level SDR spans. A development-build stereo file GUI smoke exited normally and produced `target/perfetto-smoke.json`; JSON parsing and span checks found UI frames, waterfall updates/recoloring, receiver processing, FFTs, producer audio preparation, and file reads. The trace includes the named radio-controller thread as well as Bevy threads. This is export/structure validation, not a release performance baseline or a claim of inspecting the file in Perfetto itself. All 29 integration tests pass with tracing enabled, strict Clippy and formatting pass, and the no-default-features headless build compiles. See [capture instructions](profiling.md) for an optimized capture and reduced-volume filtering. GPU timeline profiling and audio-callback instrumentation are outside this increment.

### Header and optional diagnostics increment

All 29 integration tests, formatting, and strict Clippy passed after the UI changes. Synthetic, stereo file playback, and live RTL GUI checks exercised the compact header and opening/closing Diagnostics with actual FPS/CPU measurements. The final RTL check also exercised start/stop, preset tuning, waterfall VFO tuning, center scrolling, retained capture/history, and resizing. Screenshots show the operational header and populated CPU/memory/GPU information; instrumented GPU timing entries were available on the Vulkan renderer. These short UI checks do not add a new long-run performance claim. Initial failed smoke attempts are superseded by successful reruns; acquisition now starts after renderer initialization so graphics setup does not consume short recordings.

### Latest user validation

The user completed a 30-minute manual stability test while trying different modes and reports validating most available modes. Live AM validation remains outstanding; they expect to install external conversion hardware to reach consistent AM signals. The exact tested-mode list, build profile, queue/memory measurements, and loss counters were not supplied. Record the manual duration test as completed, without inferring zero drops or satisfying every instrumented M01 criterion. Earlier references to the test being deferred describe the prior state.

The next design focus is [modular receiver controls and broadcast services](receiver-panels.md): independent mode settings and decoder results, RDS/RBDS station text first, and a separate HD Radio feasibility path. Converter mapping is planned pending equipment details. No new decoder or UI implementation is claimed by this update.

### Voice modes increment, 2026-09-24

The workspace now has **29 integration tests**, all passing with GUI/audio features enabled. Seven new DSP tests cover AM carrier/DC removal and recovered tone, AM/SSB AGC level normalization, NFM tone/squelch release, USB/LSB sideband rejection above 40 dB, stereo separation above 30 dB with a +2 Hz pilot offset, pilot loss/no-pilot mono fallback, stereo endpoint mapping, invalid configurations, and block-boundary invariance. Additional controller/tuning tests verify all six mode changes retain capture/file position, broad tuning limits, and fine HF file tuning. These are clean-fixture checks, not full RF conformance measurements.

The stereo file GUI smoke exercised stop/start, EOF, and resizing and exited successfully. `target/file-smoke.png` was visually inspected: WFM stereo selected, pilot locked, spectrum/waterfall retained, no IQ/audio loss counters. The `voice_fixture` example generates two-second recordings for every mode; the stereo fixture has 1 kHz left and 2 kHz right audio.

An eight-second live probe on the attached RTL-SDR at 106.7 MHz acquired stereo pilot lock and reported zero host IQ drops, zero audio under/overruns, and no audio error. This validates the live stereo path on the current signal; AM/NFM/SSB still need real-world listening validation. At that checkpoint the 30-minute stability test was deferred. The user subsequently completed it across most modes; AM validation and an instrumented loss/memory record remain outstanding. M01 remains incomplete.

No new library dependency was needed. Optional CW/FT8/digital-voice decoders, decoder taps/workers, and UTC metadata are designed in the documents but not implemented. HF direct sampling is also pending; normal RTL tuning is restricted to 28.8–1700 MHz, while IQ files can represent lower frequencies.

### Earlier baseline evidence

Windows, Rust 1.93.1. The original twenty integration tests cover oscillator continuity, invalid parameters, pool reuse, stop/restart, measurements, CLI behavior, FFT scaling/order/silence, display-stall isolation, source reconfiguration, byte IQ conversion, FM demodulation, file decoding/validation, and paced playback lifecycle/recovery.

FFT tests verify peak scaling within 0.001 dB of -6.0206 dBFS for amplitude 0.5. A generated 1 kHz FM tone with 37.5 kHz deviation at -250 kHz offset verifies recovered amplitude within 0.025, over 99% steady-state energy at 1 kHz, and exact equality across different input block boundaries. Tests require no radio hardware.

The initial headless release run processed 16,777,216 samples with mean power 0.250000 and tone offset +125000.00 Hz. A zero-amplitude run reported zero power and no frequency estimate.

Live RTL-SDR v3/R820T probes on 106.7 and 89.9 MHz ran approximately eight seconds each, produced nonzero decoded audio through the default output, and reported zero host IQ drops or audio under/overruns. The GUI smoke stopped/restarted, selected 89.9 MHz (acknowledged revision 4), resized to 1100 by 760, saved target/rtl-smoke.png, and exited successfully. Screenshot inspection confirmed live spectrum, selected station, controls, and diagnostics. Graphics: AMD Radeon 8060S, Vulkan, driver 26.5.2.

User validation on 2026-09-24: controls and FM demodulation work; multiple FM stations can be tuned with clearly audible audio. The user also confirmed that the FFT and waterfall look correct and that waterfall display-limit adjustments work properly.

This establishes a working live reception and listening baseline on the current setup. Sustained real-time deadlines and full M01 acceptance remain unverified; no 30-minute performance run has been recorded. Host counters cannot detect every upstream USB/device loss.

User USB recovery validation on 2026-09-24: unplugging the RTL-SDR stopped reception. After reconnecting it, clicking Start Reception resumed audio. This confirms manual restart after a USB disconnect on the current setup; automatic reconnection and repeated-cycle endurance were not assessed.

## Limits and continuation notes

- RTL mode exposes only device 0, 2.4 MS/s, and 28 dB gain. Each receiver mode has selectable bandwidth presets. VFO changes keep reception running; center changes briefly restart USB reads while keeping the device and audio endpoint open. Voice modes include stereo WFM. RDS, recording, PPM calibration, HF direct sampling, and device selection remain outstanding.
- Synthetic acquisition backpressures; live acquisition drops blocks when its host pool is exhausted. Display skips, host IQ drops, and audio losses are counted separately. Standard-library channels may synchronize internally; the device pipeline is not advertised as lock-free.
- Old synthetic leases retain their originating generation's storage until released. They cannot contaminate a new generation. Application teardown joins the controller worker.
- Audio uses the default endpoint, float32/int16/uint16 formats, and rates from 32–192 kHz. The ring holds up to 500 ms with roughly 40 ms startup/retune prefill. Linear conversion is an initial implementation, not a high-fidelity resampling guarantee. Clock drift needs long-run measurement. IQ losses reset DSP but can leave previously queued audio playing briefly.
- Native loading defaults to the build workspace; packaged runtime discovery is still needed. Windows x64 is the tested platform. The original x86 DLL is preserved; see ../third-party/README.md for runtime provenance.
- The signal meter estimates one steady tone, not a multi-signal frequency detector. Waterfall time is sample time, not a hardware timestamp. GPU palette shaders remain an optional optimization.
- Capabilities remain a documented design, not a universal device API. Keep RX, future TX, and conventional transceiver control separable; do not turn the concrete RTL session into a mandatory trait for radios without IQ streams. No operational TX or PTT exists.

## Next increment

1. User priority: validate the implemented stereo WFM, AM, NFM, USB/LSB modes and design optional decoders. See [receiver modes](receiver-modes.md) and the [mode backlog](mode-backlog.md).
2. Retain missing-audio-endpoint, repeated retuning, output-rate-change, and repeated USB recovery checks as follow-up validation. A manual RTL-SDR unplug/reconnect and restart has been confirmed by the user.
3. The user has completed the 30-minute manual multi-mode test. Instrumented release-profile measurements remain unrecorded; retain those evidence gaps separately from the completed manual test.

Preserve headless tests and future transmit/transceiver boundaries. Do not mark M01 complete until its remaining acceptance criteria have evidence.

Final regression on 2026-09-24: live and synthetic GUI smoke tests both exited successfully after requiring actual spectrum delivery before the first scripted click. An earlier GUI run timed out before completing its controls; stage/revision diagnostics were added so future failures are actionable. Formatting, strict Clippy, and the GUI build also passed.

## IQ file playback increment

User validation on 2026-09-24: the user confirmed that playing the generated test file works. This adds manual playback confirmation to the automated evidence below. Manual RTL-SDR USB recovery is also confirmed above; audio-device recovery checks and the 30-minute stability run remain outstanding.

The file source reads headerless interleaved little-endian float32 I/Q with explicit capture center and a supported rate of 2.4 MS/s. It uses bounded buffered reads and the same receiver/FFT/audio path as RTL reception. The controller paces against sample time, bounds catch-up after suspension, checks stop/reconfiguration while waiting, reports progress and EOF, and clears obsolete status on faults. Start after EOF replays from the beginning; VFO/de-emphasis changes during playback preserve the file position. No pause/seek/loop is implemented.

Files must be nonempty regular files containing complete finite I/Q pairs. A partial final processing block reaches the demodulator; incomplete FFT windows are omitted rather than padded into a misleading final spectrum. Metadata and file validation errors are recoverable through a new settings revision. The native radio is never opened for file playback.

Audio at EOF switches to draining mode so recordings shorter than startup prefill can play, then waits at most one second for queued samples while honoring cancellation. This is a bounded host-queue drain, not proof that an arbitrary output device has physically played every final sample.

Four new integration tests cover little-endian ordering and partial blocks, malformed files and metadata, paced EOF/replay, and fault recovery/stop. A regression also checks that an open failure after EOF clears the previous file's EOF/progress. The `fm_fixture` example creates a two-second 1 kHz FM recording at -250 kHz offset with 37.5 kHz deviation and amplitude 0.5. See the README for commands and format limitations.


Playback verification on 2026-09-24: all 17 integration tests passed with all features enabled; strict Clippy, formatting, and the GUI build passed. File, synthetic, and live RTL GUI smoke tests all exited successfully after the final changes. Inspected screenshots show file progress at 2.00 / 2.00 seconds with END OF FILE and decoded audio RMS 0.320, plus working synthetic and live displays. The live retune reached revision 4 at 89.9 MHz with zero reported IQ/audio losses in the captured status. These are short checks; the sustained performance gate remains open.



## Responsive tuning increment

VFO and capture center are independent. Receiver-only revisions update FM processing in the existing controller session and preserve file position, acquisition timeline, and CPAL output. Spectrum frames carry capture-center metadata; stale frames cannot be plotted on the new axis. Live center changes cancel/join USB reads, discard old blocks, set/read back frequency, reset the native buffer, and restart reads on the same device handle. Native configuration never runs concurrently with acquisition. Intentional retune discards are separate from IQ pool-overflow counters.

The UI supports click/drag VFO selection, wheel center shifts, Shift+wheel VFO shifts, and 1–1000 kHz step choices. Capture and VFO move together for center panning; in-band station selection keeps center fixed. Selection beyond the usable +/-1 MHz range automatically recenters live input; file input clamps to its fixed capture. History stores each row's original center and is reprojected on center/level changes, avoiding cumulative shifts when panning back and forth. The amber overlay indicates the 200 kHz channel.

Old-station audio is tagged with an epoch and discarded by the audio callback after a tune. CPAL remains open, with approximately 40 ms prefill and a short fade-in. Hardware transitions begin this wait before stopping USB reads, so intentional silence does not appear as an unexpected underrun. Short hardware tests measured 23 ms for VFO acknowledgement plus the next FFT frame and 188–215 ms for center retunes, versus 1481 ms initial open. Unexpected IQ loss and audio under/overruns stayed at zero in that probe; retune discards were counted separately. These measurements do not establish audible click-free switching or sustained performance.

Three added integration tests verify file-position/capture continuity, snapping/recentering/fixed-file limits, and step panning at band edges. All 20 integration tests and strict Clippy passed. The expanded live GUI smoke exercised preset retuning, a waterfall VFO click, wheel center shift, retained capture revision and history, and resize; its screenshot shows 89.9 MHz VFO with 90.25 MHz center. A user listening/interaction check and the long-run stability scenario remain outstanding.

Final UX regression: RTL, file, and synthetic GUI smoke tests all exited with code 0 after the final code changes; screenshots were inspected. Formatting, GUI build, strict Clippy, and all 20 integration tests passed. The previous USB unplug/reconnect confirmation predates this tuning change; repeat that check during subsequent recovery validation.
