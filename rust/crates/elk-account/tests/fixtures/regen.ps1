$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Net.Http
$root = (Resolve-Path "$PSScriptRoot\..\..\..\..\..").Path
$crate = "$root\rust\crates\elk-account\tests\fixtures"
$port = 52222
$env:PORT = $port
$p = Start-Process dotnet -ArgumentList @("$root\rust\original-automation-build\ElkAccountEditor.dll", "--browser", "--no-open") -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 4
$base = "http://127.0.0.1:$port"
$http = New-Object System.Net.Http.HttpClient
function Post($url, [byte[]]$body, $ctype = "application/octet-stream") {
    $c = New-Object System.Net.Http.ByteArrayContent(,$body)
    $c.Headers.ContentType = New-Object System.Net.Http.Headers.MediaTypeHeaderValue($ctype)
    $r = $http.PostAsync("$base$url", $c).Result
    return @{ status = [int]$r.StatusCode; body = $r.Content.ReadAsByteArrayAsync().Result }
}
$mr = $http.GetAsync("$base/api/meta").Result; [IO.File]::WriteAllBytes("$crate\meta.json", $mr.Content.ReadAsByteArrayAsync().Result)
foreach ($d in "model", "saved", "export", "read") { New-Item -ItemType Directory -Force "$crate\$d" | Out-Null }
foreach ($d in "model", "saved") { Get-ChildItem "$crate\$d" -File -ErrorAction SilentlyContinue | Remove-Item }
$samples = @("ElkRPAcctSample Export", "Sample Communicator", "Sample Zones Import", "Sample Zones Import Export")
try {
    for ($i = 0; $i -lt 4; $i++) {
        $bytes = [IO.File]::ReadAllBytes("$root\sample\$($samples[$i])")
        $r = Post "/api/open?name=s$i" $bytes
        "open s$i -> $($r.status)"
        [IO.File]::WriteAllBytes("$crate\model\s$i.json", $r.body)
        foreach ($f in Get-ChildItem "$crate\requests" -Filter "s${i}_*.json") {
            $tag = $f.BaseName
            $r = Post "/api/save" ([IO.File]::ReadAllBytes($f.FullName)) "application/json"
            if ($r.status -eq 200) { [IO.File]::WriteAllBytes("$crate\saved\$tag.bin", $r.body) }
            else { [IO.File]::WriteAllBytes("$crate\saved\$tag.err", $r.body); "$tag -> $($r.status): $([Text.Encoding]::UTF8.GetString($r.body))" }
        }
        foreach ($tag in "plain", "edit") {
            $req = [IO.File]::ReadAllBytes("$crate\requests\s${i}_$tag.json")
            foreach ($q in @(@("csv", "format=csv"), @("csvall", "format=csv&all=1"), @("xlsx", "format=xlsx"), @("xlsxall", "format=xlsx&all=1"))) {
                $r = Post "/api/zones/export?$($q[1])" $req "application/json"
                $ext = if ($q[0] -like "csv*") { "csv" } else { "xlsx" }
                [IO.File]::WriteAllBytes("$crate\export\s${i}_${tag}_$($q[0]).$ext", $r.body)
            }
            $r = Post "/api/export-alarm-list" $req "application/json"
            [IO.File]::WriteAllBytes("$crate\export\s${i}_${tag}_alarm.xlsx", $r.body)
            $r = Post "/api/export-alarm-list?title=My%20Site%20%26%20Co" $req "application/json"
            [IO.File]::WriteAllBytes("$crate\export\s${i}_${tag}_alarmtitle.xlsx", $r.body)
        }
    }
    Copy-Item "$crate\export\s0_plain_xlsxall.xlsx" "$crate\readinputs\zones_export.xlsx" -Force
    Copy-Item "$crate\export\s1_edit_alarm.xlsx" "$crate\readinputs\alarm_list.xlsx" -Force
    # zones/read inputs
    $inputs = Get-ChildItem "$crate\readinputs"
    foreach ($f in $inputs) {
        $r = Post "/api/zones/read?name=$($f.Name)" ([IO.File]::ReadAllBytes($f.FullName))
        [IO.File]::WriteAllBytes("$crate\read\$($f.Name).$($r.status).json", $r.body)
    }
    # open errors
    $r = Post "/api/open?name=bad" ([Text.Encoding]::GetEncoding(28591).GetBytes("hello`r`nworld`r`n"))
    "open bad -> $($r.status): $([Text.Encoding]::UTF8.GetString($r.body))"
    [IO.File]::WriteAllBytes("$crate\read\open_bad.$($r.status).txt", $r.body)
}
finally {
    Stop-Process -Id $p.Id -Force
}


