# Runs the installer the way a user's PC would, and checks what it leaves behind:
#   1. a silent install for the current user puts ShotCraft where Windows expects it, with its
#      Start menu entry, its uninstall entry and its place under Open with for pictures;
#   2. the installed program starts, shows no window, and opens a picture a second launch hands it;
#   3. installing again while it runs (an update) stops it, replaces it and starts it again;
#   4. uninstalling removes all of that, with what ShotCraft registered by itself, and keeps the
#      user's settings.
# Do not run it on a PC where ShotCraft is installed: it uninstalls it.
param([string]$Setup = (Get-ChildItem 'target\dist\ShotCraft-*-Setup.exe' | Select-Object -Last 1).FullName, [string]$Out = 'target\check-installer')
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class OpenWith {
    [ComImport, Guid("F04061AC-1659-4A3F-A954-775AA57FC083"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IAssocHandler {
        void GetName([MarshalAs(UnmanagedType.LPWStr)] out string path);
        void GetUIName([MarshalAs(UnmanagedType.LPWStr)] out string name);
    }
    [ComImport, Guid("973810AE-9599-4B88-9E4D-6EE98C9552DA"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IEnumAssocHandlers {
        [PreserveSig] int Next(int count, [MarshalAs(UnmanagedType.Interface)] out IAssocHandler handler, out int fetched);
    }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    static extern void SHAssocEnumHandlers(string extension, int filter, out IEnumAssocHandlers handlers);
    // The programs Explorer offers under Open with for a file type, by the path it starts.
    public static List<string> For(string extension) {
        var paths = new List<string>();
        IEnumAssocHandlers handlers;
        SHAssocEnumHandlers(extension, 0, out handlers);
        IAssocHandler handler;
        int fetched;
        while (handlers.Next(1, out handler, out fetched) == 0 && fetched == 1) {
            string path;
            handler.GetName(out path);
            paths.Add(path);
        }
        return paths;
    }
}
'@
if (-not $Setup -or -not (Test-Path $Setup)) { throw 'No installer: run scripts\windows\make-installer.ps1 first.' }
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$version = (Split-Path -Leaf $Setup) -replace '^ShotCraft-(.*)-Setup\.exe$', '$1'
$folder = "$env:LOCALAPPDATA\Programs\ShotCraft"
$exe = "$folder\shotcraft.exe"
$shortcut = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\ShotCraft.lnk"
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{90999C30-F67A-4490-AE8B-82287BE89D1B}_is1'
$openWithKey = 'HKCU:\Software\Classes\Applications\shotcraft.exe'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$settings = "$env:APPDATA\ShotCraft"

$failed = @()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "ok    $what" } else { Write-Host "FAIL  $what"; $script:failed += $what }
}
function Running { @(Get-Process shotcraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) }
function Install([string]$log) {
    $run = Start-Process $Setup -PassThru -Wait -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TEXTRECOGNITION=skip', "/LOG=`"$Out\$log`""
    $run.ExitCode
}

try {
    Check ((Install 'install.log') -eq 0) 'the installer runs without a question and without administrator rights'
    Check (Test-Path $exe) "shotcraft.exe is in $folder"
    Check (Test-Path "$folder\LICENSE.txt") 'the licence comes with it'
    Check (Test-Path $shortcut) 'the Start menu has ShotCraft'
    Check ((Get-ItemProperty $uninstallKey -ErrorAction SilentlyContinue).DisplayVersion -eq $version) "Windows lists ShotCraft $version under installed apps"
    Check ((Get-ItemProperty "$openWithKey\shell\open\command" -ErrorAction SilentlyContinue).'(default)' -eq "`"$exe`" `"%1`"") 'Open with starts the installed program with the picture'
    foreach ($type in '.png', '.jpg', '.webp') {
        Check ([OpenWith]::For($type) -contains $exe) "Explorer offers ShotCraft under Open with for $type"
    }
    Check ((Running).Count -eq 0) 'a silent install does not start ShotCraft'

    & "$PSScriptRoot\check-editor.ps1" -Exe $exe -Out "$Out\editor"
    Check ($LASTEXITCODE -eq 0) 'the installed program passes the editor window checks'

    # An update: ShotCraft is running, with what it registers by itself when Launch at Startup is on.
    Start-Process $exe
    Start-Sleep -Seconds 5
    $before = (Running).Id
    Check ($before.Count -eq 1) 'ShotCraft runs before the update'
    Set-ItemProperty $runKey -Name ShotCraft -Value "`"$exe`" --background"
    Check ((Install 'update.log') -eq 0) 'installing again while ShotCraft runs succeeds'
    Start-Sleep -Seconds 5
    $after = (Running).Id
    Check ($after.Count -eq 1 -and $after -ne $before) 'the update stopped the running ShotCraft and started the new one'

    $uninstaller = Get-ChildItem "$folder\unins*.exe" | Select-Object -First 1
    Check ($null -ne $uninstaller) 'the uninstaller is there'
    if ($uninstaller) {
        $run = Start-Process $uninstaller.FullName -PassThru -Wait -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
        Check ($run.ExitCode -eq 0) 'the uninstaller runs without a question'
        # It finishes from a copy of itself in the temporary folder.
        $until = (Get-Date).AddSeconds(30)
        while ((Test-Path $folder) -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 500 }
        Check ((Running).Count -eq 0) 'uninstalling stops ShotCraft'
        Check (-not (Test-Path $folder)) 'the program folder is gone'
        Check (-not (Test-Path $shortcut)) 'the Start menu entry is gone'
        Check (-not (Test-Path $uninstallKey)) 'Windows no longer lists ShotCraft'
        Check (-not (Test-Path $openWithKey)) 'Open with no longer has ShotCraft'
        Check ($null -eq (Get-ItemProperty $runKey).ShotCraft) 'the login item ShotCraft made is gone'
        Check (Test-Path "$settings\settings.json") 'the settings are kept'
    }
} finally {
    Running | Stop-Process -Force
    Remove-ItemProperty $runKey -Name ShotCraft -ErrorAction SilentlyContinue
}
if ($failed.Count) { Write-Host "$($failed.Count) check(s) failed"; exit 1 }
exit 0
