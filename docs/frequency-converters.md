# External frequency converters

Select **RTL-SDR USB**, then use **Converter offset** in the left controls. Values are signed Hz:

**RF frequency = RTL tuner frequency + converter offset + fine correction**

- **Ham It Up (-125 MHz)** sets -125,000,000 Hz. This follows [Nooelec's documented offset convention](https://support.nooelec.com/hc/en-us/articles/360005812474-Ham-It-Up-Upconverter).
- **None** restores zero for a direct antenna connection or converter bypass.
- Enter a positive offset for a downconverter using the corresponding additive translation. For example, +300,000,000 Hz maps 435 MHz RF to 135 MHz at the tuner.

For the connected Ham It Up and 40 m antenna, select the preset, enter **7.100000 MHz**, choose **LSB** for voice (or CW for Morse), and start reception. At a 7.350 MHz RF capture center, the hardware tunes 132.350 MHz; the VFO stays at 7.100 MHz. The sidebar shows the actual RTL tuner center so the translation can be checked. The converter must be powered and in conversion mode; use None when bypassed.

VFO entry, spectrum clicks, wheel panning, memory CSV frequencies and scan endpoints all use RF frequencies. Do not add 125 MHz to memory entries. Converter configuration belongs to the RTL device preferences and is saved automatically, independently of receiver modes.

The app intersects the translated RTL tuner range (28.8–1700 MHz) with its current RF model (1 Hz–1700 MHz). Unsupported tuning is clamped by controls or rejected by engine validation. This is a tuner limit, not a model of the converter's actual passband or the antenna response. Frequencies above 1700 MHz and converters that reverse spectrum orientation are not supported by this initial additive-offset implementation.

Changing offset cancels scanning, clears the old display and restarts active acquisition with the translated hardware tuning. Ordinary tuning within the captured bandwidth retains the existing fluid VFO behavior. IQ-file playback, QMX CAT tuning and synthetic generation do not apply this translation.

## Fine calibration

**Fine correction** is an independent signed Hz adjustment (1 Hz increments,
±100 kHz range). It is saved as `devices.rtl.correction_hz`; older preferences
default to zero. **Reset correction** restores zero without changing the nominal
converter offset. Converter presets do not erase the fine correction.

Use a known reference signal and add `known RF − displayed RF` to the current
correction. Positive values move a fixed physical signal higher on the RF scale.
For example, if an independently known 7.074000 MHz signal appears at 7.072100 MHz,
add +1900 Hz. The effective Ham It Up offset becomes −124,998,100 Hz. The app keeps
the requested RF VFO/capture center and retunes the hardware accordingly; memories
and scan ranges continue to use corrected RF frequencies. Changing correction
restarts acquisition and clears incompatible waterfall history.

A USB dial is not an individual signal's RF frequency: signal RF equals dial plus
audio frequency. FT8 activity spans multiple audio offsets, so seeing traffic
while the dial is at 7.0721 MHz does not by itself establish a calibration error.
This adjustment is a constant Hz translation, not a proportional PPM or sample-clock
correction. No nonzero correction is applied automatically.

Tests cover both offset signs, overflow/invalid mappings, HF tuning/panning, scan bounds, stopped engine acceptance and restoring HF preferences. `--gui --converter-smoke` renders a stopped 40 m/LSB view and checks the RF-to-tuner mapping without opening hardware.
