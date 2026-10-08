# CW practice

User validation, 2026-09-28: MIDI and gamepad input work well, the 600 Hz sidetone is satisfactory, and decoding is useful with imperfect keying. The reported joined-BK failure has been addressed in the shared decoder by adding [prosign support](cw-decoding.md#prosign-follow-up-2026-09-28). Quantitative latency and off-air decoding performance have not been established by this practice test.

Open **CW Practice** in the header. This is a local sidetone and sending-practice window, available with any receiver mode or source, including the synthetic source. It does not tune, start, stop, or key a radio. Reception and practice have separate volume controls; stop or mute reception if you want only the practice tone.

```powershell
cargo run -p sdr-app --features desktop -- --gui
```

## Keying modes

| Mode | Behavior |
| --- | --- |
| Straight key | The assigned contact directly controls mark duration. WPM seeds the experimental decoder; it does not correct your timing. |
| Iambic A | A held dit or dah paddle repeats that element. Holding both alternates. Releasing both after a squeeze finishes the current element and stops. |
| Iambic B | Alternates like A, but remembers an opposite element from a squeeze and sends that additional element after release. |

For paddles, a dit is `1200 / WPM` milliseconds, a dah is three dits, and the inter-element gap is one dit. Speed is adjustable from 5 to 60 WPM. A brief opposite-paddle tap during an element is remembered. Simultaneous idle paddle closure starts with a dit; pressing the dah first starts with a dah. The B memory is set while squeezing during an element and retained through its following gap. A fresh independent tap during a gap is remembered in either mode. Character/word spacing remains under your control; automatic letter spacing, weighting adjustment, bug, and ultimatic modes are not implemented.

The A/B distinction follows the behavior described in the [K1EL K-42 manual, paddle modes](https://www.k1elsystems.com/files/k42man.pdf), page 10. Hardware keyers vary in exact squeeze-release sampling; this implementation's boundaries are pinned in `sdr-dsp/tests/keyer.rs` rather than claiming identical behavior to every commercial keyer.

Choose a 200–1200 Hz sidetone (600 Hz default), then **Arm practice**. Settings/bindings can be changed while disarmed. Volume can change while armed; setting it to zero keeps text feedback active. Re-arming clears the practice transcript. The decoder uses the generated key gate before volume, not microphone or speaker audio, so it reflects the keyer's output. It remains heuristic, especially for irregular straight-key timing; it is not an authoritative sending score.

## Keyboard, mouse, and controller bindings

Defaults are **Space** for the straight key and **Z / X** for dit/dah. Each contact has one replaceable binding; the two paddles can use different input types.

1. Disarm practice and click **Learn input** next to a contact.
2. Release existing controls, then press a keyboard key, mouse button, or game-controller button.
3. Repeat for the other paddle if needed. **Swap dit / dah** reverses the paddle bindings.
4. Arm and release all controls before starting.

Keyboard bindings use physical key positions. OS key repeat does not generate extra contacts. Controller bindings identify the current Bevy device and button; reconnecting a controller may require learning it again. Buttons and D-pad controls are supported; analog stick axes are not bindings. Mouse side buttons are available when the OS exposes them. One input cannot be assigned to two contacts. Escape is reserved for stopping/canceling and cannot be learned.

While armed, keyboard presses and bound mouse presses are consumed before normal UI shortcuts/widgets. In particular, if primary mouse is your key, use **Escape** to stop before clicking other controls. Closing the practice window, losing application focus, or disconnecting a bound controller disarms practice. Inputs must be released before re-arming. Bindings and settings are currently session-local.

## MIDI keyers and keyboards

The desktop feature includes [midir](https://docs.rs/midir/latest/midir/), an input backend for native MIDI. In the practice window, expand **MIDI input**, click **Refresh ports**, select the input, and **Connect**. Then use **Learn input** on each contact and operate the corresponding MIDI key/paddle.

- Note On with positive velocity closes a contact. Note Off or Note On with zero velocity releases it.
- A control-change value of 64–127 closes a contact; 0–63 releases it. Learn captures a value in the closed range.
- Bindings include channel plus note/controller number. The UI displays channels 1–16.
- All Notes Off, All Sound Off, and Reset All Controllers release mapped contacts on that channel; MIDI System Reset releases all mapped contacts.
- MIDI clock, pitch bend, aftertouch, SysEx, and unrelated messages do not key the practice tone. Running-status parsing is handled by the MIDI backend; our adapter accepts complete note/CC messages.

Use an existing hardware or virtual MIDI input exposed by your OS. RustSDR does not install a translator/virtual driver, open a MIDI output, or send messages back to the device. A translator must provide both press and release events. Stable backend port IDs identify connected devices; the input list is rechecked every 500 ms to detect removal. After removal, reconnect and re-arm explicitly.

Port enumeration can also be checked without a GUI:

```powershell
cargo run -p sdr-app --features midi --example practice_ports --offline
```

MIDI is optional in custom builds (`--features gui,audio,midi`); `desktop` includes it. Controller support is enabled in GUI builds through Bevy's gilrs backend. Linux desktop builds need ALSA and udev development libraries, already included in the repository CI setup. Windows was built locally; macOS/Linux hardware input has not been exercised here.

## Timing, lifecycle, and validation

`sdr-dsp::keyer` is renderer/device-independent. A dedicated `PracticeSession` worker advances it at 48 kHz, renders a phase-continuous tone with 4 ms edges, and feeds a separate `CwDecoder`. The worker consumes timestamped contact changes in a bounded queue. MIDI callbacks submit changes directly; keyboard/mouse/controller state is sampled each UI frame. Rendering therefore does not set paddle element lengths, but frame rate and OS/device delivery still affect when a human press is detected. Very short desktop-input taps between frames can be missed; MIDI avoids that frame polling path.

Sidetone uses the existing native audio output with approximately 40 ms startup buffering plus OS/device latency. This is not a hardware-latency guarantee. If lower latency is needed, measure the end-to-end delay before reducing buffering or moving generation into the callback. Practice does not feed the radio receiver audio subscription or any future TX queue.

Losing the UI heartbeat for 300 ms, falling more than 100 ms behind the sample clock, overflowing the input queue, or explicitly stopping cancels practice instead of replaying a backlog or leaving a key held. Cancellation discards paddle memory, including a pending B tail. GUI and MIDI contacts are aggregated separately so releasing one source cannot release another. Audio initialization/disconnection faults are reported; visual/text practice can continue without a speaker device.

Tests cover exact straight-key timing; A/B squeeze release; repeating and tapped paddles; a new gap tap after squeeze release; cancellation; sidetone edges; MIDI channel matching and release variants; muted decoding; source aggregation; queue overflow; and the heartbeat stop. The GUI smoke uses injected input states to exercise learning keyboard, mouse, and controller bindings, straight-key text, Mode B text, Escape, controller removal, and receiver independence. It does not substitute for pressing a physical MIDI key or controller button.

```powershell
cargo test --workspace --all-features --offline
cargo clippy --workspace --all-targets --all-features --offline -- -D warnings
cargo build -p sdr-app --features desktop --offline
target/debug/sdr-app.exe --gui --practice-smoke
```

For manual acceptance: try each input device, compare A/B with a squeeze released during a dit and during a dah, set several WPM values, verify independent paddle taps, switch away from the app while holding a contact, unplug/reconnect MIDI/controller input, and verify practice does not affect reception. Record the device/translator and any perceived sidetone latency for follow-up tuning.
