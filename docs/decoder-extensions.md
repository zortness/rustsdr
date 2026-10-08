# Decoder extensions and Desperado review

This guide is for testing the VOR prototype and adding receive-only decoders without coupling them to the GUI or a particular device. DAB remains deferred.

Follow-up: the Rust RTL-SDR backend is now the default after user validation; librtlsdr remains selectable. See [backend evaluation](rtl-backends.md) for usage, live checks and remaining validation. The hardware table below records the original review.

## Try VOR

The **VOR** toggle in the top bar opens an experimental radial display for RTL-SDR and IQ-file sources. It follows the VFO and runs independently of the listening mode; AM is useful for hearing the station. The panel reports a radial **FROM the station**, reference coherence and deviation. It does not report aircraft heading, position or distance. Closing the panel releases its IQ subscription. Its open preference persists with the other decoder preferences.

For a deterministic test, run from the project root:

```powershell
cargo run -p sdr-app --example vor_fixture
cargo run -p sdr-app --features desktop -- --gui --file target/vor-fixture.cf32 --center-hz 113250000 --sample-rate 2400000 --vor-smoke
```

This generates three seconds of IQ containing a 113.000 MHz signal at a 120-degree radial. The smoke test opens the panel, verifies the decoded radial, saves a screenshot and exits. It does not load or save user preferences or open hardware.

For live reception, select RTL-SDR, tune a known local VOR carrier and enable VOR. With the current Ham It Up arrangement, bypass the converter for VHF and select no converter offset; use an antenna appropriate for the signal. The ordinary RTL tuning path covers this band. Reception depends on station coverage and antenna placement.

The initial prototype measures the relative phase of the direct 30 Hz AM component and the FM-demodulated 9960 Hz reference subcarrier. It uses a 32 kHz IQ channel at 48 kS/s, preserving information that the voice AM filter removes. One-second windows produce measurements; gaps, retunes and unavailable channels clear acquisition. The GUI suppresses old-revision and stale results.

Limits: experimental signal analysis only, not a navigation instrument. Station Morse identification, station databases, calibrated error estimates, systematic CVOR/DVOR coverage and off-air validation remain future work. The quality gate rejects missing/implausible reference modulation and insufficient direct AM modulation; its coherence value is not an accuracy guarantee.

## Add a decoder

Obtain control handles from the engine client. The engine owns and joins FT and VOR workers; GUI handles configure services and consume results without extending worker lifetime. WAV import and live FT decoding remain available to headless callers through the digital handle.

Use `subscribe_channel(ChannelRequest)` for complex IQ, or `subscribe_audio()` for receiver audio. IQ requests can follow the VFO or select an absolute RF frequency within the capture. The initial implementation accepts 2.4 MS/s capture and 48/240 kS/s outputs, with bandwidth at most two thirds of the output rate. It does not yet offer arbitrary resampling or the 2.048 MS/s DAB path.

Each IQ channel filters on its own worker. Up to eight channel workers may coexist. Capture blocks are copied once into shared immutable storage only while subscribed. Each reader has a bounded eight-block queue; a slow reader loses its own incoming blocks and cannot block capture. Audio readers similarly share immutable frames and no longer replace earlier subscriptions. Cloning an individual event-stream handle shares its consuming queue; request another subscription for an independent reader.

IQ blocks retain capture/configuration revisions, sample positions, RF mapping and approximate host end time. Channel output positions restart with an incremented discontinuity counter on gaps or retunes. Check both that counter and sample continuity: output queue loss is also possible. Channel filter delay is not subtracted from the approximate host time. Out-of-capture requests produce an unavailable event. Releasing the channel handle terminates its worker. Engine shutdown joins all remaining workers.

Receiver audio includes mode AGC, squelch and de-emphasis; it precedes speaker volume and sound-card drift correction. Do not use it as raw FM multiplex or wide AM envelope. The existing internal WFM multiplex tap serves RDS. QMX currently supplies audio, not wideband IQ; VOR is therefore unavailable on that source.

## FM/RDS findings

The Desperado FM receiver offers useful stereo PLL, RDS parser and recorded-signal references. Our stereo implementation already has separation, pilot-loss and chunk-boundary tests, and our monitor path already corrects sound-card drift independently of decoder timing. No receiver replacement was necessary in this change.

A derived multiplex fixture from its real FIP recording now tests our RDS decoder against independently documented station identity and full RadioText. Fixture provenance, extraction method and the upstream MIT notice accompany the fixture. This tests real multiplex decoding; the existing synthetic RF tests cover our RF-to-multiplex path. Neither alone establishes performance on every off-air station.

Useful follow-ups: alternative frequencies and clock-time groups, broader stereo recordings with weak/multipath pilots, and station-identification scanning. Keep those separate from the current threshold-and-hold memory scanner.

## Hardware support lessons

Reviewed Desperado commit `589c0df8468159cd9acec3c74c34bd553bf9e7ae` (2026-09-30).

| Candidate | Opportunity | Integration requirement |
| --- | --- | --- |
| `rs-rtl` | Rust/nusb driver for R820T/R828D RTL dongles, including Blog V4 handling; potentially removes our rtlsdr DLL dependency | Side-by-side V3 tests for tuning, gain, PPM, bias tee, cancellation and unplug/reconnect before changing the default. Do not infer support for every RTL tuner or V3 direct sampling. |
| `rs-spy` | Rust/nusb Airspy R2, Mini and HF+ backend | Query model-specific rates and gains; accommodate rates beyond our current 2.4 MS/s voice path. |
| `rs-hackrf` | Rust/nusb HackRF backend | Explicit RF amplifier and LNA/VGA gain stages, rate negotiation and RX lifecycle validation. Hardware TX capability does not imply an implemented transmit path. |
| SoapySDR | One adapter can reach a wider plugin ecosystem | Optional native dependency and deployment of the correct device plugins; discover capabilities dynamically. |
| Pluto | Existing IIO-based acquisition route | Optional native/IIO transport dependencies, device discovery and network/USB lifecycle tests. |

Sources: [core features](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/desperado/Cargo.toml), [RTL driver](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-rtl/readme.md), [Airspy driver](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-spy/readme.md), [HackRF driver](https://github.com/xoolive/desperado/blob/589c0df8468159cd9acec3c74c34bd553bf9e7ae/crates/rs-hackrf/readme.md).

Adopt optional adapters inside the device layer, stable device identifiers and queried capability descriptors for supported rates, frequency ranges, antennas and gain stages. Distinguish unspecified settings from explicit automatic gain. Keep converter LO correction in Hz, tuner oscillator correction in PPM and sample-clock correction separate. A normalized IQ type is useful, but the adapter must also preserve error, gap and retune provenance.

Rust USB drivers simplify native packaging; they do not establish Android USB permission integration, iOS accessory support or browser WebUSB compatibility. Remote engine access remains the practical browser route. The original decoder work did not change drivers. The later RTL adapter evaluation is linked above; other target support must still be demonstrated per adapter. Keep QMX CAT/audio as its own capability family.

## Deferred DAB exploration

DAB/DAB+ remains a future possibility. Start with recorded ensemble/service discovery, then service selection, programme audio, text and artwork. The Desperado executable is a useful reference but needs a reusable decoder boundary; its AAC backend also needs native packaging/license review. A future IQ branch can resample 2.4 MS/s to the required 2.048 MS/s. Memories will need an optional service identifier in addition to RF frequency and mode.

DAB, DRM and HD Radio/NRSC-5 are distinct decoder projects. DAB would not decode digital services alongside ordinary FM broadcasts in HD Radio markets. No DAB dependency or discovery implementation is included here.
