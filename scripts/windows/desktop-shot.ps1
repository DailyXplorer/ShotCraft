# Saves a picture of the whole desktop as a PNG, so that what a build machine showed can be
# looked at afterwards.
param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($screen.Left, $screen.Top, 0, 0, $bitmap.Size)
    New-Item -ItemType Directory -Force (Split-Path -Parent ([IO.Path]::GetFullPath($Path))) | Out-Null
    $bitmap.Save([IO.Path]::GetFullPath($Path), [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
