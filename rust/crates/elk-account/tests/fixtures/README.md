Golden fixtures: answers of the C# editor (port/, built in rust/original-build) for the same inputs.

- `requests/` model JSON bodies (made by `cargo run --example make_requests -- ../../../sample tests/fixtures/requests`)
- `model/` POST /api/open, `saved/` POST /api/save (`.bin` bytes, `.err` message body), `export/` zones/export and export-alarm-list,
  `readinputs/` + `read/` POST /api/zones/read, `cli/` the CLI `export-zones` run on sample/sample-zones.csv.
- `regen.ps1` re-creates `model/ saved/ export/ read/` by starting the C# server on a spare port.
