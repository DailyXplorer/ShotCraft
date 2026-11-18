# Runs the live scrolling capture test (`live_scroll` in src/scroll.rs) on this computer: shows
# tests/live/scroll-page.html full screen in Chrome or Edge, lets ShotCraft scroll and stitch it,
# and fails when a row of the page is missing, doubled or cut. Do not touch the mouse meanwhile.
#
# -Out gets the stitched picture, the frames the capture saw and a picture of the desktop before
# and after.
param([string]$Out = 'target\live-scroll', [int]$Rows = 220)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$url = 'file:///' + ((Join-Path $root 'tests\live\scroll-page.html') -replace '\\', '/') + "?rows=$Rows"
$browser = @(
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $browser) { throw 'Neither Chrome nor Edge is installed to show the test page.' }
# A profile of its own: the page opens in a new browser, not in one that is already running.
$profile = Join-Path ([IO.Path]::GetTempPath()) "shotcraft-live-scroll-$PID"
$shown = Start-Process $browser -PassThru -ArgumentList '--kiosk', '--no-first-run', '--no-default-browser-check', "--user-data-dir=$profile", $url
$code = 1
try {
    # A browser's first start can take a while: wait for its window, which is named after the page.
    $until = (Get-Date).AddSeconds(90)
    do {
        Start-Sleep -Milliseconds 500
        $window = Get-Process (Split-Path -Leaf $browser).Replace('.exe', '') -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -like '*ShotCraft scrolling test page*' }
    } until ($window -or (Get-Date) -gt $until)
    if (-not $window) {
        & "$PSScriptRoot\desktop-shot.ps1" -Path "$Out\no-browser.png"
        throw 'The browser did not show the test page within 90 seconds: nothing was captured.'
    }
    Start-Sleep -Seconds 3
    & "$PSScriptRoot\desktop-shot.ps1" -Path "$Out\before.png"
    Add-Type -AssemblyName System.Windows.Forms
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    # From the page's left edge, where each row's strip starts, and short of its scroll bar.
    $width = [Math]::Min(900, $screen.Width - 60)
    $height = [Math]::Min(600, $screen.Height - 80)
    $env:SHOTCRAFT_LIVE_SCROLL = "0,0,$width,$height"
    $env:SHOTCRAFT_LIVE_OUT = "$Out\stitched.png"
    $env:SHOTCRAFT_LIVE_FRAMES = "$Out\frames"
    cargo test live_scroll -- --ignored --nocapture
    $code = $LASTEXITCODE
    & "$PSScriptRoot\desktop-shot.ps1" -Path "$Out\after.png"
} finally {
    taskkill /PID $shown.Id /T /F 2>&1 | Out-Null
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
}
exit $code
