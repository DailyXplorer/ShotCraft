# Takes pictures of the installer's pages on a real desktop, to look at: the welcome page, the
# options, the question about Windows' text recognition and the last page, in English and in
# French. It installs ShotCraft by pressing the wizard's buttons, then removes it again.
#
# For a build machine only: it makes German the account's only language for a moment (a language
# Windows reads no text in there, so that the installer asks its question), and it uninstalls
# ShotCraft. Do not touch the mouse or keyboard meanwhile.
param([string]$Setup = (Get-ChildItem 'target\dist\ShotCraft-*-Setup.exe' | Select-Object -Last 1).FullName, [string]$Out = 'target\installer-pictures')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Front {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetLastActivePopup(IntPtr window);
}
'@
if (-not $Setup -or -not (Test-Path $Setup)) { throw 'No installer: run scripts\windows\make-installer.ps1 first.' }
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$exe = "$env:LOCALAPPDATA\Programs\ShotCraft\shotcraft.exe"

# The installer's window belongs to the helper it unpacks, not to the .exe that was started.
function Wizard { Get-Process | Where-Object { $_.ProcessName -like 'ShotCraft-*-Setup*' -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 }
function Page([string]$name, [int]$seconds = 3) {
    Start-Sleep -Seconds $seconds
    & "$PSScriptRoot\desktop-shot.ps1" -Path "$Out\$name.png"
}
function Press([string]$keys) {
    $window = Wizard
    # A question the wizard asks is a window of its own, and the wizard takes no keys meanwhile.
    if ($window) { [void][Front]::SetForegroundWindow([Front]::GetLastActivePopup($window.MainWindowHandle)) }
    Start-Sleep -Milliseconds 400
    [System.Windows.Forms.SendKeys]::SendWait($keys)
}

# The account's languages, as a comma-separated list. Windows PowerShell has these commands;
# PowerShell 7 only borrows them.
function Languages { powershell.exe -NoProfile -Command "(Get-WinUserLanguageList).LanguageTag -join ','" }
function SetLanguages([string]$tags) {
    powershell.exe -NoProfile -Command "Set-WinUserLanguageList $tags -Force" | Out-Null
    if ($LASTEXITCODE -or (Languages) -ne $tags) { throw "The account's languages could not be set to $tags. Set them in Settings > Time & language > Language & region." }
}
$languages = Languages
if ($LASTEXITCODE -or -not $languages) { throw "The account's languages could not be read: nothing was changed." }
try {
    foreach ($language in 'en', 'fr') {
        # The French run meets a PC whose language Windows cannot read, and answers no.
        $asks = $language -eq 'fr'
        if ($asks) { SetLanguages 'de-DE' }
        $wizard = Start-Process $Setup -PassThru -ArgumentList "/LANG=$language", $(if ($asks) { '/TEXTRECOGNITION=ask' } else { '/TEXTRECOGNITION=skip' })
        Page "$language-1-welcome" 6
        Press '{ENTER}'
        Page "$language-2-options"
        Press '{ENTER}'
        if ($asks) {
            Page "$language-3-text-recognition" 12
            # To the No button, then Enter: a letter would depend on the language of Windows.
            Press '{RIGHT}{ENTER}'
        }
        Page "$language-4-finished" 12
        Press '{ENTER}'
        [void]$wizard.WaitForExit(30000)
        Get-Process shotcraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process -Force
    }
} finally {
    Get-Process | Where-Object { $_.ProcessName -like 'ShotCraft-*-Setup*' } | Stop-Process -Force -ErrorAction SilentlyContinue
    $uninstaller = Get-ChildItem "$env:LOCALAPPDATA\Programs\ShotCraft\unins*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($uninstaller) {
        Get-Process shotcraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process -Force
        $remove = Start-Process $uninstaller.FullName -PassThru -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
        [void]$remove.WaitForExit(60000)
    }
    # Last, and it throws when the account does not get its languages back.
    SetLanguages $languages
}
