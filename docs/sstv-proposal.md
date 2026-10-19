# SSTV reception, encoding and mode workspace proposal

Status: plan accepted by the user, 2026-10-13. The [codec/service exploration](sstv-spike.md) records the first prototype and validation gaps.

This proposal is for the maintainer and the next implementation session. It defines a staged path to receiving SSTV images, producing interoperable encoded audio, and transmitting through the IC-705, with a modest revision to the existing digital-mode UI.

## Current project state

The working tree was clean at inspection. History contains one initial commit, `05544e5`; subsequent project history is recorded inside the documentation. There is no dedicated handoff, continuation file or milestone state file. The implementation-status guide, feature guides, and GSD knowledge/decision notes serve as the handoff.

The four-crate workspace already provides the important receive boundaries: radio-independent DSP, engine-owned decoder workers, and independent bounded audio subscriptions. Receiver audio is 48 kHz, before monitor volume and sound-card rate correction, with sample positions, configuration/capture identity and discontinuity metadata. RTL and IQ files supply demodulated audio; QMX and IC-705 supply radio AF audio. Existing FT and VOR services demonstrate headless operation and worker ownership.

The current digital service is specifically an FT8/FT4 service. Its protocol enum, 12 kHz resampler, UTC slot assembly, WAV importer and text results should not acquire SSTV-specific branches. The GUI likewise assumes text messages, offers Waterfall/Text tabs in compact windows, and applies an FT USB preset. Its hardware compatibility checks are USB-specific.

IC-705 transmission exists as a separately routed, bounded three-second test tone. Both the audio callback and engine deadline enforce that duration. This is useful infrastructure, but not an arbitrary waveform transmitter. The QMX integration remains receive/control only.

Some handoff statements are stale: the older engine-boundary guide describes a single audio subscriber and no decoders, whereas current code broadcasts to independent subscribers and owns FT/VOR workers. The implementation-status summary says live IC-705 TX is pending; the IC-705 feature guide records a later successful dummy-load TX/RX recovery probe. Treat that as recorded evidence, not a new hardware verification. Reconcile these descriptions as implementation lands.

## Recommended scope

Deliver analog SSTV with initial acceptance coverage for **Robot 36, Martin M1, Scottie S1 and PD120**, in both directions. These exercise several timing/color families without promising every mode a library lists. Enable additional variants only after independent fixtures pass. Automatic VIS identification and explicit mode selection belong in the initial receive experience; headerless acquisition depends on the codec spike.

The first useful release receives live images, imports recorded WAV audio, exports received PNGs, and encodes a prepared image to WAV. A following stage sends the same prepared waveform through IC-705 USB audio with progress and cancellation. This makes encoding useful before full radio transmission is ready, while retaining actual transmission in the overall plan.

Defer digital SSTV/DRM, template editors, automatic CQ/repeat, QSO logging, simultaneous multiple SSTV signals, QMX transmission and a general decoder-plugin framework.

## Codec choice and validation gate

