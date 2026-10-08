# Memories and scanning

Implemented 2026-09-25; table editor updated 2026-09-25. The header has a **VFO / Memory** toggle, an independent **Scan** toggle, and **Memories…** to open the editor. Reception, scanning and signal detection run on the radio worker, independently of UI frame rate.

## Store and recall

1. Select RTL-SDR or an IQ recording and tune/configure the receiver.
2. Open **Memories…**. Scan settings are at the top, followed by CSV path/load/save controls, group selection and the editable table. Enter a CSV path and **Load**, or choose **New CSV** and enter a destination path. Save unsaved edits before replacing the library.
3. **Add row** creates a named channel using the current frequency/mode as starting values. **Store current** also preserves the full receiver configuration. Edit name, frequency (shown in MHz), mode, bandwidth and scan inclusion directly in cells. Click a numeric cell to type; Tab/Shift+Tab moves between controls. **More receiver columns** exposes squelch, AGC, deviation and de-emphasis. Changing a row's mode resets its receiver fields to that mode's defaults. Editing does not retune the radio.
4. Select a row by its number or a cell. **Update from VFO** replaces its receiver settings while keeping its name and scan inclusion; **Delete row** removes it. Use **Save** to persist every group. The unsaved indicator remains until successful saving; invalid entries produce an error and leave the previous file intact. Edits are not saved automatically on exit.
5. Toggle **VFO** to **Memory** in the main header to recall the selected entry. **Tune selected** or double-clicking a row number also recalls it. The wheel over either plot and Left/Right arrows move through the group's original order and wrap. Text cells retain arrow-key editing. Filtering affects the table, not navigation or scan order. Unchecked rows remain available for manual recall but are skipped by scans.

In VFO mode the existing wheel tuning behavior is retained. Loading a library does not start reception. Selecting a group in Memory mode recalls its first entry; importing while already in Memory mode recalls the newly selected group's first entry after loading finishes. Manual recall preserves the current reception start/stop state.

Large libraries use a virtualized row list and cached filtering. File parsing/writing runs on a separate I/O worker. Limits are 1,024 groups, 100,000 total entries and 64 MiB per CSV file. Names are limited to 256 UTF-8 bytes. All entries are validated before a library is accepted; a failed load leaves existing memories intact. Saves validate, write and sync a sibling temporary file, then replace the destination, rather than deleting the previous file first.

## CSV files

A file can be as simple as:

```csv
name,frequency_hz,mode
106.7 MHz,106700000,stereo
89.9 MHz,89900000,stereo
VHF channel,146520000,nfm
```

Use [broadcast-fm.csv](examples/broadcast-fm.csv) as a starting point. Frequencies are integer **Hz**, not MHz. Mode values are `wfm` (mono), `stereo`, `am`, `nfm`, `usb`, `lsb`, and `cw`, case-insensitive. Column names are case-sensitive. Standard CSV quoting supports commas, quotes and newlines in names. Validation errors identify the offending CSV line where applicable.

| Column | Meaning when omitted or blank |
| --- | --- |
| `name` | Required nonempty name |
| `frequency_hz` | Required integer frequency |
| `mode` | Required mode identifier |
| `group` | Use the CSV filename without its extension |
| `bandwidth_hz` | Mode default |
| `deviation_hz` | Mode default |
| `deemphasis_us` | Mode default |
| `agc` | Mode default; explicit values are `true` / `false` |
| `squelch_db` | Disable the mode's audio squelch |
| `scan_enabled` | `true`; set `false` to exclude from scanning |
| `schema_version` | Version 1; exported files include it |

A `group` column allows multiple groups in one file. Group order and entry order follow their first appearance in the file. **Import groups** appends groups, renaming duplicate group names with a numeric suffix. **Load** replaces the library and is disabled while there are unsaved changes. **Export group** writes just the selected group; **Save** writes every group. Import/export controls are above the table. Exports include all receiver settings, so a save/reload preserves more than frequency and mode. A row with group metadata and empty name/frequency/mode preserves an empty group.

The default library is loaded at startup when present:

- Windows: `%APPDATA%\RustSDR\memories.csv`
- macOS: `~/Library/Application Support/RustSDR/memories.csv`
- Linux: `$XDG_CONFIG_HOME/rustsdr/memories.csv`, or `~/.config/rustsdr/memories.csv`

`RUSTSDR_MEMORY_FILE` overrides the default. The UI also accepts explicit library and import/export paths. Paths and selected groups are session-local preferences; scan parameters and VFO ranges are also session-local. The CSV is the persistent memory data.

