# Settings and session restoration

The GUI saves preferences automatically while you work and flushes the latest snapshot on normal exit. Launch with `--gui` to restore your workspace. Reception, scanning and CW practice start stopped/disarmed. Explicit `--rtl`, `--file` and `--qmx` launch options still start the requested source, and `--mode` selects that mode's saved preferences.

Default `settings.json` location, resolved with the `directories` crate:

| OS | Location |
| --- | --- |
| Windows | `%APPDATA%\RustSDR\config\settings.json` |
| macOS | `~/Library/Application Support/RustSDR/settings.json` |
| Linux | `$XDG_CONFIG_HOME/rustsdr/settings.json`, normally `~/.config/rustsdr/settings.json` |

Set `RUSTSDR_CONFIG_DIR` to use a different settings directory (including isolated test profiles). Existing radio/UI smoke tests disable preference loading/saving. CLI-only reception and probes do not write GUI preferences.

## What is restored

- Interface, VFO, RTL capture center, IQ file path/center, synthetic tone, volume/mute, spectrum zoom and display limits.
- Signed RTL converter offset (`devices.rtl.converter_offset_hz`), restored before checking the saved RF tuning. Missing offset settings default to zero.
- Each software receiver mode's bandwidth, deviation, deemphasis, AGC, squelch and tuning step independently.
- QMX port/input/output labels, monitor boost and step. Connecting reads the actual radio tuning/mode; saved state never replays a CAT command.
- RDS enablement and CW decoder speed, timing, spacing and threshold.
- Window size, controls/diagnostics/Signal Data/practice/editor visibility.
- Memory CSV path, group/row selection, VFO/Memory choice, filter, import/export path and scan parameters/range. The CSV loads asynchronously; selection is clamped if the file changed. Missing files leave Memory mode off. **CSV edits still require Save**; preferences do not replace the memory library file.
- CW practice keyer mode, WPM, pitch, volume, keyboard/mouse/MIDI/controller bindings and selected MIDI port ID. Refresh MIDI ports and Connect to reopen the device. Controllers are matched by name and USB vendor/product IDs; two identical devices require learning again. No saved input is armed automatically.

Waterfall history, decoder text, file playback position, active scanning, practice key-down state, hardware handles and QMX per-band session history are not saved. Floating panel geometry, monitor position and maximized state are not currently restored. Audio endpoint labels may need reselection after OS/device naming changes.

## File ownership and recovery

The background writer coalesces edits for 500 ms, with a two-second maximum during continuous changes, then writes/syncs a sibling temporary file and atomically replaces the destination. Normal exit drains the writer. Abrupt termination can lose the most recent edits but does not intentionally replace the file with partial JSON.

An OS file lock permits one writer per settings directory. Additional instances load a snapshot and show a read-only warning. Lock files can remain on disk; the OS releases ownership on process exit. Failed writes are shown in the GUI and retried on subsequent edits or shutdown.

A malformed document, unknown document version or unreadable settings file is left untouched, with a visible warning and defaults for the session. Close the app and rename `settings.json` to keep a backup and start fresh. An invalid individual section falls back independently; a future section version is preserved unchanged. Unknown fields and unrelated sections survive normal saves. Files larger than 1 MiB are rejected.

## Adding a mode or panel

`sdr-app::preferences` owns the versioned JSON envelope, section access and file writer. It has no Bevy, DSP execution or device lifecycle dependency. `gui_settings.rs` composes panel snapshots; the memory and practice panels own their own serializable preference types.

Each stable section key has its own version and data:

```json
{
  "version": 1,
  "sections": {
    "decoders.rds": { "version": 1, "data": true },
    "decoders.future_mode": { "version": 1, "data": { "enabled": false } }
  }
}
```

To add a module, define a serde preference type, a stable namespaced key, validation, and restore/snapshot methods. Use `Document::get`/`put`; do not serialize runtime state or engine resources. Additive fields should use serde defaults so older sections still load. For incompatible changes, add an explicit section migration before enabling writes to its new version; the initial implementation supports version 1 and deliberately refuses to rewrite unknown versions. Add a round-trip test and invalid-data test for the new module.

Receiver sections are `modes.wfm`, `modes.stereo`, `modes.am`, `modes.nfm`, `modes.usb`, `modes.lsb`, and `modes.cw`; decoder sections are separate so a decoder can evolve without changing its receiver's schema.

## Verification

Storage tests exercise replacement of existing settings, coalesced shutdown flushing, live autosave, writer locking, unknown-field preservation and corrupt/future file protection. GUI tests cover independent mode/panel round-trips, section validation, QMX/software receiver separation and asynchronous CSV selection restoration.

The dedicated settings smoke test requires an isolated directory and opens two short, stopped GUI sessions:

```powershell
$env:RUSTSDR_CONFIG_DIR = "$PWD/target/settings-smoke"
cargo run -p sdr-app --features desktop -- --gui --settings-smoke-write
cargo run -p sdr-app --features desktop -- --gui --settings-smoke-read
Remove-Item Env:RUSTSDR_CONFIG_DIR
```

The second launch asserts restored mode, filter width, tuning step, audio preferences, display limits/zoom, decoder speed, scan hold and window dimensions. It does not open a radio or audio stream.
