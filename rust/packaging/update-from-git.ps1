# Updates the Rust editor from git and rebuilds it (the "plain git" way; the in-app "Check for updates" needs a public release instead).
#   powershell -File rust\packaging\update-from-git.ps1 [-Branch master] [-NoBuild] [-Run]
# Needs git and the Rust toolchain (rustup). It never touches your accounts library: a build from git keeps its data in
# "<repo>\ElkEditor data" when started with -Run (the folder next to a cargo build output would be deleted by `cargo clean`).
param(
    [string]$Branch = "master",
    [switch]$NoBuild,
    [switch]$Run
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$env:Path += ";$env:USERPROFILE\.cargo\bin"
Set-Location $repo
if (git status --porcelain --untracked-files=no) {
    throw "There are uncommitted changes in $repo. Commit or stash them first (nothing was changed)."
}
git fetch origin
git checkout $Branch
git pull --ff-only origin $Branch
if ($NoBuild) { Write-Host "Pulled. Skipped the build."; exit 0 }
$exe = Join-Path $repo "rust\app\target\release\elkaccounteditor.exe"
if (Test-Path $exe) {
    # a running copy blocks the build: move it aside instead of ending the program
    $running = Get-Process elkaccounteditor -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
    if ($running) { Move-Item -Force $exe (Join-Path (Split-Path $exe) "elkaccounteditor-running.old.exe") }
}
Push-Location (Join-Path $repo "rust\app"); cargo build --release; Pop-Location
Push-Location (Join-Path $repo "rust\cli"); cargo build --release; Pop-Location
Write-Host "Built: $exe"
if ($Run) {
    $data = if ($env:ELKEDITOR_DATA) { $env:ELKEDITOR_DATA } else { Join-Path $repo "ElkEditor data" }
    Start-Process $exe -ArgumentList @("--data-dir", "`"$data`"")
}
