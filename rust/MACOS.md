# macOS build (Apple Silicon first)

Goal: ElkRP-class tooling on Apple Silicon. The Rust editor uses wry/tao (WKWebView) for the window, `rfd` (NSOpenPanel/NSSavePanel) for dialogs,
`muda` for the menu bar (macOS only) and `native-tls` (Security.framework) for the panel's secure port 2601.
Nothing on the Windows path changed: all macOS code is behind `cfg(target_os = "macos")` or `cfg(any(windows, target_os = "macos"))`.

## Verified statically (on Windows, no Mac available)
- `rustup target add aarch64-apple-darwin x86_64-apple-darwin`; then `cargo check` and `cargo clippy -D warnings` for `elk-core`, `elk-account`, `elk-panel`,
  `elk-planner-window`, `rust/app`, `rust/cli` on both targets: clean (libs and bins; `--all-targets` for planner-window and app).
  This type-checks the real macOS code paths of wry 0.57, tao 0.37, rfd 0.18, muda 0.21 and native-tls.
- Not possible here: linking, running, and `--all-targets` for `elk-core` (its dev-dependency `ring` needs a C compiler for macOS).
  The build script of `objc2-exception-helper` (needs `cc` and the Apple SDK) was skipped with a cargo config override, which only works for `check`/`clippy`:
  `cargo clippy --target aarch64-apple-darwin --config "target.aarch64-apple-darwin.objc2_exception_helper_0_1.rustc-link-lib=[]"` (run in a crate folder).
- Code read against the library sources: wry's WKWebView backend defines `window.ipc.postMessage` (-> `webkit.messageHandlers.ipc`) before user init scripts
  (WKUserScript at document start), so the init script's `window.external.sendMessage` / `receiveMessage` shim works; replies go through `evaluate_script`
  on the main thread. tao's event loop and rfd run on the main thread (`main` calls `elk_planner_window::run` there; tokio runs on its own threads).

## Changes made for macOS
- `native_window.rs`: `window.external` shim is more robust (WebKit has its own native `window.external`): define, else delete and define, else assign, else patch the methods. Same result on WebView2.
- `native_window.rs`: macOS menu bar through `muda` (new macOS-only dependency in `elk-planner-window`): app menu with Hide / Quit (Cmd+Q ends the event loop normally so the panel session is closed),
  Edit menu (Undo, Redo, Cut, Copy, Paste, Select All: WKWebView needs it for Cmd+C/V/X/A/Z; a bare tao app has no menu), Window menu (Minimize, Close = Cmd+W).
