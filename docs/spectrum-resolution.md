# Automatic spectrum resolution

For a narrow signal, choose its receiver mode and click **Fit receiver** in the Display controls. USB/LSB views include the correct side of the dial frequency. For FT8 monitoring at 7.074 MHz, select USB and Fit receiver. This improves the signal display; it does not add an FT8 decoder.

**Span kHz** selects the visible width from 500 Hz to 2.4 MHz on a logarithmic slider. **Full span** returns to the complete capture. Narrow views follow the receiver passband (the test tone for synthetic input), constrained to the captured frequencies. Existing click/drag tuning, wheel center tuning and Shift+wheel VFO tuning remain available. Dragging uses the frequency axis at drag start so following the VFO cannot cause stationary-pointer drift.

The span is saved with workspace preferences. Resolution is automatic:

- The readout shows actual FFT bin spacing, approximate Hann-window equivalent noise bandwidth (RBW), and observation duration.
- An ordinary 2.7 kHz USB/LSB receiver fits into a 3.78 kHz view. At 2.4 MS/s capture, this yields approximately **2.29 Hz/bin**, **3.43 Hz Hann ENBW**, and a **437 ms observation**.
- After changing span, new fine-resolution data requires a fresh observation window. The interface indicates this wait; audio continues. FFT overlap provides roughly 9–30 analysis updates per second, depending on span.
- Bin spacing is not a guarantee of resolving arbitrarily close signals. Window shape, signal strength differences, observation duration and screen pixel width all matter. Smoothing or interpolation does not create additional signal detail.

The waterfall retains 256 rows with each row's original center and span. New rows are reduced to 1,024 columns **after selecting the visible frequency span**. Older coarse rows stay coarse when enlarged; areas outside a row's recorded coverage are marked as unavailable. Rows from different resolution settings can represent different time intervals, so history is no longer described as a fixed number of seconds. Spectrum and waterfall levels remain coherent-tone dBFS rather than noise power spectral density; a narrower analysis bandwidth can lower the displayed noise floor.

## Implementation

`sdr-dsp::zoom_spectrum::ZoomSpectrum` translates the requested view to baseband with a continuous oscillator, applies cascaded 63-tap Blackman-windowed half-band FIR decimators, and computes overlapping Hann FFTs. The visible region stays within the flat central portion of the decimation filters. Transform sizes are capped at 4,096 samples and adapt downward at low analysis rates to keep observation duration at or below half a second for supported GUI spans. Storage is bounded and reused across input blocks.

`Command::SpectrumView` updates a coalesced display preference without increasing the radio revision. The worker reconfigures only the analyzer for a display-only change. Capture, receiver filters, audio sample clock and scanning are preserved. Capture retunes and IQ discontinuities reset analysis state to avoid combining unrelated samples. This analysis currently runs on the existing engine worker; it is not a separate thread. Perfetto includes `sdr.dsp.zoom_fft` spans for its processing cost.

Spectrum frames carry their requested view and actual analysis sample rate. The GUI checks view provenance, projects data in frequency coordinates and keeps native frequency coverage for historical rows. `sdr-app::spectrum_view::project` performs peak-preserving reduction without inventing historical detail.

Automatic zoom analysis applies to RTL-SDR, IQ-file playback and synthetic input. QMX's existing USB audio spectrum view remains unchanged; it is not a wideband IQ capture. The converter offset remains independent of display analysis.

## Verification

DSP tests distinguish 20 Hz-separated tones in an offset view, reject a would-be decimation alias by more than 60 dB, and verify input chunk boundaries do not change the result. Engine testing changes resolution during file playback and asserts uninterrupted audio sample indices, unchanged capture/revision/discontinuity, and finer delivered FFT bins. GUI tests cover USB/LSB/CW passband fitting and source changes after a variable-size FFT.

Run `cargo run -p sdr-app --features desktop -- --gui --zoom-smoke` for a synthetic GUI test. It clicks the actual Fit receiver button, checks the refined resolution and capture continuity, saves `target/zoom-smoke.png`, then restores full span and exits. It bypasses saved preferences and does not open hardware or audio playback.