Evaluate [unexcellent/sstv](https://github.com/unexcellent/sstv) first. Its upstream documentation describes a MIT-licensed Rust encoder/decoder, progressive row iteration, automatic or forced mode selection, and independent-program/recorded-audio tests. These properties match our needs, but upstream claims are not RustSDR validation. Pin an exact compatible release or revision only after checking source, licensing, Rust 1.93 compatibility and transitive dependencies.

The key risk is its pull-based sample iterator: temporary queue starvation must not become EOF, while stop, source changes and missing audio must interrupt acquisition promptly. Prove that with a bounded worker adapter before UI work. Repeated frames, truncated input and idle noise must also terminate or cancel predictably.

The fallback receive candidate is [slowrx](https://docs.rs/slowrx/latest/slowrx/), which exposes chunk processing and events. Its published status distinguishes recorded PD/Robot validation from synthetic-only Martin/Scottie validation. Pairing a separate encoder with it adds two adapters and requires license review. [komitoto-sstv](https://docs.rs/komitoto-sstv/latest/komitoto_sstv/) offers encode/decode file APIs worth comparing, but progressive reception needs further investigation. [libsstv](https://libsstv.org/) explicitly describes its present implementation as encoding-only, so it does not solve reception.

Do not write a full codec from scratch unless these candidates fail a documented requirement.

## Engine and data design

Add an SSTV service alongside FT and VOR. The engine owns its lifetime; headless callers and the GUI use a control handle. Keep a narrow codec adapter independent of Bevy, audio devices and CAT. Start inside the application crate, with pure helpers in DSP only where they genuinely belong; a new workspace crate is unnecessary initially.

Feed a dedicated subscription from the existing 48 kHz audio tap. Convert channels and sample rate at the codec boundary only as required; do not inherit FT's slot clock. SSTV frame timing follows sample positions. UTC is capture metadata, not a decode scheduling requirement.

Represent searching, identified mode, receiving, complete, interrupted and error states explicitly. Each image has a stable ID, dimensions, source identity, optional dial frequency, mode, start time and completed-row count. Imported images carry file provenance rather than the current radio frequency.

Use bounded audio queues and a coalesced latest image snapshot, with bounded completed-image history. The UI may skip preview refreshes without losing final image data. A dropped line-update event must never silently corrupt the saved image; retain an authoritative snapshot retrievable by image ID/version. Throttle preview uploads, initially around 10 Hz, and bound history by bytes as well as count.

On sample gaps, incompatible receive settings, tuning or source changes, retain the partial image marked interrupted and restart acquisition. Never stitch audio from different captures into one image. EOF finalizes a complete frame or preserves a clearly partial one. Closing the panel does not disable reception; an explicit enable control does. Stop and engine shutdown must join promptly even when no audio arrives.

Image preparation and WAV I/O run off the radio/UI threads. Accept bounded PNG/JPEG image inputs, show exact output geometry, offer Fit/Crop with an explicit background, and export mono PCM16 WAV at 48 kHz. Start WAV reception with bounded PCM16 mono/stereo 44.1/48 kHz files, using proper resampling where needed. Keep its importer separate from FT's single-slot 12 kHz reader. Specify and test file size, duration and image-pixel limits before implementation.

## UX proposal

Keep **radio mode** (USB/LSB/NFM etc.) distinct from **signal mode** (Off/FT8/FT4/SSTV). SSTV belongs in this operating workflow even though conventional SSTV is analog. Rename the header's text-specific action to **Mode workspace**, and show the selected mode in its title. Retain the existing Signal Data and VOR panels initially.

| Area | Proposed behavior |
| --- | --- |
| Wide window | Resizable workspace alongside the spectrum/waterfall; retain the existing floating-window approach initially. |
| Compact window | Waterfall / Mode tabs replace Waterfall / Text; opening SSTV goes to its image view. |
| SSTV Receive | Large progressive image; Auto/forced mode; acquisition status, progress and elapsed time; Restart, Open WAV, Save image; bounded thumbnail history. |
| SSTV Compose | Open image, Fit/Crop preview, encoding mode, resolution/duration, Encode WAV and explicit monitor Preview. Add Send when a supported transmitter is ready. |
| FT8/FT4 | Preserve the current table, WAV import, callsign overlays and decoder settings. |

Separate workspace selection from decoder enable state internally; changing a view must not unexpectedly reset an image. Initially expose one primary audio signal mode at a time, preserving FT history when switching to SSTV. Do not expose a multi-decoder routing matrix.

For SSTV, do not blindly reuse the FT preset's 6 kHz display and USB-only eligibility. Offer an explicit HF USB setup using the existing 2.7 kHz voice bandwidth (300–3000 Hz), and permit compatible LSB/NFM audio paths. Validate NFM processing with fixtures, particularly de-emphasis and squelch. On audio radios, show the required mode/filter conditions; do not silently issue CAT changes when a workspace opens. Actual radio filter response remains a hardware validation item.

Persist SSTV preferences separately from `decoders.ft`, and workspace selection separately from either codec. Migrate existing FT window/tab state without losing its settings. Persist neither active transmission nor pending send jobs. Image history can remain session-only initially; saving an image is explicit.

## Radio transmission

Replace the IC-705 test-tone-specific audio source with a finite prepared-audio job. Preserve explicit TX endpoint selection, route/mode/frequency checks, reduced-power test behavior and confirmed-RX cleanup. Add capability reporting for prepared-audio TX rather than interpreting a build-level transmit boolean as readiness.

Prepare the complete bounded waveform before keying. Use a job identity, sample budget and duration-derived hard deadline; account for output buffering and the final audio tail before unkeying. An audio callback only consumes prepared samples, without image work or allocation. TX level is independent of monitor volume. Monitor preview must use the existing radio-output exclusion rules.

Define Idle, Preparing, Ready, Keying, Sending, Draining, Unkeying and Fault states. Stop TX, stop reception, source change, audio failure and normal shutdown cancel the job and attempt RX cleanup. An uncertain unkey result remains visible and blocks another send. Never resume or replay after reconnect/restart. Suspend the receive decoder during owned TX and reset its audio timeline on return.

A multi-minute image requires more validation than the existing three-second tone. Begin with headless fake transport/output tests, then an explicitly configured dummy-load trial on the intended radio. Full-image over-air interoperability follows separately.

## Implementation stages and acceptance

| Stage | Deliverable | Completion evidence |
| --- | --- | --- |
| 1. Codec spike | Pinned adapter plus headless WAV-to-image and image-to-WAV probes | Four initial modes tested against independently produced fixtures; our encoded WAVs decoded by an independent implementation; repeated frames, noise, truncation, chunk boundaries and cancellation checked. Record fixture licenses/hashes. |
| 2. Receive service | Live/offline engine-owned decoding with progressive snapshots | IQ-to-demodulator-to-image test; independent subscriber and muted-monitor tests; gaps/retunes/EOF/slow UI preserve correct identity and bounded memory. No hardware required. |
| 3. Workspace and composer | Receive image/history, import/save, image preparation and WAV export; FT migration | Desktop and compact GUI smoke with inspected screenshots; FT WAV/table/overlay regression; preferences migration; independent encode interoperability. |
| 4. IC-705 send | Finite audio jobs, progress, cancel and RX recovery | Fake-device failure-path tests, exact audio length/tail tests, then configured dummy-load full-image send and RX recovery. Hardware results clearly separated from automated tests. |
| 5. Reception hardening | Documented offset/drift/noise tolerance and additional proven modes | Recorded corpus comparisons, slant/sample-clock behavior, sustained release-profile performance, and manual RTL/radio reception. |

Round-tripping our own encoder and decoder is necessary but insufficient: shared timing or color mistakes can cancel out. Use independent recordings and decoded references, checking mode, geometry, line alignment and image error tolerances rather than exact pixel equality for noisy audio. Include silence/voice false-positive rejection and multiple back-to-back images. Decide measured CPU and memory budgets during the spike, then retain regression checks.

Run the repository's workspace tests, formatting, strict Clippy, desktop build and relevant feature-minimal checks after implementation changes. No new application tests or hardware probes were run for this proposal; existing results above are historical documentation, and codec suitability remains to be demonstrated.

The proposed starting point is stage 1, followed by stages 2–3 as the first user-facing increment. Full IC-705 image transmission is stage 4, not implied by successful WAV encoding.