- `elk-panel` `default_backup_dir()`: on macOS the backup folder is `~/Documents/ElkEditor backups` (the page text and the Mac README say Documents; the C# build used the home folder on macOS). Windows and Linux unchanged.
- Browser fallback: `open <url>` was already in `main.rs`; unchanged.
- Packaging in `rust/packaging/` (`make-mac-app.sh`, `make-win-zip.ps1`, `README-mac.txt`, `README-windows.txt`, `Start in browser.command`) and the workflow `.github/workflows/build.yml`
  (the old C# workflow is now `build-csharp.yml`). The app bundle gets `NSAppTransportSecurity/NSAllowsLocalNetworking` and `NSLocalNetworkUsageDescription`; an optional `rust/packaging/AppIcon.icns` is picked up if present.

## Test checklist on a real Mac (arm64, then Intel if available)
Get the zip from the workflow artifacts (`ElkAccountEditor-mac-arm64`), or build: `cd rust/app && cargo build --release`.
1. [ ] Unzip, run `xattr -dr com.apple.quarantine "Elk Account Editor.app"`, double-click: no "damaged" message after that; `codesign --verify --deep --strict` passes.
2. [ ] The window opens (1500x950, centered, title "Elk account editor"), the page loads from http://127.0.0.1:51980 and is styled and interactive.
3. [ ] Window shows a menu bar; Cmd+C / Cmd+V / Cmd+A / Cmd+Z work in text fields; Cmd+Q and the red close button both quit and the process ends (no leftover `elkaccounteditor` in Activity Monitor).
4. [ ] Open dialog: "Open export file..." shows the NSOpenPanel, opening a sample from `sample/` loads the account (proves `window.external.sendMessage` and the reply path).
5. [ ] Save dialog: "Save edited export..." shows the NSSavePanel with the suggested name; the saved file equals the input byte for byte when nothing was edited (`cmp in out`; compare with the Windows result).
   Also save a zone list, a data bus plan and a CSV/xlsx export; cancel each dialog once (page must not hang).
6. [ ] Dialogs do not freeze the window or beep when used repeatedly (rfd inside the WKWebView message handler).
7. [ ] Live panel, secure port: Panel dialog, address of an M1XEP, "Secure port, TLS" (2601), serial number, RP code. The TLS handshake with the self-signed certificate succeeds through
   Security.framework (TLS 1.2 only, `danger_accept_invalid_certs`), then the issuer check after the handshake accepts the module's certificate. If it fails, note the exact error text
   (a `tls` message names the step: handshake, no certificate, certificate unreadable, untrusted issuer).
8. [ ] Non-secure port 2101 (AES) works too, if the XEP allows it; Compare changes nothing; Send writes a backup first.
9. [ ] Backups land in `~/Documents/ElkEditor backups` (macOS may ask once for Documents access: allow).
10. [ ] `--browser`: `"Elk Account Editor.app/Contents/MacOS/elkaccounteditor" --browser` prints the address and opens the default browser; the page works there (open/save then use the browser's own upload/download). `Start in browser.command` does the same. `--no-open` only prints.
11. [ ] A failed window (for example run over SSH without a GUI session) prints "Could not open a native window ... using your browser instead" and continues in browser mode.
12. [ ] Local-storage persistence: change the theme or panel address, quit, restart: remembered (fixed port 51980 keeps the page origin).
13. [ ] A second copy while the first runs: gets another free port and still works.
14. [ ] macOS Local Network prompt (Sequoia and later) appears on first panel connection; after allowing, connection works.
15. [ ] Intel Mac with the `universal2` zip (optional): `lipo -info` shows both slices and the app starts.

Likely trouble spots if something fails: the TLS step (older M1XEP firmware, weak certificate parameters that Secure Transport refuses: report the exact message),
Cmd+Q/menu behaviour (tao `run_return` plus the muda menu), and the shim (open the Web Inspector is not enabled in release builds; use `--browser` to compare behaviour).

## Reporting results
Copy the checklist into a reply and mark each line pass/fail. For failures add: macOS version, chip, how it was started (double-click, Terminal, `--browser`),
the Terminal output of `"Elk Account Editor.app/Contents/MacOS/elkaccounteditor" ...`, and for TLS the exact error text plus the M1XEP firmware version.
Never paste RP access codes or the real panel address. Then record the outcome in `HANDOFF.md` ("Not matched / not verified").

## CI notes
- `windows` and `macos` (macos-14) jobs: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` in each of the six Cargo projects, release builds, the recorded-case replay
  (`rust-anything app replay . --build "<binary> --browser --no-open"`, installed with `cargo install rust-anything --version 2.1.0 --locked`; it needs no dotnet, the cases are stored), then the zips.
  `elk-core`'s tests run a real TLS server on localhost, so the macOS job also exercises Security.framework against a self-signed certificate (without the M1XEP's own certificate shape).
- Artifacts: `ElkAccountEditor-win-x64`, `ElkAccountEditor-mac-arm64`, `ElkAccountEditor-mac-universal2` (arm64 + x86_64 joined with `lipo`; the x86_64 slice is cross-compiled on macos-14), `elk-cli-mac`.
- No Intel runner job: Intel is covered by the universal2 build, not by a run.
- The workflow was never executed (no remote). Expect first-run fixes; `elk-account` currently has `cargo fmt --check` differences from the parallel Automation port that must be formatted before the fmt step passes.
- Signing: ad hoc only. For Developer ID and notarization set `SIGN_ID` and `NOTARY_PROFILE` when running `make-mac-app.sh` on a Mac.
