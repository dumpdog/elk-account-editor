# elk-panel porting notes

Standalone crate (edition 2021, own `[workspace]` and `Cargo.lock`, path dependencies on `elk-core` and `elk-account`). Ports `Elk.Protocol/PanelSync.cs`,
`Elk.Editor/LivePanel.cs` and the request/response logic of the panel and live routes of `Elk.Editor/Program.cs`. Blocking `std` code only (the server calls it
through `spawn_blocking`). The C# under `port\` was not touched.

## Public API

| C# | Rust | Main items |
|---|---|---|
| PanelSync.cs | `sync` | `PanelChange`, `SyncResult` (`changes`, `counts: BTreeMap<String,(same,differ)>`, `skipped`), `PanelSync::{diff, pull, apply, estimate_diff_transfers, estimate_pull_transfers, write_transfers}`, `Settings` (= `PanelConnection.Settings`), `PanelConnection::open(settings, log, attempts, delay)` |
| LivePanel.cs | `live` | `LivePanel::{connect, disconnect, use_session, use_open, change_access_code, enroll, poll, connected, host, error, firmware, serial}`; pub fields `attempts` (6), `retry_delay` (5 s), `enroll_wait` (12 s) so tests can shorten them |
| Program.cs routes | `routes` | `handle_panel_{compare, send, pull, backup, backups, restore, progress}`, `handle_live_{connect, control, modules, enroll, time, settime, log, accesscode, disconnect, status}`, each `fn(&Shared, body: &[u8], &Query) -> HttpResult`; `handle(&Shared, method, path, body, &Query) -> Option<HttpResult>` dispatches by method and path (`None` = not a panel/live route) |
| - | `routes` | `HttpResult {status, content_type, body}`, `Query::{new, parse, get}`, `Shared`, `Session`, `template_text()`, `default_backup_dir()`, `parse_datetime()` |
| - | `error` | `PanelError {Elk, Account, Other}` with `kind() -> Kind` (the .NET exception class) |
| tests\SimulatedPanel use | `testkit` | `seed`, `seed_defaults`, `add_reset_leftovers`, `add_pull_extras`, `setup(name, panel, file)` |

### Shared state

```rust
pub struct Shared {
    pub session: Mutex<Session>,        // the open file: original_text: Option<String>, source_name: Option<String>
    pub live: LivePanel,                // the shared panel session (own internal locks)
    pub progress: Arc<PanelProgress>,   // /api/panel/progress
    pub backup_dir: PathBuf,            // Documents\ElkEditor backups
    pub control_delay: Duration,        // 600 ms pause before the status read after /api/live/control
}
```

`Shared` is internally synchronised: hold it as `Arc<Shared>`, no outer mutex is needed (and none should be held across a handler call: a compare can take
a minute, and `/api/panel/progress` and `/api/live/status` must answer meanwhile). The non-panel routes (`/api/open`, `/api/new`, `/api/model`,
`/api/save`, ...) share the open file through `shared.session()` (`original_text` is the Latin-1 decoded text; `source_name` the `?name=` / file name /
`New account` / `From panel`). `/api/panel/pull` creates the blank account itself when nothing is open (the template is embedded:
`src/template-m1g.txt`, a copy of `Elk.Editor/template-m1g.txt`). Call `shared.shutdown()` on exit (`ApplicationStopping`). `Shared::with_backup_dir`
is for tests; `Shared::new()` uses `dirs::document_dir()` on Windows (same as .NET `MyDocuments`, a redirected Documents folder included) and the home folder
elsewhere (what .NET returns on macOS/Linux).

## Decisions

