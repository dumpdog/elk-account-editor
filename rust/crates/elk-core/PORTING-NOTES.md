# elk-core porting notes

Rust port of the panel-protocol half of `port\src\Elk.Protocol` (Framing, Records, DefaultRecords, Handshake, PanelLive, PanelControl,
PanelModules, PanelProgress, ElkSession, PanelBackup, PanelClockLog). Standalone crate, edition 2021, own `Cargo.lock`, `[workspace]` root of its own.

## Public API (one module per C# file, all re-exported from the crate root)

| C# | Rust module | Main items |
|---|---|---|
| Framing.cs | `framing` | `Crc16::compute`, `Framing::encode`, `FrameParser::new(ff_is_debug_text).feed`, `Frame`, `FrameKind`, `ElkCrypto::{derive_key, encrypt, decrypt}` |
| Records.cs | `records` | `ZoneRecord`, `AreaRecord`, `KeypadRecord` (public `bytes`, getters/setters, `crc`/`crc1`/`crc2`), `ZoneFlag1` (bit-set newtype), `ZoneFlag2` |
| DefaultRecords.cs | `default_records` | `DefaultRecords::{zone,user,area,keypad,phone,area_rc,user_rc,zone_rc}`; also `ZoneVoice` (lives in ZoneImport.cs in C#, copied here because `zone()` needs it) |
| Handshake.cs | `handshake` | `Section`, `ConnectMode`, `Handshake::{build_connect, identity_block, keep_alive, disconnect, read_request, crc_query, write_request}`, `ConnectReply::parse` |
| ElkSession.cs | `elk_session` | `ElkSession::{new(mac, pass), plain(), tls(serial)}`, `connect`, `login`, `send`, `receive*`, `read_record`, `write_record`, `read_block`, `write_block`, `query_crc`, `read_zone/area/keypad`, `write_keypad`, `keep_alive`, `disconnect`; fields `panel`, `timeout`, `on_transfer`; `issuer_string`, `TRUSTED_ISSUERS` |
| PanelLive.cs | `panel_live` | `PanelLive::{read, decode_areas, decode_zones, decode_system}`, `AreaLive`, `ZoneLive`, `LiveSnapshot` |
| PanelControl.cs | `panel_control` | `PanelControl::{toggle_bypass, arm, arm_checked, silence}`, `ARM_LEVELS` |
| PanelModules.cs | `panel_modules` | `PanelModules::{scan, enroll, decode_enrolled, decode_versions, model_for}`, `BusModule` |
| PanelProgress.cs | `panel_progress` | `PanelProgress::{begin, next, add_total, note, tick, end, snapshot}`, `ProgressSnapshot` |
| PanelBackup.cs | `panel_backup` | `PanelFile::{to_json, from_json}`, `ZoneJson/AreaJson/KeypadJson/BlockJson`, `PanelBackup::{read, update, estimate_transfers, estimate_transfers_for, block_label}`, `BLOCKS` |
| PanelClockLog.cs | `panel_clock_log` | `PanelClock`, `PanelAccess`, `PanelLog`, `LogEntry` (times are `chrono::NaiveDateTime`) |
| tests\SimulatedPanel.cs | `simulated_panel` | `SimulatedPanel` (public so later crates, e.g. PanelSync tests, can use it) |
| - | `error` | `ElkError` (maps the .NET exceptions), `Result`, `hex_upper`, `hex_decode` |

Naming: C# `PascalCase` to `snake_case`; `FooAsync` becomes `foo`; static classes become unit structs with associated functions;
C# "`Type?` result" becomes `Option`; `byte` items and ids are `u8`, counts `i32`.
Section ids are passed as `impl Into<u8>` (a `Section` or a raw `u8`), because the C# casts any byte to `Section` for the generic blocks.

## Decisions

- **Blocking I/O, no async runtime.** `std::net` with read timeouts. `CancellationToken` parameters are dropped; a caller aborts by lowering
  `ElkSession::timeout`. The receive timeout is a single deadline per call, like `CancelAfter(Timeout)` in C#.
- **TLS: `native-tls`** (Schannel on Windows, Secure Transport on macOS, OpenSSL on Linux), pinned to TLS 1.2 only, SNI/host `M1XEP`. It was
  chosen over rustls because rustls has no legacy RSA/CBC suites and the module's cipher list is unknown. Validation is done after the
  handshake: the peer certificate's issuer must equal `CN=M1XEP, OU=Engineering, O=Elk, L=Hildebran, S=NC, C=US` or `CN=HTTPS Certificate`
  (`issuer_string` renders the DER issuer in the .NET `X509Certificate.Issuer` form: most specific RDN first, `S` for state). Nothing is sent
  before this check. Tested against a local TLS server (Schannel to Schannel, RSA self-signed cert) for both a trusted and an untrusted issuer.
- **AES identity block**: AES-128-CBC with zero IV over one 16-byte block equals ECB, so it is a single `aes` block encrypt with the fixed key.
  Golden-matched against the C# with a fixed "random" source (`identity_block` takes `&mut dyn FnMut() -> u8` yielding 1..=255; `None` in
  `build_connect` uses the thread RNG).
