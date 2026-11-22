# UI changes of 2026-11-20 (Rust page) and how to apply them to the C# editor

Applies to `port/src/Elk.Editor/index.html` in Elk-main if the C# editor is ever used again. Everything was made in
`rust/app/assets/index.html` (the Rust app embeds it byte for byte). The full unified diff of the page against the previous
commit is `rust/ui-changes-2026-11-20.diff` (`git diff -- rust/app/assets/index.html`); the old/new snippets below are the
parts that matter. Line numbers are those of the new Rust page (about 1886 lines); in the old page subtract roughly 0-130.
The base page is the automation-items page (same as ADR-009), so the old/new text matches the C# page line for line except
where the C# page was edited later.

Order of work if you port it: 1 (one line), 3 + 2 (tab layout), 4 (live), 5 (needs a C# model change, see the end).

## 1. The literal "null" on the Zones tab

Cause: `zonesTab` ends with `p.append(sheetBar, sheetHint, sheetNote, bar, filter, ...)` and `sheetNote` is `null` when no import
has been done (`const sheetNote = ZIMP.note ? el(...) : null`). `el()` skips null children, but `Element.append(null)` writes the
text "null". It showed after a panel read because that is when the tab is first opened without an import note; it was always there.

The same pattern is in four more places: `logView` (`LOGV.note ? ... : null` as a direct argument of `p.append`), `timeView`
(`TIMEV.note`), `codeView` (`CODEV.note`) and `modulesTab` (`MOD.note`), so the Live sub-views showed "null" too until a note
existed. No other tab appends a possibly-null value directly.

Fix (one place, covers every current and future call): right after the definition of `$` (about line 151) add

```js
// Element.append(null) writes the text "null" ... Every append in this page goes through here, so a null/undefined/false child is skipped.
(function () { const app = Element.prototype.append; Element.prototype.append = function (...kids) { return app.apply(this, kids.filter(k => k != null && k !== false)); }; })();
```

Verified by driving the page in headless Edge: no text node equal to `null`, `undefined` or `NaN` in any tab, sub-tab or Live view.

## 2. Tab layout with a second row (ElkRP folder tree)

New markup: `<nav id="subtabs" style="display:none"></nav>` after `<nav id="tabs">`. New CSS (after `nav button .badge.w`):

```css
#subtabs { padding-top:3px; } #subtabs button { font-size:12px; padding:3px 10px; } #subtabs button.on { box-shadow:inset 0 -2px 0 var(--acc); } #subtabs button .cnt { margin-left:6px; font-size:11px; opacity:.8; }
```

Script: the old `const TABS = {...}` plus the old `render()` (about lines 1536-1546 in the old page) are replaced by the block that starts
at `const TABS` (line 1648) and ends with the new `render()` (line 1666). Design:

- `TABS` stays a flat map leaf key -> `[label, pageFunction]` (so `TAB = 'zones'`, `TABS[i.tab][0]` in the "Go to" buttons, `load()` setting
  `TAB = 'overview'`, the data bus code setting `TAB = 'databus'`, and `TAB === 'live'` checks all keep working). New leaf keys:
  `cutoff` (Alarm durations). Labels changed: `comm` is now "Communicator" (was "System events").
- `SECTIONS` is the main tab bar: Overview, Users, Areas, Keypads, Zones, Wireless, Data bus, Telephones, Globals, Communicator,
  Automation, Live. A section may have `subs` (the second row).
  - Wireless (`pages: true`): sub-tabs are the pages `groups` (Zone groups) and `wireless` (Devices).
  - Globals (`pages: true`): `globals` (General) and `cutoff` (Alarm durations).
  - Communicator: one page (`commTab`), sub-tab held in `COMMTAB`: `COMM_SUBS` = Area RCs, User RCs, Zone RCs, System RCs, Keypad panic RCs.
    The current `commTab` content split cleanly: its own button bar was removed (3 lines at the top of `commTab`) and the row is now
    the section's sub row. "System RCs" is the old "System events" sub-tab (key `system`); the intro hint was reworded.
  - Automation: one page (`automationTab`), sub-tab in `AUTO.sub`; its own `.subnav` bar (with the texts / rules counters) was removed from
    `automationTab` and `AUTO_SUBS` is the sub row. Order is now Tasks, Lighting, Outputs, Voice, Custom settings, Counters and
    thermostats, Texts, Rules (the default `AUTO.sub` is `'tasks'`). Not present in the account file or the model, so not shown:
    Sunrise/Sunset (the Dawn and Dusk block is not parsed) and a separate Thermostats page (the 16 thermostat names are on the Counters page).
  - Live: no sub row (the existing Status / Event log / Modules / Time / RP access code buttons in `liveTab` stay).
  - Sections without subs show no sub row (the row is `display:none`).
- `UIPREF` holds the last sub-tab per section (wireless, globals, comm, automation). It is kept in memory and in
  `localStorage['elkui']` (read and written inside try/catch); `COMMTAB` and `AUTO.sub` are initialised from it. Clicking a section opens
  its last sub-tab (`openSection`); clicking a sub-tab calls `openSub`.
- `render()` builds the main row from `SECTIONS` (validate badge on Overview and the Live dot are unchanged), then the sub row, then calls
  `TABS[TAB][1](p)` exactly as before.
- Text changes: the validation message "Add rows on System events > Zone RCs" now says "Communicator > Zone RCs"; the history labels (`describe`)
  say "Communicator" instead of "System events" and describe `details` changes ("Account details: <field>").

## 3. Alarm durations moved to Globals

In `autoCustoms` (old lines 1438-1462) the left half of the `cols2` grid, the 12-row "Alarm durations" table, was removed. The code of the
value cell (`hhmm`, `valueCell`) became the top-level functions `hhmm` and `customValueCell(c)` (line 1547) so both pages share it. New
`cutoffTab(p)` (line 1567) renders the 12 rows (custom values 1-12: `#`, `Alarm`, `Duration`, hint "seconds; 65535 = until reset") and
is the page of the sub-tab Globals > Alarm durations (`TABS.cutoff`). Without an automation block it shows a hint instead.
Hint texts: Outputs hint "(The durations of the alarm outputs are under Custom values.)" now says "under Globals > Alarm durations."; the
Custom settings hint was reworded. The data (`MODEL.automation.customs[0..11]`) and the saved file are unchanged.

## 4. Live tab: the zone list jumped to the top while connected

Cause: `livePoll` (every 2 s) calls `paintLive()`, which built the whole view and ended with `box.replaceChildren(...kids)`. That
discarded the scrolling zone table (`.tablewrap` has `max-height:66vh; overflow:auto`, so its own `scrollTop` went back to 0) and the page
height collapsed for a moment, so the window scroll was clamped as well.

Fix: `paintLive` (line 674) was replaced by `paintLive(force)` + `liveUpdate(refs, sn, rows, zdef)` (line 729) and the helpers `LV`,
`liveStatusLine`, `liveAreaView`, `liveZoneColor` (lines 659-673).

- `paintLive` computes a structure signature (connected, snapshot present, Force arm, Show every zone, the areas shown and whether each is in
  alarm, the system alarm list, the list of zone rows with name and area). When it equals the last one, it only calls `liveUpdate`, which
  rewrites text and colour of the existing area cards and zone cells, matched by area number / zone number (maps kept in `refs`). No element is
  created or removed on the poll path, and `replaceChildren` is not called.
- Otherwise it rebuilds, but first saves `window.scrollY` and the `.tablewrap` `scrollTop/scrollLeft` and restores them afterwards
  (this is the "Show every zone" toggle, Force arm, a zone appearing, an alarm starting).
- Rebuilds triggered by the poll are postponed (`LV.wait`) while a text box, list or checkbox in `#livebox` has focus or the pointer is down inside it
  (`pointerdown` inside `#livebox`, cleared by `pointerup` / `pointercancel`, stale after 5 s); they run on the next `pointerup` / `focusout`.
  Buttons do not block, so after clicking Bypass the values keep updating. Explicit toggles call `paintLive(true)`.
- `livePoll`, `liveControl`, `statusView` are unchanged (`statusView` creates a fresh `#livebox`, so the first paint is a full build).

Verified against the simulated panel (`crates/elk-panel/examples/sim_panel`) in headless Edge: with all 208 zones shown, scrolled to
700 px (list) and 150 px (window), three polls later: 0 `replaceChildren` calls on `#livebox`, same scroll positions, the same 208 `<tr>`
elements. Toggling Show every zone with the list at 500 px and the window at 120 px kept both.

## 5. Account Details on the Overview tab

New `accountDetails(p)` (line 1180) and `DETAIL_MAX` (line 1179), called first from `overviewTab` (the counts, "Things to check" and the quick guide
follow below a rule); CSS classes `.dl` (three rules before `.stat`). Groups as in ElkRP: Name and Address, Contact Information, Notes, System,
Network Adapter. Inputs write straight into `MODEL.details`; the existing `#panel` input/change listeners track undo, drafts and the unsaved flag.
Double quotes cannot be stored (ElkRP writes strings quoted); the inputs replace `"` by `'` and say so.

Which fields exist in the export (from ElkRP's `Account_Details.Form_Export`, decompiled in `Elk/src/M1G2/M1G/Account_Details.cs`): header record
(25 fields) = 1 account, 2 name, 3 address 1, 4 address 2, 5 city, 6 state, 7 zip, 8 voice, 9 fax, 10 system telephone, 11 system URL/IP, 12 port,
13 C1M1 port(s), 14 e-mail, 15 web page, 16 folder (ElkRP column "Group"), 17 system type, 18 firmware, 19 event list version, 20 hardware,
21 boot, 22 voice list version, 23 date installed, 24 notes, 25 attributes; then 8 lines serial number, 6 lines RP access code, 3 x 6 lines
older RP codes, 12 lines MAC address (all XOR-ed with `"ElkRP Copyright 2004 Elk Products"`: serial at key offset 0, RP code at 8, MAC at 0 again),
passphrase line, network adapter line (0 M1XEP, 1 C1M1, 2 M1XIP), non-secure flags line (bit 1 M1XEP, bit 2 M1XIP). That is 47 lines after the
header, then the users block.

| Field | State in the page |
|---|---|
| Account | read-only (`MODEL.account`) |
| Name, Address 1/2, City, State/Province, Zip/Post Code, Folder | editable |
| Voice, Fax, Email, Web Page, System Telephone, System URL/IP | editable |
| Notes | editable (line breaks allowed; the header record then spans several lines) |
| Port to use for connecting (M1XEP), Port(s) (C1M1) | editable, digits only, up to 5 |
| Network adapter radios (M1XEP / C1M1 / M1XIP) | editable |
| Connect Non-Secure | editable for M1XEP (bit 1) and M1XIP (bit 2); not shown for C1M1 |
| Date Installed | editable (date picker when the text is `yyyy-mm-dd`) |
| System Type, Hardware, Boot, Firmware, Voice List, Serial Number, RP Access Code | read-only (RP code masked, Show button; Change... jumps to Live > RP access code) |
| Changed & Saved, Last Connected | NOT in the export file (shown empty, "not stored in the export file") |
| M1XIP port, Folder Browse button | not in the file / no native bridge command (omitted) |

Server and model changes the C# needs (the page is useless without them): `AccountModel` gets a last member `Details` (JSON `details`, camelCase,
written only when not null) filled by `AccountFile`, and `ApplyTo` writes it back.

- `DetailsDto` members, in this order: `name, address1, address2, city, state, zip, voice, fax, email, web, folder, notes, installed, systemPhone,
  systemUrl, port, portC, networkAdapter (int), nonSecure (int), eventList, hardware, boot, voiceList, serial, rpCode` (the last six read-only).
- Read: head = lines before the users block; the header record spans lines until the double quotes balance (`span`); require
  `head.Length == span + 47` and 25 fields, otherwise `details` is null. Fields are taken from the raw comma split (commas inside quotes kept, quote characters dropped).
- Write (`SetDetails`): do nothing unless an editable value differs from what the file has (so unedited accounts are byte-identical). Otherwise
  replace only the raw tokens of the changed fields with `"value"`, rebuild the header lines (split on `\n`, one line each, written with the file's line ending),
  keep the other 45 lines as they are, and replace the adapter / non-secure lines only if their value changed. Refuse a `"` in any text, a line break outside Notes,
  text longer than the ElkRP limits (name 50, address 50, city 30, state 3, zip 10, folder 100, voice 20, fax 20, e-mail 30, web 30, notes 255, system phone 20,
  system URL 50), non-digit ports, adapter outside 0-2 and non-secure outside 0-3. Error texts: see `crates/elk-account/src/details.rs`.
- Keep the new head in a separate field (the Rust `AccountFile.head`), because line indexes (users_at, zones_end, ...) refer to the original lines.
- Tests added in Rust: `crates/elk-account/tests/details.rs` (read, untouched round trip, edit, refusals); the golden model JSON files recorded from the old
  C# server are compared with the `details` member cut out (`without_details` in `tests/golden.rs`).

## 6. Automation > Rules: ElkRP's wording and a rule editor (2026-11-20, ADR-011)

Diff of the page: `rust/ui-changes-rules.diff` (`git diff -- rust/app/assets/index.html` before this commit). It replaces the whole `autoRules` function
(`// rules: a structural view ...` up to `// lights (X10 / lighting list)`) with the block starting `// ---- rules: ElkRP's Rules page` and adds
about ten CSS lines (`.rbar .rtab .rsel .rtxt .rw .ra .rt .rcom .rfoot .rpart .rhead .rprev`, a wider `#modal .box`) before `input.t.wide`.
`engFetch` turns the 400 body (a JSON string) into the error text.

Server side the C# needs (all of it is in Rust: `crates/elk-account/src/english.rs`, `rules.rs`, `app/src/server.rs`):
1. `POST /api/rules/english` (body = the page's model, like `/api/save`): apply the model to a copy of the opened file with the save code, then answer
   `{"rules":[[line...]...],"rulePacketsUsed":n,"textPacketsUsed":n,"percent":n}`; a refused model gives the save's 400 text. `percent` = round-half-even((rule + text packets) * 100 / 528).
2. The wording: a port of ElkRP `WhenSelect.ConstructEnglishTrigger / Condition / Result`. Lines of a rule: `WHENEVER ...`, one `AND ...` per condition, one `THEN ...` per result.
   Packet grouping = `WhenThen.PopulateRules`: first packet; while byte 4 / 8 == 1 the next packet is an AND; the THEN is in the packet that ends the chain; more THENs in
   packets with byte 0 / 8 == 18; a packet with byte 0 / 8 == 19 straight after a THEN is its FOR. Names: zone `name (Zn n)`, area `name (Area n)`, keypad `name (KP n)`, user `name (User n)`
   (201 = `"Installer" (User 201)`, 202 = `"ElkRP" (User 202)`), task `name (Task n)` or `Task n`, output `name (Out n)` or `Output n`, counter `name (Counter n)`, thermostat `name (Tstat n)`,
   custom value `name (CO Timer n)` for 1-12 and `name (Cust Set n-12)` above, light `name [n (A1)]` or `Lighting n (A1)`, telephone `name (number)`, voice message = the part after the last `|`.
   Events: `event-list-m1g.tsv` columns TrigPre/PostPro/Con and CondPre/PostPro/Con; Packet[1] != 0 uses the Pro column; `GetName` swaps `ZONE n`, `AREA n`, `KEYPAD n`, `OUTPUT n`,
   `AUTOMATION TASK n` inside the event text for the account's names.
3. `/api/meta` member `eventWords` (event number to its 8 phrases, in the order above).
4. `rules::apply` validation of rules that are new or changed (list in ADR-011), messages like `rule 3: zone must be 1-208 (it is 0)`.

Page behaviour: selection (click, Shift, Ctrl); double-click or Edit opens the dialog. The dialog keeps a draft `{trig, conds[], thens[{..., for}]}` and `compose()` makes the packets
(trigger bytes 0-3 plus the first THEN's bytes 4-7 in the last chain packet, `[8,0,0,0]` as the result half of the other chain packets, extra THENs `[144,0,0,0,...]`, FOR `[152,...]`);
`decompose()` is the inverse and falls back to hex mode for anything the form does not know. Each kind is `{id,label,fields,def,build,parse}` in `KINDS.trig / cond / then`.
Every edit calls `/api/rules/english` with the page's model (the dialog puts its draft in a copy), so the preview and the refusal text are the server's. The list's wording is cached by a
signature of packets and names (`engSig`) and refreshed after a render.

## 7. Panel sync of automation items (2026-11-20, ADR-012): Rust only
The C# editor has no automation sync; this is a feature, not a page patch. What the C# would need:
- Server: `PanelSync.Diff/Pull/Apply` get the same extra kinds (`Elk.Protocol`): section 2 with part bytes 0 (custom values 1-32, 16 bytes), 1 (outputs 1-64, 28), 2 (counters 1-64 and thermostat names 65-80, 12), 3 (tasks 1-32, 28); section 14 lighting 1-256 (28 bytes each, 16-bit item number, read 4 per request, write 1); section 13 voice messages (file 1-115 = items 209-323, 12 bytes each, read 16 per request, firmware 4.0 or later); section 121 / write 249 rule and text packets (8 bytes each, 16 per request, 16-bit first packet; rules from packet 0, texts the last T of 528, a packet starting with two zero bytes ends each). Check the three echo bytes of every reply. Details and sources: `rust\DECISIONS.md` ADR-012 and `rust\crates\elk-core\src\panel_auto.rs`.
- Routes: an optional boolean `automation` in `PanelRequest` (compare, send, pull, backup); with it `counts` / `changes` / `skipped` also carry the automation rows (labels in ADR-012) and the backup (`PanelFile`) gets an `AutomationBlocks` array (`Kind`, `Number`, `Hex`; `packets` = all 528 packets raw). Without the flag nothing changes.
- Page: in the Panel dialog one checkbox row before the Advanced section (`const autoBox = el('input', { type: 'checkbox', checked: true })`), `automation: autoBox.checked` added to the objects returned by `req()` and `conn()`, and one extra sentence in the result hint (`git diff` of `rust/app/assets/index.html` for this commit).

## 8. M1XEP settings (Rust editor only, ADR-013)

A new dialog opened from the Account Details page (Overview tab, Network Adapter card, shown when the adapter is M1XEP: row "M1XEP Setup", button "M1XEP settings..."). It is a Rust-only feature: the
C# editor has no M1XEP code at all (no `/api/m1xep/*`), so there is nothing to port unless the same feature is wanted there; the protocol and layout are in `M1XEP-NOTES.md`.

What the page does, for anyone who wants to port it:
- Connection fields like the Panel dialog (address, mode plain / TLS / AES, port, serial, RP code), prefilled from the account details (System URL/IP, port, serial, RP code).
- Buttons: Read from the M1XEP, Find, Compare with this account file, Review changes, Send changes to the M1XEP, Close.
- After a read: sub-tabs named like ElkRP's window (TCP/IP Settings, Passwords, Email, Dynamic DNS, Time Server, Audio System, Central Station read only), the fields in ElkRP's wording; fields that
  decide how the module is reached or who may log in are marked `*`. Passwords are hidden unless "Show passwords" is ticked.
- Review changes (`/api/m1xep/write` with `apply:false`) lists tab, field, old and new value. Two ticks gate Send: always "I understand this writes ... a backup is saved first", and, only when an
  identity field changed, a separate red warning that a wrong value can lock you out of the panel's network connection. The result shows what was sent, the backup file, whether the read-back matched,
  and that a reboot is needed (the editor never reboots the module).
- Compare with this account file lists the settings that differ between the module and the M1XEP block stored in the open file (`/api/m1xep/file`); the file is not changed.
- Backups of the module settings (folder "ElkEditor backups", files `m1xep-...`): Load into the form, then review and send like any change.

Implementation: one `<script>` block at the end of `assets/index.html` (`XEP_FIELDS`, `openM1xep()`), plus one row in `accountDetails()`:
`ad === 0 ? row('M1XEP Setup', el('button', { type: 'button', onclick: () => openM1xep() }, 'M1XEP settings...'), 'Rust editor only') : null`.
Recorded case changed: `index-page` (the page). New routes have no recorded cases (no real module to record against); they are covered by `elk-core/tests/xep.rs`,
`elk-panel/tests/m1xep.rs` and `server.rs`'s `m1xep_routes_are_wired`.


## 9. Automation > Rules: Enable / Disable (Rust editor only, ADR-014)
ElkRP keeps a per-rule `Disabled` flag only in its own database (the rule is struck through and grey; Send skips it; Receive is refused while any rule is disabled). The export file cannot carry it,
so this is an editor-side flag, kept in the page's model (`rules[i].disabled = true`, absent when false) and therefore in Undo / Redo / History and the unsaved-draft copy.
- Page: the Enable and Disable buttons of the Rules toolbar act on the selected rules (a rule with AND / THEN lines is one group, so the whole group). Disabled rows get class `rdis` (line-through, grey).
  A note under the list says that saving the file keeps all rules and that disabled rules appear enabled in ElkRP; the save messages repeat it (`saveNote()`). The history describes the change as `Rule 3: disabled`.
- Model: `RuleDto.disabled` (serde, `skip_serializing_if` false), ignored by `rules::apply`, so a save of a model with disabled rules equals the unedited file byte for byte.
- Panel: `/api/panel/compare` and `/send` with `"automation": true` drop the disabled groups from the packet area before comparing or writing (`rules::drop_disabled`: the enabled rules follow each other
  from packet 0, the end marker follows them, the texts at the top are untouched) and add the `skipped` line "N disabled rule(s) are not sent ...". `/api/panel/pull` takes `"disabledRules": n` (the page counts)
  and answers 400 "Rules cannot be received while some are disabled. To receive Rules, you must first enable all of them." when n > 0 and automation is on. The "n% used" figure counts every rule, as ElkRP does.
- The flag is lost when a file is reopened from disk (the file cannot hold it); the accounts library (ADR-015) keeps it next to the stored file.
Recorded case changed: `index-page` (the page).


## 10. Accounts library (Rust editor only, ADR-015)
A start page and a dialog that keep saved accounts in folders in a SQLite file beside the program. Nothing of this needs the C# editor, but the page part can be copied.
- Header: `Library...` (before Open export file...) opens the list in the modal; `Save to library...` (after Save edited export...) saves what is open. The start page (`#welcome`) shows the same list in `#libHome`
  when no file was given on the command line. The page code is the last `<script>` block of `assets/index.html` (`libApi`, `libPaint`, `libSaveDialog`, ...), prefixed `lib`.
- List: folders (nested, rename, move, delete: an empty folder with one confirming click, a full one only with "Delete folder and its N account(s)"), accounts (name, saved time, size, versions; Open, Details, Move,
  Export, Delete), search over name / notes / tags, "Import export file...", "Back up library" and "Keep the last N saves of each account". The data folder in use is printed under the list.
- Opening an account loads it exactly as Open does (`POST /api/library/open?id=` answers `{file, model, library}`; the server also sets its open-file state). Saving sends `{id?, name, folderId, notes, tags, panel, model}` to
  `/api/library/save`: the server applies the model to the open file exactly like `/api/save`, stores the bytes, and keeps the previous state as a history version when something changed. Disabled rules (ADR-014) are stored
  next to the file and come back on open. `load()` sets `LIB` from `d.library`, so a normal Open forgets the link.
- Panel connection: optional (address, port, mode, serial, MAC); the RP access code and any passphrase are dropped by the server unless `rememberCode` is true.
- Native window: import uses `{cmd:'openFile', purpose:'libimport'}`, export `{cmd:'saveFile', purpose:'libexport'}` (the host passes unknown purposes through, so `elk-planner-window` is unchanged).
- Routes: see ADR-015. Recorded case changed: `index-page` (the page).


## 11. About and updates (Rust editor only, ADR-016)
- Header: a small `v0.2.0` after the title (`#verTag`, from `GET /api/update/info`) and an `About / Updates` button. The dialog shows the version, where the program and the library live, the feed address and a
  `Check for updates` button (also in the Library footer and on the start page). Nothing is requested until a button is clicked. A newer release shows its notes; `Download and install X...` needs a second click
  ("Yes: download ..., check it, install and restart"), then the server downloads the zip, checks its SHA-256 against the release's `SHA256SUMS`, swaps the files and restarts (browser mode: the page polls
  `/api/update/info` and reloads when the new version answers).
- Routes: `GET /api/update/info`, `POST /api/update/check`, `POST /api/update/install` with `{"confirm":true}`. Code: `app/src/update.rs`, `update_routes.rs`; the page part is the end of the last `<script>` block (`showUpdates`, `updPaint`).
- Recorded case changed: `index-page` (the page).

## Windows: no console window (Rust only, ADR-017)
The Rust release exe is a GUI-subsystem program, so double-clicking it opens only the editor window. It attaches to the parent console when started from a terminal, so the startup line and errors still print. No page or route change; nothing to port to the C# editor.

## Rust only: Panel dialog, Tools menu, backups in the library (ADR-018)
Not for the C# editor (it has no library). Summary of the page changes, all in `assets/index.html`: Panel dialog: no prefill / placeholder for address and serial, RP access code in the main row, "Save this account to the library after connecting / reading" in the main part, "Include automation items" under Advanced, "Saved connection (library)" picker, "Backups... (in Library)" link; Live tab form: same fields, code in the main row; header: Tools menu (`#toolsBtn`, `#toolsBox`) with "Data bus plan..." and "Save edited export..." (`#saveBtn` is hidden and still the code path), primary "Save to library..."; the Data bus is no tab (`sectionOf` returns `{k:'databus'}` for it); backups per account: `libBackups(a)`; autosave: `libAutoSave`; start page without Check for updates / Plan the data bus. New routes `POST /api/panel/backup/delete`, `POST /api/panel/backups/open`; setting `autoSavePanel`.
