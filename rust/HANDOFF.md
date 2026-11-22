# Handoff (2026-11-20, rules editor added; see the last update)

Rust port of the Elk M1G account editor, using rust-anything 2.1.0. The C# original stays untouched under `port/` (except the recording harness patch in `Elk.Editor/Program.cs`, still uncommitted in this repo) and in `rust/original-build`. Local git only, no remote. Elk-main stays the source of truth for the C# editor.

## State
- `rust/app`: the editor, all routes native (`src/server.rs`), `src/main.rs` = args, port binding (PORT env, 51980, else any free port, 127.0.0.1 only), axum glue, native window on the main thread (`elk_planner_window::run`) with browser fallback and the C# console messages, `Shared::shutdown` on exit. `assets/index.html` is a byte copy of the C# page (`include_str!`). The blank account template comes from `elk_panel::template_text()` (one embedded copy, in elk-panel).
- `rust/cli`: the `elk` command line (crc, frame, decode, inspect, export-zones, export-alarm-list, download, upload, upload-export, modules, enroll, status, control, raw-read, raw-write).
- `rust/crates/*`: the four libraries (each has `PORTING-NOTES.md` with the known differences). Edits made during assembly: `elk-panel` gained `json_syntax_check` and `parse_model_body` (public, used by save/export/databus routes for the .NET `JsonException` wording) with `tests/model_body.rs`.
- Replay: 23/23, fallback 0.0%. Two cases were changed (time-of-day wildcard), see DECISIONS.md ADR-006: needs the user's OK.
- `app lift` could not run (ADR-007); the app has no proxy anyway. Do not run `app wire` again.

## Commands (PowerShell, `$env:Path += ";$env:USERPROFILE\.cargo\bin"`)
- App: `cd rust\app; cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test; cargo build --release`
- Replay (repo root): `rust-anything app replay . --build "rust/app/target/release/elkaccounteditor.exe"` (`--case <name>` for one case)
- CLI: `cd rust\cli; cargo test; cargo build --release` (binary `target\release\elk.exe`)
- Library crates: `cargo test` in each folder under `rust\crates`.
- Run: `elkaccounteditor [--browser] [--no-open] [export-file]`.

## Not matched / not verified
- Native window checked only by launching it on Windows (WebView2): it opens, serves the page and closes with exit code 0; dialogs and the page's file open/save were not driven by hand. macOS (WebKit, `rfd`, `open`) is not compiled or run; Linux has no native window (browser mode only).
- Wrong method on a known path answers an empty 405 without an `Allow` header; unknown paths an empty 404. HEAD is not handled like ASP.NET (answers 405).
- Errors from `.NET` runtime text that the crates only reproduce for probed cases (see each PORTING-NOTES.md); JSON type errors inside the model on `/api/save` use serde's wording plus `Path: $`, not System.Text.Json's.
- CLI: the C# crashes on a missing option or bad number (unhandled exception, stack trace); the Rust prints `error: ...` and exits 1. Panel commands were only run against the simulated panel (modules/enroll scan needs bus module records the simulator lacks; no real panel or hardware test).
- CLI `export-alarm-list` output matches the C# part for part (zip bytes differ, allowed by ADR-003).
- Default backup folder `Documents\ElkEditor backups` on Windows (a redirected Documents folder is honoured through `dirs`).

