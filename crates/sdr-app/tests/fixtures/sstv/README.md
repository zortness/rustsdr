# SSTV spike fixtures

`generate.py` creates original RGB gradient artwork and WAVs using PySSTV 0.5.7
and Pillow 11.3.0. No third-party artwork or recordings are redistributed.
PySSTV is an independent encoder, installed only for development; it is not an
application dependency. The generated files and SHA-256 manifest are under
`target/sstv-spike`. Audio quantization/dither can affect exact file hashes.

The opt-in Rust interoperability test fails if fixtures are missing. It is
explicitly ignored by ordinary offline tests and must be invoked with
`--include-ignored` after generation. Normal tests generate their own audio.

`check_encoder.py` tests our probe's WAVs using the independent colaclanth/sstv
decoder. It also decodes the PySSTV fixtures through that decoder as controls.
The pinned reference environment is recorded in `requirements-reference.txt`.
It intentionally fails its quality gate at present: three modes have similar
reference-decoder errors for both encoders, and PD120 is unsupported by that
reference. See the [spike report](../../../../../docs/sstv-spike.md).
