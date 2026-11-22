# Rust migration plan: Elk-rust (C#)

4 packages, 23 source files (3216 lines), 19 test files, 0 UI files, grouped into 23 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 15 | 8 | 0 | 0 |

> 1 files had parse errors; their units are marked partial.

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `port/src/Elk.Protocol/PanelBackup.cs` | 94 | 67 | 72 | 0 | P/Invoke (C ABI) | Async and promises (15); Dynamic typing (4); Reflection and prototypes (4) |
| 2 | `port/src/Elk.Protocol/AccountModel.cs` | 278 | 63 | 55 | 0 | P/Invoke (C ABI) | Callback-shaped APIs (38); Dynamic typing (26); Integer width and overflow (18) |
| 3 | `port/src/Elk.Protocol/DefaultRecords.cs` | 26 | 59 | 96 | 0 | P/Invoke (C ABI) | Dynamic typing (2) |
| 4 | `port/src/Elk.Protocol/PanelLive.cs` | 72 | 58 | 82 | 0 | P/Invoke (C ABI) | Async and promises (8); Integer width and overflow (4); Division and modulo (1) |
| 5 | `port/src/Elk.Protocol/Framing.cs` | 127 | 57 | 81 | 0 | P/Invoke (C ABI) | Integer width and overflow (8); Dynamic typing (5); Division and modulo (3) |
| 6 | `port/src/Elk.Protocol/AccountFile.cs` | 222 | 57 | 70 | 0 | P/Invoke (C ABI) | String offsets and slicing (1); Callback-shaped APIs (9); Integer width and overflow (5) |
| 7 | `port/src/Elk.Protocol/ZoneImport.cs` | 106 | 54 | 69 | 0 | P/Invoke (C ABI) | Hash iteration order (1); Dynamic typing (12); Division and modulo (7) |
| 8 | `port/src/Elk.Protocol/ZoneSheet.cs` | 250 | 53 | 62 | 0 | P/Invoke (C ABI) | Hash iteration order (1); Callback-shaped APIs (20); Dynamic typing (11) |
| 9 | `port/src/Elk.Protocol/Records.cs` | 113 | 51 | 78 | 0 | P/Invoke (C ABI) | Dynamic typing (9); Integer width and overflow (9); Null references (2) |
| 10 | `port/src/Elk.Protocol/PanelModules.cs` | 79 | 51 | 67 | 0 | P/Invoke (C ABI) | Async and promises (11); Integer width and overflow (7); Division and modulo (5) |

### `port/src/Elk.Protocol/PanelBackup.cs`

94 lines in 1 files, wave 0, priority 67, readiness 72, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Async and promises x15: `port/src/Elk.Protocol/PanelBackup.cs:54`, `port/src/Elk.Protocol/PanelBackup.cs:57`, `port/src/Elk.Protocol/PanelBackup.cs:58`
- Dynamic typing x4: `port/src/Elk.Protocol/PanelBackup.cs:61`, `port/src/Elk.Protocol/PanelBackup.cs:61`, `port/src/Elk.Protocol/PanelBackup.cs:61`
- Reflection and prototypes x4: `port/src/Elk.Protocol/PanelBackup.cs:76`, `port/src/Elk.Protocol/PanelBackup.cs:84`, `port/src/Elk.Protocol/PanelBackup.cs:92`
- Integer width and overflow x2: `port/src/Elk.Protocol/PanelBackup.cs:49`, `port/src/Elk.Protocol/PanelBackup.cs:52`
- Callback-shaped APIs x1: `port/src/Elk.Protocol/PanelBackup.cs:49`
- Null references x1: `port/src/Elk.Protocol/PanelBackup.cs:69`
- Exceptions x1: `port/src/Elk.Protocol/PanelBackup.cs:24`

### `port/src/Elk.Protocol/AccountModel.cs`

