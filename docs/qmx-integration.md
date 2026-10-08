# QMX USB receive and control exploration

The exploration below led to [implemented desktop QMX reception](qmx-reception.md). Descriptions of missing GUI support below refer to the earlier probe sessions.

Hardware session: 2026-09-28, Windows, user's high-band QMX connected to a 50-ohm load. This is a successful hardware probe, not yet a GUI source. No transmission, radio playback stream, firmware update, or persistent configuration write was performed.

## Observed on this radio

| Item | Result |
| --- | --- |
| USB identity | VID 0483, PID A34C; Windows QMX Transceiver audio driver |
| CAT interface | COM18, 38400 baud, 8N1, no flow control, DTR/RTS cleared |
| Model / firmware | `OMQC`, `VN1_03_002QMX` |
| Initial and restored tuning | VFO A/B 24,924,600 Hz, VFO A simplex, CW (`MD3`) |
| Receive status | `TQ0` |
| USB stream interpretation | `Q90`: demodulated audio, not raw I/Q |
| Input endpoint | `Digital Audio Interface (2- QMX Transceiver)` |
| Windows capture format | Float32, 48,000 frames/s, two channels |
| Five-second capture | 238,560 frames; no ring-buffer loss, stream errors, clipping, or nonfinite samples |
| Level per channel | RMS -81.00 dBFS, peak -68.08 dBFS |
| Control test | Set VFO A to 24,924,700 Hz, read back exact value, restore and verify 24,924,600 Hz |

The low-level capture is consistent with noise into a dummy load. It proves USB sample delivery, not off-air voice/CW reception, receiver sensitivity, or decoder performance. Both channels reported the same levels; do not interpret them as quadrature while IQ mode is disabled. The capture is at `target/qmx-cw-audio.f32le`: headerless interleaved little-endian float32 channels, 48 kHz. It is **not** an IQ fixture for the existing file source.

Windows had also made the QMX the default **playback** endpoint. The main application's monitor and CW practice currently follow the OS default output. Select headphones/speakers in Windows before ordinary playback with this radio attached. Radio audio output must become a separately selected, explicitly enabled TX path; it must never be used implicitly as the monitor output. The new probe never opens an output stream.

Windows PnP/WMI enumeration failed with a provider error even outside the sandbox. Registry identification, CPAL enumeration, actual audio capture and CAT communication provided independent identification. Audio capture required execution outside the sandbox; CAT worked inside it.

## Repeating the probes

### Follow-up: raw I/Q capture

After the user changed Windows playback routing, CPAL confirmed the default output was **Speakers (BEHRINGER UMC202HD 192k)**. A receive-only follow-up temporarily enabled `Q91`, verified it, and captured 238,560 two-channel frames at 48 kHz into `target/qmx-raw-iq.f32le`. There were zero reported dropped samples, stream errors, clipped samples or nonfinite values. Channel RMS levels were -83.39 and -78.20 dBFS; peaks were -70.09 and -66.25 dBFS.

The test restored and verified `Q90`, 24,924,600 Hz, CW (`MD3`), and receive (`TQ0`). No playback endpoint was opened. This confirms raw-mode USB sample delivery; with a dummy load it does not establish I/Q orientation, frequency calibration, usable bandwidth, or reception of a known signal. The temporary orchestration script is `target/qmx-iq-capture.ps1`; production session ownership and restoration still belong in Rust.

### VNA reference test: 14.055 MHz

The user placed an unconnected VNA CW source at 14.055 MHz near the QMX antenna/dummy-load arrangement. CAT confirmed VFO A at 14,055,000 Hz, USB (`MD2`), simplex, receive. Three five-second captures were made: raw mode at the original dial setting, raw mode after tuning down 1 kHz, and normal USB audio at that lower dial setting. All reported zero sample drops, stream errors, clipping and nonfinite samples.

Analysis treats channel 0 as real and channel 1 as imaginary, with a forward complex FFT. It discards the first second and averages five 32,768-point Hann-windowed spectra (1.464844 Hz bins).

| Capture | Dial Hz | CAT receiver LO Hz | Strongest spectral bin |
| --- | ---: | ---: | ---: |
| Raw I/Q | 14,055,000 | 14,042,999 | +11,998.535 Hz |
| Raw I/Q | 14,054,000 | 14,041,999 | +12,997.559 Hz |
| USB audio | 14,054,000 | 14,041,999 | +/-997.559 Hz |

