# FT8 / FT4 reception

Choose **FT8** or **FT4** in the header's decoder selector. For RTL-SDR or IQ-file reception this applies USB with a 300–5000 Hz receive passband, disables AGC and squelch, and fits a 6 kHz spectrum span. Tune the VFO to the station's **dial frequency**, not to an individual audio tone. With the existing upconverter configuration, 7.074 MHz is a useful FT8 starting point. The converter offset remains independent of the decoder.

The selector is separate from the radio's demodulation mode. On QMX, select **USB or Digi USB** using the radio mode control; selecting a decoder does not issue CAT commands. Its existing 0–4 kHz audio display is already a narrow view. Actual decoded bandwidth is limited by the radio's filter. Switching to an incompatible demodulation mode suspends live decoding; the selected decoder and message history remain available.

Keep the computer's clock synchronized. FT8 collects 15-second UTC slots; FT4 collects 7.5-second slots. The first partial slot is discarded, so initial results can take almost two slots. Retuning or losing samples discards the interrupted slot. Muting or changing monitor volume does not affect decoding. The first implementation searches 300–5000 Hz audio; it does not decode signals outside the receiver's passband.

**Digital text** opens a compact, single-line table with UTC, dB, DT, audio Hz,
mode, dial MHz and selectable message text. Newest slots appear first; messages
within a slot retain ascending audio-frequency order. New results return the table
to the top. Up to 1000 messages are retained, with the oldest removed first.
Narrow views omit mode/dial columns (available in the row tooltip), and horizontal
scrolling preserves complete messages. On windows narrower than 760 logical pixels,
**Waterfall / Text** tabs share the main area. Compact layout is desktop-tested,
not a claim that mobile builds are available.

**Waterfall callsigns** enables vertical sender labels at decoded frequencies.
Each label anchors to the historical row nearest the decoded waveform's end,
including DT, and moves with that row until it ages out. RF coordinates keep labels
aligned through RTL/file zoom and pan; QMX uses audio coordinates. Rendering FPS
and pauses do not move labels independently of the waterfall. Stronger labels win
when text overlaps; small frequency ticks remain. The checkbox is saved alongside
the protocol/window preferences in `decoders.ft`; old settings enable labels by
default. Message history and labels are not persisted.

Sender identity comes from structured protocol fields, not guessing a word in
free text. Unresolved hashes, ambiguous message types, stale captures, missing
history and WAV imports are not annotated. Alignment uses approximate host
timestamps (or the IQ-file sample clock), with a 0.75 s nearest-row tolerance;
USB/DSP latency can introduce some visual timing error. Decodes that arrive after
their signal has scrolled out of history cannot be labelled. See
[fine RF calibration](frequency-converters.md#fine-calibration) for converter errors.

## Static sample verification

The Digital text window's **Decode WAV** action accepts one mono PCM16 12000 Hz slot. Short reference slots are zero padded; oversized or incompatible files are rejected. Import runs on the same background decoder as live reception, and works with reception stopped. Imported results are labelled WAV and are not assigned the current radio's frequency.

From the repository root:

```powershell
cargo run -p sdr-app --example decode_ft -- ft8 crates/sdr-app/tests/fixtures/digital/ft8.wav
cargo run -p sdr-app --example decode_ft -- ft4 crates/sdr-app/tests/fixtures/digital/ft4.wav
cargo test -p sdr-app --test digital
```

Fixture provenance, hashes and license are in [the fixture README](../crates/sdr-app/tests/fixtures/digital/README.md). Tests assert known messages and frequency tolerances, silence rejection, input validation, slot alignment, discontinuities and the audio resampler. Missing fixtures fail the tests; they are never silently skipped. Decode counts are not claims of WSJT-X sensitivity parity.

## Library selection (2026-09-30)

| Candidate | Fit | Decision |
| --- | --- | --- |
| [mfsk-core](https://github.com/jl1nie/mfsk-core), pinned 0.11.0 | Native Rust FT8/FT4 API, encoding primitives and additional protocol families; Rust 1.93 matches this workspace. GPL-3.0-or-later. | Selected, with only std, FT8, FT4 and rustfft features. |
| [ft8-engine / wsjtr](https://github.com/bodiya/wsjtr), 0.5.0 | Rust implementation with stateful FT8/FT4 sessions; engine crate presents a C FFI surface. GPL-3.0-only. | Viable alternative; less direct fit for our Rust service and broader mode roadmap. |
| [ft8_lib](https://github.com/kgoba/ft8_lib) | MIT-licensed C routines for small FT8/FT4 receivers and encoders. | Useful alternative if permissive licensing becomes essential; requires C integration and additional packaging. |

The registry source, rather than cached `latest` documentation, was checked for the pinned API and license. The GPL dependency matters when distributing a linked application: plan distribution and corresponding source accordingly; this change does not assign a blanket license to the project's original code. Keep the adapter narrow so the decoder can be replaced if project licensing requirements change.

The broad library roadmap is useful, but only FT8 and FT4 are exposed here. [JS8Call](https://github.com/js8call/js8call) uses a distinct message protocol; JS8 is not obtained by relabelling FT8 results.

## Service boundary and remaining work

`sdr_app::digital` owns the library adapter, WAV reader, bounded worker queues and slot assembler. It depends on no GUI or audio device API. The existing pre-monitor audio subscription supplies sample sequence, capture/revision identity, discontinuity and an approximate host publication timestamp. A separate collector filters 48 kHz audio to 12 kHz continuously. A separate decoder thread processes complete slots, with one pending job, a 60-candidate cap and a cooperative four-second budget. Rayon is disabled to avoid taking over all cores. Slow decoding drops pending work explicitly rather than accumulating latency; counters are shown in the panel.

Live slot alignment anchors the sample clock to host UTC and resets after a clock jump exceeding 0.5 seconds. Publication time is approximate: USB buffering, receiver filter delay and device-clock drift affect DT. IQ files use sample zero as slot zero, so arbitrary recordings need correctly aligned starts. WAV import is the reproducible offline path. Audio gaps, retuning and source changes reset assembly. Results carry their tuning identity; obsolete live results are rejected. The audio subscription currently has one consumer, owned by this service in the GUI.

Follow-ups:

- Validate live RTL/upconverter and QMX reception, crowded-band sensitivity, CPU and DT against WSJT-X; no live-radio sensitivity claim yet.
- Callsign hash context across slots, deeper subtraction/search controls, UTC capture timestamps closer to the device, arbitrary-rate/long recording import and exported decode logs.
- A separate JS8 backend with framing, speed variants and conversation reassembly; evaluate WSPR, FST4 and other mfsk-core protocols behind the same service contract.
- Encoding should produce timestamped audio through a separate TX service. Scheduling, station identity, band rules, CAT/PTT arbitration, abort and watchdog behavior must be implemented before connecting encoder output to hardware. This implementation opens no transmit stream and sends no PTT commands.
