<#
.SYNOPSIS
  Publishes a cleaned copy of this (private) working repository to the public test repository.

.DESCRIPTION
  1. `git archive HEAD` of this repository (tracked, committed files only) into a temp folder.
  2. Deletes everything listed in rust/packaging/publish-exclude.txt.
  3. LEAK SCAN: every file (and every entry of every .xlsx/.zip) is searched for the regular expressions in `.publish-deny.txt`
     in the repository root. That file is NOT tracked (git-ignored): it holds the strings that must never be public (customer and
     account names, real panel addresses and serials, user and path names). The scan prints file:line, never the matched text,
     and stops the publish when anything matches. The scan fails when the file is missing unless -NoDenyList is given.
  4. Clones / updates the public repository into a sibling folder (default ..\Elk-rust-public), replaces its working tree with
     the cleaned export (keeping .git), writes `.published-from` (the commit of this repository that was published), commits
     and pushes to master. The first run makes one commit, "Initial public version (squashed)"; later runs name the commit
     subject(s) of this repository since the last published commit.

  The public repository's visibility is never touched by this script.

.PARAMETER DryRun
  Steps 1-3 only: reports what would be published (counts, exclusions, scan result, commit message). Changes nothing outside
  the temp folder and does not touch the public clone.
.PARAMETER NoPush
  Does everything including the local commit in the public clone, but does not push.
.PARAMETER NoDenyList
  Allows a run without `.publish-deny.txt` (the scan then only runs when the file exists). Use only when you know why.
