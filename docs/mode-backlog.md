# Potential receive modes and decoders

Updated 2026-09-27. This is a candidate backlog, not a promise to implement every entry. WFM mono/stereo, AM, NFM, USB, LSB, CW listening, and an initial Morse decoder are implemented. See [CW research and limits](cw-decoding.md). The user asked to include SSB now and design for CW, FT8, digital voice, and other future signals. Transmission remains a separate future capability; selecting a receiver mode must never key equipment.

| Candidate | Input and proposed experience | Prerequisites / acceptance evidence |
| --- | --- | --- |
| CW extensions / CW reverse | CW listening has 200/500/1000 Hz filters and a fixed 600 Hz tone; add adjustable pitch and reverse | Pitch must not retune RF; tuning aids and additional interference fixtures |
| Morse decoder improvements | Initial seeded adaptive decoder implemented; automatic acquisition and thresholding remain candidates | Compare cwdit-morse and ggmorse using fading/noisy and hand-keyed recordings; measure errors without hiding unknowns |
| FT8 / FT4 receive improvements | Initial mfsk-core decoder, UTC slots, message window, compact tabs and static reference tests implemented; see [FT decoding](ft-decoding.md) | Live validation and sensitivity comparison, hash context, deeper search, precise capture timestamps and logging |
| JS8 | Separate protocol backend, speed variants and conversation reassembly in the digital text view | JS8Call-compatible reference recordings and framing tests; FT8 payload decoding is insufficient |
| WSPR / FST4 / other weak-signal modes | Additional protocols behind the digital service; protocol-specific slot lengths and presentation | Independent fixtures and bounded CPU/memory budgets before exposing each mode |
| RDS/RBDS extensions | Basic PS/RT/PI/PTY/traffic decoding is implemented; see [FM data](fm-broadcast-data.md) | Full character mapping, regional labels, RT+, CT/AF/EON/ODA, recorded weak-signal corpus and adaptive recovery |
| HD Radio / NRSC-5 | Dedicated wide complex-IQ path; digital programs, audio, and metadata | Backend/codec and packaging feasibility; recorded corpus; independent of the analog WFM audio path |
| Subsidiary FM subcarriers / SCA | Multiplex tap; service-specific audio or data presentation | Identify actual service and required filters; fixtures before promising text or audio |
| CTCSS / DCS | NFM channel tap; detected code and optional tone squelch | Tone/code fixtures, false-open tests, acquisition and release timing |
| Digital voice: DMR, D-Star, System Fusion/C4FM, P25, NXDN | Protocol-specific baseband/framing workers; call metadata and optional decoded audio | Prioritize one protocol first. Review vocoder availability, licensing, native-library packaging, CPU budget, and public test recordings. Frame decoding and intelligible voice are separate acceptance gates; encrypted voice is outside this backlog |
| Additional digital voice: FreeDV, M17 | Dedicated decoder adapter and audio route | Evaluate reference implementations, licenses, rates, and Windows support; compare known recordings before integration |
| RTTY / PSK31 and related text | Audio or complex channel tap; text and tuning aid | Synchronization and character framing; tune/error tolerance and noisy fixtures |
| Packet / AX.25 / APRS | Protocol-specific demodulation and packet panel | Framing/checksums; distinguish decoded packets from application interpretation |
| AIS / ADS-B and other telemetry | Protocol-specific complex channel; structured message panel | Correct RF coverage, signal bandwidth/rate, checksums, and recorded corpus; do not force through voice audio |
| DRM / DAB | Deferred wideband digital broadcast exploration | DAB ensemble/service discovery first when resumed; 2.048 MS/s branch, codec packaging, service IDs and resource measurements. See [Desperado review](decoder-extensions.md). |
| VOR | Experimental IQ-to-radial decoder implemented | Live validation, station Morse identification, station database and wider CVOR/DVOR corpus remain. |
| SAM / selectable sideband AM | Carrier-tracking AM detector | Tracking and fading fixtures; compare with existing envelope AM |

Basic RDS/RBDS decoding and its separate results panel are implemented. Next, validate more recorded signals and prioritize service extensions. Converter-aware tuning is implemented; live AM validation remains. Keep V3 direct sampling independent from CW decoder improvements. See [modular panels and broadcast services](receiver-panels.md). Validate live FT8/FT4 using the implemented backend and reference corpus. Choose digital voice scope after identifying actual equipment, signals, and a usable vocoder implementation.

## Integration contract

Keep `ReceiverConfig` and `Receiver::process` as the built-in listening boundary. Do not add protocol strings to the device adapter. Add optional consumers at explicit channel IQ, discriminator/multiplex, or fixed-rate audio taps, before monitor AGC/squelch, volume, mute, and sound-card drift correction as required by the decoder. The optional WFM multiplex tap and pre-monitor receiver audio subscription are implemented; bounded channel-IQ subscriptions are implemented for 2.4 MS/s captures with 48/240 kS/s channel output. FT and VOR worker ownership is in the engine. Arbitrary sample-rate negotiation and generalized UTC uncertainty remain follow-ups.

Each future block/job must carry source identity, capture/configuration generation, sample index, rate, RF mapping, and discontinuity status. Timed modes additionally need a UTC mapping with uncertainty. Reject stale results after retune or mode changes. A gap invalidates the affected window rather than joining samples across it. Raw cf32 files currently lack UTC metadata; extending the recording format is part of timed-decoder work.

Run expensive decoders on bounded workers with explicit dropped-window reporting. Decoder failures cannot block acquisition or the waterfall. Prefer Rust application code; native libraries or isolated processes are allowed behind versioned adapters after license, packaging, and resource review. FT8/FT4 use mfsk-core; VOR currently uses a small Rust prototype with recorded Desperado regression fixtures. Other backends remain unselected. Decoder output is a typed event stream, separate from speaker frames and future TX requests.

See [receiver modes](receiver-modes.md) for CW/FT8 details and primary references, and [architecture](architecture.md) for separate RX, TX, and radio-control capabilities. The user completed a 30-minute manual multi-mode stability test; AM and instrumented performance measurements remain outstanding.

