# Desperado regression fixtures

Source: https://github.com/xoolive/desperado at commit
`589c0df8468159cd9acec3c74c34bd553bf9e7ae`. Upstream MIT notice is in LICENSE.txt.

- `klo.f32`: first floor(3 * 1800000/38) samples of voracious's
  `gqrx_20250925_144051_114647000_1800000_fc_audio.f32`.
- `arl.f32`: same-length excerpt of
  `gqrx_20251107_182558_116000000_1800000_fc_audio.f32`.
  Both are headerless little-endian f32 wide AM envelope at exactly
  1800000/38 samples/s. Upstream radial ranges: KLO 115–125 degrees;
  ARL 110–120 degrees. These are downstream recordings, not raw IQ.
- `fip-mpx.f32`: six seconds of headerless little-endian f32 normalized FM
  multiplex at 240000 samples/s. Derived from fmradio's
  `rtlsdr_20210215_103300000_1102500_cu8.bin`: center 103.3 MHz,
  input 1102500 samples/s, selected station FIP at 103.5 MHz.
  `extract-fm.cjs` reproduces the frequency shift, 161-tap windowed-sinc
  resampling and FM discriminator. Run it from the repository root with
  the upstream capture path as its argument; it overwrites the derived fixture.

Independent expected values are from upstream receiver tests: PI F204,
trimmed PS `F I P`, RadioText `MOON RIVER - MELODY GARDOT (2020) - FIP`.
No executable decoder code from Desperado is linked into the application.
