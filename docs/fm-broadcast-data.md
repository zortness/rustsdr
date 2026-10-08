# FM broadcast data: RDS / RBDS

Implemented 2026-09-25. This first broadcast-data decoder is Rust code in `sdr-dsp`; it needs no extra DLL, external decoder or network service.

## Use it

```powershell
cargo run -p sdr-app --release --features desktop -- --gui --rtl --mode stereo
```

Tune a strong FM broadcast station. Its decoded station name and RadioText appear above the spectrum without opening a window. Text that exceeds the available width scrolls with a pause at either end; short text stays still, and hovering reveals the complete message. New text starts at the beginning, and stale text clears on tuning or signal loss.

Click **Signal Data** for the full details window. The button is currently available only in WFM mono/stereo with RTL-SDR or file playback; leaving those modes closes the window. **Decode RDS / RBDS** is enabled by default and continues while the speakers are muted. Closing the panel leaves decoding enabled; unchecking the box disables its DSP work and the inline text. The preference is session-local and is not part of a memory's voice configuration.

The panel shows acquisition/receiving/signal-loss status, checked group count, Programme Service station name (PS), RadioText (RT), hexadecimal Programme Identification (PI), numeric Programme Type (PTY), and traffic programme/announcement flags. PTY is deliberately numeric in this increment because RDS and North American RBDS assign different meanings to several codes. No automatic traffic retuning or unmuting is performed. Station name and RadioText remain separate; title/artist fields require RT+ and are not inferred from arbitrary text.

Allow several seconds for text segments to arrive. Some stations send no RDS or omit particular fields. Clear audio does not guarantee decodable data. Retuning, receiver mode/filter changes, source discontinuities, or disabling the decoder discard its old text. Three seconds of sample time without a valid group clears metadata and shows reacquisition. Stopped/ended reception is labeled as such rather than showing old data as live. Every scan target gets fresh decoder state; short scan pauses may not allow a complete name/text cycle.

Live check with the connected RTL-SDR V3: **89.9 MHz** decoded PI `1718`, rotating PS text, and `KCRW Music, News, Culture & NPR for Southern California`. A 20-second release/headless run received 207 checked groups with zero reported IQ drops. A separate 20-second probe at 106.7 MHz received no valid groups; that does not establish whether the station transmits RDS. The probe was muted and built without speaker audio, so it is not an audio-underrun measurement or a long stability test.

## Supported scope and limits

- Groups 0A/0B assemble eight-character PS; groups 2A/2B assemble 64/32-character RadioText. PI, PTY and TP come from checked groups; TA comes from group 0. Version B checks the repeated PI in C'.
- All four 26-bit blocks must pass their CRC/offset checks in sequence. Corrupted groups are dropped; bit-error correction is not implemented. Count is accepted groups, not a BER estimate.
- Text is published after all segments through the terminator or full field length arrive. RT A/B or format changes clear the previous message. Changed segments restart assembly to reduce mixed old/new messages, but dynamic PS is not an atomic over-the-air update.
- Basic Latin text and a small set of RDS punctuation/control mappings are supported. Extended alphabet characters display a replacement character. Full international character mapping, regional PTY labels/call-sign interpretation, RT+, clock time, alternate frequencies, EON, ODA and RDS2 remain follow-ups.
- The first demodulator uses a nominal 57 kHz mixer, channel filter, eight bit-timing hypotheses and complex differential detection at 1187.5 bits/s. It does not require the stereo pilot. Tests exercise small carrier/clock errors; strong-signal live decoding is verified. Large frequency error, severe multipath and weak-signal performance need a recorded corpus and potentially adaptive carrier/timing recovery.
- HD Radio/NRSC-5 and SCA are separate services and are not decoded by this feature.

Definitions and further background: [RDS Forum field glossary](https://www.rds.org.uk/pages/glossary-of-terms.html). The local SDR++ RDS implementation was inspected as a protocol reference; its C++ decoder was not linked or ported.

## Architecture

`Receiver::process_with_multiplex` optionally appends the 240 kS/s WFM discriminator output before audio low-pass/de-emphasis. It preserves the exact speaker samples produced by `process`. The engine reuses a bounded block buffer and runs `RdsDecoder` on its existing radio worker. DSP state consists of fixed filter/timing/framing/text storage; there is no unbounded packet history or job queue. Small bounded strings are assembled on checked groups, never on the audio callback or UI thread.

`Settings.rds_enabled` controls this branch without resetting the audio receiver, capture, waterfall, or scan pause. New scan plans are distinguished by their shared plan identity; a decoder-only command retains the existing plan's position/deadline. Receiver resets and IQ gaps recreate the decoder. `Update.rds` is a coalesced immutable typed snapshot attached to the enclosing update's station/revision/capture identity. The UI checks that identity before displaying text. There is no asynchronous decoder task whose results can arrive for an old station. Future expensive data decoders should use bounded workers with explicit sample/generation metadata instead of extending this lightweight synchronous branch indefinitely.

The `perfetto` feature adds the block-level `sdr.dsp.rds` span inside receiver processing. Closing the data window does not remove that work; disable its checkbox for an on/off CPU comparison.

## Reproduce verification

```powershell
cargo test -p sdr-dsp --test rds
cargo test -p sdr-app --no-default-features --test rds
cargo run -p sdr-app --example rds_fixture
cargo run -p sdr-app --features desktop -- --gui --file target/rds-demo.cf32 --center-hz 106950000 --sample-rate 2400000 --mode stereo --rds-smoke
cargo run -p sdr-app --release --example rds_probe -- 89900000 20
```

The fixture generator creates a receive-only cf32 recording; it never opens a transmitter. The GUI smoke clicks the header button and decoder checkbox, checks PS/RT, disables/re-enables and reacquires without reopening capture, and writes `target/rds-smoke.png`. The hardware probe is an explicit example, not an automated test. Tests also cover CRC rejection/noise, version B, A/B changes, PI changes, loss of synchronization, mono/stereo RF filtering, speaker-sample equivalence, mute independence, retune clearing, audio epoch continuity, and preserving a scan pause.
