# Glossary (C# name -> Rust name)

- Export file / AccountFile: ElkRP "Export Account" text file, one value per line -> `elk_account::AccountFile`.
- AccountModel: editable JSON view of an export -> `elk_account::AccountModel` (camelCase JSON, C# property order).
- ModelMeta: flag tables and labels served by `/api/meta` -> `elk_account::ModelMeta`; `busModels` and `zoneExpanderSwitches` come from `elk_planner_window::data_bus_plan`.
- Zone definition (ZoneDto.Definition): 0-36 type of zone (Fire Alarm = 10) -> `definition: u8`.
- Panel / session: M1G control reached through an M1XEP -> `elk_core::ElkSession`.
- LivePanel / Shared: the shared panel session plus the open file (`originalText`, `sourceName` of Program.cs) -> `elk_panel::Shared` (hold as `Arc<Shared>`, handlers are blocking: call them through `spawn_blocking`).
- HttpResult: status, content type and body of one answer -> `elk_panel::HttpResult`; `Results.Json` = `HttpResult::json`, `Results.BadRequest(string)` = `HttpResult::bad_request`, `Results.File` = a plain struct with the given content type, `Results.NoContent` = status 204 with no content type.
- Dispatch: the route table of Program.cs -> `rust/app/src/server.rs::dispatch(&Shared, method, target, body)`.
- Front door: the Rust axum entry point that used to proxy to C#; now it is the server itself (`rust/app`).
- Native window: Photino.NET `NativeWindow.cs` -> wry + tao (`elk_planner_window::run`), `--browser` skips it.
- CLI: `Elk.Cli` -> `rust/cli`, binary `elk`, `elk_cli::run(args, out, err)`.- AutomationBlock / AutomationCodec / AutomationDto: the automation items after the report codes (tasks, counters, custom values, outputs, lights, voice messages, rule packets, texts, rule comments) -> `elk_account::{AutomationBlock, AutomationCodec, AutomationDto}` (`automation.rs`); `AccountFile.automation`, `AccountModel.automation` (JSON key `automation`).
- RuleCodec / RuleDto: structural view of the rules -> `elk_account::rules` (`rules.rs`), `RuleDto`.
- AutomationMeta: label tables (words, voice messages, outputs, events, custom types, light types and formats) embedded from `crates/elk-account/src/data/*.tsv`; `/api/meta` members `words, voiceMessages, outputNames, eventNames, customTypes, lightTypes, lightFormats` -> `AutomationMeta::{words, voice_message_names, output_names}`, `rules::event_names`, `OrderedMap` (a .NET `Dictionary` that keeps insertion order for JSON).
