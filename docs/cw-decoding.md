# CW listening and Morse decoding

Implemented 2026-09-27. Select **CW** in the mode menu, tune the VFO to the RF carrier, and choose a 200, 500, or 1000 Hz filter. The default is 500 Hz with a 100 Hz tuning step. A fixed 600 Hz beat oscillator makes the carrier audible; this pitch does not change RF tuning. Audio AGC is optional. CW memories use `cw` in the existing CSV mode column; bandwidth and AGC round-trip normally.

**Signal Data** opens the Morse decoder. Recent text also appears above the spectrum, independently of the window. Decoding is enabled by default in CW and continues while muted. Its controls are session-local, independent of RDS settings:

- **Starting WPM:** approximate character speed, 5–60 WPM. Start near the sender's speed; this is not blind speed acquisition. An isolated dash and a slow dot cannot be distinguished without timing context.
- **Track speed:** follows gradual changes using bounded updates. Disable for a known fixed rate.
- **Key threshold:** normalized channel power in dBFS, default -45 dBFS. Set above background noise and below keyed signal power. This is a manual threshold with 4 dB release hysteresis, not a calibrated S-meter or automatic noise estimator.
- **Word spacing:** increase for stretched character/word spacing. This raises the word boundary, keeping long character gaps from being mistaken for spaces. It does not automatically infer a sender's Farnsworth rate.
- **Reset decoder / clear text:** resets timing and transcript without resetting capture or audio. Changing decoder controls also resets decoding. Copy text exports the currently retained text to the clipboard.

The panel displays estimated WPM, key state, rejected marks, and unknown patterns. Unknown patterns appear as `?`; a valid decoded letter can still be wrong. No dictionary correction or callsign completion is applied. ASCII letters, digits, common punctuation, and joined prosigns are supported. Joined groups display as `<BK>`, `<AR>`, `<AS>`, `<BT>`, `<SK>`, `<KN>`, `<KA>`, `<AA>`, `<VE>`, `<HH>`, and `<SOS>`. Normally spaced letters remain letters (for example BK). AR and BT share waveforms with + and =; the decoder displays the prosign labels because timing cannot distinguish intent. SK/VA, KA/CT, and VE/SN likewise use one canonical label. HH is shown explicitly rather than deleting earlier text. Unknown and overlong groups remain ?; extended alphabets remain future work. The transcript retains the latest 4096 characters. Retuning, source/mode/filter changes, IQ gaps, and decoder disabling clear it. Normal reception stop/file EOF can retain the last snapshot for copying, labeled as stopped. An unfinished mark at EOF is not guessed or force-flushed.

## Library research and decision

Primary sources inspected on 2026-09-27; these are code/API assessments, not comparative receiver benchmarks.