The strongest raw peak moves approximately +1 kHz when the receiver LO moves -1 kHz. Thus **RF frequency approximately equals reported LO plus signed complex-bin frequency** for this unit in this USB configuration, using channel 0 + j*channel 1. Do not label raw DC as the dial frequency: the dial is approximately 12 kHz above the LO here. The normal audio capture places the reference about 1 kHz above the lowered USB dial, as expected. At the original USB dial, the reference is near zero beat, so it need not be an audible tone.

The reference peak's raw mirror-bin power was about 27 dB below its desired bin in both captures; that is an observation of this uncalibrated path, not a receiver image-rejection specification. The few-hertz offset cannot be assigned to either oscillator without a calibrated reference, clock-rate characterization and finer frequency estimation. RF levels, sensitivity and usable bandwidth cannot be inferred from uncontrolled radiated coupling. Other operating modes and bands still require validation.

Files are `target/qmx-vna-iq-14055000.f32le`, `target/qmx-vna-iq-14054000.f32le`, and `target/qmx-vna-audio-14054000.f32le`. Reproduce the offline analysis with:

```powershell
cargo run -p sdr-dsp --example iq_capture_spectrum -- target/qmx-vna-iq-14055000.f32le target/qmx-vna-iq-14054000.f32le target/qmx-vna-audio-14054000.f32le
```

The analyzer passed Clippy with warnings denied. Capture orchestration is in `target/qmx-vna-capture.ps1`. Final readback verified 14,055,000 Hz, USB, `Q90` and `TQ0`; VFO B remained untouched. No transmission or playback into the radio occurred.

### Basic probe commands

List audio inputs, including their underlying driver names, without opening capture:

```powershell
cargo run -p sdr-app --no-default-features --features audio --example radio_audio_probe
```

Capture five seconds from the exact, unique input name reported above. Pick a **new** output path; the probe refuses to overwrite files:

```powershell
cargo run -p sdr-app --no-default-features --features audio --example radio_audio_probe -- "Digital Audio Interface" target/qmx-audio-next.f32le
```

The probe currently accepts the host's float32 default format only. It uses a bounded callback queue, reports loss/errors, and writes samples outside the audio callback. Production capture still needs device IDs, explicit format negotiation, discontinuity metadata, hotplug handling, and a recording container.

Enumerate serial ports, then query the selected QMX:

```powershell
cargo run -p sdr-devices --no-default-features --features cat --example qmx_probe
cargo run -p sdr-devices --no-default-features --features cat --example qmx_probe -- COM18
cargo run -p sdr-devices --no-default-features --features cat --example qmx_probe -- COM18 --tune-test
```

Port numbers can change. There is no arbitrary CAT command option. The default is query-only. The tune test requires receive, VFO A simplex, and room for +100 Hz within a supported high-band amateur allocation. It attempts restoration even after a write/readback error, verifies restoration, and reports failure explicitly. Power loss, unplugging, or forcibly killing the process can prevent restoration; this is a probe, not a transactional device API.

The CAT probe's hardware opening is currently restricted to Windows. The serial library documents that Linux can briefly assert DTR during open even when requested false. A portable implementation must validate transport line behavior and the radio's DTR keying configuration before enabling it on another platform.

## Audio, I/Q and CAT are separate capabilities

