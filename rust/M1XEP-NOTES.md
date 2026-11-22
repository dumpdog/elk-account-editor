# M1XEP settings: what ElkRP does, what the Rust editor does

Task: ElkRP's Account Details page has an "M1XEP Setup" button. It opens a window that configures the M1XEP Ethernet module itself. This file records
what that window is, how it talks to the module, where each value lives, and what the editor implements. Sources: the decompiled ElkRP under
`Elk\src\M1G2\M1G` (`XEPConfig.cs`, `XEPCS.cs`, `XEPFind.cs`, `Account_Details.cs`, `M1Fns.cs` `SendReceive`, `M1GControl.cs` `ExportAccount`), plus one
read-only probe of the user's own module (see "What was checked on hardware"). Everything is derived from those sources; nothing was guessed on hardware.

## Verdict

- **Reading is solid and implemented.** The module's setup is read with plain read requests (`75 N`) on the same login the panel uses, which is exactly what
  ElkRP's "Receive" button does. A read changes nothing on the module. The decoded fields were checked against the real module (read only, once).
- **The account export file already holds the whole M1XEP Setup.** ElkRP writes it as the LAST block of every export (`XEPConfig.Form_Export`, after the
  rules). Earlier notes in this repo (`english.rs` comments, ADR-011) said the e-mail and audio names "are not in the export file": that is wrong, they are
  in this block. The editor reads that block (read only) and can compare it with the module. It does not edit it (see "Not done").
- **Writing is implemented but verified only against the simulator.** The protocol for writing is plain in ElkRP's code (`Form_Send`: `75 80+N data`, acknowledged
  by an echo), so the write path is built on it with a diff, a backup, two confirmations and a read-back. It has never touched a real module. HANDOFF.md lists
  the steps for the first real test. Treat it as unproven until then.
- **Risk.** A wrong IP address, subnet, gateway, port, DNS or login written to the module can cut the editor and ElkRP off from the panel's network connection;
  the way back may be a visit to the module (or a reset). That is why identity fields need a separate explicit warning tick (below).
- **Not implemented, deliberately:** Central Station tab, M1Cloud, local time zone, the daylight-saving block, Reboot, the Test buttons, Trace, firmware update,
  modules older than firmware 1.3.19, and writing the block into the account export file.

## The window (ElkRP wording)

`Account_Details.cs` button "M1XEP Setup" calls `frmXEPConfig.Show`. The window (`XEPConfig.cs`, title "M1XEP Setup") has these buttons: **Reboot**, **Find**,
**Receive** (label "M1 -> ElkRP"), **Send** (label "ElkRP -> M1"), **Close**. They are only enabled while ElkRP is connected to the M1XEP over a network
connection (connect methods 48/49/50 and the cloud 54).

