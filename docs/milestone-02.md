# M02: First live radio

Status: in progress. Live reception arrived before M01's acceptance record. Three [instrumented 30-minute Vulkan runs](soak-results.md) measured working reception and repeated software recovery but failed the memory gate. A fourth 30-minute run with Direct3D 12 collected counters across restarts every GUI frame and passed the resource and host-loss gates on the named AMD/Windows host. Device selection, gain/sample-rate control, and repeated physical reconnect still block the M02 gate in the [roadmap](roadmap.md).

## Current equipment and behavior

The first target is the user's RTL-SDR v3 with an R820T tuner on Windows 11. The desktop build defaults to the Rust `rs-rtl` backend and also offers `librtlsdr`. It opens device index 0 at 2.4 MS/s with requested manual gain of 28 dB. Tuning, center retuning, and manual stop/start work in short live checks. The user has confirmed unplug, reconnect, and manual restart with the Rust backend. The audio producer uses bounded output-queue occupancy feedback for clock drift. See [RTL backend results](rtl-backends.md), [implementation status](implementation-status.md), and [runtime provenance](../third-party/README.md).

## Open acceptance work

- [ ] Discover and select among connected RTL devices. The engine currently opens index 0.
- [ ] Select and validate tuner gain and capture sample rate. Both are fixed today; the DSP, FFT, file, and engine paths assume 2.4 MS/s. Test a live rate change without stale spectrum/audio state before claiming this gate.
- [x] Exercise repeated software stop/start cycles on the named device, recording recovery time and any stuck or busy device state. Nine cycles in each full Vulkan soak resumed, generally 1.7–1.8 seconds after each start request; no busy or stuck state was observed.
- [ ] Exercise repeated physical unplug/reconnect cycles. One user-confirmed reconnect is existing evidence, not an endurance result.
- [x] Run an instrumented release build for at least 30 minutes and record rate, loss counters, audio queue, memory, and rendering with retune/startup intervals identified. Four [runs and raw evidence](soak-results.md) were recorded. The fourth used Direct3D 12, collected host counters every GUI frame across restarts and measured zero unexplained IQ drops or audio faults.
- [x] Bound the observed memory increase and repeat the full run with per-frame counter accumulation. Direct3D 12 passed at 2.31 MiB/hour fitted resident-memory growth and 1.06 MiB quartile increase on this host; default Vulkan remains a documented failure.
- [ ] Record device identity, backend and driver, OS, build revision, output device/rate, run command, and limitations with the measurements. The run record has the RTL-SDR v3/R820T identity, Rust backend, OS, application/dependency revisions, output details and GPU driver. The exact RTL USB driver version remains unverified. Host counters cannot prove that the tuner or USB bus never lost samples upstream.

The [soak command](../README.md#instrumented-reception-soak) supplies a reproducible interaction and counter record for the long-run gate. It does not implement device discovery, gain or sample-rate selection, or prove recovery from physical USB removal.
