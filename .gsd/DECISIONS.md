# Decision index

- 2026-09-25: User prioritized memories and scanning before additional modes, prefers simple CSV files, and chose a fixed configurable signal pause followed by continued scanning. Stop scan must hold the current frequency/mode and allow restart. Implemented CSV groups, recall navigation, worker-owned memory/VFO scans and explicit stop-at-current; see [memory and scan design](../docs/memories-and-scanning.md).


- User authorized implementing the portability-oriented architectural changes before more features and requested before/after performance checks. The local engine/client and output boundaries are now implemented; network/browser/mobile runtimes remain future explorations. See [implemented boundary](../docs/engine-boundary.md) and [matched trace comparison](../docs/architecture-trace-comparison.md).


- User requested recording cross-platform and WebSDR findings as future explorations. Preserve local in-process operation; favor native dependency isolation, engine/client and monitor/decoder boundaries, and modular UI now. Network protocols, browser/mobile adapters, and multi-user control remain deferred. See [platform exploration](../docs/platform-explorations.md).

- Latest user follow-up: 30-minute manual multi-mode stability test completed; most modes validated, AM pending suitable RF/converter equipment. Preserve measurement gaps without treating the manual test as still deferred. Proposed next increment: [modular controls and broadcast services](../docs/receiver-panels.md), RDS/RBDS text before separate HD Radio feasibility work. Converter details remain unknown.

- 2026-09-23: Rust-first SDR architecture, provisional Bevy frontend, receive-first milestones, and future transmit/transceiver capability boundaries: [decision record](../docs/decisions.md).

- 2026-09-24: User validated improved tuning and deferred the 30-minute test. Then authorized voice modes including SSB, with design/backlog for CW, FT8, digital voice, etc. WFM mono/stereo, AM, NFM, USB/LSB are implemented; optional message decoding remains separate future work. See [receiver modes](../docs/receiver-modes.md) and [backlog](../docs/mode-backlog.md). Keep TX/control capabilities separate.

- 2026-09-30: Engine owns FT/VOR worker lifetime; bounded independent audio and IQ subscriptions preserve capture provenance. Selective Desperado fixture reuse, existing RTL driver retained, DAB deferred. See [decoder extensions](../docs/decoder-extensions.md).

- 2026-09-30: Keep librtlsdr as default; add pinned rs-rtl 0.5.0 under rtl-rust, included by desktop. Both use pooled IQ delivery and shared RF settings. See [RTL backend evaluation](../docs/rtl-backends.md).

- 2026-10-03: Promote rs-rtl to the default following user-confirmed unplug/replug recovery and extended runtime. Remove the experimental label, preserve explicit saved choices, and retain librtlsdr as a selectable fallback and native-only build default. Supersedes the 2026-09-30 default decision; gain/sensitivity parity remains uncalibrated. See [RTL backend evaluation](../docs/rtl-backends.md).

- 2026-10-03: Add an IC-705 CI-V adapter in Rust with CPAL USB AF input and explicit separate TX output. Initial TX is a bounded test tone; no persisted arm, PTT or replay on reconnect. Hamlib remains an option for broader radio coverage. See [IC-705 integration](../docs/ic705.md).
