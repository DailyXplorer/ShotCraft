# Windows' own text recognition (OCR), which ShotCraft uses to read the text in a picture. The
# installer runs this script; it does nothing to ShotCraft itself.
#
#   -Check -LanguagesFile <file>   exits 0 when Windows can read text in one of the user's
#                                  languages and 3 when it cannot, and writes those languages,
#                                  the preferred one first, to <file>
#   -Install -Languages a,b        adds text recognition for the first of these languages that
#                                  Windows offers it for. Needs administrator rights and Windows
#                                  Update. Exits 0 when it is there, 4 when Windows is still
#                                  installing it after -WaitSeconds (it goes on by itself), 2 when
#                                  Windows offers none of the languages, 1 on an error
param([switch]$Check, [switch]$Install, [string]$Languages = '', [string]$LanguagesFile = '', [int]$WaitSeconds = 300)
$ErrorActionPreference = 'Stop'
try {
    if ($Check) {
        if ($LanguagesFile) {
            (Get-WinUserLanguageList).LanguageTag -join ',' | Set-Content -Encoding ASCII $LanguagesFile
        }
        [void][Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
        # What ShotCraft asks Windows for: a reader for the user's languages.
        if ([Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()) { exit 0 }
        exit 3
    }
    if ($Install) {
        $offered = @(Get-WindowsCapability -Online -Name 'Language.OCR*')
        foreach ($language in ($Languages -split ',' | Where-Object { $_ })) {
            $capability = $offered | Where-Object { $_.Name -like "Language.OCR~~~$language~*" } | Select-Object -First 1
            if (-not $capability) {
                # The same language in another region: Belgian French is read with France's.
                $capability = $offered | Where-Object { $_.Name -like "Language.OCR~~~$(($language -split '-')[0])-*" } | Select-Object -First 1
            }
            if (-not $capability) { continue }
            if ($capability.State -eq 'Installed') { exit 0 }
            # Windows Update decides how long this takes, and it can be long. The work runs in a
            # process of its own, which goes on when this script stops waiting for it.
            $work = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-NonInteractive', '-Command', "Add-WindowsCapability -Online -Name '$($capability.Name)'"
            if (-not $work.WaitForExit($WaitSeconds * 1000)) { exit 4 }
            if ((Get-WindowsCapability -Online -Name $capability.Name).State -eq 'Installed') { exit 0 }
            exit 1
        }
        exit 2
    }
    throw 'Pass -Check or -Install.'
} catch {
    [Console]::Error.WriteLine($_)
    exit 1
}
