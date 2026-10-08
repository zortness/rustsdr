# Receiving with QMX

The desktop app now supports QMX CAT tuning and the radio's demodulated 48 kHz USB receive audio. Windows is the validated platform. Build with `--features desktop`, or `--features gui,qmx` without RTL/MIDI.

## Start reception

Choose **QMX USB** from Interface. While stopped, enter the CAT port, refresh audio devices, and select the radio input and a speaker/headphone **Monitor output**. Then click **Start reception**. The initial connection reads the current radio frequency, receive VFO, mode, firmware and band table; it does not retune the radio.

The connected unit was tested with:

```powershell
cargo run -p sdr-app --features desktop -- --gui --qmx COM18 --radio-input "Digital Audio Interface (2- QMX Transceiver)" --monitor-output "Speakers (BEHRINGER UMC202HD 192k)"
```

Use the names reported by Refresh or `radio_audio_probe` with the displayed name and driver joined as `Name (Driver)`. Port numbers and endpoint names can change. Duplicate endpoint labels are rejected instead of choosing an arbitrary device. Endpoint labels, CAT port, monitor boost and tuning step are saved between launches. Restoring a session leaves reception stopped; Start reception adopts the current radio state. Stable endpoint IDs remain a follow-up. See [settings](settings.md).

The radio must already be receiving in normal USB audio mode (`Q90`). Raw-IQ mode is rejected rather than silently changed. USB, LSB, CW, CW-R, Digi USB and Digi LSB are supported; select one on the radio before connecting if it is in another mode. The radio performs filtering and demodulation. Existing software receiver filter/AGC controls and decoder panels are therefore hidden for this source.

## Tuning and bands

The top bar displays the active receive VFO (A or B), band, mode and dial frequency. Edit the frequency or mode there. Left/Right keys, the step buttons, and the mouse wheel over either plot tune by the selected step. Keyboard shortcuts yield to text editors. Stepping stays within the current configured receive range; use the band selector to move deliberately between bands. Numeric tuning may select any frequency in the union of the radio's configured ranges; gaps are rejected.

Band metadata comes from the radio's CAT menu interface, including band names, minimum, center and maximum frequencies. The connected high-band unit reports 20, 17, 15, 12, 11 and 10 m. Its configured receive ranges are broader than amateur allocations; they are hardware/configuration limits, not a table of transmit permissions.

On a band's first visit, newer firmware uses `BN` when available and reads back the resulting tuning and mode. Firmware 1_03_002 rejects `BN`, so the adapter tunes the band's configured center and reads back the mode the radio retains/selects. Revisited bands restore the last frequency and mode remembered during this receive session. Overlapping receive ranges on older firmware are identified using the nearest configured band center. Stop/restart creates a fresh session and adopts the radio again.

CAT polls also follow changes made using the radio's front-panel knobs. The app controls the active receive VFO; it does not change A/B/split selection. There is no transmit, keying, SWR tune, configuration-write or firmware-update API in this adapter.

## Audio and displays

Monitor output is explicitly selected and never falls back to the Windows default. Known QMX output endpoints are excluded and rejected. The common default-output path used by RTL reception and CW practice also rejects a QMX endpoint, preventing those features from implicitly routing sound into it.

Volume and mute affect only the monitor. **Audio boost dB** adds up to 60 dB of software monitor gain, with output clamping; use it when USB audio is quiet. This is separate from radio RF gain and the radio's speaker-volume control. The CLI QMX launch starts with 20 dB boost; the panel allows adjustment. The audio subscription and spectrum receive unboosted samples.

Spectrum and waterfall show **audio frequencies from 0 to 4 kHz**, not an RF band around the dial. A continuous 14.055 MHz carrier produces about 1 kHz of USB audio when the dial is 14.054 MHz; at the carrier frequency it is near zero beat. A future raw-IQ source will need separate sample-rate and IF-offset handling.

Acquisition and CAT run on the engine worker, not the render thread. Input uses a bounded queue of complete stereo frames. Loss discards stale queued audio and resets analysis; tuning invalidates monitor audio and allows a short settling interval. CAT timeout, unexpected TX/IQ state, capture starvation/error or monitor failure stops the session with a visible error. Stop/start retries discovery and connection. Hardware unplug/replug has not yet been manually validated for this source.

## Validation and remaining work

### Digi mode correction

CAT modes 6 (FSK/Digi USB) and 9 (FSR/Digi LSB) now remain connected during polling, can be selected in the mode dropdown, and survive band-memory recall. Previously the receive-mode allowlist rejected these modes, terminating the worker and disabling the app's mode controls. The engine reports the corresponding sideband while retaining the exact CAT mode in QMX state; it continues forwarding the radio's audio without another demodulation stage. SWR tune (mode 8) remains excluded.

The [operating manual](https://www.qrp-labs.com/images/qmx/manuals/operation_1_03_002.pdf) describes sideband selection for Digi; [CAT documentation](https://www.qrp-labs.com/images/qmx/manuals/cat_1_03_000.pdf) distinguishes modes 6/9 and reports a 3200 Hz Digi filter. Raw I/Q remains the separate Q9 setting. No digital protocol decoding or transmit functionality is enabled by choosing Digi here.

Live verification cycled 6 -> 2 -> 9 -> 3 on firmware 1_03_002. Audio advanced more than 12,000 frames after each change, capture generation remained 1, and reported input loss was zero. Original 21,094,600 Hz CW/receive state was restored. Six device protocol tests, all-target/all-feature Clippy, and the desktop build passed for this correction. Reproduce with `--modes` appended to the hardware probe command below. Restart an older running app to load this fix; a session that already faulted needs Start reception again.

Verification: 70 workspace tests passed; all-feature/all-target Clippy with warnings denied, formatting, desktop build, and GUI-only build without native audio/CAT passed. The protocol tests run without hardware; the Digi follow-up below adds two regression cases.

The integrated hardware probe verified receive audio, a -1 kHz VFO change, a trip to the 17 m configured center, return to the remembered 20 m frequency/mode, and restoration of 14.055 MHz USB. No input drops were reported. Its monitor was muted. The GUI smoke exercised arrow-key tuning and return, rendered the spectrum/waterfall and captured `target/qmx-gui-smoke.png`; CAT verified the final settings. Front-panel changes, VFO B, receive-range gaps and band memory have simulated protocol tests; physical VFO B and knob synchronization still need user validation.

For reproducible checks:

```powershell
cargo run -p sdr-app --features qmx --example qmx_receive_probe -- COM18 "Digital Audio Interface (2- QMX Transceiver)" "Speakers (BEHRINGER UMC202HD 192k)"
```

The hardware probe changes bands and restores the initial dial/mode. `--qmx-smoke` with the GUI command above performs a 100 Hz arrow-key round trip and exits. Do not change the radio concurrently during either test.

Remaining: QMX memory/scanning integration, audio-input decoding, persistent device identities, raw-IQ demodulation, additional modes and platforms. Transmission remains a separate milestone. See [probe measurements and backend research](qmx-integration.md).
