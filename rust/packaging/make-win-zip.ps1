# Builds ElkAccountEditor-win-x64.zip from the release binaries.
#   powershell -File rust/packaging/make-win-zip.ps1 -Exe rust/app/target/release/elkaccounteditor.exe [-Cli rust/cli/target/release/elk.exe] [-OutDir dist]
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Cli = "",
    [string]$OutDir = "dist",
    [string]$Name = "ElkAccountEditor-win-x64"
)
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
# Stage in a temp folder, never inside $OutDir: people run the app from an unzipped copy in dist, and its "ElkEditor data" library folder lives next to the exe.
$stageRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("elk-stage-" + [guid]::NewGuid().ToString("N"))
$stage = Join-Path $stageRoot $Name
New-Item -ItemType Directory -Force $OutDir | Out-Null
$zip = Join-Path $OutDir "$Name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $Exe (Join-Path $stage "ElkAccountEditor.exe")
if ($Cli -ne "") { Copy-Item $Cli (Join-Path $stage "elk.exe") }
# Windows line endings so Notepad shows the README properly
$text = (Get-Content -Raw (Join-Path $here "README-windows.txt")) -replace "`r?`n", "`r`n"
[System.IO.File]::WriteAllText((Join-Path $stage "README.txt"), $text, (New-Object System.Text.UTF8Encoding($false)))
Compress-Archive -Path $stage -DestinationPath $zip -Force
Remove-Item -Recurse -Force $stageRoot -ErrorAction SilentlyContinue
Write-Host "Done: $zip"
