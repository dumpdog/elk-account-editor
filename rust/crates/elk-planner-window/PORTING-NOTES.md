# elk-planner-window: porting notes

Standalone crate (edition 2021, own `[workspace]`, own Cargo.lock). Ports `Elk.Protocol/DataBusPlan.cs` and `Elk.Editor/NativeWindow.cs`.

## Public API
- `data_bus_plan`: `check(&BusPlan) -> PlanResult`, `PlanResult::to_json()`, `BusPlan::from_json(&str)`, `check_json(&str) -> Result<String, String>`
  (= `POST /api/databus/check`: `Ok` is the 200 body, `Err` the 400 body), `models_json()` / `zone_expander_switches_json()` (the two `/api/meta` members),
  plus `MODELS`, `ZONE_EXPANDER_SWITCHES`, `TRANSCEIVER_SWITCHES`, `model()`, `zones_at()`.
- `bridge`: `trait Dialogs { pick_open, pick_save }`, `enum Reply { None, Cancelled, Opened, Saved, Error }` with `Reply::to_json() -> Option<String>`,
  `handle_message(json, &dyn Dialogs) -> Reply`. No GUI code.
- `dialogs` (Windows/macOS): `RfdDialogs` (rfd crate).
- `native_window`: `run(url) -> Result<()>` (blocks until the window closes; call on the main thread; `Err` = no window, caller falls back to the browser),
  `run_with_dialogs(url, Box<dyn Dialogs>)`, `INIT_SCRIPT`, `deliver_script()`.
  wry/tao/rfd are target-gated to Windows and macOS; on other targets `run` returns an error and the rest still builds.

## Decisions
- Output JSON is written by hand (`jsonw.rs`) to match System.Text.Json: camelCase in declaration order, the default encoder's escaping
  (`\u0022`, `\u0027`, `\u002B`, `\u003E`, all non-ASCII as `\uXXXX`), and its double format. The double format was probed against the C# server:
  plain digits for 1e-4 <= |v| < 1e17, otherwise `1E+17` / `1.5E-05` style.
- `F()` (`ToString("0.#")`) reproduces .NET: 15 significant digits first, then half away from zero (0.35 gives 0.4, 1.25 gives 1.3, -0 prints "-0").
- Input parsing mimics web defaults: case-insensitive names, numbers may be quoted strings, unknown members ignored, `null` = default. Error texts for a 400 are
  not the same words as System.Text.Json's. Not matched: `"run": null` or null list elements (C# throws NullReferenceException and answers 500; Rust uses defaults).
- Window: 1500x950 logical, min 900x600, resizable, centered on the current monitor, title "Elk account editor". WebView2 profile folder is
  `%LOCALAPPDATA%\ElkAccountEditor\webview` (keeps localStorage across runs; the page origin is fixed by its port).
- `window.external` is defined by an initialisation script over wry's `window.ipc.postMessage`; replies go through `evaluate_script("window.__elkReceive(<json string literal>)")`.
  `index.html` is unchanged.
- Dialogs run on the UI thread inside the ipc handler (modal, as Photino did). The save dialog also gets the suggested file name (Photino could only put it in the title,
  which is kept too). The open dialog has no file-type filter, as before.
- Message errors: missing members / wrong types / bad base64 / IO errors come back as `{cmd:'error', message}`; the message wording is not the .NET text.
  Unknown commands get no reply, as in the C#.

## Tests
- 35 unit tests: 20 data bus (15 ported `[Fact]`s, `address_ranges` = the 11 `[InlineData]` rows plus one, 4 extras for number format / JSON in / defaults / output), 10 bridge, 2 jsonw, 2 native_window... (see `cargo test`).
- `tests/golden.rs`: 17 plans in `tests/fixtures/databus/*.plan.json` were POSTed to the running C# server (`rust\original-build\ElkAccountEditor.dll`, PORT=52333);
  responses are `*.expected.json`; Rust output equals them byte for byte. `_meta.expected.json` is `/api/meta` (busModels and zoneExpanderSwitches checked).
- `examples/bridge_check.rs`: opens the real window on a local test page with fake dialogs and walks openFile / saveFile / cancelled / unknown command / error.
  Run on Windows (WebView2): PASS. `cargo run --example bridge_check` (exits by itself).

## Not verified
- macOS WebKit path: not compiled or run here (needs CI on macOS). The code is shared with Windows apart from the target-gated crates.
- Linux: deliberately unsupported (`run` returns an error).
- Closing the window by the user (CloseRequested -> `run` returns) was not exercised live; it is tao's standard `run_return` pattern.
- `window.external` property redefinition worked in WebView2; WebKit's `window.external` is a different native object, so `defineProperty` with a fallback to assignment is used. Check on macOS.
- Real `rfd` dialogs were not driven (they need a human); only the fake dialogs were.
- Number formatting beyond the probed values (e.g. lengths above 1e17 in `F()` text) follows the probes, not a proof.