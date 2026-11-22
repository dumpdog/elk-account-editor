# ElkRP3 trace findings (v3.0.7.41847, ElkRP "Version 2.0.38" readme)

Status labels: **[V]** verified by reading decompiled code, **[I]** inferred (not yet confirmed),
**[?]** open question. Source: analysis of the ElkRP3 program (local reference material, not published).

## 1. Layout of the program

| Piece | What it is | Port impact |
|---|---|---|
| `ElkRP3.exe` | WinForms shell, VB.NET origin, net46 x86. Accounts, operators, login, options, print, sync, updates, log. 27 forms. [V] | UI rewrite; logic reusable |
| `M12.dll` (`M1`), `EZ82.dll` (`EZ8`), `M1G2.dll` (`M1G`) | One plugin per panel family: M1, EZ8, M1G. Near-identical code (file sizes differ by under 3%). [V] | Port once, parametrize per family |
| `ElkSafe2.dll`, `ConvertDB2.dll` | Small helpers (5 and 27 files). [V] | Small |
| `ElkComm2.dll` | **Native x86 COM DLL**, serial/modem layer (`ElkSerialComm`). Not .NET. [V] | Must be reimplemented |
| `FTD2XX_NET.dll` | FTDI wrapper | libftd2xx exists for macOS |
| `AxInterop.PVExplorerLib` | ActiveX control, 64 files reference it [V] | No macOS equivalent |
| Access `.mdb` via ADODB/ADOX/JRO | Account, operator and reference storage | Replace with SQLite |

## 2. Transports (`ConnectMethod` byte) [V]

| Value | Meaning | Implementation |
|---|---|---|
| 16 | Direct serial | `ElkSerialComm` (native `ElkComm2.dll`) |
| 17 | USB to C1M1 | `USBCom`, managed, `SerialPort` at 115200 baud |
| 32-38, 46, 47 | Modem dial / wait / auto-answer | `ElkSerialComm`; probably droppable |
| 48, 49 | Network, plain / secure | |
| 50 | Network TLS | managed TLS class |
| 51 | Network AES | managed AES class |
| 52, 53 | ElkLink cellular / ethernet (TLS, longer timeouts) | the TLS class |
| 54 | M1 Cloud proxy | cloud proxy class |

Default ports in the UI: **2601** secure, **2101** non-secure. [V]

## 3. Wire framing [V for AES and USB; I for native serial]

Both managed transports (`NetAES.SendMessage/ParseM1Msg`, `USBCom`) use the same framing:

```
10 02 | payload (every 0x10 byte doubled to 10 10) | 10 03 | CRC_hi CRC_lo
```

- CRC is CRC-16/XMODEM style (poly 0x1021, init 0), computed over the unescaped payload bytes.
- Receive state machine: `10 02` starts a frame, `10 03` ends it, `10 06` is ACK and `10 15` is NAK (single frames).
- A leading `0xFF` in a body marks printf/debug text that is ignored.
- AES variant (51): the whole framed message is zero-padded to 16 bytes and encrypted **AES-128-ECB, no IV**.
  The key is derived in `ConnectAES`: AES-128-ECB-encrypt the ASCII `DBPassphrase` using the ASCII
  `DBMacAddress` (zero-padded to 16 bytes) as the key. The result is the session key. [V]
  The same constructs exist in `System.Security.Cryptography` on .NET 8/macOS.
- Native serial (16) is probably the same framing inside `ElkComm2.dll`. [I] Confirm in the native library.

## 4. Application commands [V unless noted]

Request layout: `[opcode][0][0][item#][data...]`. The reply echoes `opcode` in byte 0 (`Expect`).
Config sections are **read with opcode N and written with N+128**.

| Read / Write | Section (source file) |
|---|---|
| 1 / 129 | User codes |
| 3 / 131 | Area definitions |
| 4 / 132 | Keypad definitions |
| 5 / 133 | Zone definitions (one zone per request, bytes from `Zones[n].ZNBytes`) |
| 7 / 135 | Dawn and dusk / globals |
| 8 / 136 | Telephone numbers |
| 9-12 / 137-140, 16 / 144 | Report codes (several sub-tables) |
| 13 / 141 | Voice messages |
| 14 / 142 | X10 outputs |
| 15 / 143 | Wireless zones |
| 2 / 130 | Outputs / tasks (also used in PgmFlash; exact split [?]) |
| 118-122, 249 | Output, X10, task, when-then, voice/zone auxiliary commands |

Session and utility commands:

