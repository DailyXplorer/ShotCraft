# Asks Windows whether it can read text (OCR) in one of the user's languages, which is what
# ShotCraft needs to read the text in a picture. The installer runs this script. It needs no
# administrator rights and changes nothing.
#
# Exits 0 when Windows can, 3 when it cannot, 1 when the question could not be asked. With
# -LanguagesFile it writes the user's languages to that file, the preferred one first.
param([string]$LanguagesFile = '')
$ErrorActionPreference = 'Stop'
try {
    if ($LanguagesFile) {
        (Get-WinUserLanguageList).LanguageTag -join ',' | Set-Content -Encoding ASCII $LanguagesFile
    }
    [void][Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
    # What ShotCraft asks Windows for: a reader for the user's languages.
    if ([Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()) { exit 0 }
    exit 3
} catch {
    [Console]::Error.WriteLine($_)
    exit 1
}
