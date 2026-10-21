# Comparing RTL-SDR backends

The desktop **Interface** selector offers:

- **RTL-SDR - librtlsdr**: native library backend, retained as a selectable fallback.
- **RTL-SDR - Rust**: default backend, using pinned `rs-rtl` 0.5.0 and `nusb`, without an rtlsdr DLL dependency for reception.

Backend selection persists independently under `devices.rtl.backend`. Explicitly saved choices are preserved; new sessions and older settings without a backend selection use Rust. RF/converter settings, mode, decoder and display controls are shared. Switching stops the old session and releases the dongle before opening the new one. Scanning stops on this configuration change. Only one backend owns the device at a time.

```powershell
cargo run -p sdr-app --features desktop -- --gui
```

Explicit startup overrides retain normal saved RF/converter settings:

```powershell
cargo run -p sdr-app --features desktop -- --gui --rtl --rtl-backend rs-rtl
cargo run -p sdr-app --features desktop -- --gui --rtl --rtl-backend librtlsdr
```

Default and `desktop` builds include both adapters and prefer Rust. A native-only build can use `--no-default-features --features rtl`; it defaults to librtlsdr and warns/falls back when restoring a saved Rust selection. `--no-default-features` retains the hardware-free build. A live driver error does not silently switch backends.

## What to compare

Both adapters discover the connected RTL devices and use the selected USB serial when available. They default to normal tuner operation, 2.4 MS/s and 28 dB manual gain, and offer 1.92 MS/s and other supported tuner gain settings. They share unsigned IQ normalization, bounded pooled delivery, sample-index accounting and DSP. This change adds no direct-sampling, bias-tee or TX controls.

Diagnostics shows the active backend, driver-reported physical tuner frequency/rate, requested gain and available gain readback. These are configuration values, not calibrated RF measurements. The Rust driver's gain getter exposes an encoded register value, so its dB readback is explicitly unavailable.

Keep antenna, converter, VFO, bandwidth, AGC and display limits unchanged:

1. On 7.074 MHz USB/FT8 through the Ham It Up, compare signal placement and decoding across several complete slots, using the same fine correction.
2. Tune within the capture and move the capture center; check responsiveness and old-frequency data being cleared after a center retune.
3. Stop/start repeatedly and switch backends in both directions while receiving.
4. Unplug during reception, reconnect and press Start Reception; note errors or a device that remains busy.
5. Run each backend for several minutes with Diagnostics visible. Separate intentional retune discards from host overload drops.
6. Optionally compare stereo/RDS with a suitable VHF antenna and converter bypass.

Sequential FT8 slots contain different transmissions and propagation conditions. Counts alone cannot establish sensitivity. Short initial hardware probes also showed different input powers at the same requested gain; gain/initialization parity remains an evaluation item. Do not automatically normalize away that difference.

## Repeatable hardware probes

The device probe receives for three seconds, retunes +100 kHz, deliberately stalls its consumer for 200 ms and checks shutdown. Host drops during that forced stall are expected. Arguments here are physical tuner frequencies:

```powershell
cargo run -p sdr-devices --features rtl-rust --example rtl_probe -- 132321300 librtlsdr
cargo run -p sdr-devices --features rtl-rust --example rtl_probe -- 132321300 rs-rtl
```

The headless FT8 probe runs DLL -> Rust -> DLL in one engine, 35 seconds each. Its second argument is converter fine correction in Hz: 2700 was the saved value for the initial check. It assumes a -125 MHz converter offset and 7.074 MHz dial. It never reads or writes preferences:

```powershell
cargo run -p sdr-app --features rtl-rust --example rtl_backend_probe -- 35 2700
```

Initial Windows/V3 checks (2026-09-30): both drivers opened, retuned and stopped after a forced queue stall. Both engine handoffs completed, with zero reported host IQ drops during all three 35-second receive periods. Rust decoded four FT8 messages; the DLL periods decoded none. This demonstrates a functioning Rust receive path, not comparative sensitivity. Physical unplug/reconnect and extended runtime were subsequently validated by the user on 2026-10-03.

## Integration details

User validation follow-up: the Rust backend successfully decodes FT8 signals very low in the noise and receives broadcast FM. The user observes lower signal levels and a lower displayed noise floor than librtlsdr, with subjectively better broadcast-FM signal/noise ratio and no apparent reception problem. This confirms practical reception on the connected setup; it does not establish calibrated gain or comparative sensitivity. Matched-recording comparisons remain future work; no gain compensation was introduced.

On 2026-10-03 the user confirmed unplug/replug recovery and sufficient extended runtime to promote Rust to the default. The experimental label was removed. This validation covers the connected Windows/RTL-SDR V3 setup, not every tuner or platform. Existing saved librtlsdr selections remain intact.

The Rust bridge continuously drains the driver's bounded queue into our nonblocking pooled delivery path. Shutdown requests stop and drains pending delivery before joining, avoiding the upstream 0.5.0 full-queue shutdown hazard. Retunes stop/join reads, apply the synchronous frequency setter and restart; the upstream fire-and-forget tuning command does not acknowledge success and is not used. Faults reach the engine, and cancellation timeouts explicitly report a potentially busy device.

Host counters measure application-side loss. They cannot establish that the USB/device FIFO never lost samples; the upstream driver does not expose a complete hardware discontinuity counter. Broader tuner/OS support and sample-clock/gain parity remain areas for further testing.

On 2026-10-20, `m02_live_probe` verified 2.4 → 1.92 → 2.4 MS/s and 28.0 → 32.8 → 28.0 dB on both adapters, with fresh spectrum/audio revisions and no reported host IQ drops. `m02_reconnect_probe` resumed the Rust backend after three physical unplug/reconnect cycles without restarting the process. The connected device reported serial `00000001`; its Windows tuner interface used WinUSB through libwdi `oem169.inf`, version `6.1.7600.16385`. Two dongles were not available to verify simultaneous physical selection.

References: [rs-rtl](https://github.com/xoolive/desperado/tree/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-rtl), [stream lifecycle](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-rtl/src/rtlsdr.rs), [gain implementation](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-rtl/src/tuner.rs).
