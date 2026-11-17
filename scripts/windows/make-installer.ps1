# Builds the Windows installer: target\dist\ShotCraft-<version>-Setup.exe.
#
# Needs the Rust toolchain (https://rustup.rs) and Inno Setup 6 (winget install JRSoftware.InnoSetup).
# The PC that runs the installer needs neither: shotcraft.exe is a plain Windows program.
param(
    # Use the shotcraft.exe that is already in target\release.
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $SkipBuild) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo was not found. Install Rust from https://rustup.rs, then run this script again.' }
    cargo build --release
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
$exe = 'target\release\shotcraft.exe'
if (-not (Test-Path $exe)) { throw "$exe is missing: run this script without -SkipBuild." }

# The installer promises that nothing else has to be installed: shotcraft.exe may only need what
# every Windows 10 and 11 has. The C runtime is the usual stowaway (.cargo\config.toml links it in).
$studio = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$dumpbin = if (Test-Path $studio) {
    $where = & $studio -latest -products * -property installationPath
    Get-ChildItem "$where\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" -ErrorAction SilentlyContinue | Select-Object -Last 1
}
if ($dumpbin) {
    $needed = & $dumpbin.FullName /dependents $exe | Where-Object { $_ -match '^\s+\S+\.dll\s*$' } | ForEach-Object { $_.Trim() }
    $runtime = $needed | Where-Object { $_ -match '^(vcruntime|msvcp|ucrtbase|concrt|vccorlib|api-ms-win-crt)' }
    if ($runtime) { throw "shotcraft.exe needs the Visual C++ runtime ($($runtime -join ', ')): it would not start on a PC without it." }
    Write-Host "shotcraft.exe needs $($needed.Count) system libraries and no Visual C++ runtime."
} else {
    Write-Warning 'dumpbin was not found: not checked that shotcraft.exe runs without the Visual C++ runtime.'
}

$compiler = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $compiler) { throw 'Inno Setup 6 was not found. Install it (winget install JRSoftware.InnoSetup), then run this script again.' }
& $compiler /Qp "/DAppVersion=$version" packaging\windows\shotcraft.iss
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Write-Host "Built target\dist\ShotCraft-$version-Setup.exe"
