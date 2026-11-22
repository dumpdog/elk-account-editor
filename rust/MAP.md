# Port map

Goal: pure Rust Elk account editor, behaviour identical to C# (see DECISIONS.md ADR-001..008).

| Wave | Unit | Rust | State |
| --- | --- | --- | --- |
| 1 | Framing, Records, DefaultRecords | `crates/elk-core` | done (63 tests, golden vs C#) |
| 1 | AccountFile, AccountModel, AccountExport, ZoneImport, ZoneSheet, AlarmZoneList | `crates/elk-account` | done (49 tests, golden vs C# server) |
| 1 | DataBusPlan, NativeWindow (wry/tao/rfd) | `crates/elk-planner-window` | done (35 tests, 17 golden plans; window checked on Windows) |
| 2 | Handshake, ElkSession, PanelSync/Backup/Control/Live/Modules/ClockLog/Progress | `crates/elk-core`, `crates/elk-panel` | done |
| 3 | Elk.Editor Program.cs (all routes), LivePanel | `rust/app` (`src/server.rs`, `src/main.rs`) + `elk-panel` handlers | done: 23/23 replay, fallback 0.0% |
| 4 | Automation items (`Automation.cs`, `Rules.cs`, `Data\*.tsv`; branch automation-items) | `crates/elk-account` (`automation.rs`, `rules.rs`, `src/data/*.tsv`); `/api/meta` members and `assets/index.html` in `rust/app` | done: 14 tests, golden vs the automation oracle (model JSON, meta, 64 saved files, 136 error messages), 48-case replay |
| 3 | Elk.Cli Program.cs | `rust/cli` (binary `elk`) | done: offline commands compared with the C# CLI, panel commands against the simulated panel |

Results (2026-11-19, Windows, Rust 1.100):
- `rust-anything app replay . --build rust/app/target/{debug,release}/elkaccounteditor.exe`: 23/23, fallback rate 0.0% (0/23).
- `rust/app`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (10 tests) all pass. `rust/cli`: same, 13 tests.
- Release sizes: `elkaccounteditor.exe` 3.0 MB, `elk.exe` 1.1 MB (opt-level s, lto, strip).
- `app lift`: refused ("no unit is wired yet"), see ADR-007; the proxy was removed by hand.

Out of scope: none. The Automation tab (branch `automation-items` of Elk-main, commit 7265d2b) is ported, see ADR-009; panel sync of automation items and the M1XEP settings are not done in the C# either. Recording: `rust/app-cases` (HTTP), see HANDOFF.md.

Rust-only addition: M1XEP settings (`elk-core::xep_config`, `elk-panel::m1xep`, the dialog in `assets/index.html`), see `M1XEP-NOTES.md` and ADR-013.