278 lines in 1 files, wave 0, priority 63, readiness 55, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Callback-shaped APIs x38: `port/src/Elk.Protocol/AccountModel.cs:109`, `port/src/Elk.Protocol/AccountModel.cs:109`, `port/src/Elk.Protocol/AccountModel.cs:136`
- Dynamic typing x26: `port/src/Elk.Protocol/AccountModel.cs:113`, `port/src/Elk.Protocol/AccountModel.cs:113`, `port/src/Elk.Protocol/AccountModel.cs:115`
- Integer width and overflow x18: `port/src/Elk.Protocol/AccountModel.cs:109`, `port/src/Elk.Protocol/AccountModel.cs:113`, `port/src/Elk.Protocol/AccountModel.cs:113`
- Null references x5: `port/src/Elk.Protocol/AccountModel.cs:189`, `port/src/Elk.Protocol/AccountModel.cs:194`, `port/src/Elk.Protocol/AccountModel.cs:202`
- Exceptions x16: `port/src/Elk.Protocol/AccountModel.cs:126`, `port/src/Elk.Protocol/AccountModel.cs:174`, `port/src/Elk.Protocol/AccountModel.cs:181`

### `port/src/Elk.Protocol/DefaultRecords.cs`

26 lines in 1 files, wave 0, priority 59, readiness 96, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Dynamic typing x2: `port/src/Elk.Protocol/DefaultRecords.cs:20`, `port/src/Elk.Protocol/DefaultRecords.cs:25`

### `port/src/Elk.Protocol/PanelLive.cs`

72 lines in 1 files, wave 0, priority 58, readiness 82, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Async and promises x8: `port/src/Elk.Protocol/PanelLive.cs:35`, `port/src/Elk.Protocol/PanelLive.cs:37`, `port/src/Elk.Protocol/PanelLive.cs:38`
- Integer width and overflow x4: `port/src/Elk.Protocol/PanelLive.cs:46`, `port/src/Elk.Protocol/PanelLive.cs:48`, `port/src/Elk.Protocol/PanelLive.cs:48`
- Division and modulo x1: `port/src/Elk.Protocol/PanelLive.cs:68`
- Null references x1: `port/src/Elk.Protocol/PanelLive.cs:68`
- Exceptions x1: `port/src/Elk.Protocol/PanelLive.cs:39`

### `port/src/Elk.Protocol/Framing.cs`

