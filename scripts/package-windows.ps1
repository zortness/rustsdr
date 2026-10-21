param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$runtime = Join-Path $workspace 'third-party/rtl-sdr-blog-v1.4.0/runtime/x64'
$executable = Join-Path $workspace 'target/release/sdr-app.exe'
$versionMatch = Select-String -Path (Join-Path $workspace 'Cargo.toml') -Pattern '^version = "([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) { throw 'Cannot read workspace version from Cargo.toml' }
$version = $versionMatch.Matches[0].Groups[1].Value

if (-not $SkipBuild) {
    Push-Location $workspace
    try {
        cargo build -p sdr-app --release --features desktop --offline
        if ($LASTEXITCODE -ne 0) { throw "Release build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Missing build: $executable" }
$files = @('rtlsdr.dll', 'pthreadVC2.dll', 'msvcr100.dll')
foreach ($name in $files) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtime $name) -PathType Leaf)) {
        throw "Missing native runtime file: $name. See third-party/README.md."
    }
}

$dist = Join-Path $workspace 'target/dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$stage = Join-Path $dist "rustsdr-$version-windows-x64-$stamp"
if (Test-Path -LiteralPath $stage) { throw "Package path already exists: $stage" }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item -LiteralPath $executable -Destination $stage
foreach ($name in $files) { Copy-Item -LiteralPath (Join-Path $runtime $name) -Destination $stage }
@'
@echo off
set WGPU_BACKEND=dx12
cd /d "%~dp0"
start "" "%~dp0sdr-app.exe" --gui
'@ | Set-Content -LiteralPath (Join-Path $stage 'Launch-RustSDR.cmd') -Encoding ASCII
@'
RustSDR Windows x64 local acceptance build

Requirements: Windows 11, a supported graphics driver, an RTL-SDR with WinUSB on interface 0,
and a working default audio output device. Keep the files in this folder together.

Double-click Launch-RustSDR.cmd to start with the Direct3D 12 renderer.
For command-line options, from PowerShell in this folder:
  $env:WGPU_BACKEND = 'dx12'
  .\sdr-app.exe --gui --rtl

The bundled rtlsdr.dll is used with --rtl-backend librtlsdr. The default rs-rtl backend
also supports the RTL-SDR v3. Choose source and audio controls in the application.
If the radio is missing or busy, reconnect it, close other SDR applications, and use
Stop reception / Start reception. To try the interface without a radio, run:
  .\sdr-app.exe --gui

Decoded WAV recordings are made through the Reception controls. Choose a writable .wav
path outside this folder if it is installed in a protected directory.

Native runtime provenance: RTL-SDR Blog V1.4.0 Release.zip. See the project's
third-party/README.md and docs/milestone-03.md for source and acceptance limits.
This folder is a local acceptance candidate, not a published release.
'@ | Set-Content -LiteralPath (Join-Path $stage 'START-HERE.txt') -Encoding UTF8

$archive = "$stage.zip"
if (Test-Path -LiteralPath $archive) { throw "Archive already exists: $archive" }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -CompressionLevel Optimal
Write-Output "Staged $stage"
Write-Output "Archived $archive"
