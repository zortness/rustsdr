# M02: First live radio

Status: accepted on 2026-10-20 for the named Windows 11, RTL-SDR v3/R820T and Direct3D 12 configuration. The earlier [30-minute Direct3D 12 soak](soak-results.md) passed the resource and host-loss gates. The device, gain and sample-rate controls and three physical reconnect cycles completed the remaining checks below. Vulkan still fails the documented memory gate on this host.

## Current equipment and behavior

The first target is the user's RTL-SDR v3 with an R820T tuner on Windows 11. The desktop build defaults to the Rust `rs-rtl` backend and also offers `librtlsdr`. It discovers RTL devices, selects by USB serial when available, and offers 1.92 or 2.4 MS/s and validated manual tuner gain. Defaults remain 2.4 MS/s and 28 dB. A configuration change releases the old capture, resets audio and spectrum state, and opens a new capture revision. The audio producer uses bounded output-queue occupancy feedback for clock drift. See [RTL backend results](rtl-backends.md), [implementation status](implementation-status.md), and [runtime provenance](../third-party/README.md).

## Acceptance record

- [x] Discover and select among connected RTL devices. Both adapters enumerate index, name and USB serial; the selected serial is persisted and resolved again on open. A two-device selection/reordering fixture passed. One physical dongle was available for live verification; selecting between two connected physical dongles remains untested.
- [x] Select and validate tuner gain and capture sample rate. The device checks the requested gain against its supported list and verifies the rate readback. The receiver and channel extractor decimate either 1.92 or 2.4 MS/s to their existing rates; file and synthetic sources remain at 2.4 MS/s. Both live backends passed a 2.4/28.0 dB → 1.92/32.8 dB → 2.4/28.0 dB sequence with fresh spectrum and audio revisions and zero reported host IQ drops.
- [x] Exercise repeated software stop/start cycles on the named device, recording recovery time and any stuck or busy device state. Nine cycles in each full Vulkan soak resumed, generally 1.7–1.8 seconds after each start request; no busy or stuck state was observed.
- [x] Exercise repeated physical unplug/reconnect cycles. Three removal/reconnect cycles on 2026-10-20 resumed Rust-backend reception in one process. Each recovery produced at least 163,840 samples and two new spectrum frames; no busy device state was observed.
- [x] Run an instrumented release build for at least 30 minutes and record rate, loss counters, audio queue, memory, and rendering with retune/startup intervals identified. Four [runs and raw evidence](soak-results.md) were recorded. The fourth used Direct3D 12, collected host counters every GUI frame across restarts and measured zero unexplained IQ drops or audio faults.
- [x] Bound the observed memory increase and repeat the full run with per-frame counter accumulation. Direct3D 12 passed at 2.31 MiB/hour fitted resident-memory growth and 1.06 MiB quartile increase on this host; default Vulkan remains a documented failure.
- [x] Record device identity, backend and driver, OS, build revision, output device/rate, run command, and limitations with the measurements. The [soak record](soak-results.md) has the 30-minute run command, build and output details. For the 2026-10-20 follow-up the device was `USB\VID_0BDA&PID_2838&MI_00`, serial `00000001`, using WinUSB (`winusb.sys`) installed through libwdi `oem169.inf`, driver version `6.1.7600.16385` dated 2017-02-10. The Rust adapter uses pinned `rs-rtl` 0.5.0; the native adapter uses the bundled RTL-SDR Blog library. The follow-up probes ran on the working tree based on `332086f`. Host counters cannot prove that the tuner or USB bus never lost samples upstream.

The [soak command](../README.md#instrumented-reception-soak) supplies the long-run record. The receive-only `m02_live_probe` and `m02_reconnect_probe` examples reproduce the follow-up checks. They do not calibrate RF gain, prove upstream USB loss is zero, or test two physical dongles together.

```powershell
cargo run -p sdr-app --example m02_live_probe --features desktop --offline
cargo run -p sdr-app --example m02_live_probe --features desktop --offline -- librtlsdr
cargo run -p sdr-app --example m02_reconnect_probe --features desktop --offline
```