## Remaining work
- (done 2026-11-20, see the update below) Port the `automation-items` branch of Elk-main.
- macOS build and test (ElkRP on Apple Silicon is the goal): build on a Mac, check `native_window.rs` `window.external` shim, rfd dialogs, `open` for the browser.
- M1XEP settings: done on branch `m1xep-settings` (see the last update; write path awaits a real-device test).
- Optional: a CI job running the replay with the release binary; a `tests/app_cases.rs` using trycmd was not added (server cases are replayed with `app replay --build`).
- Decide on ADR-006 (the two time-of-day cases).
## Update 2026-11-20: Automation items ported
- Done: Automation tab model in `crates/elk-account` (`automation.rs`, `rules.rs`), `/api/meta` members and the new `assets/index.html` (byte copy) in `rust/app`; see ADR-009 for the changed recorded cases and the new `auto-*` cases. No other route changed in the C# (`Program.cs` differs only in `/api/meta`); the CLI is unchanged in the branch.
- Oracle: `rust/original-automation-src` (exported branch + harness patch) and `rust/original-automation-build` (`dotnet publish port\src\Elk.Editor -c Release -o <repo>\rust\original-automation-build`, run from the src folder), both git-ignored. Re-record cases: `powershell -File rust\make-http-inputs-automation.ps1`, then `rust-anything app record . --http --inputs rust/http-inputs.jsonl --arg --browser --arg --no-open --arg "<repo>\sample\ElkRPAcctSample Export" --no-traps -- dotnet rust/original-automation-build/ElkAccountEditor.dll`; then set `"at":"[..]"` again in `live-status-idle.body` and `live-disconnect.body` (ADR-006).
- Fixtures regenerated from the automation oracle: `crates/elk-account/tests/fixtures/regen.ps1`, `crates/elk-panel/tests/fixtures/regen.ps1`.
- Not done (as in the C#): panel sync of automation items, M1XEP settings, English wording and creation of rules, ElkRP import test of an edited automation file.

## Update 2026-11-20 (later): page changes, Account Details (ADR-010)
- `assets/index.html` is now Rust-only (C# notes: `rust/UI-CHANGES-FOR-CSHARP.md`, diff `rust/ui-changes-2026-11-20.diff`): no stray "null" (append filter), two tab rows (`SECTIONS`, `UIPREF`, `<nav id="subtabs">`), Globals > Alarm durations (`cutoffTab`), Communicator and Automation sub-tabs hoisted into the second row, Live status updated in place (`paintLive` / `liveUpdate`, scroll kept), and the Account Details form on Overview (`accountDetails`).
- `crates/elk-account`: `details.rs` + `AccountModel.details` (last JSON member, omitted when None). Golden model/pull fixtures from the C# oracle are compared with `details` cut out; `tests/details.rs` covers it.
- Replay 48/48 after changing seven recorded bodies on purpose (ADR-010). To re-run the page checks: build `elk-panel` example `sim_panel`, start the app with `--browser --no-open <sample>` and drive it with headless Edge over the DevTools protocol (not kept in the repo).
- Not verified: clicking through the native window, ElkRP importing an edited account-details file, macOS.
- A running `elkaccounteditor.exe` blocks `cargo build --release` on Windows: rename it (`elkaccounteditor-running.old.exe`, git-ignored) instead of killing the user's window.

## Update 2026-11-20 (evening): rules in ElkRP wording and a rule editor (ADR-011)
- Done: `crates/elk-account/src/english.rs` (ElkRP's WHENEVER / AND / THEN / FOR wording for every trigger, condition and result type), `POST /api/rules/english`, `/api/meta.eventWords`, validation of new / changed rules in `rules.rs`, and the Rules tab of `assets/index.html` (toolbar, English list, "n% of Rules/Text space used", rule dialog with live wording and refusals, hex fallback). C# notes: `UI-CHANGES-FOR-CSHARP.md` section 6, diff `ui-changes-rules.diff`.
- Verified: tests in every touched crate (elk-account, elk-panel, app, cli), clippy and fmt, replay 48/48 (two cases changed on purpose: `index-page`, `meta`), the page driven in headless Edge (list wording, decompose / compose round trip of every rule, every kind of the form builds and reads back, a new rule saved, reopened, wording identical, only the rule block of the file changed), and the 10 rules of a real local export (`TESTING\ElkRPAcct.exp`, git-ignored) read exactly like ElkRP's screen.
- Not verified: ElkRP importing a file with a rule made here, the native window, macOS (no OS-specific code was added).
- Not offered: Enable / Disable (no flag in the export file), Test, forms for send / display text, "text received" and audio (packet bytes only), e-mail and audio names (not in the file).

## Update 2026-11-20 (night): panel sync of automation items (ADR-012)
- Done: Read / Compare / Send of tasks, counters, thermostat names, alarm durations, custom settings, outputs, lights, voice messages and the rule and text packets (`"automation": true` on `/api/panel/*`, a checkbox in the Panel dialog, `elk download --automation`, `elk upload-export --automation`). Code: `elk-core/src/panel_auto.rs`, `elk-panel/src/auto_sync.rs`; tests: `elk-core/tests/auto.rs`, `elk-panel/tests/auto_sync.rs`, `auto_routes.rs`, `cli/tests/simulated.rs`. Replay 48/48 (only `index-page` re-recorded). Not mapped: sunrise / sunset (ADR-012).
- Read-only on the real panel (192.0.2.10): done, see ADR-012. Nothing was ever written to it.
- THE USER'S TEST ON THE REAL PANEL (send has only run on the simulator). Do it in this order, with one harmless value:
  1. Close ElkRP and anything else that holds a panel session. In the editor open the account export of the panel (File > Open), then Panel... > Connection "Secure port, TLS", address 192.0.2.10, serial 00000001, Advanced > RP access code. Leave "Include automation items" ticked.
  2. Click "Back up panel now" (Backups and restore). Note the file name; it now contains the automation items (`AutomationBlocks`). Optionally also keep an ElkRP "Receive from control" export.
  3. Click "Compare with panel". For an export that was just read from this panel every row should say "Would change 0", including the new rows output / task / counter / ... / rule packets / text packets. Stop if the automation rows differ unexpectedly.
  4. In the editor change ONE value on the Automation tab, for example rename Output 64 (or an unused custom setting) by adding one letter. Click "Compare with panel": exactly one change, `output 64` (or `custom setting N`), nothing else.
  5. Tick "I understand...", click "Send 1 record(s) to panel". Expected: "Wrote 1 record(s). Read back afterwards: the panel now matches this file in every section", and a backup path.
  6. Check in ElkRP (Receive from control, Outputs) or on a keypad that the name changed. Change it back in the editor, Compare (one change), Send again (Wrote 1).
  7. Rules and texts only after steps 1-6 worked: add one harmless text string on Automation > Texts (not used by a rule), Compare (expected `text packets N`), Send, then receive Texts in ElkRP and check it; delete the text again and Send. Rules are written the same way as a unit; do not change a rule on the first run.
  8. To undo: Panel... > Backups and restore > pick the backup from step 2 > "Check restore" (lists the differing records) > "Restore panel from this backup".
  If anything looks wrong (many automation changes for an export just read from the panel, an error text, ElkRP showing a different value) stop and report the message text; Compare never writes.
- Open: a voice board that is absent, firmware below 4.0 / 4.4, large rule / text areas and ElkRP importing a file saved after a Read with automation are untested.
## Update 2026-11-20 (night): M1XEP settings (ADR-013), branch `m1xep-settings`
- Done: the findings and protocol in `M1XEP-NOTES.md`; `elk-core/src/xep_config.rs` + `tests/xep.rs` (11 tests); `elk-panel/src/m1xep.rs` + `tests/m1xep.rs` (11 tests); `server.rs` test `m1xep_routes_are_wired`;
  the dialog in `assets/index.html` (Account Details > Network Adapter > "M1XEP settings..."); `UI-CHANGES-FOR-CSHARP.md` section 8. Replay 48/48 after changing the `index-page` case on purpose.
  The dialog was driven in headless Edge against a simulated module (read, edit, review, both ticks, send, backup written, read-back, Central Station tab, compare with file): no page errors.
- Verified on the real module: read only, once (login, `75 06`, five block reads, 16 e-mail reads). Decoded network values matched. Discovery (`XEPID`) got no answer from this computer (other subnet).
- NOT verified: any write to a real module; discovery on the module's own subnet; macOS (no OS-specific code was added); firmware below 1.3.19 (refused with a message).
- What the user should test on the real device (the first time any write happens), in this order, with ElkRP closed (the panel allows one session):
  1. Build and start the editor from this branch. Open an account (any) and set Account Details > Network adapter to M1XEP, System URL/IP, port 2601, serial; open "M1XEP settings...".
  2. "Read from the M1XEP": check the Device Name, IP address, mask, gateway, ports and every tab against ElkRP's M1XEP Setup window (Receive there first if unsure). They must be identical. If any field differs, stop and report it.
  3. "Compare with this account file": expect differences only where ElkRP's last Receive/Send differs from the module.
  4. Make a backup the ElkRP way too: in ElkRP, M1XEP Setup > Receive, then File > Export Account, keep the file.
  5. First write: change ONLY the Device Name (a harmless field), Review changes (one row), tick the first box, Send. Expect "Sent: network settings and flags (block 1)", "identical to what was sent", and a file `m1xep-<serial>-...-before-write.json`
     in Documents\ElkEditor backups. In ElkRP, Receive and confirm the new name. The new name only takes effect on the module after a reboot: reboot it from ElkRP (M1XEP Setup > Reboot) and reconnect.
  6. Restore test: Read, "Load into the form" the backup file, Review (one row: the name back), Send. Confirm the old name returns.
  7. Only after 5 and 6 worked, and only if wanted: a non-identity change such as the time server or an e-mail address. Do NOT change IP, mask, gateway, DHCP, DNS, ports or logins on a module you cannot reach physically.
  8. Identity fields: first with a harmless value equal to the current one (the diff must be empty), then a real change only when you can power-cycle or reset the module. The second warning tick must appear for them.
  Report anything unexpected (a read-back mismatch, a timeout on a block, the module not answering afterwards) with the exact text shown.
- Not done: Central Station, M1Cloud, time zone, daylight-saving block (`75 8A`), Reboot (`75 AA`), the Test buttons (`75 D0/D1/D2/D4`), firmware below 1.3.19, writing the M1XEP block into the account file, using the file's e-mail and audio names in the rules wording.

## Update 2026-11-21: Rule Enable / Disable (ADR-014), accounts library (ADR-015), updater (ADR-016)
- ADR-014 done: `RuleDto.disabled`, `rules::drop_disabled`, compare / send skip disabled groups, pull refused while rules are disabled; tests `elk-account/tests/disabled.rs`, `elk-panel/tests/auto_routes.rs` (disabled group not written, enabled again written, pull refusal), `server.rs`. Page: Enable / Disable buttons, strike-through, notes; C# notes in UI-CHANGES-FOR-CSHARP.md section 9. Replay: `index-page` re-recorded.
- ADR-015 done: `rust/crates/elk-library` (SQLite, folders, accounts BLOB, history, settings.json, data folder choice, backup zip; 13 tests), `app/src/state.rs` (`App`), `app/src/library_routes.rs` (+4 route tests), page (header buttons, start page, dialogs; driven in headless Edge, see below), `--data-dir` / `ELKEDITOR_DATA`, READMEs, CI. Replay: `index-page` re-recorded.
  Page test: Node 24 + headless Edge over the DevTools protocol (the script was not kept in the repo: import into a folder, open, disable a rule, Undo / Redo, Save to library, history, restore, search, backup: no page errors).
- ADR-016 done: `app/src/update.rs` (+ `update/tests.rs`), `update_routes.rs`, About / Updates in the page, `release` job in build.yml, `rust/packaging/update-from-git.ps1` / `.sh`, version 0.2.0. Tests use a local release server; the real GitHub path is untested until the repository is public (steps in ADR-016).

## Update 2026-11-21: no console window on Windows (ADR-017)
- Release exe is GUI-subsystem (`windows_subsystem = "windows"`); `app/src/console.rs` attaches to the parent console only when std handles are invalid, and a panic hook shows a message box + `error.log` when there is no console. Debug builds and `elk` stay console programs. Replay 48/48.
- Not verified: the updater restart on a real install (same exe, no console by construction); macOS (no-op).

## Update 2026-11-21: Panel dialog / Tools menu / backups in the library / autosave (ADR-018)
- Done: all ten requests (see ADR-018 for how each was read). Driven in headless Edge against `sim_panel` (header, Tools entries, Data bus plan page and Back, Panel dialog fields and defaults, Compare / Read / Send / Live connect each autosave, Panels folder, library backups: make, list by serial, check restore, delete with confirm, all backups, Save dialog defaults, narrow width without overflow, About still checks updates): no page errors (the favicon 404 is old). Replay 48/48 (`index-page` re-recorded). The script was not kept in the repo.
- Not verified: macOS, native window, Open folder, Restore apply from the library view (only Check restore was driven; the code path is the old one), the real panel.
- The RP access code, when remembered by the user, is plain text in `library.sqlite` (ADR-018).
## Update 2026-11-21: public copy (ADR-019)
- Done: generic alarm zone template, fake panel IP (192.0.2.10) and serial (00000001), path and name cleanup, TRACE-FINDINGS without keys, optional event table (`elk-account/build.rs`), `rust/packaging/publish-clean.ps1` + `publish-exclude.txt` + README, README notice. Details and what remains in each file: ADR-019.
- Publishing: work and test in this (private) repository, commit, push, then `powershell -File rust\packaging\publish-clean.ps1 -DryRun`, then without `-DryRun` (add `-NoPush` to only commit in `..\Elk-rust-public`). The script needs `.publish-deny.txt` in the repository root (git-ignored, one regular expression per line: names, real addresses, serials, paths; add to it whenever something new must never be public). It never changes the public repository's visibility. The public commit records the source commit in `.published-from`.
- Without `src/data/event-list-m1g.tsv` (the public build) rule events read "Event n"; to check both builds run the tests and the replay once with the file renamed.
- RP access code shown in clear text (user request): the RP code inputs (Panel dialog, Live connect, M1XEP dialog, library save dialog, Account Details, Change RP code) are plain text instead of password fields; passphrase fields stay masked. `index-page` re-synced (byte copy of `assets/index.html`).