| Opcode | Use |
|---|---|
| 127 (0x7F) | Session control. `7F 00 00 01 ...` = connect request; the 6-digit RP access code from the account is appended one digit per byte, and in the main path `AES128_Encrypt_B` output is also appended (challenge handling, details [?]) [V for digits, ? for the rest]; `7F 00 00 05` + 6 access-code digits (bytes 4-9, total 10 bytes) is a second connect form used by the XEP/XIP path, purpose [?]; `7F 01 ...` = disconnect (panel replies with `7F 01`, the app treats it as "Control has disconnected"); `7F 00 00 A5` = keep-alive/ack [V]; `7F xx 00 0D` = change baud (codes 1-9) [V] |
| 117 (0x75) | Info queries. `75 06` returns firmware: app ver bytes 13-15, HW ver 8-9, boot ver 10-12, MAC bytes 16-21. `75 09` returns 8 ASCII characters at bytes 2-9; on XIP bytes 23-34 hold a serial number and board checksum. What the 8 characters are is [?]. [V for offsets] |
| 124 (0x7C) | System version |
| 126 (0x7E) | Used by area, keypad, output modules (16 sites); meaning [?] |
| 123 (0x7B) | Receive event log |
| 252 (0xFC), 40 (0x28), 0 | Firmware flash (`PgmFlash`) |
| 45-47 | Manufacturing dialog; ignore |

`SendReceive` retries 4 times at 800 ms by default; the timeout is doubled or more on slow serial links, 3000 ms minimum for ElkConnect, and multiplied for cellular. [V]

## 5. Storage [V]

- `C:\ProgramData\RP\ElkAccts2.mdb` is the account database. It is access-protected (Jet refused a
  read-only open of a copy, and the app has `frmAuthenticateDB`). I did not try to get around that.
  Tables I could see referenced in code: Accounts, ZoneDefinitions, AreaDefinitions, UserCodes,
  TelephoneNumbers, KeypadDefinitions, SystemReportCodes, EmailDefinitions, SunriseSunset1/2, Globals,
  Operators, ElkConnectProxy, Variables, Constants, Administration.
- `Ops2.mdb` is the operator list, same protection presumably.
- `Controls2.mdb` (3.2 MB) opens fine. It holds reference data only: Cities (5650), EventLists (18960),
  OutputLists (2870), VoiceLists (1243), WordLists (956), AutoArmDefinitionLists (98), CountryModemVals
  (39), AudioModels (4) plus version tables. These convert easily to SQLite/JSON.
- The panel's per-item config is kept as raw byte arrays (for example `ZNBytes`), so a library can store
  the same bytes it sends, which keeps compatibility with the wire format.

## 6. Answer to "what can a CLI do?"

Everything in section 4 is plain request/response over a byte stream, so these are viable without any GUI:
connect (USB, TCP/AES, TLS), read panel identity and versions, download any section, edit section
bytes, upload sections, read the event log, disconnect. Missing for a faithful tool: the **byte layout
of each section** (offsets and meaning of the bytes inside `ZNBytes` etc.). That is held in the
big per-section classes (the zone class and the rest) and in the rule classes, and it is the main remaining tracing job.

## 6b. Section sizes found so far [V: array declarations; arrays are 1-based so count = length - 1]

| Section | Record size | Count |
|---|---|---|
| Zones | 32 bytes (`ZNBytes`) | 208 (array of 209) |
| Areas | 24 bytes (`AreaBytes`) | 8 (array of 9) |
| Keypads | `KeypadType` struct | 16 (array of 17) |
| Wireless zones | 260 bytes (`WZNBytes`) | per-zone, count [?] |
| User report codes | `UserRCType` | 203 (array of 204) [I: may be 0-based] |

These sizes are identical in the M1, EZ8 and M1G plugins. The three plugins share the same file set apart from the
control class (`M1Control` / `EZ8Control` / `M1GControl`), plus `XIPTestDNS` and `XIPTrace` (EZ8 only) and
`NoElkLinkConnect` (M1G only). So one port with a per-model descriptor is realistic.
Not yet checked: whether limits like user count differ per model.

## 7. Open items

1. Native serial framing in `ElkComm2.dll` (section 3). Can wait; USB and TCP cover M1G/M1 with XEP/XIP.
2. TLS path (the TLS class): handshake, certificate pinning, ElkLink credentials.
3. Per-section byte maps and item counts (to be read from the code; I have not extracted them yet).
4. Firmware flash sequence (`PgmFlash`): treat as out of scope for a first port; it can brick hardware.
5. Whether the E27 and M1G plugins differ in protocol, not just constants (diff `EZ8` vs `M1` vs `M1G`).
6. Access to the protected account DB. Ask the developer for the export format or password.

## 8. Account database mapping (from the TESTACCT-ALPHA1 / TESTACCT-BETA test accounts)

Method: ElkRP's own account database is access-protected and is not opened here. The mapping below comes from changing single values in ElkRP, exporting, and comparing.

Verified against your entries [V]:
- `Accounts` columns: Group=Folder(Group), Name, Address1/2, City, State, ZipCode, VoiceTelephone, FAX,
  UDLTelephone=panel phone, UDLURLIP=panel IP, Email, IPAddress=web page, Notes, ControlType=System Type
  (`M1G`), DateInstalled, IPPort (secure port you typed, 2623). The key column `Account` is the Account ID.
