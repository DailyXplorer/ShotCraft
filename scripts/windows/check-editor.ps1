# Checks, on a real desktop, what only a real window can show:
#   1. starting ShotCraft shows no window (it lives in the tray);
#   2. a second launch with a picture hands it to the copy that runs, which opens it in a window
#      of the picture's size under the bar;
#   3. choosing a tool makes that window taller by the options bar, and leaving it gives the
#      points back.
# -Out gets a picture of the desktop at each step. Do not touch the mouse or keyboard meanwhile.
# The display must be at 100% scaling and large enough for the window (1280x800 or more).
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
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    // The visible windows of a process that are on screen, with their titles.
    public static List<KeyValuePair<IntPtr, string>> Visible(uint process) {
        var found = new List<KeyValuePair<IntPtr, string>>();
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            RECT at;
            if (owner == process && IsWindowVisible(window) && GetWindowRect(window, out at) && at.Right > 0 && at.Bottom > 0) {
                var title = new StringBuilder(256);
                GetWindowText(window, title, title.Capacity);
                found.Add(new KeyValuePair<IntPtr, string>(window, title.ToString()));
            }
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

# A picture of 900 x 560 pixels: at 100% scaling the window opens 900 points wide.
$picture = Join-Path $Out 'picture.png'
$bitmap = New-Object System.Drawing.Bitmap 900, 560
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([System.Drawing.Color]::FromArgb(245, 246, 248))
$graphics.FillRectangle([System.Drawing.Brushes]::SteelBlue, 520, 60, 380, 200)
$bitmap.Save($picture, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose(); $bitmap.Dispose()

function Editor($app) {
    [ShotCraftWindows]::Visible($app.Id) | Where-Object { $_.Value -eq 'ShotCraft' } | Select-Object -First 1
}
function ClientSize($window) {
    $rect = New-Object ShotCraftWindows+RECT
    [void][ShotCraftWindows]::GetClientRect($window, [ref]$rect)
    "$($rect.Right) x $($rect.Bottom)"
}
function WaitFor([scriptblock]$test, [int]$seconds = 20) {
    $until = (Get-Date).AddSeconds($seconds)
    do { if (& $test) { return $true }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $until)
    $false
}
$failed = @()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "ok    $what" } else { Write-Host "FAIL  $what"; $script:failed += $what }
}

$app = Start-Process $Exe -PassThru
try {
    Start-Sleep -Seconds 6
    Check (-not $app.HasExited) 'ShotCraft keeps running after it starts'
    Check ($null -eq (Editor $app)) 'starting it shows no window'
    & $shot -Path "$Out\1-started.png"

    $second = Start-Process $Exe -PassThru -ArgumentList "`"$picture`""
    Check ($second.WaitForExit(15000)) 'a second launch with a picture quits by itself'
    Check (WaitFor { $null -ne (Editor $app) }) 'the copy that runs opens the picture in a window'
    $editor = Editor $app
    if ($editor) {
        # The window gets its size a few frames after it shows.
        [void](WaitFor { (ClientSize $editor.Key) -eq '900 x 612' } 10)
        Check ((ClientSize $editor.Key) -eq '900 x 612') "the window is the picture under its 52 point bar: $(ClientSize $editor.Key), expected 900 x 612"
        & $shot -Path "$Out\2-picture.png"

        [void][ShotCraftWindows]::SetForegroundWindow($editor.Key)
        Start-Sleep -Milliseconds 500
        [System.Windows.Forms.SendKeys]::SendWait('a')
        [void](WaitFor { (ClientSize $editor.Key) -eq '900 x 660' } 10)
        Check ((ClientSize $editor.Key) -eq '900 x 660') "the Arrow tool makes the window taller by the 48 point options bar: $(ClientSize $editor.Key), expected 900 x 660"
        & $shot -Path "$Out\3-arrow-tool.png"

        [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
        [void](WaitFor { (ClientSize $editor.Key) -eq '900 x 612' } 10)
        Check ((ClientSize $editor.Key) -eq '900 x 612') "leaving the tool gives the points back: $(ClientSize $editor.Key), expected 900 x 612"
        & $shot -Path "$Out\4-tool-left.png"
    }
} finally {
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
}
if ($failed.Count) { Write-Host "$($failed.Count) check(s) failed"; exit 1 }
