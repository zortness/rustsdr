# RTL-SDR reception soak, 2026-10-18–19

This is the instrumented M01/M02 baseline for the user's RTL-SDR v3/R820T on Windows 11 Pro. The application was built from base revision `f53753e7356c17fd462f11d096322a792fe9b3d8` with the uncommitted soak runner and diagnostics changes in this working tree. The command was:

```powershell
cargo build -p sdr-app --release --features desktop --offline
.\target\release\sdr-app.exe --gui --rtl --mode stereo --soak 1800 --soak-interval 10
```

Rust/Cargo 1.93.1; Bevy 0.18.1, bevy_egui 0.39.1, CPAL 0.17.3, ringbuf 0.4.8, rs-rtl 0.5.0 and rustfft 6.4.1. Host: AMD Ryzen AI MAX+ 395, 16 physical cores, 95.6 GiB RAM. Graphics: AMD Radeon 8060S, Vulkan, AMD driver 26.5.2. Input: `rs-rtl` backend, device index 0, requested manual gain 28 dB, 2.4 MS/s, 4096-point FFT, WFM stereo. Output: Speakers (BEHRINGER UMC202HD 192k) at 48 kHz.

The 200-second cycle settles, resizes the window, fits and restores the spectrum span, tunes the VFO, scrolls the RTL capture center, then stops and restarts reception. Each action must have an observable effect. The report samples the receiving process every 10 seconds; the memory trend starts after the first 40 seconds. Retune discards and display skips are separate from unexplained host IQ drops. Audio queue occupancy is in frames; its configured capacity at 48 kHz is 24,000 frames.

## First full run

The first 1800-second run **failed the memory gate**. Resident memory increased from 276.30 to 314.78 MiB; the fitted slope was 90.82 MiB/hour, above the runner's 10 MiB/hour limit, and the last-quarter median exceeded the first-quarter median by 33.54 MiB, above its 16 MiB limit. Large steps occurred around 551 and 1091 seconds. The series flattened after roughly 1200 seconds, but this run cannot establish that memory stays bounded across longer reception.

Other measured counters were favorable: median observed input rate 2,398,324 samples/s, zero unexplained host IQ drops, 147,456 intentional retune discards, two skipped display frames, zero audio under/overruns, errors, or sampled stalls, median audio queue 2,167 frames (45 ms) and peak 2,498 frames (52 ms), median 59.83 FPS and median 1.51% app CPU of total host capacity. Nine software stop/start cycles resumed reception, generally within 1.7–1.8 seconds after the start request. A 415-second diagnostic on a later build verified two full interaction cycles and kept the Bevy image asset count at five, but it was too short to satisfy the duration gate and memory still rose 8.15 MiB ([summary](evidence/rtl-soak-2026-10-18/texture-preflight-summary.json), [time series](evidence/rtl-soak-2026-10-18/texture-preflight.csv)).

The first full run's later VFO clicks hit the already-selected frequency. Its initial VFO change was verified, while later cycles' tune actions were not; the runner now alternates click positions. The runner also accumulated counters only at ten-second sampling points in these runs, so the delivered-sample total is a lower bound across restarts. A subsequent collector change observes counters every GUI frame. The zero sampled loss counters remain host-side observations; they cannot rule out unreported tuner or USB loss.

Raw evidence: [first summary](evidence/rtl-soak-2026-10-18/baseline-summary.json), [first time series](evidence/rtl-soak-2026-10-18/baseline.csv), and [final GUI image](evidence/rtl-soak-2026-10-18/baseline-final.png). The image shows the running receiver, spectrum and waterfall after the interaction cycle, including the visible gap from the final restart.

## In-place waterfall comparison

The next full release build retained the same waterfall texture and used a full-size partial image update when reprojecting history. It also alternated VFO click locations and checked each cycle's effect. Its 1800-second run verified all nine VFO tunes and nine restarts, with zero unverified interactions and a constant peak of five Bevy image assets. The final screenshot was inspected; reception, trace and waterfall were present after the last restart.

| Measurement | First run | In-place texture run |
| --- | ---: | ---: |
| Memory after settling / final | 276.30 / 314.78 MiB | 276.77 / 316.30 MiB |
| Fitted memory slope | 90.82 MiB/hour | 95.41 MiB/hour |
| First-to-last quartile median increase | 33.54 MiB | 35.75 MiB |
| Median input rate | 2,398,324/s | 2,398,287/s |
| Skipped display frames | 2 | 3 |
| Median FPS | 59.83 | 60.01 |
| Host IQ drops / audio under/overruns at CSV samples | 0 / 0 | 0 / 0 |

