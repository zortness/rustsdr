# M03 local candidate: 2026-10-20

This is intermediate evidence for the [M03 acceptance checklist](../milestone-03.md), not a completion record. The build used the M03 working tree based on `c4dce848196793fe7d7fa7d008b657f145b1b7d1` before these changes were committed.

## Decoded audio recording

The recorder saves the receiver's 48 kHz stereo monitor signal after the scan audio gate and before speaker gain/mute to 16-bit PCM WAV. It writes a new `.wav.part` file and publishes the final `.wav` only after finalization. An existing destination, queue loss, and format/write failures produce errors; the partial file is removed on failure. The GUI keeps the recorder active through retunes and mode changes; source changes and reception stop finish it. Those transitions still need an interactive recording check.

`cargo test -p sdr-app --test recording --offline` passed the IQ-file-to-WAV integration test. On this host, the test also invoked the installed `ffprobe`, which accepted `pcm_s16le`, 48,000 Hz, two channels. The recorder unit tests passed for a short playable WAV, non-overwrite, and queue overflow cleanup. GUI interaction, mode transitions during recording, disk failure, and listening to a live saved file still need acceptance checks.

## Windows package candidate

`./scripts/package-windows.ps1` built the release desktop app, then staged the executable, `rtlsdr.dll`, `pthreadVC2.dll`, `msvcr100.dll`, launch script, and instructions in a Windows x64 ZIP. `Launch-RustSDR.cmd` selects Direct3D 12. The final local archive is `target/dist/rustsdr-0.1.0-windows-x64-20261020-214005.zip` (16,073,076 bytes; SHA-256 `F222C4E77240C641569DE40C475BA8E32080FE0AE531D5B3C0152E168E3DA082`). Native runtime provenance is in [third-party setup](../../third-party/README.md).

On Windows 11 Pro build 26200 with AMD Radeon 8060S, the staged executable returned help from outside the build tree. Direct3D 12 GUI smoke tests passed for synthetic and generated IQ-file playback, saving screenshots in the staged folder. A Direct3D 12 live smoke using `--rtl --rtl-backend librtlsdr` loaded the adjacent native library, opened the connected R820T, received spectrum rows, exercised stop/start and tuning, and exited successfully. The first synthetic attempt missed a scripted stop click and timed out at stage 2; an unchanged retry completed. The ZIP with the launch script was restaged after these executable tests; its payload executable and DLLs are the same build. The final ZIP was extracted under `target/dist/m03-verify-extracted`, where its executable returned `--help` successfully and all six expected files were present.

`cargo test --workspace --all-features --offline` exited 0 (one existing SSTV reference-fixture test is ignored). `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` and `cargo fmt --all -- --check` exited 0. Rust reported incremental-cache hard-link fallbacks, not code diagnostics.

This was a local extraction on the development host. A fresh user environment, manual audible reception, record-button workflow, missing-device recovery, and redistribution terms remain to be checked before the Windows distribution item can be accepted.