The [QMX operating manual matching this firmware](https://www.qrp-labs.com/images/qmx/manuals/operation_1_03_002.pdf) describes a 24-bit, 48 ksps USB soundcard. In normal operation it supplies receiver audio. IQ mode instead supplies raw ADC channels. Windows float conversion is the host format, not a claim of additional ADC precision.

For normal audio, bypass our RF demodulator: capture -> audio conditioning/metering -> optional CW or future digital decoder -> selected monitor. Show an **audio-frequency** spectrum unless the radio provides sufficient metadata to label an RF axis correctly. The existing CW decoder can be reused, but pitch and filter configuration must match the radio rather than assuming our current 600 Hz BFO.

For IQ, build a distinct 48 ksps complex source with device-reported tuning metadata. The current receiver path assumes 2.4 Msps and cannot accept this stream unchanged. A 48 ksps complex stream has a nominal 48 kHz sampling span; analog filters and the IF arrangement constrain the useful RF coverage. Validate channel order, polarity, LO/IF offset and frequency-axis direction with a known signal before displaying absolute RF frequencies. This session queried `PL24911900...` while the dial was 24,924,600 Hz in CW: the LO and dial are demonstrably different. Raw IQ sample delivery was subsequently verified as described below; channel order and RF-axis calibration remain unverified.

The [CAT manual](https://www.qrp-labs.com/images/qmx/manuals/cat_1_03_000.pdf) specifies semicolon framing without line endings; a carriage return enters terminal mode. `Q9` reads/sets stream interpretation, and `PL` reports receiver synthesizer information. Future integration should version capabilities rather than assume every firmware implements the latest command set. `TB` can retrieve the radio's own decoded CW text, but reading it consumes that buffer; keep it distinct from RustSDR's decoder. Do not treat generic Kenwood mode values as universally safe: newer QMX documentation identifies mode 8 as SWR tune.

QRP Labs lists an [upper-band variant including 11 m](https://www.qrp-labs.com/qmx). That supports investigating CB reception. This session did not change bands or measure 11 m sensitivity. Confirm this unit's band configuration and required modulation: an SSB-capable receiver alone does not imply every AM/FM CB signal can be demodulated in its current firmware. Host IQ demodulation is another possible path.

## Control backend recommendation

| Option | Where it fits | Cost / limitation |
| --- | --- | --- |
| Rust QMX CAT over a transport interface | First integrated QMX source, device-specific queries, mobile transport reuse | We maintain framing, recovery, command semantics and firmware capabilities |
| Hamlib C API behind Rust FFI | Broad native radio support without implementing every protocol | Native packaging, ABI/bindings, license review, and per-radio behavior still need testing |
| Rust client for Hamlib `rigctld` | Optional desktop/server control backend and remote engine deployments | Extra process and lifecycle; carries radio control, **not** receive audio/IQ |

Recommend the first option for the QMX, keeping the backend boundary open for the other two. Hamlib explicitly lists [QMX model 2057](https://github.com/Hamlib/Hamlib/wiki/Supported-Radios). It is appropriate when broader equipment support becomes a priority. No Hamlib runtime was installed or tested in this session.

Hamlib **can run on Android**: its [current NDK instructions](https://github.com/Hamlib/Hamlib/blob/master/README.android) describe cross-compilation, minimum API 28, and a build without libusb. That does not solve phone USB access automatically. Android's [USB host API](https://developer.android.com/develop/connectivity/usb/host) uses device enumeration, user permission and interface/endpoint access. A transport bridge would be needed for ordinary app USB serial access; a serial device path from desktop should not be assumed to work. A Rust CAT codec can share its protocol logic with a JNI/USB-host transport just as it can with desktop serial or a network transport. USB audio routing is a separate mobile integration task. Android support has not been built or tested here.

## Next integration slices

1. Explicit monitor-output selection, persistent endpoint identities, and separate radio audio input/output roles. Never silently fall back to a radio output if a monitor disappears.
2. Engine-owned receive-audio source and CAT session. Keep requested/reported tuning separate; serialize commands off the render thread, coalesce tuning, and handle timeout, hotplug and front-panel changes. Publish capability-based controls without exposing raw CAT strings to UI widgets.
3. QMX audio listening, audio spectrum and CW decoder integration; test with an antenna or known receive signal. Preserve existing RTL behavior and CW practice isolation.
4. Optional IQ capture, calibrated frequency mapping, and sample-rate-independent DSP. Treat stream interpretation or tuning changes as new generations and discard stale queued samples.
5. Add a Hamlib/rigctld backend when a second radio justifies it. A remote renderer consumes engine control/state plus a separate timestamped audio/IQ stream.
6. TX remains a separate milestone: explicit arming, permitted capabilities, PTT ownership, watchdog/unkey behavior, output routing, and disconnect tests. Do not connect practice keying to CAT implicitly.

Verification for this exploration: three CAT probe tests cover framing/errors, rejecting a tuning attempt in TX, and restoring after failed tune readback; both new examples passed Clippy with warnings denied. Actual capture and frequency-control results are recorded above. There is no QMX GUI or continuous receiver session yet.
