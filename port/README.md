# Elk port (work in progress)

.NET 8 library + CLI reimplementing the ElkRP panel protocol, derived from decompiled ElkRP 3.0.7 and checked against
sample accounts. Layout and CRC rules are verified against stored data; the network session is verified only against a
simulated panel (tests/Elk.Tests/SimulatedPanel.cs). Read-only `download` over the non-secure port (2101) has been run once against a real M1G (2026-11-09).

    dotnet test                         # 77 tests
    dotnet run --project src/Elk.Cli -- decode zone <hex>
    dotnet run --project src/Elk.Cli -- download --host H --mac M --pass P --code C --out panel.json
    dotnet run --project src/Elk.Cli -- download --plain --host H --port 2101 --code C --out panel.json   # non-secure port, no MAC/passphrase
    dotnet run --project src/Elk.Cli -- upload   --host H --mac M --pass P --code C --in panel.json        # dry run
    dotnet run --project src/Elk.Cli -- upload   ... --yes                                                  # writes
    dotnet run --project src/Elk.Cli -- upload-export --plain --host H --port 2101 --code C --in "ElkRP export file" [--yes]   # apply an export to the panel

Covered: framing + CRC, AES session key, connect/login, zone/area/keypad records (read, write, CRC query), keep-alive, disconnect.
Not covered: other sections (users, phones, report codes, outputs, tasks, rules, X10, wireless), TLS and serial transports,
firmware flashing, the Access database import. See ../docs/TRACE-FINDINGS.md.

## Account editor (current approach)

`src/Elk.Editor` is a local web app that edits an ElkRP **Export Account** file (zones, users, areas, keypads, telephones, communicator
report codes). Load the saved file in ElkRP with **File > Import Account**. It works on M1G exports (tested on software 5.0.12 and 5.3.44) and
never touches the database or a panel. Unedited sections are copied through unchanged, so an open/save with no edits is byte-identical.

    dist\Editor-win-x64\ElkAccountEditor.exe "path\to\export file"      # file argument optional; opens its own window (add --browser to use a browser tab)

See ../docs/ELK-ACCOUNT-EDITOR-HANDOFF.md for the file format, what has been verified against ElkRP, and open items. The AccountTool
below is the older database-editing approach and is obsolete.

## Zone import into a saved account (Windows, obsolete)

    dist\AccountTool\Elk.AccountTool.exe template --out zones.csv
    dist\AccountTool\Elk.AccountTool.exe import-zones --account "NEW ACCOUNT ID" --csv zones.csv            # dry run
    dist\AccountTool\Elk.AccountTool.exe import-zones --account "NEW ACCOUNT ID" --csv zones.csv --yes      # writes, backs up first

CSV columns: Zone,Name,Definition,Type,Area,Attributes (attributes separated with |). Create and save the account in ElkRP from your
template first, then close ElkRP. Voice descriptions are filled in automatically. Use `--db` to try it on a copy first.