- Zones: one row per zone (`ZoneNum`), name stored as 16 ASCII codes in `ZnZoneName1..16`, definition in
  `ZnFunction`. Changing zone 1 to "Definition 02" changed `ZnFunction` 1 -> 2 and the name to
  `ZONE-ONE-CHANGED` (ASCII 90,79,78,69,... decoded correctly).
- Each record carries `UDCRC` (CRC at last upload/download) and `ModCRC` (current CRC). Editing zone 1 moved
  `ModCRC` 2259 -> 3116 while `UDCRC` stayed 2259. This is the "modified" marker and the CRC algorithm
  is still to be found (probably in the zone class).
- Area/keypad names are 16 space-padded ASCII codes (`PName1..16`, `KeypadName1..16`), defaults "Area 1",
  "Keypad 01".
- Time zone "+3 Moscow" is stored as `SunriseSunset1.TimeZn = 1180`, with `LatDeg=41`, `LonDeg=-87`.
  Encoding of 1180 not yet decoded [?] (1180 may be 1000 + 180 minutes; unconfirmed).

Not stored in plain form [V]: users (names and codes), phone numbers, global/installer program code,
`ControlSerialNumber`, and `RPAccCode1..3` hold random-looking bytes even for default content, so those
tables are encrypted or scrambled by ElkRP. This is local storage only, not the panel wire format. A port
needs its own storage and does not have to reproduce this, unless you want to import existing accounts.

Not covered by the test data: in ALPHA the zone 2, 3, 208, area, keypad and phone entries were not saved
(zones still read "Zone 00n", area "Area 1", keypad "Keypad 01"), so those edits were not captured. Only
zone 1 in BETA, the Account Details fields, users (encrypted) and time zone were captured.

## 9. Zone and area flag bits (verified with sample accounts FLAG-1..FLAG-16)

Zone flag1 (byte 1), one checkbox per sample: bit0 Bypassable (F10), bit1 Periodic trip (F13), bit2 Fast loop response (F14),
bit3 Force armable (F11, confirmed on re-save), bit4 Enable chime (F15), bit5 In cross zone pool (F16),
bit6 Use dialer delay (F8), bit7 Listen in (F9).
Zone flag2 (byte 2): bits0-2 zone type, bit3 swinger shutdown (F12), bits4-6 area minus one (F4: area 4 = 48), bit7 silent alarm (F7).
Area flags: see `AreaRecord` in `port/src/Elk.Protocol/Records.cs`. Keypad area field is a plain number (F6: area 8 stored as 8).

## 10. Connect handshake and record transfer (traced from ElkRP)

Connect request (AES mode 51), 10 bytes: `7F 00 00 01` + the six RP access-code digits, LAST DIGIT FIRST, each as a value 0-9.
Modes 49/50 (secure/TLS) append a 16-byte block: AES-128-CBC, zero IV, with a fixed key, over: [0] random 1..255, [1..8] the 8 serial-number hex nibbles,
[9] random 1..255, [10..15] the six digits in forward order. (This block is built by `Handshake.IdentityBlock`; the TLS transport itself is not ported.)
Reply (opcode 0x7F): [1] status 0 = accepted | [4..6] firmware (if [4] < 4 the panel is in its boot loader) | [8..11] serial as hex |
[12..13] hardware | [14..16] boot | [22..24] minimum ElkRP version.
Keep-alive `7F 00 00 A5` (no reply expected). Disconnect `7F 01 00 A5`. A panel-initiated disconnect arrives as `7F 01`.
Record read: `[section 0 0 item]` -> reply echoes the header, record bytes start at offset 4. Record write: `[section+128, 0, 0, item, bytes]`, panel
echoes section+128. CRC query: `7E section 00 item` -> `7E section 00 item crcHi crcLo` (ElkRP compares this with its stored ModCRC/UDCRC to skip unchanged items).
Keypads use request byte 1 as a part number: part 0 = bytes 0..27, part 1 = bytes 28..147 (matching the two stored CRCs).
Unknown: retry/timing details on real hardware, the 0x7E sub-commands used by other modules, other sections' item counts and byte layouts.

## 11. Zone import (CSV -> saved account)

Default zone voice (verified on 912 stored default zones, 1-16 and 161-208; EZ8 not verified): "Zone", then for 100-299 "One"/"Two"
(+ "Hundred" when the remainder is under 10), then the teens word or tens word, then the ones digit when the remainder is under 10 or over 20.
Word ids (M1G / M1 Std, Controls2.mdb WordLists v0.8): Zone 471, One..Nine 22..30, Ten..Nineteen 31..40, Twenty..Ninety 41..48
(40 is spelled "Fourty"), Hundred 49. Zone 186 = 471, 22, 47, 27 (matches the screenshot).
Stored zone row: ZnFunction, ZnFlag1, ZnFlag2, ZnZoneName1..16 (ASCII codes, 0 padded), ZnVoice1..6 H/L, ModCRC, UDCRC. New rows use
UDCRC 0 (never sent). Accounts only hold rows for zones that exist (1-16, 161-208 by default; others appear when an expander is added).
