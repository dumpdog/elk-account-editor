# Packaging and publishing

## Release packages

`make-win-zip.ps1`, `make-mac-app.sh`, `update-from-git.ps1` / `.sh`: see ADR-015 and ADR-016 in `rust/DECISIONS.md` and the workflow `.github/workflows/build.yml`.

## Publishing the public copy (`publish-clean.ps1`)

This repository is private and keeps its full history. The public repository (`dumpdog/elk-account-editor`) is a cleaned copy: one squashed first
commit, no history, and some files left out. `publish-clean.ps1` makes and pushes that copy.

    powershell -File rust\packaging\publish-clean.ps1 -DryRun     # export, exclude, leak scan, print the plan; changes nothing
    powershell -File rust\packaging\publish-clean.ps1 -NoPush     # also commit in the public clone, do not push
    powershell -File rust\packaging\publish-clean.ps1             # commit and push to master

What it does:

1. `git archive HEAD` of this repository into a temp folder (committed, tracked files only; uncommitted changes are not published).
2. Deletes everything listed in `rust/packaging/publish-exclude.txt` (paths and globs).
3. Leak scan: searches every file name, every file and every entry of every `.xlsx` / `.zip` for the regular expressions in `.publish-deny.txt`
   in the repository root. That file is git-ignored and never published: keep it local and add to it whenever a new name, address, serial or
   path must never appear (customer and account names, real panel IP and serials, user names, local paths). A match prints `file:line` (never the
   text) and stops the run. A missing deny list also stops the run, unless `-NoDenyList` is given.
4. Clones the public repository next to this one (`..\Elk-rust-public`, or `-PublicDir`), replaces its working tree with the cleaned export
   (keeping `.git`), writes `.published-from` (the commit of this repository that was published), commits and pushes. The first run makes
   "Initial public version (squashed)"; later runs name the commit subject(s) since the last published commit.
   Commits in the public repository use the GitHub no-reply identity (`-AuthorName` / `-AuthorEmail`), set in the clone only.

The script never changes the public repository's visibility. Work happens in this repository; run the script after the commit you want to publish
(and after CI here is green; run the tests of the exported tree first with `-NoPush` and `cargo test` in `..\Elk-rust-public`).

## Event table

`rust/crates/elk-account/src/data/event-list-m1g.tsv` (the full event table with ElkRP's wording) stays in this repository so its own builds have
the full wording, but it is in `publish-exclude.txt`. `crates/elk-account/build.rs` embeds the file when it exists and an empty table when it does
not. Without the table: event numbers in rules read "Event 1003", the rule editor accepts any event number, `/api/meta` has empty `eventNames`
and `eventWords`. Tests that depend on the table's wording skip themselves (`elk_account::rules::event_table_present()`), and the recorded replay
cases that show event wording (`meta`, `auto-open-sample`, `auto-open-zones-import`, `model-open-file`, `open-communicator`) use the `[..]` wildcard for
those parts (rule `summary` text, `eventNames`, `eventWords`), so replay and tests pass with and without the file and the public repository's CI is green.