The image asset count stayed fixed, but this change did not improve resident-memory growth. The cause is not established; a stable asset count alone does not rule out allocations elsewhere in Bevy, egui, the graphics driver, the SDR engine, or the operating system. Both full runs fail the memory limits. The revised counter collector was made **after** this comparison, so both full runs retain the ten-second counter limitation described above.

Raw comparison evidence: [summary](evidence/rtl-soak-2026-10-18/texture-comparison-summary.json), [time series](evidence/rtl-soak-2026-10-18/texture-comparison.csv), [final GUI image](evidence/rtl-soak-2026-10-18/texture-comparison-final.png).

## Counter collector diagnostic, 2026-10-19

After moving cumulative counter observation to every GUI frame, a 215-second release run completed one full interaction cycle. It verified all six interactions, including stop/start, with zero reported host IQ drops and audio faults. The revised collector totaled 500,498,432 delivered samples; the earlier 215-second diagnostic totaled 481,148,928 with the same ten-second interval. The 19,349,504-sample difference is consistent with the earlier collector missing part of the stream around restart. The newer run sampled every five seconds, so the two totals are a diagnostic comparison rather than a controlled rate benchmark. The only reported failure was the deliberate `215 < 1800` duration check. See its [summary](evidence/rtl-soak-2026-10-19/collector-preflight-summary.json) and [time series](evidence/rtl-soak-2026-10-19/collector-preflight.csv).

## Corrected full Vulkan run and process resources

The full 1800-second run with per-frame counter collection delivered 4,212,768,768 samples at a median observed 2,398,375 samples/s. All nine VFO changes and nine restarts were verified. Host-side unexplained IQ drops, audio under/overruns, audio errors, engine faults, sampled stalls and unverified interactions were zero. The peak audio queue was 2,480 frames; four display frames were skipped; median FPS was 60.15 and median app CPU was 1.50% of host capacity. These results satisfy the M01 host input/audio/render target on this configuration, subject to the counters' inability to see losses inside the tuner or USB bus.

The run **failed memory** again: 275.93 to 315.88 MiB resident, 95.32 MiB/hour fitted slope, and 35.06 MiB first-to-last quartile increase. An independent Windows process sampler recorded working set, private bytes, handles and threads every 30 seconds. Handles rose from 1,969 near 45 seconds to 54,166 near the end, approximately 30 per second, while thread count remained near 50–60. A read-only system handle-table snapshot identified the growing class as Windows Event handles: 14,072 at the earlier snapshot versus 48,276 later; other handle classes were essentially unchanged. Resident-memory steps coincided with handle counts near 8,000, 16,000 and 32,000. That alignment suggests handle-table expansion is driving the steps, but the snapshots alone do not identify the creator of each Event handle.

Raw evidence: [summary](evidence/rtl-soak-2026-10-19/corrected-full-summary.json), [soak time series](evidence/rtl-soak-2026-10-19/corrected-full.csv), [Windows process series](evidence/rtl-soak-2026-10-19/process.csv), [early handle types](evidence/rtl-soak-2026-10-19/handle-types-early.txt), [late handle types](evidence/rtl-soak-2026-10-19/handle-types-late.txt), [read-only snapshot script](evidence/rtl-soak-2026-10-19/handle-types.ps1) and [final GUI image](evidence/rtl-soak-2026-10-19/corrected-full-final.png). The image was visually inspected after the final restart.

## Graphics backend isolation

A hardware-free synthetic GUI zoom smoke reproduced roughly 30 new handles per second on the default Vulkan renderer ([series](evidence/rtl-soak-2026-10-19/synthetic-vulkan-handles.csv)). The same smoke on Direct3D 12 held at 519 handles after startup ([series](evidence/rtl-soak-2026-10-19/synthetic-dx12-handles.csv), [renderer log](evidence/rtl-soak-2026-10-19/synthetic-dx12-log.txt)). This narrows the growth to the Vulkan rendering route on this AMD/Windows setup; the synthetic probe has neither RTL input nor receiver audio output.