127 lines in 1 files, wave 0, priority 57, readiness 81, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Integer width and overflow x8: `port/src/Elk.Protocol/Framing.cs:19`, `port/src/Elk.Protocol/Framing.cs:21`, `port/src/Elk.Protocol/Framing.cs:21`
- Dynamic typing x5: `port/src/Elk.Protocol/Framing.cs:14`, `port/src/Elk.Protocol/Framing.cs:19`, `port/src/Elk.Protocol/Framing.cs:36`
- Division and modulo x3: `port/src/Elk.Protocol/Framing.cs:119`, `port/src/Elk.Protocol/Framing.cs:128`, `port/src/Elk.Protocol/Framing.cs:139`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. Elk.Cli (`port/src/Elk.Cli`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `port/src/Elk.Cli/Program.cs` | ready | 188 | 67 | P/Invoke (C ABI) | String offsets and slicing (1); Async and promises (37) |

### 2. ElkAccountEditor (`port/src/Elk.Editor`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `port/src/Elk.Editor/NativeWindow.cs` | ready | 50 | 79 | P/Invoke (C ABI) | Null references (7); Reflection and prototypes (3) |
| 0 | `port/src/Elk.Editor/LivePanel.cs` | ready | 105 | 73 | P/Invoke (C ABI) | Async and promises (36); Null references (15) |
| 0 | `port/src/Elk.Editor/Program.cs` | caution | 366 | 40 | P/Invoke (C ABI) | Async and promises (70); Callback-shaped APIs (57) |

### 3. Elk.Protocol (`port/src/Elk.Protocol`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `port/src/Elk.Protocol/DefaultRecords.cs` | ready | 26 | 96 | P/Invoke (C ABI) | Dynamic typing (2) |
| 0 | `port/src/Elk.Protocol/PanelProgress.cs` | ready | 21 | 93 | P/Invoke (C ABI) | Concurrency (7) |
| 0 | `port/src/Elk.Protocol/AccountExport.cs` | caution | 84 | 84 | P/Invoke (C ABI) | Integer width and overflow (5); Callback-shaped APIs (3) |
| 0 | `port/src/Elk.Protocol/Handshake.cs` | caution | 65 | 82 | P/Invoke (C ABI) | Dynamic typing (8); Null references (3) |
| 0 | `port/src/Elk.Protocol/PanelLive.cs` | ready | 72 | 82 | P/Invoke (C ABI) | Async and promises (8); Integer width and overflow (4) |
| 0 | `port/src/Elk.Protocol/Framing.cs` | ready | 127 | 81 | P/Invoke (C ABI) | Integer width and overflow (8); Dynamic typing (5) |
| 0 | `port/src/Elk.Protocol/Records.cs` | ready | 113 | 78 | P/Invoke (C ABI) | Dynamic typing (9); Integer width and overflow (9) |
| 0 | `port/src/Elk.Protocol/PanelControl.cs` | caution | 44 | 76 | P/Invoke (C ABI) | Dynamic typing (12); Async and promises (6) |
| 0 | `port/src/Elk.Protocol/PanelBackup.cs` | ready | 94 | 72 | P/Invoke (C ABI) | Async and promises (15); Dynamic typing (4) |
| 0 | `port/src/Elk.Protocol/AccountFile.cs` | ready | 222 | 70 | P/Invoke (C ABI) | String offsets and slicing (1); Callback-shaped APIs (9) |
| 0 | `port/src/Elk.Protocol/ZoneImport.cs` | ready | 106 | 69 | P/Invoke (C ABI) | Hash iteration order (1); Dynamic typing (12) |
| 0 | `port/src/Elk.Protocol/PanelModules.cs` | ready | 79 | 67 | P/Invoke (C ABI) | Async and promises (11); Integer width and overflow (7) |
| 0 | `port/src/Elk.Protocol/DataBusPlan.cs` | caution | 280 | 66 | P/Invoke (C ABI) | Callback-shaped APIs (21); Integer width and overflow (10) |
| 0 | `port/src/Elk.Protocol/PanelClockLog.cs` | ready | 110 | 62 | P/Invoke (C ABI) | Integer width and overflow (19); Async and promises (12) |
| 0 | `port/src/Elk.Protocol/ZoneSheet.cs` | ready | 250 | 62 | P/Invoke (C ABI) | Hash iteration order (1); Callback-shaped APIs (20) |
| 0 | `port/src/Elk.Protocol/AccountModel.cs` | ready | 278 | 55 | P/Invoke (C ABI) | Callback-shaped APIs (38); Dynamic typing (26) |
| 0 | `port/src/Elk.Protocol/ElkSession.cs` | caution | 148 | 53 | P/Invoke (C ABI) | Async and promises (39); Dynamic typing (6) |
| 0 | `port/src/Elk.Protocol/AlarmZoneList.cs` | caution | 180 | 49 | P/Invoke (C ABI) | String offsets and slicing (12); Hash iteration order (1) |
| 0 | `port/src/Elk.Protocol/PanelSync.cs` | caution | 208 | 41 | P/Invoke (C ABI) | Hash iteration order (1); Async and promises (47) |

## NuGet dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `Photino.NET` | 1 | ? | unknown |  |

## Risk guide

- **String offsets and slicing** (High): JS indexes strings in UTF-16 code units, Rust in UTF-8 bytes. Offsets that cross the boundary must be converted, or non-ASCII text gets wrong ranges silently.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Exceptions** (Low): Thrown errors become `Result`. Errors crossing the bridge must map back to the same JS error shape.
- **Async and promises** (Medium): Promises become futures. Bridge with napi-rs `async fn` or `wasm-bindgen-futures`, or keep the async shell in JS and port the sync core.
- **Class inheritance** (Medium): Class hierarchies become traits plus composition; `instanceof` checks become enums.
- **Reflection and prototypes** (Medium): Proxy, Reflect, Symbol and prototype tricks need a redesign, not a translation.
- **Callback-shaped APIs** (Medium): Function-typed parameters and methods cross the bridge on every call. Keep boundaries coarse: pass data in, get data out.
- **Integer width and overflow** (Medium): Python ints never overflow; C, Go and Java ints wrap or are undefined on overflow. Pick i64, i128 or a bignum crate deliberately, and use wrapping_* where the source wraps.
- **Division and modulo** (Medium): Python `//` and `%` floor toward negative infinity; Rust `/` and `%` truncate: use `div_euclid`/`rem_euclid` or explicit floor math. C division by zero is undefined.
- **Hash iteration order** (High): Set and map iteration order comes from the source language's hash function. A port cannot reproduce it: sort before output, or make the order part of the contract.
- **Concurrency** (Medium): Threads, goroutines and locks need a design in Rust (channels, Send/Sync), not a line-by-line translation; keep concurrency in the caller where possible.
- **Null references** (Medium): Any reference may be null in the source: decide per parameter whether it is `Option` and what null means, instead of mapping every value to `Option`.

## Whole app

Shape: **server**.

| Entry point | Kind | Found from |
| --- | --- | --- |
| Elk.Cli: `port/src/Elk.Cli` | cli | `port/src/Elk.Cli/Elk.Cli.csproj` builds an executable |
| Elk.Editor: `port/src/Elk.Editor` | server | `port/src/Elk.Editor/Elk.Editor.csproj` uses the web SDK |

### Every dependency

| Dependency | Declared | Used by | Rust | Fit | Note |
| --- | --- | --- | --- | --- | --- |
| `Photino.NET` | yes | 1 | ? | unknown |  |

### Pieces, in port order

- wave 0: `port/src/Elk.Cli/Program.cs` (188 lines)
- wave 0: `port/src/Elk.Editor/LivePanel.cs` (105 lines)
- wave 0: `port/src/Elk.Editor/NativeWindow.cs` (50 lines)
- wave 0: `port/src/Elk.Editor/Program.cs` (366 lines)
- wave 0: `port/src/Elk.Protocol/AccountExport.cs` (84 lines)
- wave 0: `port/src/Elk.Protocol/AccountFile.cs` (222 lines)
- wave 0: `port/src/Elk.Protocol/AccountModel.cs` (278 lines)
- wave 0: `port/src/Elk.Protocol/AlarmZoneList.cs` (180 lines)
- wave 0: `port/src/Elk.Protocol/DataBusPlan.cs` (280 lines)
- wave 0: `port/src/Elk.Protocol/DefaultRecords.cs` (26 lines)
- wave 0: `port/src/Elk.Protocol/ElkSession.cs` (148 lines)
- wave 0: `port/src/Elk.Protocol/Framing.cs` (127 lines)
- wave 0: `port/src/Elk.Protocol/Handshake.cs` (65 lines)
- wave 0: `port/src/Elk.Protocol/PanelBackup.cs` (94 lines)
- wave 0: `port/src/Elk.Protocol/PanelClockLog.cs` (110 lines)
- wave 0: `port/src/Elk.Protocol/PanelControl.cs` (44 lines)
- wave 0: `port/src/Elk.Protocol/PanelLive.cs` (72 lines)
- wave 0: `port/src/Elk.Protocol/PanelModules.cs` (79 lines)
- wave 0: `port/src/Elk.Protocol/PanelProgress.cs` (21 lines)
- wave 0: `port/src/Elk.Protocol/PanelSync.cs` (208 lines)
- wave 0: `port/src/Elk.Protocol/Records.cs` (113 lines)
- wave 0: `port/src/Elk.Protocol/ZoneImport.cs` (106 lines)
- wave 0: `port/src/Elk.Protocol/ZoneSheet.cs` (250 lines)

### Decisions for the user

**D1** (shape): The app is a server, entered through Elk.Cli (port/src/Elk.Cli), Elk.Editor (port/src/Elk.Editor). Port every entry point, in that shape?

> Yes: an axum front door (`app front-door`) takes the server over first

**D2** (dependency Photino.NET): Which crate replaces Photino.NET (used by 1 unit)?

> Research it against primary sources into rust/research/Photino.NET.md before porting what uses it