- **PanelFile JSON** is written by hand to match System.Text.Json (`WriteIndented`, PascalCase, `\uXXXX` escaping of `" ' & < > + \`` and non-ASCII,
  `null` for missing firmware/serial, `[]` for empty lists). Parsing uses serde with the same case-sensitive names and defaults.

## C# behaviour not matched exactly

1. **Line ends of `PanelFile::to_json`**: C# uses `Environment.NewLine`; the Rust uses `\r\n` on Windows and `\n` elsewhere (the same rule).
   The golden file was produced on Windows; the golden test converts line ends to the host's.
2. **TLS acceptance of a CA-signed certificate**: C# also trusts any certificate that validates cleanly (`SslPolicyErrors.None`). With
   `native-tls` the chain check is switched off to allow the self-signed module certificate, so only the two issuers are accepted; a CA-signed
   cert is refused. A real M1XEP always uses its self-signed one.
3. **`ElkSession::new` with an empty (or over 32 character) passphrase** returns an error at construction; .NET fails on first use. A 33 to 48 char
   passphrase gives a 48-byte key, which .NET rejects at first use as well.
4. **Exceptions that C# lets escape as `IndexOutOfRangeException`** return `ElkError::InvalidData` instead: `ConnectReply::parse` with exactly
   16 bytes, `read_block` on a reply shorter than 4 bytes, `query_crc` on a reply shorter than 6 bytes.
5. **`DefaultRecords::zone(n)` panics** for n outside 1..=208 (C#: `ArgumentOutOfRangeException`). `ZoneVoice::words` returns a `Result`.
6. **`PanelBackup::update`** returns an error for a hex string that is longer than the record (C# silently truncates zone hex via `ZoneRecord.FromBytes`
   but throws for areas and keypads); invalid hex is an error in both.
7. **Name fields** use UTF-16 semantics like .NET: the cut at 16 is in code units and every non-ASCII code unit becomes `?` (an astral character gives two).
   Golden-verified with `"cafeé\U0001F600x"` giving `caf???x`.
8. **SimulatedPanel**: stores records as `(section id, item)` byte keys, has a plain (no AES) mode, accepts more than one connection in turn, and
   answers nothing (instead of crashing its server loop) for records that were never stored. Behaviour for the paths the C# tests use is unchanged.
9. **Not tested on hardware**, like the C#: AES secure mode; the TLS path was only tested against a local TLS server (the C# TLS path was used on the real
   spare panel, 2026-11-10).

## Tests

63 integration tests in `tests\` (no unit tests in `src`), all passing; `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean.

| file | tests | content |
|---|---|---|
| `protocol.rs` | 20 | ProtocolTests, AreaTests, ZoneFlag tests, KeypadTests, name setters (PanelSupportTests), DeleteTests default records, HandshakeTests |
| `session.rs` | 26 | SessionTests (AES, split writes, wrong code, wrong passphrase, keypad two-part), plain session, LiveTests, ModulesTests, ClockLogTests, control and enroll against `SimulatedPanel` |
| `golden.rs` | 10 | values produced by running the C# Elk.Protocol (CRC, frames, AES keys and ciphertext, identity block and connect bytes, all 208 default zones and other default records, live/modules decoding, clock/access requests, `PanelFile` JSON bytes) |
| `backup.rs` | 4 | full backup read over the simulator with exact transfer count vs `PanelProgress`, compare/restore, bad hex, progress |
| `tls.rs` | 3 | issuer rendering, TLS login with a trusted issuer, refusal of an untrusted issuer |

The golden files in `tests\golden` were generated from a copy of `port\src\Elk.Protocol` compiled in a scratch project (the `port` tree was not touched);
regenerate by compiling those sources with a small `Main` that writes the same lines.
`PanelBackup::read/update` and `PanelProgress` have no C# unit tests in this scope (they are exercised via PanelSyncTests, which belongs to a later wave),
so `backup.rs` is new.

## Automation items (Rust only, ADR-012)
- `panel_auto.rs`: `AutoKind` (layout per kind from ElkRP's `Form_Receive` / `Form_Send`), packet read / write / scan / match, `read_all_items`, `read_all_packets`; `ElkSession::read_block16 / write_block16` (16-bit item, echo check). `PanelFile.automation` / `AutoBlockJson` (JSON `AutomationBlocks`, only written when not empty), `PanelBackup::read_with_automation`, restore handles the blocks.
- `SimulatedPanel` answers sections 2, 13, 14, 121 (and the writes 130, 141, 142, 249); `set_auto_item`, `auto_item`, `packets`, `set_packets`. Tests: `tests/auto.rs`.
## Rust-only addition: `xep_config` (M1XEP settings, ADR-013)

Not a port: the C# `Elk.Protocol` has no M1XEP code. `xep_config.rs` is derived from ElkRP's `XEPConfig.cs` / `XEPFind.cs` (see `rust/M1XEP-NOTES.md`): `XepBlocks` (five setup blocks + 16 e-mail slots, file lines and hex forms),
`XepSettings::{decode, validate, apply}`, `XepModule::{info, read, write}` over an `ElkSession` (opcode 0x75), `discover` (UDP `XEPID` on 2362) and `XepSim`. `SimulatedPanel::with_xep` switches the simulated module on
(off by default so existing module-scan expectations are unchanged). Tests: `tests/xep.rs` (11). Verified only against the simulator, plus one read-only probe of a real module; no write was ever sent to hardware.