- **Locking.** `SemaphoreSlim` becomes a `Mutex` around the session. The five status fields live behind a second small mutex so `/api/live/status` and
  `/api/live/connect` can read `connected/host/error/firmware/serial` while a long operation holds the session (the C# reads them without the gate).
  `host` stays set after a disconnect or a broken link, exactly like `_settings` in the C#.
- **Errors.** `PanelError` carries an `ElkError`, an `elk_account::Error` or an "unhandled" message. `Kind` mirrors the .NET exception class, and every
  route has the same catch list as `Program.cs` (e.g. `compare`: Timeout, IO/Socket, UnauthorizedAccess, ArgumentException (incl. OutOfRange), InvalidData).
  Anything not in the list returns the empty **500** ASP.NET sends for an unhandled exception (`HttpResult::server_error`), also for malformed JSON on
  `/api/panel/backup` and `/api/panel/restore` (the C# has no try there).
- **Link errors** (`Timeout`, `Io`, `InvalidData`) close the shared session and set `error`, as `IsLinkError` does. `poll` swallows them (returns `None`),
  `use_session`/`use_open` close and re-raise, `enroll` only reopens when the follow-up read breaks.
- **JSON output** is written by hand (key order = the C# anonymous objects), `Results.Json` strings with the default encoder (`'` ...) and doubles with
  the .NET double format (`jsonw.rs`, copied from elk-planner-window). **`Results.BadRequest(string)` uses a different encoder**: the body is a JSON string
  where only `"`, `\` and control characters are escaped and `' & < > + \`` and non-ASCII BMP text stay raw (found by probing; astral characters become
  surrogate escapes). `push_str_relaxed` does that. Content type is `application/json; charset=utf-8` for both.
- **Request parsing** follows System.Text.Json web defaults: member names ignore case, numbers may be quoted (`"port":"2101"`), unknown members are ignored,
  `null` is fine for strings but an error for int/bool, a missing member is the default. The `JsonException` texts are reproduced for the cases probed
  (empty body, bad start of a value, text after the value, trailing commas, unterminated string/container, "could not be converted to `<Record>`" with the
  byte position after the value, first bad member in document order wins) in `jsonerr.rs`.
- **Connect errors**: a refused connection or a host that does not resolve gives the same text as .NET on Windows (the OS text with Rust's ` (os error N)`
  suffix removed). A port outside 0..65535 gives .NET's two different texts (stock text for an IP literal, the long `port ('65536') must be ...` text for a
  host name). Retry log lines are `login attempt N failed (TimeoutException|SocketException|IOException); retrying in 5 s`.
- **Backups** are written with `fs::write` (UTF-8, no BOM, `PanelFile::to_json` line ends of the host, like `File.WriteAllText`); the restore reads them
  with BOM stripping and takes only the file-name part of `file` (`Path.GetFileName`: both separators on Windows).
- **Progress** uses elk-core's `PanelProgress` through `ElkSession::on_transfer` (an `Arc` clone ticks); the transfer hook is removed again after every
  operation like the `finally` in the C#; `end()` runs after every tracked route.

## C# behaviour not matched exactly

1. **`DateTime.TryParse` for `/api/live/settime`** accepts a fixed list of formats here (`yyyy-MM-dd[ T]HH:mm[:ss[.f]]`, `yyyy-MM-dd`, `MM/dd/yyyy [HH:mm[:ss]]`
   with optional AM/PM, a few month-name forms); .NET accepts many more.
2. **`JsonException` wording** for syntax errors other than the ones listed above (e.g. a bad escape, an invalid number, a control character inside a string)
   and for errors inside the nested `model` object (`$.model.zones[0].n ...`: .NET names the .NET type, here serde's text plus `Path: $.model`) differ.
   A wrong-typed `model` member itself (`"model":5`) matches.
3. **Culture**: the code assumes the invariant/en-US culture for number and date text, as the other crates do.
4. **elk-core texts worked around here** (elk-core was not edited): `PanelControl` and `PanelClock::set` raise `OutOfRange` with their own wording; the C#
   `new ArgumentOutOfRangeException()` has the framework text `Specified argument was out of the range of valid values.` and the year check ends in
   `(Parameter 't')`. `fix_range` in `routes.rs` restores both before the message goes to the page. `PanelBackup::from_json` failures say
   `not a panel backup file: ...` where .NET gives the `JsonException` text (`/api/panel/restore` message after `Restore failed: `).
5. **Non-Windows**: connect error texts are the OS's, not .NET's (only the Windows texts were compared). `GetFolderPath(MyDocuments)` on Linux/macOS is
   assumed to be the home folder.
6. **Unhandled-exception cases** that the C# turns into a 500 are 500 here too where they were found (see Decisions), but a few internal index errors the C#
   would throw as `IndexOutOfRangeException` (a panel reply shorter than the record) are `PanelError::Other` here, which is also a 500.
7. **`SortedDictionary<string,...>`** orders by the culture comparer in C#; `BTreeMap` orders ordinally. They agree for every label the code produces
   (checked by the `comm_sync`/`template_reset` golden scenarios, which include the `(reset)` labels).
8. **No timing fidelity**: C# waits 600 ms after a control command and 12 s in enroll; both are fields (`Shared::control_delay`, `LivePanel::enroll_wait`) so tests set
   them to zero. Defaults are the C# values.

## Tests (40, all passing; `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean)

| file | tests | content |
|---|---|---|
| `tests/sync.rs` | 9 | PanelSyncTests (identical, diff-apply-diff, pull, estimates, pull progress, progress growth), PanelResetTests, plus local code / globals and label ordering |
| `tests/live.rs` | 21 | LivePanel (connect/poll/disconnect, AES, borrowing the open session, other-panel session, link errors, access code, enroll, retries, rejected code), `Query`, date formats, relaxed JSON string, backups list, idle status/progress, progress cleared after a tracked route |
| `tests/golden.rs` | 8 | seven scenario replays against the C# answers (below) and the placeholder normaliser |
| `src/jsonw.rs` | 2 | string and double formats |

### Golden fixtures

`tests/fixtures/scenarios.json` lists seven scenarios (about 125 steps): `comm_sync` (compare clean/edit/reset/globals, send with backup, restore preview/apply/after, manual
backup, path tricks, missing and blank file), `template_reset` (reset of a default panel from the blank template), `pull_extras`, `pull_blank`, `errors` (about 40 bad bodies and checks,
wrong code, no MAC / serial, empty bodies on backup/restore/pull, type errors), `unreachable` (closed port: six attempts, log text), `live_session` (every live route including borrowing,
error state after a timeout, access code change and reconnect, enroll). `tests/fixtures/expected/<scenario>__<step>.{status,body,file}` is what the C# editor
(`rust\original-build\ElkAccountEditor.dll`) answered when it talked to the Rust `SimulatedPanel` in plain mode; `.file` is the backup file the editor wrote. The Rust tests replay the same
requests (`requests/*.json`, generated by `cargo run --example gen_requests`) against `handle(...)` and a fresh `SimulatedPanel` and compare status, content type and body
byte for byte, and the backup files byte for byte. Only clock times (`at`, `computer`), the backup folder and the time stamp in backup names are replaced by placeholders on both sides.

Regenerate with `powershell -File tests\fixtures\regen.ps1 [-Only <scenario>]` (about four minutes in all: the C# retries a refused connection for 25 s, and enroll waits 12 s). It builds
`examples/sim_panel.rs` (a plain-mode simulated panel on a loopback port, `sim_panel <setup> <sample|@template>`, prints `PORT n`), starts a fresh C# editor per scenario, and removes the
backups the editor wrote into `Documents\ElkEditor backups` afterwards. Needs `dotnet` and the samples in `sample\`.

Not covered by fixtures: `/api/panel/backups` against the real Documents folder (tested with a temp folder), the AES and TLS panel modes of the routes (the sync code is mode-independent;
AES is tested in `live.rs`), a real panel.

## Automation sync (Rust only, ADR-012)
- `auto_sync.rs`: `diff` (compare per kind, rules / texts as units via `rules_match` / `texts_match`), `pull`, `apply_change`, `packets_of` (the refusals: budget, zero-start rule packet), estimates. `PanelSync::diff_with / pull_with` add the `automation` switch; `diff` / `pull` keep their C# behaviour. `PanelChange` is reused: items use the kind label and the display number, `rule packets` / `text packets` use section 121 and part 0 / 1.
- Routes: `"automation": true` in `PanelRequest` (compare, send, pull, backup); the send backup then holds the automation items. Without it the answers equal the C# ones.
- `testkit::seed_automation` fills the simulator; `tests/auto_sync.rs` (compare, send, read back, zero writes, budget refusal, estimates, 8-character text) and `tests/auto_routes.rs`.
## Rust-only addition: `m1xep` routes (ADR-013)

`src/m1xep.rs` serves `/api/m1xep/{read,file,find,write,backups,load}` (not in the C#). `routes::handle` calls `m1xep::handle` first; `routes.rs` only gained that call and `pub(crate)` on `PanelReq`, `parse_panel_req`,
`settings_of`, `blank`, `fail`, `PANEL_CATCH` and `now_hms`. The write route reads the module itself, answers a diff, and sends only with `confirm` (and `allowIdentity` for IP / mask / gateway / DHCP / DNS / ports / logins) after saving a
`m1xep-<serial>-<time>-before-write.json` backup. Tests: `tests/m1xep.rs` (11, against `SimulatedPanel` with the simulated module on).
