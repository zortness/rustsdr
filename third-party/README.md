# RTL-SDR native runtime

The FT8/FT4 decoder uses the Cargo dependency `mfsk-core = 0.11.0`,
GPL-3.0-or-later. See [selection and distribution implications](../docs/ft-decoding.md)
and the separately preserved [reference audio license](../crates/sdr-app/tests/fixtures/digital/LICENSE).

RustSDR's Windows x64 build needs a 64-bit `rtlsdr.dll` and its dependent DLLs. The user-supplied `third-party/rtlsdr.dll` is x86 and is preserved unchanged. The working development runtime is separate:

`third-party/rtl-sdr-blog-v1.4.0/runtime/x64/rtlsdr.dll`

Source: official [RTL-SDR Blog V1.4.0 release](https://github.com/rtlsdrblog/rtl-sdr-blog/releases/tag/V1.4.0), asset [Release.zip](https://github.com/rtlsdrblog/rtl-sdr-blog/releases/download/V1.4.0/Release.zip).

Downloaded archive SHA-256, verified during setup:

```text
7ef33f1304647f65e5e0fde43637a73d54f076e91e651a3cecc4f55a17fd9815
```

To reproduce, download that asset, verify its hash with `Get-FileHash -Algorithm SHA256`, and extract it into `third-party/rtl-sdr-blog-v1.4.0/runtime/`, retaining the archive's `x64` folder and companion DLLs (`pthreadVC2.dll`, `msvcr100.dll`). Native binaries/archives are ignored by version control. This records development provenance; check upstream and dependency redistribution terms before packaging a release.

Override runtime discovery with an absolute library path:

```powershell
$env:RUSTSDR_RTLSDR_DLL = 'C:\path\to\x64\rtlsdr.dll'
cargo run -p sdr-app --features gui -- --gui --rtl
```

The override takes precedence; otherwise the downloaded x64 path is preferred, followed by `third-party/rtlsdr.dll`. Windows resolves dependent DLLs beside the selected library and in standard loader locations. The runtime executes native code; use a known source.

The RTL-SDR v3's Interface 0 must use WinUSB. That binding is already present on the development machine. If device opening fails, close other SDR applications, check the USB connection and driver, then stop/start reception. A missing DLL or architecture mismatch appears as an application error; synthetic mode remains available.