Tabs and fields (the wording is ElkRP's; "where" is the byte position described in the next section):

| Tab | Field (ElkRP wording) | Where | Writable here | Identity |
|---|---|---|---|---|
| Introduction | text only | - | - | - |
| TCP/IP Settings | Device Name ("Used during the Find process to help identify M1XEPs...") | block 1, 0..15 | yes | no |
| | Assigned an IP address via DHCP / Use a static IP address | block 1, byte 40 bit 0 (set = DHCP) | yes | yes |
| | IP Address, Subnet Mask, Default Gateway | block 1, 16..19, 20..23, 24..27 | yes (static only) | yes |
| | Enable Non-Secure Port + its port (default 2101; 0 = disabled) | block 1, 28..29 | yes | yes |
| | Secure Port (ElkRP shows 2601 when the module holds 0) | block 1, 30..31 | yes | yes |
| | Obtain DNS automatically / Specify DNS Addresses to use: Primary, Secondary | block 1, byte 40 bit 3; 32..35, 36..39 | yes | yes |
| | Enable Discovery of M1XEP for AMX and Control4 systems | block 1, byte 40 bit 2 | yes | no |
| | Copy IP Address and Secure Port above to the Account Details Screen | (copies into the file's header fields) | n/a | - |
| | Test DNS | command `75 D4` | no | - |
| Passwords | 8 rows User name / Password (16 characters each) | block 2, `32*i` and `32*i+16` | yes | yes |
| | Disable username/passwords (not recommended) | block 1, byte 40 bit 5 | yes | yes |
| Email | Server URL/IP Address (48), Port (default 25) | block 2, 448..495; 496..497 | yes | no |
| | "FROM" email address, Username, Password (48 each) | block 2, 500, 548, 596 | yes | no |
| | 16 rows Email Address (48) and Message (255) | the 16 email slots, bytes 0..47 and 48..302 | yes | no |
| | Test (email 1) | command `75 D2` | no | - |
| Central Station | per CS 1-8: Receiver Type (G.E. OH2000E, DSC Sur-Gard, DSC Sur-Gard (MAC)), IP Address, IP Port, Account #, Use Encryption (key required), Key, Supervise the Connection, Heartbeat Interval, Receiver, Line, DNIS | blocks 4 and 6 (see code `XepSettings::decode`) | read only | - |
| Dynamic DNS | URL or IP Address of Dynamic DNS Provider, User name, Password, Host name | block 2, 256, 304, 352, 400 (48 each) | yes | no |
| | Email address to send error reports; Send a notice ... whenever the public IP address changes | block 4, 104..151; byte 154 bit 0 | yes | no |
| | Test | command `75 D0` | no | - |
| Time Server | URL or IP Address of Time Server; Time of day to perform update; error notice | block 4, 156..203; 206, 207; byte 208 bit 0 | yes | no |
| | Local time zone (cannot be changed here) | block 4, byte 209 = zone in hours * 4 + 52 | no | - |
| | Test | command `75 D1` | no | - |
| Audio System | Audio Controller IP Address or URL; TCP Port; Using TCP/UDP (ElkRP always shows TCP); Manufacturer/Model; 18 Audio Zone Names; 12 Audio Source Names | block 5: 0..47 (bit 1 of byte 50 = "is a host name"), 48..49, byte 50 bit 0, byte 51, `52+16*i`, `340+16*i` | yes except TCP/UDP | no |
| M1Cloud | Enable this M1XEP to connect to the M1Cloud; Account ID; contact information | block 1 byte 40 bit 4; the ID comes from ElkRP's cloud service | no | - |

Bytes the code has no field for (block 1 byte 41, block 2 bytes 498..499, block 4 bytes 0..31 and 80.. apart from the CS use, the rest of block 6) are
carried through untouched: a change only encodes the fields that changed, onto a copy of what the module held.

## Where each value lives

1. **In the module** (flash of the M1XEP): everything above. Read with Receive, replaced with Send.
2. **In ElkRP's own database** (tables `XEPConfig` with the columns `xepbytes`, `AudioBytes`, `SurgardBytes`, and `EmailDefinitions`): ElkRP's working copy of the
   same bytes. Receive fills it from the module, Send writes it to the module.
3. **In the account export file**: the same bytes again, as the last block of the file (`ExportAccount` calls `frmXEPConfig.Form_Export` last):
   - the five blocks one value per line: block 1 (42 values), block 2 (644), block 4 (210), block 5 (532), block 6 (596), in that order;
   - the number of defined e-mails, then for each: its number (1-16) and its 303 values.
   So 2025 lines without e-mail, plus 304 per e-mail. `Form_Import` reads the same shape back (and clears the block with a warning if the account came from an
   older ElkRP). The editor's check: the lines after the automation block must be exactly that size.
4. **Also in the file's header** (`details.rs`): System URL/IP, Port, Connect Non-Secure and Network adapter. They are ElkRP's way to reach the module and are
   separate from the module's own settings; the Find / "Copy" buttons only copy values between the two places.

There is no per-field wording in the file: it is raw bytes. The mapping above is what `XepSettings::decode` implements.

## Protocol

The module is spoken to over the same framed session as the panel (`ElkSession`: plain, AES or the TLS secure port 2601; login with the RP access code; the
panel allows ONE session at a time). The payload's first byte is the opcode 0x75 ('u'). `M1Fns.SendReceive` sends the payload and waits for a reply whose first
byte is 0x75 and whose second byte echoes the sub-command (and, for e-mails, the third byte echoes the slot).

| Request | Reply | Meaning |
|---|---|---|
| `75 06` | `75 06 ...` 16+ bytes: hardware [8..9], boot [10..12], firmware [13..15] | versions (the module scan already used this). Firmware as `major*65536+minor*256+patch` is ElkRP's `XEP_XIP_AppVer` |
| `75 01` | `75 01` + 42 bytes | block 1 (network) |
| `75 02` | `75 02` + 644 bytes | block 2 (logins, e-mail server, dynamic DNS) |
| `75 04` | `75 04` + 210 bytes | block 4 (ports, receivers, time server) |
| `75 07` | `75 07` + 532 bytes | block 5 (audio) |
| `75 08` | `75 08` + 596 bytes | block 6 (central station addresses and keys) |
| `75 03 n` (n = 0..15) | `75 03 n` + 303 bytes; the slot is defined when its first data byte is not 0 | e-mail slot n+1 |
| `75 81 <42 bytes>` ... `75 82`, `75 84`, `75 87`, `75 88` | echo `75 <sub>` | writes of blocks 1, 2, 4, 5, 6 (Form_Send; "may take up to a minute") |
| `75 83 n <303 bytes>` | echo `75 83 n` | write e-mail slot n+1 (all zero removes it) |
| `75 8A <4 bytes>` | echo | daylight-saving table from the Sunrise/Sunset screen (firmware 2.0.25+). NOT implemented |
| `75 AA` | echo | Reboot. NOT implemented |
| `75 D0`, `D1`, `D2`, `D4` | echo | Test DDNS, Test time server, Test e-mail, Test DNS. NOT implemented (they make the module use the network) |

Layout depends on the module's firmware: ElkRP's `Form_Receive` has older, shorter reply layouts for firmware below 1.3.19 (`66323`), 1.2.11, 1.1.12 and 1.1.3.
This editor handles only 1.3.19 and later (the layout above) and says so for older ones.

**Find** (`XEPFind.cs`): the ASCII text `XEPID` as a UDP broadcast to 255.255.255.255 port 2362 from local port 2362, sent twice a second apart. A module
answers with at least 33 bytes: `M1XEP`, MAC (6), IP (4), secure port (2, big endian), name (16). It only reaches modules on the same network segment.
(`XEPFind` also has a firmware update over TFTP: not touched.)

## What the editor implements (Account Details > Network Adapter > "M1XEP settings...", M1XEP adapter only)

Code: `elk-core/src/xep_config.rs` (layout, decode, validation, encode, read / write, discovery, `XepSim`), `elk-panel/src/m1xep.rs` (routes),
`AccountFile::lines_after_automation` in elk-account, the dialog at the end of `assets/index.html`. Routes (all new, Rust only, ADR-013):

- `POST /api/m1xep/read` `{host, port, code, plain/tls/serial/mac/passphrase}`: reads the module, answers `{module, settings, at}`. Read only.
- `POST /api/m1xep/file`: the M1XEP block of the open account file, decoded (or why it was not found). Read only.
- `POST /api/m1xep/find` `{host}`: the `XEPID` discovery (unicast to `host` if given, else broadcast). Read only.
- `POST /api/m1xep/write` `{connection, settings, apply, confirm, allowIdentity}`: see below.
- `GET /api/m1xep/backups`, `POST /api/m1xep/load` `{file}`: the saved module backups and their settings (nothing is sent).

The write path (`apply: true`) never trusts the page:

1. It reads the module itself and decodes what the module holds now.
2. It computes the changes between that and the sent settings (`XepSettings::apply`): only changed fields are validated and encoded, onto a copy of the module's
   own bytes. Validation follows ElkRP: static IP needs a contiguous subnet mask, a non-zero gateway in the same subnet, ports 1-65535 (non-secure also 0), the two
   ports different, text limits and printable ASCII only, an e-mail needs an address.
3. `apply: false` answers the diff only (field, old, new, identity flag; passwords are shown as asterisks). Nothing is sent.
4. `apply: true` needs `confirm: true`. If any change is an identity field (IP address, mask, gateway, DHCP, DNS, either port, user names, passwords, "disable
   passwords") it also needs `allowIdentity: true`, which the page asks for with a separate red warning tick.
5. Before sending it saves the module's current settings to `m1xep-<serial>-<time>-before-write.json` in the backup folder (raw hex of every block plus the decoded
   settings). If the backup cannot be written, nothing is sent.
6. It sends only the blocks that differ, in ElkRP's order, each acknowledged; then reads the module again and says whether it is identical. After a network
   change the module may drop the link while it re-configures itself: then the read-back reports "could not read back", not a failure.
7. It never reboots the module. The module applies the new setup after a reboot (ElkRP: M1XEP Setup > Reboot, or power-cycle); the answer says so.

A backup is restored by loading it into the form and running the same review / confirm / send steps.

## What was checked on hardware

Once, read only, on the user's own M1XEP, with ElkRP closed: TLS login, `75 06`, then the five block reads and the 16 e-mail reads (no writes of any kind). The
reply sizes matched ElkRP's layout, the decoded static address, mask, gateway and both ports were the module's known values, and the block round-trips through the
file format. The `XEPID` discovery returned nothing from this computer (it is on another subnet than the module; a unicast and a broadcast try both gave no
answer), so discovery is checked only against a loopback simulator.

## Not done / unverified

- Writing to a real module (everything about the write path is simulator-only).
- Discovery against a real module.
- Firmware older than 1.3.19; the Central Station, M1Cloud, time-zone and daylight-saving parts; Reboot; the Test buttons; Trace.
- Writing the M1XEP block back into the account export file (what ElkRP's Send would then send), or "Receive into the file". This needs an ElkRP import test of a
  file whose block was changed, and the file's block depends on the account's telephone entries (a Central Station entry needs a telephone with format 6).
- Using the names in this block (e-mail addresses, audio zone and source names) in the rules wording of `english.rs`, which still falls back to "Email 3".
  The data is available now through `XepBlocks::from_lines`.
- The audio model list (ElkRP reads it from its database), so the page shows the model as a number.
