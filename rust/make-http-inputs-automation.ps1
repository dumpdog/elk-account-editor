# Adds the automation cases (names starting "auto-") to rust\http-inputs.jsonl. Idempotent: existing auto- lines are dropped first.
# The save bodies are the request fixtures of elk-account (tests\fixtures\requests\s0_auto_*.json; sample 0 is the file the recorded
# server is started with), the open bodies are sample files. The new cases go after "save-bad-user-code" (the session still holds
# sample 0) and the opens go last.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..").Path
$inputs = "$root\rust\http-inputs.jsonl"
$req = "$root\rust\crates\elk-account\tests\fixtures\requests"
$old = Get-Content $inputs -Encoding UTF8 | Where-Object { $_ -and ($_ -notmatch '^\{"name":"auto-') }

function Case($name, $method, $path, $ctype, $body) {
    $o = [ordered]@{ name = $name; method = $method; path = $path }
    if ($ctype) { $o.headers = [ordered]@{ "Content-Type" = $ctype } }
    $o.body = $body
    return ($o | ConvertTo-Json -Compress -Depth 5)
}
function Save($name, $fixture) {
    $body = [IO.File]::ReadAllText("$req\$fixture.json", [Text.Encoding]::UTF8)
    return Case $name "POST" "/api/save" "application/json" $body
}
$saves = @(
    @("auto-save-plain", "s0_auto_plain"),
    @("auto-save-outputs", "s0_auto_outputs"),
    @("auto-save-tasks", "s0_auto_tasks"),
    @("auto-save-counters", "s0_auto_counters"),
    @("auto-save-customs", "s0_auto_customs"),
    @("auto-save-lights", "s0_auto_lights"),
    @("auto-save-voice", "s0_auto_voice"),
    @("auto-save-texts", "s0_auto_texts"),
    @("auto-save-rules-comment", "s0_auto_rules_comment"),
    @("auto-save-rules-delete", "s0_auto_rules_delete"),
    @("auto-save-rules-reorder", "s0_auto_rules_reorder"),
    @("auto-save-rules-packets", "s0_auto_rules_packets"),
    @("auto-save-all-zone", "s0_auto_all_zone"),
    @("auto-save-bad-taskname", "s0_auto_bad_taskname"),
    @("auto-save-bad-voiceword", "s0_auto_bad_voiceword"),
    @("auto-save-bad-customtime", "s0_auto_bad_customtime"),
    @("auto-save-bad-lightnum", "s0_auto_bad_lightnum"),
    @("auto-save-bad-textsclear", "s0_auto_bad_textsclear"),
    @("auto-save-bad-textlong", "s0_auto_bad_textlong"),
    @("auto-save-bad-textfull", "s0_auto_bad_textfull"),
    @("auto-save-bad-rulezero", "s0_auto_bad_rulezero"),
    @("auto-save-bad-rulefull", "s0_auto_bad_rulefull")
)
$newSaves = $saves | ForEach-Object { Save $_[0] $_[1] }
$openBody = { param($n) [Text.Encoding]::GetEncoding(28591).GetString([IO.File]::ReadAllBytes("$root\sample\$n")) }
$tail = @(
    (Case "auto-open-sample" "POST" "/api/open?name=ElkRPAcctSample%20Export" "application/octet-stream" (& $openBody "ElkRPAcctSample Export")),
    (Case "auto-open-zones-import" "POST" "/api/open?name=Sample%20Zones%20Import" "application/octet-stream" (& $openBody "Sample Zones Import")),
    (Case "auto-open-old-software" "POST" "/api/open?name=old" "application/octet-stream" ((& $openBody "ElkRPAcctSample Export").Replace('"5.0.12"', '"4.3.6"')))
)
$out = New-Object System.Collections.Generic.List[string]
foreach ($l in $old) { $out.Add($l); if ($l -match '^\{"name":"save-bad-user-code"') { $newSaves | ForEach-Object { $out.Add($_) } } }
$tail | ForEach-Object { $out.Add($_) }
[IO.File]::WriteAllLines($inputs, $out, (New-Object Text.UTF8Encoding($false)))
"$($out.Count) cases"