## Memory and VFO scans

In **Memory mode**, Start scan visits the group's checked memories, restoring each receiver configuration. It starts at the selected entry if included, otherwise the first included entry, and wraps through the group. In **VFO mode**, configure Start MHz, Stop MHz and Step Hz. The scan uses the current receiver configuration and visits `start + n × step` without exceeding stop; stop is included only when aligned to the step. Range plans use arithmetic rather than allocating an entry for every frequency. If the current VFO lies in the range, starting/resuming uses the aligned position at or below that frequency; otherwise it starts at the range start.

Both modes use the same policy:

1. Tune, preserving the current capture and audio device where possible.
2. Wait for the configurable **settle** period, starting after any hardware retune completes. IQ discontinuities restart settling and reset the receiver.
3. Observe filtered channel power for the **listen** period. A signal at or above the scan threshold starts the pause immediately.
4. Pause for the configured duration, then continue even if the signal remains. Further signal detections do not extend this fixed pause.

Defaults: 150 ms settle, 250 ms listen, 5 s pause, −55 dBFS threshold. Adjust the threshold using the displayed channel-power reading on a quiet channel. Power is measured within the selected receiver's filter, before demodulation, AGC, speaker volume and mute; it is not audio loudness or calibrated antenna power. Different bandwidths and RF gain/noise levels change the useful threshold. The stored mode's audio squelch remains a separate setting and can still suppress audio during a scan pause.

The monitor is silent while scanning/settling and plays during signal pauses; the receiver audio subscription remains independent of that monitor gate. Muting the speaker does not stop signal detection. Scanning speed includes hardware tuning latency; short dwell settings cannot eliminate that cost.

Toggle **Scan** off in the main header, or use **Stop scan · stay here** in the editor, including during a signal pause. This holds the engine's actual tuned frequency/configuration without resetting the receiver or stopping reception. The last scan position remains available for resuming. Manual tuning/mode/source changes or Stop reception cancel scanning. EOF, source faults and shutdown stop scanning too. Toggle Scan on to resume; on a recording that reached EOF, it starts playback again. Editing is disabled while scanning, since the active plan holds a snapshot of the group.

Normal RTL range remains 28.8–1700 MHz; direct sampling and external-converter translation are not added here. Stored HF memories are permitted, but recall/scan must fit the selected source. File scans must stay within ±1 MHz of the recorded center. Unsupported enabled memories reject scan startup with an error; exclude those entries rather than silently tuning to a different frequency. These are receive-only operations.

## Implementation and verification

- `memories.rs`: CSV schema, validation, ordered groups and atomic file replacement.
- `scanning.rs`: deterministic clock-driven scan state machine and immutable scan plans.
- `engine/runtime.rs`: applies scan targets, reads channel power and owns scan lifetime. `Settings.scan` starts a plan; `Command::StopScan { revision }` holds the current channel. Automatic visits retain the caller's command revision; scan visit/index and audio discontinuity metadata distinguish subsequent tuning.
- `gui_memories.rs`: editor, asynchronous file tasks, recall/navigation and scan controls.
- `Receiver::channel_power_dbfs()`: filtered IQ power for all seven implemented listening modes.

Behavioral checks cover 10,000-entry CSV round trips, quoted/minimal CSV and physical error lines, invalid-save preservation, scan settling/fixed pauses/range wrap, invalid plans, background scanning without UI polling, stop-and-hold, and per-mode channel power. Existing DSP, playback, tuning and engine tests remain in the suite.

The GUI memory smoke uses a generated recording and a separate CSV to exercise actual arrow/wheel input, recall/wrap, memory pause/stop/restart and VFO scanning. A live RTL probe scanned 106.7/89.9 MHz and stopped on the actual channel with zero reported IQ/audio losses in that short run. This does not replace a longer scan stability/listening test.

```powershell
cargo test --workspace --all-features
# Explicit hardware check, with the dongle available:
cargo run -p sdr-app --features audio --example scan_probe
```

For the GUI smoke, `--memory-smoke` requires a sufficiently long recording centered at 106.95 MHz, and a CSV set through `RUSTSDR_MEMORY_FILE` with these entries in order: 106.7 MHz stereo, 106.8 MHz AM, 106.75 MHz USB. Set `scan_enabled` to `true,false,true` respectively; the smoke verifies that manual recall includes the AM entry while scanning skips it. The local validation fixtures/logs are under `target/memory-scan/` and its screenshot is `target/memories-scan-smoke.png`. Normal operation does not create test fixtures or modify the user's library automatically.
