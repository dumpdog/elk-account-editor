# Elk M1G account editor

A cross-platform (Windows / macOS) editor for Elk M1G alarm panel programming, plus the protocol library and CLI behind it. It edits ElkRP "Export Account" files (import the result with ElkRP's **File > Import Account**) and can also read from, compare with, back up, and write to a panel directly over the network, so ElkRP is optional.

Status: working and tested on a spare M1G (firmware 5.3.44). Nothing here is an official Elk product.

## What is here

| Path | What |
|---|---|
| `port/src/Elk.Editor` | The desktop app: a native window (Photino.NET) around a local web UI. `--browser` opens a browser tab instead. |
| `port/src/Elk.Protocol` | Account-file model, panel protocol (framing, AES and plain sessions), sync/backup/pull logic |
| `port/src/Elk.Cli` | Command line: `download`, `upload`, `upload-export`, `raw-read`, `raw-write`, ... |
| `port/tests` | 91 xUnit tests, including a simulated panel |
| `docs/ELK-ACCOUNT-EDITOR-HANDOFF.md` | Start here: file format, what has been verified (and how), open items |
| `docs/TRACE-FINDINGS.md` | Protocol notes |
| `docs/ELK-ZONE-EXPANDER-TABLES.md` | Zone expander data bus address tables and the wireless zone rules (zones 17-160); kept in the private working repository only, not in the public copy |

## Build

    cd port
    dotnet test
    dotnet publish src/Elk.Editor -c Release -r win-x64   -o dist/Editor-win-x64   -p:PublishSingleFile=true --self-contained
    dotnet publish src/Elk.Editor -c Release -r osx-arm64 -o dist/Editor-osx-arm64 -p:PublishSingleFile=true --self-contained

Windows needs the Edge WebView2 runtime (included with current Windows). On macOS keep `Photino.Native.dylib` next to the app; see the handoff doc for the Gatekeeper commands (`chmod +x`, `xattr -dr com.apple.quarantine .`, `codesign -s - --force`).

## What it does

- **Edit an account:** zones (one grid with report codes), wireless devices and groups, users, areas, keypads, telephones, system events and the global settings, with undo and a History list of changes. Delete records you do not need; start from a blank account or read one straight from a panel.
- **Talk to a panel directly (no ElkRP needed):** Compare, Send (a backup is made first and the result is read back), Read, backups and Restore, and a **Status (live)** tab that keeps one connection open and shows areas, zones, system troubles and zone voltages, with bypass, arm/disarm and silence buttons. The default RP access code 246801 is pre-filled.
- Not yet: voice messages, X10, outputs, tasks and rules. The secure port works (TLS, no passphrase). See docs/ELK-ACCOUNT-EDITOR-HANDOFF.md.

## Mac app and disk image

On a Mac, `port/packaging/make-mac-app.sh path/to/Editor-osx-arm64` builds `Elk Account Editor.app` and a `.dmg` (ad-hoc signed; the first start needs right-click > Open). With an Apple Developer ID it can sign and notarize so the app opens with a double-click (see the comments in the script). The GitHub workflow `Build apps` (Actions tab, started by hand) produces the Windows exe and the Mac .dmg without needing a Mac.

The zips that are handed out (a folder with the app and a README, no debug files) are made by `python port/packaging/make-zips.py <dist folder>` from the two `dotnet publish` folders. The Mac zip keeps the executable bit and includes `Start Elk Account Editor.command`, which does the first-run trust steps (xattr and ad-hoc codesign); `README-mac.txt` and `README-windows.txt` in that folder are the files that go in the zips.

## Not included in this repository

The project was developed alongside material that is deliberately left out: a decompilation of ElkRP and of its serial library, copies of ElkRP's databases, and real account exports and panel backups. Some tests use files from those folders (`sample/`, `TESTING/`) and skip themselves when the files are absent, so a clean checkout still builds and passes, with less coverage than the original machine.

## Notice

This is an unofficial, independent project. It is not affiliated with, endorsed by or supported by Elk Products, Inc. "Elk", "M1", "M1G" and "ElkRP" are names and trademarks of their owners and are used here only to say which products the software works with. The editor exists for interoperability: it reads and writes the file formats and the network protocol that Elk's own programming software uses, so that panels can be programmed from other platforms. Some data tables in the source (word lists, voice message and output names, rule wording) are ElkRP-derived wording kept as data so that rule text and names read the same as in ElkRP; they are there for compatibility only. The full event table with ElkRP's wording is not part of the public source: without it, events show a generic name ("Event 1003") and everything else works as described. No ElkRP program code, database or account data is included.

## Safety notes

Writing to a panel changes its programming. The editor makes a full backup before every send, shows a diff first, and verifies by reading back. Try changes on a spare panel before a customer's.

## Rust editor: library, updates and git

The Rust editor under `rust/` keeps an accounts library (folders, saved accounts, version history) in a folder `ElkEditor data` next to the program (next to the `.app` on macOS; a per-user folder when that is not writable; `--data-dir` or `ELKEDITOR_DATA` choose another). Releases are published when a tag `v<version>` is pushed (`.github/workflows/build.yml` builds the zips and `SHA256SUMS`); the editor's About / Updates > Check for updates reads the latest release, checks the download against `SHA256SUMS`, and installs it after a second click. To update from git instead, run `rust/packaging/update-from-git.ps1` (Windows) or `rust/packaging/update-from-git.sh` (macOS / Linux): they check the tree is clean, `git pull --ff-only` and `cargo build --release`. Details: `rust/DECISIONS.md` ADR-015 and ADR-016.