.PARAMETER PublicUrl
  The public repository (default https://github.com/dumpdog/elk-account-editor).
.PARAMETER PublicDir
  Where the public clone lives (default: a sibling folder named Elk-rust-public).
.PARAMETER Branch
  Branch of the public repository (default master).
.PARAMETER Trailer
  Last line of the commit message.
.PARAMETER AuthorName / AuthorEmail
  Identity for the public commits (set in the public clone only, never globally). The default is the GitHub no-reply address so
  that no personal e-mail address ends up in the public history.

  Usage:  powershell -File rust\packaging\publish-clean.ps1 -DryRun
          powershell -File rust\packaging\publish-clean.ps1 -NoPush
          powershell -File rust\packaging\publish-clean.ps1
#>
param(
    [switch]$DryRun,
    [switch]$NoPush,
    [switch]$NoDenyList,
    [string]$PublicUrl = "https://github.com/dumpdog/elk-account-editor",
    [string]$PublicDir = "",
    [string]$Branch = "master",
    [string]$Trailer = "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>",
    [string]$AuthorName = "dumpdog",
    [string]$AuthorEmail = "86878172+dumpdog@users.noreply.github.com"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Invoke-Git {
    # simple function on purpose: dashed arguments such as -q or -B pass through untouched
    $dir = [string]$args[0]
    $rest = @($args | Select-Object -Skip 1)
    # Windows PowerShell 5.1 turns git's stderr progress lines into errors under -Stop: relax it for the native call only
    $prev = $ErrorActionPreference; $ErrorActionPreference = "Continue"
    $out = & git -C $dir @rest 2>&1
    $code = $LASTEXITCODE; $ErrorActionPreference = $prev
    if ($code -ne 0) { throw "git $($rest -join ' ') failed in ${dir}: $out" }
    return $out
}

$repo = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim().Replace("/", "\")
if (-not $PublicDir) { $PublicDir = Join-Path (Split-Path $repo -Parent) "Elk-rust-public" }
$head = (Invoke-Git $repo rev-parse HEAD | Out-String).Trim()
$headShort = $head.Substring(0, 9)
$dirty = (Invoke-Git $repo status --porcelain | Out-String).Trim()
if ($dirty) { Write-Warning "The working tree has uncommitted changes. Only the committed state (HEAD $headShort) is published." }

# ---- 1. export ----------------------------------------------------------------------------------------------------------
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("elk-publish-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Path $tmp | Out-Null
$tar = "$tmp.tar"
try {
    & git -C $repo archive --format=tar -o $tar HEAD
    if ($LASTEXITCODE -ne 0) { throw "git archive failed" }
    & tar -xf $tar -C $tmp
    if ($LASTEXITCODE -ne 0) { throw "tar extract failed" }
    Remove-Item $tar -Force
    $all = Get-ChildItem $tmp -Recurse -File -Force
    $exported = $all.Count

    # ---- 2. exclusions ---------------------------------------------------------------------------------------------------
    function GlobToRegex([string]$g) {
        $g = $g.Trim().Replace("\", "/").TrimStart("/")
        $anyDepth = -not $g.Contains("/")
        $sb = New-Object Text.StringBuilder
        $i = 0
        while ($i -lt $g.Length) {
            $c = $g[$i]
            if ($c -eq "*" -and $i + 1 -lt $g.Length -and $g[$i + 1] -eq "*") {
                if ($i + 2 -lt $g.Length -and $g[$i + 2] -eq "/") { [void]$sb.Append("(?:.*/)?"); $i += 3; continue }
                [void]$sb.Append(".*"); $i += 2; continue
            }
            if ($c -eq "*") { [void]$sb.Append("[^/]*") }
            elseif ($c -eq "?") { [void]$sb.Append("[^/]") }
            else { [void]$sb.Append([regex]::Escape([string]$c)) }
            $i++
        }
        $re = $sb.ToString()
        if ($anyDepth) { $re = "(?:.*/)?" + $re }
        return "^" + $re + "$"
    }
    $excludeFile = Join-Path $PSScriptRoot "publish-exclude.txt"
    if (-not (Test-Path $excludeFile)) { throw "missing $excludeFile" }
    $patterns = Get-Content $excludeFile | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith("#") }
    $rules = $patterns | ForEach-Object { [pscustomobject]@{ Pattern = $_; Regex = [regex]::new((GlobToRegex $_), "IgnoreCase") } }
    $removed = @()
    foreach ($f in $all) {
        $rel = $f.FullName.Substring($tmp.Length + 1).Replace("\", "/")
        foreach ($r in $rules) {
            if ($r.Regex.IsMatch($rel)) { $removed += $rel; Remove-Item -LiteralPath $f.FullName -Force; break }
        }
    }
    Get-ChildItem $tmp -Recurse -Directory -Force | Sort-Object { $_.FullName.Length } -Descending | Where-Object { -not (Get-ChildItem $_.FullName -Force) } | Remove-Item -Force
    $files = Get-ChildItem $tmp -Recurse -File -Force
    Write-Host ("Exported {0} files from {1}; excluded {2}; {3} remain." -f $exported, $headShort, $removed.Count, $files.Count)

    # ---- 3. leak scan ------------------------------------------------------------------------------------------------------
    $denyFile = Join-Path $repo ".publish-deny.txt"
    $latin1 = [Text.Encoding]::GetEncoding(28591)
    $hits = New-Object Collections.Generic.List[string]
    if (-not (Test-Path $denyFile)) {
        if (-not $NoDenyList) { throw "Deny list $denyFile is missing. Create it (one regular expression per line) or pass -NoDenyList." }
        Write-Warning "No deny list: the leak scan was skipped (-NoDenyList)."
    }
    else {
        $lines = Get-Content $denyFile -Encoding UTF8 | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith("#") }
        if (-not $lines) { throw "$denyFile has no entries." }
        $parts = New-Object Collections.Generic.List[string]
        foreach ($l in $lines) {
            try { [void][regex]::new($l) } catch { throw "Bad regular expression in the deny list (entry $($parts.Count + 1))." }
            $parts.Add("(?:" + $l + ")")
        }
        $deny = [regex]::new(($parts -join "|"), "IgnoreCase")
        function ScanText([string]$label, [string]$text) {
            foreach ($m in $deny.Matches($text)) {
                $line = 1; $idx = $m.Index
                for ($k = $text.IndexOf("`n"); $k -ge 0 -and $k -lt $idx; $k = $text.IndexOf("`n", $k + 1)) { $line++ }
                $hits.Add("${label}:$line")
                if ($hits.Count -gt 200) { return }
            }
        }
        function ScanBytes([string]$label, [byte[]]$bytes) {
            ScanText $label ($latin1.GetString($bytes))
            if ($bytes.Length -gt 4 -and $bytes[0] -eq 0x50 -and $bytes[1] -eq 0x4B -and $bytes[2] -eq 3 -and $bytes[3] -eq 4) {
                try {
                    $ms = New-Object IO.MemoryStream(, $bytes)
                    $za = New-Object IO.Compression.ZipArchive($ms)
                    foreach ($e in $za.Entries) {
                        ScanText ("${label}!" + $e.FullName + " (name)") $e.FullName
                        $es = $e.Open(); $buf = New-Object IO.MemoryStream; $es.CopyTo($buf); $es.Dispose()
                        ScanBytes ("${label}!" + $e.FullName) $buf.ToArray()
                    }
                    $za.Dispose()
                }
                catch { Write-Warning "Could not open $label as a zip: scanned as plain bytes only." }
            }
        }
        $n = 0
        foreach ($f in $files) {
            $rel = $f.FullName.Substring($tmp.Length + 1).Replace("\", "/")
            ScanText "$rel (name)" $rel
            ScanBytes $rel ([IO.File]::ReadAllBytes($f.FullName))
            $n++
        }
        if ($hits.Count -gt 0) {
            Write-Host "LEAK SCAN FAILED: the deny list matched at the places below (text not shown). Fix repository A, commit, run again." -ForegroundColor Red
            $hits | Select-Object -First 100 | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
            throw "Leak scan failed ($($hits.Count) match(es))."
        }
        Write-Host ("Leak scan: {0} files, {1} deny pattern(s): clean." -f $n, $lines.Count)
    }

    # ---- commit message ---------------------------------------------------------------------------------------------------
    $firstRun = $true; $last = $null
    $pubGit = Join-Path $PublicDir ".git"
    if (Test-Path $pubGit) {
        $pf = Join-Path $PublicDir ".published-from"
        if (Test-Path $pf) { $last = (Get-Content $pf -Raw).Trim(); $firstRun = $false }
    }
    if ($firstRun) { $subject = "Initial public version (squashed)"; $body = "Exported from commit $headShort of the private working repository." }
    else {
        $subs = @(Invoke-Git $repo log --format=%s "$last..$head")
        if ($subs.Count -eq 0) { $subs = @("Update") }
        $subject = if ($subs.Count -gt 1) { "$($subs[0]) (+$($subs.Count - 1) more)" } else { $subs[0] }
        $body = (($subs | ForEach-Object { "- $_" }) -join "`n") + "`n`nExported from commit $headShort of the private working repository."
    }
    $message = "$subject`n`n$body`n`n$Trailer"

    if ($DryRun) {
        Write-Host "`n-- DryRun summary --"
        Write-Host "Source commit : $head"
        Write-Host "Files kept    : $($files.Count)   excluded: $($removed.Count)"
        $removed | Select-Object -First 15 | ForEach-Object { Write-Host "   excluded $_" }
        if ($removed.Count -gt 15) { Write-Host "   ... and $($removed.Count - 15) more" }
        Write-Host "Public clone  : $PublicDir ($(if ($firstRun) { 'first publish' } else { "last published $last" }))"
        Write-Host "Commit message:`n$message"
        return
    }

    # ---- 4. public clone, replace tree, commit, push ----------------------------------------------------------------------------
    if (-not (Test-Path $pubGit)) {
        & git clone $PublicUrl $PublicDir
        if ($LASTEXITCODE -ne 0) { throw "git clone failed" }
    }
    $remoteHasBranch = (& git -C $PublicDir ls-remote --heads origin $Branch | Out-String).Trim()
    if ($remoteHasBranch) {
        Invoke-Git $PublicDir fetch origin | Out-Null
        Invoke-Git $PublicDir checkout -B $Branch "origin/$Branch" | Out-Null
    }
    else { Git $PublicDir checkout -B $Branch | Out-Null }
    Invoke-Git $PublicDir config user.name $AuthorName | Out-Null
    Invoke-Git $PublicDir config user.email $AuthorEmail | Out-Null
    Invoke-Git $PublicDir config core.autocrlf false | Out-Null
    Invoke-Git $PublicDir config core.safecrlf false | Out-Null
    # keep .git only
    Get-ChildItem $PublicDir -Force | Where-Object { $_.Name -ne ".git" } | Remove-Item -Recurse -Force
    Copy-Item -Path (Join-Path $tmp "*") -Destination $PublicDir -Recurse -Force
    # dot-files at the top (.gitignore, .github, ...) are not matched by "*" on every PowerShell version
    Get-ChildItem $tmp -Force | Where-Object { $_.Name.StartsWith(".") } | ForEach-Object { Copy-Item $_.FullName (Join-Path $PublicDir $_.Name) -Recurse -Force }
    [IO.File]::WriteAllText((Join-Path $PublicDir ".published-from"), "$head`n")
    Invoke-Git $PublicDir add -A | Out-Null
    $status = (Invoke-Git $PublicDir status --porcelain | Out-String).Trim()
    if (-not $status) { Write-Host "Nothing to publish: the public repository already matches $headShort."; return }
    $msgFile = Join-Path $tmp "message.txt"
    [IO.File]::WriteAllText($msgFile, $message, (New-Object Text.UTF8Encoding($false)))
    Invoke-Git $PublicDir commit -q -F $msgFile | Out-Null
    $pubHead = (Invoke-Git $PublicDir rev-parse HEAD | Out-String).Trim()
    if ($NoPush) { Write-Host "NoPush: committed $pubHead locally in $PublicDir; not pushed." }
    else {
        Invoke-Git $PublicDir push origin "${Branch}:${Branch}" | Out-Null
        Write-Host "Pushed $pubHead to $PublicUrl ($Branch)."
    }
    Write-Host "`n-- Summary --"
    Write-Host "Source (private) : $head"
    Write-Host "Public commit    : $pubHead"
    Write-Host "Files published  : $($files.Count)   excluded: $($removed.Count)"
    Write-Host "Public clone     : $PublicDir"
}
finally {
    if (Test-Path $tar) { Remove-Item $tar -Force -ErrorAction SilentlyContinue }
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
}
