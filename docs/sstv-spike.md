# SSTV service exploration

Updated 2026-10-13. The agreed [proposal](sstv-proposal.md) now has a working,
feature-gated codec/service prototype. The production engine and GUI do not
start this service yet. Stage 1's full interoperability gate remains open.

## Findings and implementation

Pinned **sstv 0.3.1**, MIT, with only its `std` feature. Its registry source
identifies upstream commit `9761c4c61a6353d7ae2173e1579e434e6c1b1ce3`.
It declares Rust 1.88 and compiled here with Rust 1.93.1. Only the SSTV crate
was added to the lockfile; its sole enabled dependency, libm, was already present.
The optional application feature is `sstv`; normal desktop builds do not enable it.

The adapter exposes Robot36, Martin M1, Scottie S1 and PD120; exact-size packed
RGB encodes to a 48 kHz PCM16 iterator. Decoding supports Auto or a forced mode,
progressive RGB snapshots, partial frames and back-to-back images. Third-party
mode and image types stay inside the adapter.

The prototype worker consumes one contiguous PCM session through eight bounded
blocks, each at most 4096 samples. Nonblocking submission returns Full for retry;
skipping samples becomes an explicit Gap. An empty queue waits instead of
signalling EOF. Cancellation is checked during sample consumption, waits poll
every 10 ms, and two seconds without input ends the session with Timeout.
The worker joins on drop. Decoding runs off the caller thread.

Image previews coalesce at approximately 10 Hz, with first/final updates always
published. Four finished/interrupted images remain available even if nobody
polls. At the largest enabled resolution, history RGB storage is at most
3,809,280 bytes; current/working snapshots and codec buffers are additional.
The PCM queue is at most 65,536 bytes of sample payload. Retained external Arc
snapshots are the caller's responsibility. Whole-process memory and sustained
realtime CPU budgets have not been measured.

This deliberately tests the risky boundary without premature engine coupling.
A worker has session-local frame IDs and zero-based sample offsets. It does not
yet carry RF/capture metadata, convert stereo float audio, subscribe to the
engine, or automatically resume after a gap. The synchronous snapshot accessor
uses a short mutex; production GUI integration should expose nonblocking polling.

## Evidence

Nine targeted tests pass when independent fixtures are included. They cover
four-mode Auto detection/image reconstruction, forced mode on truncated input,
silence rejection, arbitrary 997-sample chunks, temporary starvation, two frames,
sample gaps, active/idle cancellation, timeout, input limits and bounded history.

Independent PySSTV 0.5.7 input produced these complete images. Errors are mean
absolute RGB-channel errors on a 0–255 scale against the original gradient:

| Mode | Resolution | RGB error |
| --- | --- | --- |
| Robot36 | 320 x 240 | 1.615 |
| Martin M1 | 320 x 256 | 0.311 |
| Scottie S1 | 320 x 256 | 2.028 |
| PD120 | 640 x 496 | 1.295 |

The headless WAV-import probe decoded the Robot36 fixture in about 0.052 seconds
on this development machine. That is a single optimized-codec run, not an
end-to-end realtime performance guarantee.

The reverse check is **not a clean pass**. The independent colaclanth/sstv
decoder at `3e556eee8ad4c4425799cb652bac26ee58f8e113` identifies and renders
our Robot36, Martin M1 and Scottie S1 WAVs, but misses the chosen absolute
quality threshold of 12. Decoding PySSTV through the same reference reveals
similar errors:

| Mode | Our WAV vs original | PySSTV control vs original | Our decode vs control decode |
| --- | --- | --- | --- |
| Robot36 | 14.295 | 14.441 | 1.228 |
| Martin M1 | 14.454 | 14.464 | 0.819 |
| Scottie S1 | 14.437 | 15.276 | 2.072 |

Robot36 output was visually inspected and has visible color artifacts. The
reference also reports reaching EOF for Robot36 and Scottie, including the
Robot36 control. PD120 fails with `SSTV mode is unsupported (VIS: 95)`.
The close controls suggest reference-decoder limitations, but do not establish
our encoder's correctness. The diagnostic retains its original failure threshold.
Use another independent receiver supporting all four modes before approving
encoder interoperability. No off-air recordings have been tested in this spike.

All-feature workspace tests, strict all-target/all-feature Clippy, formatting,
the desktop-plus-SSTV build and hardware-free SSTV tests passed. The optional
generated-fixture test is ignored in the normal suite and was explicitly run
separately. No GUI or RF behavior changed; no hardware was opened.

## Reproduce

From the repository root, run the ordinary hardware-free tests:

```powershell
cargo test -p sdr-app --no-default-features --features sstv --test sstv --offline
```

Generate independent input, then explicitly run all nine tests:

```powershell
python -m pip install --target target/sstv-python PySSTV==0.5.7 Pillow==11.3.0
python crates/sdr-app/tests/fixtures/sstv/generate.py
cargo test -p sdr-app --no-default-features --features sstv --test sstv --offline -- --include-ignored --nocapture
```

The probe encodes original gradient artwork or decodes a WAV to a portable RGB
PPM file. It refuses to overwrite outputs. Its intentionally narrow import is
mono PCM16 at 48 kHz, at most ten minutes; full application import/resampling and
PNG/JPEG handling belong to the next implementation stage.

```powershell
cargo run -p sdr-app --no-default-features --features sstv --offline --example sstv_probe -- encode Robot36 target/sstv-spike/Robot36-rust.wav
cargo run -p sdr-app --no-default-features --features sstv --offline --example sstv_probe -- decode target/sstv-spike/Robot36-pysstv.wav target/sstv-spike/Robot36-decoded.ppm
```

Reproduce the reverse diagnostic (currently expected to fail its quality gate):

```powershell
python -m pip install --target target/sstv-reference --no-deps -r crates/sdr-app/tests/fixtures/sstv/requirements-reference.txt
python crates/sdr-app/tests/fixtures/sstv/check_encoder.py
```

Generated media, decoder logs, manifests and comparison JSON stay under target.
The diagnostic reuses existing Rust WAVs; use a fresh output directory after
encoder changes. The script currently targets the Windows probe executable.

## Next implementation boundary

Proceed with this codec as the receive candidate, while keeping encoding
experimental until the independent-receiver gate closes. Add an engine-owned
supervisor that converts subscribed 48 kHz float stereo frames to PCM, carries
revision/capture/dial identity, and restarts this worker on tuning/discontinuity.
Live capture must never retry a full queue on the radio thread: drop/reset the
decode session explicitly. Keep file-import backpressure separate.

Before making that service user-facing, add IQ-demodulation-to-image fixtures,
recorded-audio/noise/offset/drift checks, headerless acquisition tests and
nonblocking authoritative status. Retain the proposal's Receive/Compose UI and
IC-705 send work as subsequent stages.