A 215-second live RTL Direct3D 12 preflight verified all six scripted interactions with zero host IQ/audio faults. Resident memory was 251.07 to 251.30 MiB, the Bevy image asset peak was five, and a read-only process check found about 560 handles at 161 seconds versus 566 near startup. Its only reported failure was the deliberate short-duration gate. [Preflight summary](evidence/rtl-soak-2026-10-19/dx12-preflight-summary.json) and [time series](evidence/rtl-soak-2026-10-19/dx12-preflight.csv).

## Full Direct3D 12 acceptance run

The release build was run on the same AMD/Windows host with `$env:WGPU_BACKEND = 'dx12'` and the 1800-second command above. It **passed all instrumented gates**. Across 1800.008 seconds, the per-frame collector recorded 4,212,736,000 delivered samples, a median observed input rate of 2,398,855 samples/s, zero unexplained host IQ drops, zero audio under/overruns or output errors, zero engine faults or sampled stalls, and zero unverified interactions. All nine VFO changes and nine software restarts were verified; the last restart resumed in about 1.7 seconds. There were five skipped display frames, a peak audio queue of 2,458 frames (51 ms at 48 kHz), median 59.96 FPS, and median app CPU of 1.46% of total host capacity. The Bevy image asset peak remained five.

Resident memory after settling was 251.72 MiB and finished at 252.28 MiB, with a 252.88 MiB observed peak, 2.31 MiB/hour fitted slope and 1.06 MiB first-to-last quartile median increase. These meet the runner's 10 MiB/hour and 16 MiB limits on this backend and host. An independent 30-second Windows process series recorded 58 points, with handle counts from 543 to 575; it began at 568 and ended at 575. This contrasts with the Vulkan handle growth, but does not establish behavior on other graphics adapters or longer runs. The final screenshot was visually inspected: the receiver was running and the trace and waterfall were present after the last restart.

Raw evidence: [summary](evidence/rtl-soak-2026-10-19/dx12-full-summary.json), [soak time series](evidence/rtl-soak-2026-10-19/dx12-full.csv), [Windows process series](evidence/rtl-soak-2026-10-19/dx12-full-process.csv), and [final GUI image](evidence/rtl-soak-2026-10-19/dx12-full-final.png). The renderer identification is also in the [Direct3D 12 preflight log](evidence/rtl-soak-2026-10-19/synthetic-dx12-log.txt); the full run used the same `WGPU_BACKEND` setting and reported graphics driver `32.0.31007.5012`.

An isolated synthetic GUI was opened with Direct3D 12, sent a normal Windows `WM_CLOSE`, and exited with code 0 ([probe script](evidence/rtl-soak-2026-10-19/window-close-probe.ps1), [result](evidence/rtl-soak-2026-10-19/window-close-probe.txt)).

## Audio-output interruption and recovery

The user powered off the BEHRINGER UMC202HD while a separate release probe was receiving live RTL samples through an open audio stream. The receiver reported `Audio output disconnected or failed; stop/start to retry`, continued from 67,207,168 to 74,416,128 delivered samples with spectrum frames and zero host IQ drops, and stopped cleanly. After the user restored the interface, the same controller process restarted reception and reopened `Speakers (BEHRINGER UMC202HD 192k)` at 48 kHz. It reported 4,014,080 new samples, 2,047 queued audio frames, RMS 0.2152, zero IQ drops, and exited with code 0. See the [probe source](../crates/sdr-app/examples/audio_recovery_probe.rs) and [run record](evidence/rtl-soak-2026-10-19/audio-recovery-probe.txt).

This establishes recovery from an active endpoint failure through stop/start. It does not establish audible quality after recovery. Windows selected ROG headphones as a fallback when BEHRINGER was unavailable before startup, so a no-default-endpoint startup was not exercised.

## Current decision

M01's documented acceptance checks are satisfied on this host with Direct3D 12, including the separate audio-output recovery check. At the time of these runs, M02's device controls and repeated physical reconnect were still open; they were completed in the [2026-10-20 M02 follow-up](milestone-02.md). The default Vulkan renderer still fails the memory gate; retain Bevy provisionally with an explicit graphics-backend choice here. The Vulkan Event-handle growth and its memory steps remain a limitation to investigate with a minimal renderer reproduction or upstream fix. These soak runs themselves do not establish physical unplug/reconnect endurance, device discovery, selectable gain/sample rate, or audible quality throughout the soak.
