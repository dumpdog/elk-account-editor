# elk-account porting notes

Port of `port/src/Elk.Protocol/{AccountFile,AccountModel,AccountExport,ZoneImport,ZoneSheet,AlarmZoneList}.cs` (standalone crate,
own Cargo.lock; target dir used while developing: `rust/target-agent-account`).

## Public API (all re-exported from `lib.rs`; one module per C# file)
- `account_file`: `AccountFile::{parse(&str), to_text(), globals(), set_globals(), wireless(), set_wireless(), code_length(), software_version(),
  account_id(), panel_type(), has_comm(), has_phone_section()}`; pub fields `users/areas/keypads/zones/phones/area_rcs/user_rcs/zone_rcs`
  (`Section { entries, trailer, .. }` with `get/get_mut/get_or_add`), `system_rc`, `keypad_rc`, `header`; `decode_latin1/encode_latin1`
  (the file is Latin-1 text), `split_csv_line`.
- `account_model`: `AccountModel::{from(&AccountFile), apply_to(&mut AccountFile), validate_wireless_groups, decode_code, encode_code,
  to_json(), from_json()}`, all DTOs (serde, camelCase, C# property order, nulls written), `ModelMeta` (flag tables, labels, definitions/types).
- `zone_import`: `ZoneVoice`, `ZoneSpec` (+`to_record`), `ZoneCsv::parse`.
- `account_export`: `AccountExport::{find_zone_block, apply_zones}` (CLI `export-zones`).
- `zone_sheet`: `ZoneSheet::{read, read_delimited, lookup, table, to_csv, to_xlsx}`; `ZoneSheetResult::to_json()` is the `/api/zones/read` body.
- `alarm_zone_list`: `AlarmZoneList::{build(&model, title, now), template_bytes()}`; template is `src/AlarmZoneTemplate.xlsx` via `include_bytes!`.
- `json`: `to_string` writes JSON like ASP.NET `Results.Json(.., Web)`: camelCase from serde, compact, and strings escaped like
  `JavaScriptEncoder.Default` (`" & ' + < > ``, every non-ASCII char as `\uXXXX`). Plain-string 400 bodies from
  `Results.BadRequest(msg)` are NOT escaped that way (they are written relaxed); use `serde_json::to_string` for those.
- `error::Error` mirrors the .NET exception kinds (`Format`, `InvalidData`, `OutOfRange`, `InvalidOperation`, `Argument`); `message()` is the C# `Message`.
  The server catches Format/InvalidData/OutOfRange (and JSON errors) as HTTP 400.
- `records` / `xml`: small private-style helpers (ZoneRecord, ZoneFlag1/2, CRC-16, a mutable XML tree) made public for tests; do not depend on them from elk-core.

## Tests (all pass; `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`)
- `tests/ported.rs`: 39 ports of the C# unit tests (AccountExport, AccountFile, wireless groups, Communicator, Telephone, Delete, NoPhone,
  Globals, Wireless, ZoneImport, ZoneSheet, AlarmZoneList). Tests that need `TESTING\` files skip when absent (they are absent here).
- `tests/golden.rs`: 10 tests against fixtures produced by the real C# server: every file in `sample/` saves byte-identical (also through JSON),
  model JSON equals `/api/open` byte for byte (all 4 samples), 60 save requests (16 edited/pruned saves compared byte for byte, 44 error
  messages compared), zone CSV and xlsx exports, Alarm Zone List xlsx (every zip part byte-identical, including sheet XML), zones/read
  (15 inputs incl. xlsx, BOM, UTF-16, Latin-1, errors, JSON escaping), CLI `export-zones` output and log.
- Fixtures: `tests/fixtures/` (README, `regen.ps1` re-runs the C# server).

## Not matched exactly / known differences
- .NET exception texts that come from the runtime are only reproduced for the cases tested: integer parse (`The input string 'x' was not in a
  correct format.`, `Value was either too large or too small for an Int32.`), byte parse, and the zip-open failure of `ZoneSheet::read`
  (always `Could not read this Excel file: Central Directory corrupt.`). Other XML/zip error texts inside "Could not read this Excel file:" differ.
- Unicode classification (`char.IsLetter/IsDigit/IsLetterOrDigit/IsWhiteSpace`) uses Rust's `char` methods; they differ only for exotic
  characters. User code digits accept ASCII digits only (C# `char.IsDigit` also accepts other Unicode decimal digits and then produces garbage bytes).
- Cases where C# throws an unhandled exception (HTTP 500) return an `Err` here instead: area cell with a digit run above Int32 in a sheet,
  a workbook with no sheets, duplicate relationship ids.
- Name cut/length checks count UTF-16 units where C# does, chars elsewhere (zone sheet name cut to 16 chars uses chars); identical for BMP text.
- XML text escaping of CR/LF inside `Element` text follows `XmlWriter` only for `\r`; a title containing a newline or control characters is
  not handled like C# (C# would throw for invalid XML characters).
- `AlarmZoneList::build` takes the date as `Option<chrono::NaiveDate>` (local today when None) so tests can pin it.
- The C# `NumberStyles`/culture-dependent `int.Parse` calls assume the invariant/en-US culture.

## Automation items (port of `Automation.cs`, `Rules.cs`, `Data/*.tsv` of Elk-main branch automation-items)
- Modules `automation` (`AutomationBlock` with `try_parse`/`emit`, DTOs, `AutomationCodec::{from, apply}`, text packing, `AutomationMeta`, `OrderedMap`) and `rules` (`group`, `decode`, `apply`, `text_refs`, `remap_text_refs`, `event_names`). TSVs in `src/data/` (the `autoarm` table is not used by the C# code either).
- `AccountFile.automation: Option<AutomationBlock>`, `AccountModel.automation: Option<AutomationDto>`; unedited files stay byte-identical (the block is only parsed when every value is a canonical byte).
- Tests: `tests/automation.rs` (14: the 13 C# `AutomationTests` plus the "no automation section" message), golden tests in `tests/golden.rs` against the automation oracle: `model/` (4 samples), `meta.json` (the 7 new members), `saved/s<i>_auto_*` (64 saved files, 136 error messages, from `requests/s<i>_auto_*.json` made by `examples/make_auto_requests.rs`). `regen.ps1` now starts `rust/original-automation-build`; the 16 older saves came back byte-identical.
- Known differences: JSON `null` for a non-null string field (C# throws a NullReferenceException, HTTP 500) is a serde error here (HTTP 400), like the other DTOs; a rule/text header whose two counts overflow Int32 when added is rejected (C# wraps); `byte.TryParse` HexNumber quirks in `{ff:xx:xx:xx}` are reproduced for white space and trailing NULs only. The unverified points of the C# (ElkRP import of an edited file, disabled rules) stay unverified.

## Rule wording and creation (Rust only, ADR-011)
- `english.rs`: `Names::from_file`, `Ctx` (trigger / condition / result text), `lines_of` (one rule), `rule_lines`, `file_rule_lines`, `english_report` (the route answer), `percent_used`, `event_words_json` (meta), `printable` (text strings). Port of ElkRP's decompiled `WhenSelect` builders; ADR-011 lists what is exact and what is a fallback (e-mail and audio names, control characters in texts).
- `rules.rs`: `validate_rule` (new or changed rules only) with `check_trigger / check_condition / check_result`.
- Tests: `tests/english.rs` (sample account wording, per-type wording, FOR, footer rounding, round trip of a created rule, refusals, "old odd rules still save", a TESTING check that only looks at line shapes).
- The model JSON is untouched; `RuleDto.summary` is still the structural reading.

## Panel sync of automation items (ADR-012)
No change in this crate. The sync works on `AutomationBlock` (record arrays, `packets`, `rule_packets_used`, `text_packets_used`, `comments`) in `elk-panel/src/auto_sync.rs`.
