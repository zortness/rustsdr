# Modular receiver controls and broadcast services

Design proposal following the user's 30-minute manual test. Audience: contributors implementing the next UI/decoder increment. The goal is to add a receiver or decoder without expanding one shared options panel indefinitely. Update 2026-09-25: modular controls and the separate RDS/RBDS results window are implemented; see [FM broadcast data](fm-broadcast-data.md). Other services below remain proposals.

## Panel organization

Keep source, capture center, VFO, tuning step, start/stop, and monitor volume/mute in stable shared sections. Render a separate mode settings component for bandwidth, AGC, squelch, de-emphasis, or future specialized controls. Put optional decoder enable/configuration controls in their own section. Show decoded content in a separate resizable results area, leaving tuning accessible while text or message tables grow. Advanced diagnostics should collapse independently.

Use typed Rust configuration and dedicated egui components for built-in modes. A small static catalogue can provide labels, defaults, supported controls, and compatible decoder taps. Avoid an external plugin ABI or a general schema-generated UI at this stage. Preserve per-mode settings in session when switching modes; persistent profiles can follow separately. The current app resets mode settings on selection, so remembering them is a deliberate behavior change to test.

Each component edits a draft and submits validated controller commands. Apply only the affected scope: display-only, monitor-only, decoder-only, receiver DSP, or capture/device. Decoder toggles must not rebuild the audio receiver or clear the waterfall. Reject unsupported combinations explicitly. Keep decoder enablement independent of stereo pilot lock and monitor mute.

Results have identity, generation, status, and freshness. Clear or visibly mark old station metadata on retune; discard stale worker results. Show acquiring, receiving, lost synchronization, unsupported service, and error states rather than displaying stale text as current. Bound result history and coalesce frequent status updates. UI rendering must never run protocol decoding.

## Broadcast FM candidates

| Feature | Signal path | Proposed presentation and first acceptance gate |
| --- | --- | --- |
| RDS/RBDS | Tap the FM discriminator's multiplex before audio low-pass filtering and de-emphasis | Station identity/service name and RadioText, then program type and optional tagged title/artist; verified block/group checks, text assembly, retune clearing, mute independence |
| HD Radio / NRSC-5 | Separate sufficiently wide complex-IQ channel and digital demodulation/decoding | Services/program selector, digital lock/quality, metadata, then decoded audio; backend/codec feasibility and known-recording comparison first |
| Other subsidiary FM subcarriers (SCA) | Multiplex tap and service-specific filtering/demodulation | Inspect/select a detected or known subcarrier; determine whether it carries audio or a supported data format before choosing an output panel |

RDS is a digital data service on a 57 kHz subcarrier of analog FM. Programme Service and RadioText are distinct fields; only display separate title/artist when the service supplies structured tags such as RT+. RDS and RBDS need explicit regional interpretation where definitions differ. References: [NRSC RDS/RBDS introduction](https://www.nrscstandards.org/news/rbds-1998.asp), [RDS Forum overview](https://rds.org.uk/pages/what-is-rds.html), and [field glossary](https://www.rds.org.uk/pages/glossary-of-terms.html).

HD Radio carries digital audio and data services under NRSC-5, including hybrid analog/digital operation. It is not another RDS text group. The existing 200 kHz nominal WFM channel filter is not a suitable HD Radio input contract; branch from capture IQ and prepare a dedicated channel/rate. Evaluate backend, codec, redistribution terms, Windows integration, and bounded CPU/memory before committing implementation. Reference: [NRSC-5-E](https://www.nrscstandards.org/standards-and-guidelines/documents/standards/nrsc-5-e/nrsc-5-e.pdf). Subsidiary subcarrier services are another independent candidate; see the [NRSC SCA compatibility report](https://www.nrscstandards.org/reports/nrsc-r203-part-ii.pdf). Local service availability has not been established.

The Rust receiver now exposes an optional 240 kS/s multiplex tap used by the RDS decoder; the 48 kHz speaker output has already discarded the needed subcarrier. Check end-to-end passband quality using RDS-bearing IQ fixtures rather than assuming the existing RF filter is sufficient. RDS should work with either mono or stereo listening and should continue decoding when speakers are muted.

## Converter-aware tuning

The user expects to install an external converter for consistent AM signals. Its model, oscillator frequency, translation direction, and inversion are not known. Do not assume a particular upconverter/downconverter or silently enable V3 direct sampling.

Design a source-level converter profile containing valid RF range, oscillator/offset, translation sign, and spectral inversion. UI tuning and waterfall labels use actual RF; device commands use the mapped tuner frequency. Validate both frequency domains and correct IQ orientation when needed. Store the mapping in recording metadata so playback is labeled consistently. Test round trips, boundaries, recentering, inversion/USB-LSB behavior, and bypass mode. The existing raw IQ format needs explicit accompanying metadata for this feature.

## Suggested delivery

1. Extract shared, mode-specific, and decoder/results UI components, keeping current listening and tuning behavior covered by regression checks.
2. Implement RDS/RBDS as the first optional decoder: generated/recorded fixtures, synchronization and checked groups, station name/RadioText assembly, then a live text panel. This proves the decoder contract with the current FM setup if a local station transmits RDS.
3. Add converter configuration when equipment details are available and finish live AM validation.
4. Run a separate HD Radio backend feasibility spike. Keep CW, FT8, digital voice, and other candidates in the [mode backlog](mode-backlog.md).

The user has completed a 30-minute manual multi-mode stability test and validated most available modes. AM remains outstanding. No exact tested-mode list, release-build identification, memory trace, or loss-counter measurements were supplied; retain that distinction in milestone evidence without requiring a repeat manual test merely to proceed with this design.

