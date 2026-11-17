# Checks, on a real desktop, what only a real window can show:
#   1. starting ShotCraft shows no window (it lives in the tray);
#   2. a second launch with a picture hands it to the copy that runs, which opens it in a window
#      of the picture's size under the bar;
#   3. choosing a tool makes that window taller by the options bar, and leaving it gives the
#      points back;
#   4. a picture that opens while a tool shows its options, or while the window is maximized or
#      minimized, gets a window of its own size too.
# -Out gets a picture of the desktop at each step. Do not touch the mouse or keyboard meanwhile.
# The display must be at 100% scaling and 1280 x 800 or larger.
param([string]$Exe = 'target\debug\shotcraft.exe', [string]$Out = 'target\check-editor')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class ShotCraftWindows {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    delegate bool EnumProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int max);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int how);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr window);
    public static string ClientSize(IntPtr window) {
        RECT rect;
        GetClientRect(window, out rect);
        return rect.Right + " x " + rect.Bottom;
    }
    // The visible windows of a process that have this title.
    public static List<IntPtr> Titled(uint process, string wanted) {
        var found = new List<IntPtr>();
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            var title = new StringBuilder(256);
            GetWindowText(window, title, title.Capacity);
            if (owner == process && IsWindowVisible(window) && title.ToString() == wanted) found.Add(window);
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Exe = (Resolve-Path $Exe).Path
$shot = Join-Path $PSScriptRoot 'desktop-shot.ps1'

# At 100% scaling a picture of 900 x 560 pixels opens in a window 900 points wide.
function Picture([string]$name, [int]$width, [int]$height) {
    $path = Join-Path $Out $name
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.Clear([System.Drawing.Color]::FromArgb(245, 246, 248))
    $graphics.FillRectangle([System.Drawing.Brushes]::SteelBlue, [int]($width * 0.58), 60, [int]($width * 0.42), 200)
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    $path
}
$picture = Picture 'picture.png' 900 560
$wide = Picture 'wide.png' 1000 500

# The editor, not the 1 x 1 window of the same name that keeps ShotCraft awake while it is hidden.
function Editor {
    [ShotCraftWindows]::Titled($app.Id, 'ShotCraft') | Where-Object { [ShotCraftWindows]::ClientSize($_) -ne '1 x 1' -and [ShotCraftWindows]::ClientSize($_) -ne '0 x 0' -or [ShotCraftWindows]::IsIconic($_) } | Select-Object -First 1
}
function WaitFor([scriptblock]$test, [int]$seconds = 15) {
    $until = (Get-Date).AddSeconds($seconds)
    do { if (& $test) { return $true }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $until)
    $false
}
$failed = @()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "ok    $what" } else { Write-Host "FAIL  $what"; $script:failed += $what }
}
function Sized([string]$size, [string]$what) {
    [void](WaitFor { $editor = Editor; $editor -and [ShotCraftWindows]::ClientSize($editor) -eq $size })
    $editor = Editor
    $now = if ($editor) { [ShotCraftWindows]::ClientSize($editor) } else { 'no window' }
    Check ($now -eq $size) "${what}: $now, expected $size"
}
function Open([string]$file) {
    $launch = Start-Process $Exe -PassThru -ArgumentList "`"$file`""
    $launch.WaitForExit(15000)
}
function Press([string]$keys) {
    [void][ShotCraftWindows]::SetForegroundWindow((Editor))
    Start-Sleep -Milliseconds 500
    [System.Windows.Forms.SendKeys]::SendWait($keys)
}

$app = Start-Process $Exe -PassThru
try {
    Start-Sleep -Seconds 6
    Check (-not $app.HasExited) 'ShotCraft keeps running after it starts'
    Check ($null -eq (Editor)) 'starting it shows no window'
    & $shot -Path "$Out\1-started.png"

    Check (Open $picture) 'a second launch with a picture quits by itself'
    Sized '900 x 612' 'the copy that runs opens the picture in a window of its size under the 52 point bar'
    & $shot -Path "$Out\2-picture.png"

    Press 'a'
    Sized '900 x 660' 'the Arrow tool makes the window taller by the 48 point options bar'
    & $shot -Path "$Out\3-arrow-tool.png"
    Press '{ESC}'
    Sized '900 x 612' 'leaving the tool gives the points back'

    Press 'a'
    Sized '900 x 660' 'the tool is back'
    [void](Open $wide)
    Sized '1000 x 600' 'a picture that opens while a tool shows its options gets its size and the bar'
    & $shot -Path "$Out\4-wide-with-tool.png"

    [void][ShotCraftWindows]::ShowWindow((Editor), 3)
    Start-Sleep -Seconds 1
    Check ([ShotCraftWindows]::IsZoomed((Editor))) 'the window is maximized for the next step'
    [void](Open $picture)
    Sized '900 x 660' 'a maximized window takes the size of the next picture'

    [void][ShotCraftWindows]::ShowWindow((Editor), 6)
    Start-Sleep -Seconds 1
    Check ([ShotCraftWindows]::IsIconic((Editor))) 'the window is minimized for the next step'
    [void](Open $wide)
    Sized '1000 x 600' 'a minimized window comes back for the next picture'
    & $shot -Path "$Out\5-back-from-minimized.png"
} finally {
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
}
if ($failed.Count) { Write-Host "$($failed.Count) check(s) failed"; exit 1 }
exit 0