| Candidate | Useful properties | Fit and limitation |
| --- | --- | --- |
| [cw-dit / cwdit-morse](https://github.com/swilcox/cw-dit) | Pure Rust streaming run-length timing, adaptive dot estimate, bootstrap speed estimation; optional mark-plus-gap classification can mitigate eroded edges. Workspace metadata declares MIT OR Apache-2.0. | Closest small Rust candidate. Inspected timing, bootstrap, and decoder source. Upstream gap classification assumes fixed unit ratios; our spacing controls and live silence finalization require an adapter or changes. Keep as a candidate for a recording-based comparison. Root LICENSE-MIT/LICENSE-APACHE fetches returned 404; clarify notice packaging before vendoring. |
| [morse-codec](https://github.com/burumdev/morse-codec) | MIT Rust `no_std` timing codec, fixed storage, multiple timing policies including Farnsworth. | Useful for key-event streams. It does not replace our RF filtering or signal detector; documentation acknowledges the initial dit/dah timing ambiguity. |
| [ditdah](https://github.com/yuvadm/ditdah) | MIT Rust audio decoder with spectral pitch detection, Goertzel filtering and calibration. | Convenient file/sample-slice API for an offline comparison. Its published accuracy claims concern its fixtures, not an independently verified corpus of hand-sent radio signals. |
| [ggmorse](https://github.com/ggerganov/ggmorse) | MIT C++ real-time audio library; automatic pitch and speed estimation, documented 200–1200 Hz and 5–55 WPM operating ranges. | Strong candidate if automatic acquisition is needed. Requires FFI/native packaging; its README notes integration/API work still needed. We have not benchmarked it against this implementation. |
| [Fldigi CW documentation](https://www.w1hkj.org/FldigiHelp/cw_page.html) | Established application reference for CW operation and resetting receive speed tracking. | Useful UX/reference implementation to compare with later; no Fldigi code was copied or linked. |

Decision: implement a small, replaceable Rust power-to-text decoder for the existing single tuned CW channel. Our IQ filter already isolates the carrier, so an audio pitch-search backend would duplicate work for this first version. No third-party decoder source was copied and no native dependency was added. Standard Morse timing/alphabet follows [ITU-R M.1677-1](https://www.itu.int/rec/R-REC-M.1677-1-200910-I/). The researched projects provide useful comparison points; this first decoder does **not** claim their full feature sets or superior performance. Reconsider cwdit-morse/ggmorse after comparing real recordings and measuring character/word error rates.

## Processing and lifecycle

`Receiver` translates the RF carrier to DC, shares the existing 2.4 MS/s → 240 kHz → 48 kHz front end, then decimates to 6 kHz for a 257-tap narrow filter. Linear interpolation returns the band-limited complex envelope to 48 kHz, where the BFO generates speaker audio. The expensive narrow filter runs at 6 kHz. Filter history lasts about 43 ms; allow at least 100 ms scan settling for narrow CW before evaluating channel power.

`Receiver::process_with_cw_power` provides explicit 48 kHz power samples before audio AGC/squelch. `CwDecoder` averages these into 1 ms observations, applies threshold hysteresis and a 3 ms debounce, then classifies marks relative to a seeded dot estimate. Dots/dashes use a 2-unit boundary; complete characters flush during silence without waiting for the next signal. Word boundaries use five units times the spacing multiplier. Adaptive updates normalize dash duration by three, reject large outliers, and move the dot estimate slowly. Extremely short marks are ignored and excessively long marks rejected. These are bounded heuristics, not probabilistic confidence or error correction.

The engine owns the decoder and publishes typed `Arc<CwStatus>` snapshots tagged by the surrounding capture/configuration state. Decoder-only edits preserve receiver/audio epochs and scan progress. Mode/tuning/gap changes reset state. UI frame cadence, speaker resampling, mute, and output-device availability do not determine decoder timing. No transmission or equipment keying is implemented.

## Reproducible listening fixture

Generate an irregularly keyed `CQ RUST SDR` IQ recording, then play it:

```powershell
cargo run -p sdr-app --example cw_fixture --offline
cargo run -p sdr-app --features desktop --offline -- --gui --file target/cw-demo.cf32 --center-hz 106950000 --sample-rate 2400000 --mode cw
```

The file has a carrier at 106.7 MHz relative to its declared capture center. This is a local mathematical fixture, not a recording of an actual station or a transmitted signal. It consumes roughly 400 MB. The existing RTL V3 normal tuner range remains unchanged; HF reception still needs the planned converter/direct-sampling support.

Automated checks:

```powershell
cargo test --workspace --all-features --offline
cargo clippy --workspace --all-targets --all-features --offline -- -D warnings
cargo build -p sdr-app --features desktop --offline
target/debug/sdr-app.exe --gui --file target/cw-demo.cf32 --center-hz 106950000 --sample-rate 2400000 --mode cw --cw-smoke
```

DSP tests cover uneven sending, gradual speed changes, non-3:1 dash weighting, brief dropouts, impulse rejection, continuous carrier, stretched spacing, unknown patterns, bounded history, incomplete final marks, 600 Hz audio, all three CW filters, nearby-carrier rejection, and decoding with audio squelched. Engine tests cover muted playback, reset without audio discontinuity, disabling, retuning, switching modes, and CSV round trips. The GUI smoke exercises the inline ticker and actual clicks for opening Signal Data, clearing, disabling, and restarting decoding.

Still needed: off-air hand-keyed recordings with independently transcribed text; fading/noise/SNR sweeps; abrupt speed changes and poor initial-speed recovery; automatic threshold estimation; independent dit/dah/gap clusters; pitch adjustment and CW reverse; a tuning aid at narrow zoom. The present test evidence establishes deterministic behavior on generated cases, not reliable copy of arbitrary human Morse.

## Prosign follow-up (2026-09-28)

A user-reported joined BK exposed the former six-element limit. The decoder now accepts up to nine elements (including SOS) and preserves an overflow flag so noise cannot decode as a valid prefix. Prosign tokens remain intact when old transcript content is discarded. Tests cover jittered joined versus spaced BK, common groups, punctuation ambiguity, invalid long groups, bounded history, and BK through IQ filtering at all CW bandwidths. Both practice and live CW use this decoder. This addresses alphabet coverage; it does not establish off-air weak-signal performance.

Reference conventions: [Hamfesters CW operating aids](https://www.hamfesters.org/main/wp-content/uploads/2021/02/Learning-Morse-Code-v2021.02-B-1415-GDR.pdf) and [Fldigi prosign presentation](https://www.w1hkj.org/FldigiHelp/cw_page.html). Joined BK is supported as requested; sending conventions vary, and the decoder also accepts separately spaced B and K.
