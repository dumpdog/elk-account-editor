# Regenerates tests\fixtures\expected\* from the C# editor (rust\original-automation-build\ElkAccountEditor.dll).
# For every scenario in scenarios.json it starts the Rust simulated panel (examples\sim_panel.exe, plain mode) and a fresh C# editor, plays the steps against
# the editor and saves status + body of each response (and the backup file a step names). Backups the C# editor writes into Documents\ElkEditor backups are
# removed again afterwards. Takes about three minutes (the C# retries a refused connection for 25 s and enrollment waits 12 s).
#
#   powershell -File regen.ps1 [-Only scenario_name]
param([string]$Only = "")
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Net.Http
$env:Path += ";$env:USERPROFILE\.cargo\bin"
$root = (Resolve-Path "$PSScriptRoot\..\..\..\..\..").Path
$crate = "$root\rust\crates\elk-panel"
$fx = "$crate\tests\fixtures"
$exp = "$fx\expected"
$env:CARGO_TARGET_DIR = "$root\rust\target-agent-auto"
$started = Get-Date

$ErrorActionPreference = "Continue"
Push-Location $crate
cargo run --quiet --example gen_requests
cargo build --quiet --examples
Pop-Location
$ErrorActionPreference = "Stop"
$sim = "$env:CARGO_TARGET_DIR\debug\examples\sim_panel.exe"
New-Item -ItemType Directory -Force $exp | Out-Null
$http = New-Object System.Net.Http.HttpClient
$http.Timeout = [TimeSpan]::FromSeconds(300)
$docs = [Environment]::GetFolderPath("MyDocuments")
$backups = Join-Path $docs "ElkEditor backups"
$manifest = Get-Content "$fx\scenarios.json" -Raw | ConvertFrom-Json
$apiPort = 52500

function Send($base, $method, $path, [byte[]]$body, $ctype) {
    if ($method -eq "GET") { $r = $http.GetAsync("$base$path").Result }
    else {
        $c = New-Object System.Net.Http.ByteArrayContent(,$body)
        $c.Headers.ContentType = New-Object System.Net.Http.Headers.MediaTypeHeaderValue($ctype)
        $r = $http.PostAsync("$base$path", $c).Result
    }
    return @{ status = [int]$r.StatusCode; body = $r.Content.ReadAsByteArrayAsync().Result; type = "$($r.Content.Headers.ContentType)" }
}

foreach ($sc in $manifest.scenarios) {
    if ($Only -and $sc.name -ne $Only) { continue }
    "== $($sc.name)"
    Get-ChildItem $exp -Filter "$($sc.name)__*" -ErrorAction SilentlyContinue | Remove-Item -Force
    $samplePath = if ($sc.sample -eq "@template") { "@template" } else { "$root\sample\$($sc.sample)" }
    $outFile = [IO.Path]::GetTempFileName()
    $simProc = Start-Process $sim -ArgumentList @($sc.setup, "`"$samplePath`"") -RedirectStandardOutput $outFile -PassThru -WindowStyle Hidden
    $panelPort = $null
    for ($i = 0; $i -lt 100 -and -not $panelPort; $i++) {
        Start-Sleep -Milliseconds 100
        $line = Get-Content $outFile -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($line -match "^PORT (\d+)") { $panelPort = $Matches[1] }
    }
    if (-not $panelPort) { throw "simulated panel did not start" }
    $apiPort++
    $env:PORT = $apiPort
    $srv = Start-Process dotnet -ArgumentList @("$root\rust\original-automation-build\ElkAccountEditor.dll", "--browser", "--no-open") -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 4
    $base = "http://127.0.0.1:$apiPort"
    $last = ""
    $made = @()
    try {
        foreach ($st in $sc.steps) {
            $tag = "$($sc.name)__$($st.id)"
            if ($st.kind -eq "open") {
                $r = Send $base "POST" "/api/open?name=$($sc.name)" ([IO.File]::ReadAllBytes($samplePath)) "application/octet-stream"
                "  $($st.id) -> $($r.status)"
                continue
            }
            if ($st.kind -eq "new") {
                $r = Send $base "POST" "/api/new" ([byte[]]@()) "application/json"
                "  $($st.id) -> $($r.status)"
                continue
            }
            if ($st.request) { $text = [IO.File]::ReadAllText("$fx\requests\$($st.request).json") } elseif ($null -ne $st.body) { $text = $st.body } else { $text = "" }
            $text = $text.Replace("@PORT@", $panelPort).Replace("@LASTBACKUP@", $last)
            $r = Send $base $st.method $st.path ([Text.Encoding]::UTF8.GetBytes($text)) "application/json"
            [IO.File]::WriteAllText("$exp\$tag.status", "$($r.status)`n$($r.type)")
            [IO.File]::WriteAllBytes("$exp\$tag.body", $r.body)
            "  $($st.id) -> $($r.status) ($($r.body.Length) bytes)"
            if ($st.file -and $r.status -eq 200) {
                $j = [Text.Encoding]::UTF8.GetString($r.body) | ConvertFrom-Json
                $p = $j.($st.file)
                if ($p) {
                    Copy-Item $p "$exp\$tag.file" -Force
                    $made += $p
                    $last = Split-Path $p -Leaf
                }
            }
        }
    }
    finally {
        Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue
        Stop-Process -Id $simProc.Id -Force -ErrorAction SilentlyContinue
        Remove-Item $outFile -Force -ErrorAction SilentlyContinue
        foreach ($p in $made) { Remove-Item $p -Force -ErrorAction SilentlyContinue }
        # anything else the editor saved for the simulated panel (serial 31415926) during this run
        Get-ChildItem $backups -Filter "panel-31415926-*" -ErrorAction SilentlyContinue | Where-Object { $_.CreationTime -ge $started } | Remove-Item -Force
    }
}
# The recorded bodies name the backup folder of this machine (JSON-escaped). The tests normalise the folder, so keep a generic one in the repo.
$docsJson = $docs.Replace("\", "\\")
foreach ($f in Get-ChildItem $exp -Filter "*.body") {
    $t = [IO.File]::ReadAllText($f.FullName, [Text.Encoding]::UTF8)
    if ($t.Contains($docsJson)) { [IO.File]::WriteAllText($f.FullName, $t.Replace($docsJson, "C:\\Users\\user\\Documents"), (New-Object Text.UTF8Encoding($false))) }
}
"done"
