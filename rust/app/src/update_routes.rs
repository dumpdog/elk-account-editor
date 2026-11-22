//! `/api/update/*` (Rust editor only, ADR-016): version info, "Check for updates" and "Install". Nothing runs unless the page asks for it.

use elk_panel::{HttpResult, Query};
use serde_json::{Value, json};

use crate::state::App;
use crate::update::{self, Install};

/// The release feed: `ELKEDITOR_UPDATE_URL`, else `updateUrl` in the data folder's `settings.json`, else GitHub's latest release of this project.
pub fn feed_url(app: &App, env: Option<&str>) -> String {
    env.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            app.library
                .settings()
                .0
                .update_url
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| update::DEFAULT_URL.to_string())
}

fn names() -> Vec<&'static str> {
    update::asset_names(std::env::consts::OS, std::env::consts::ARCH)
}

fn install_of(app: &App) -> Install {
    Install::detect(&app.exe, app.library.dir())
}

fn info(app: &App) -> Value {
    let inst = install_of(app);
    json!({
        "version": update::CURRENT,
        "url": feed_url(app, std::env::var("ELKEDITOR_UPDATE_URL").ok().as_deref()),
        "program": inst.bundle.as_ref().unwrap_or(&inst.exe).display().to_string(),
        "canInstall": !names().is_empty(),
        "download": names().first(),
        "dataDir": app.library.dir().map(|d| d.display().to_string()),
    })
}

fn check(app: &App) -> HttpResult {
    let url = feed_url(app, std::env::var("ELKEDITOR_UPDATE_URL").ok().as_deref());
    match update::check(&url, &names()) {
        Ok(c) => HttpResult::json(
            json!({
                "current": c.current, "latest": c.latest, "newer": c.newer, "tag": c.release.tag, "name": c.release.name,
                "notes": c.release.notes, "page": c.release.page,
                "asset": c.asset.as_ref().map(|a| a.name.clone()), "size": c.asset.as_ref().map(|a| a.size),
                "hasChecksums": c.has_sums, "canInstall": c.asset.is_some() && c.has_sums,
            })
            .to_string(),
        ),
        Err(m) => HttpResult::bad_request(&m),
    }
}

fn install(app: &App, body: &[u8]) -> HttpResult {
    let confirmed = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("confirm").and_then(Value::as_bool))
        .unwrap_or(false);
    if !confirmed {
        return HttpResult::bad_request("Installing needs your confirmation (confirm: true).");
    }
    let url = feed_url(app, std::env::var("ELKEDITOR_UPDATE_URL").ok().as_deref());
    let inst = install_of(app);
    match update::install(&url, &names(), &inst) {
        Ok(out) => {
            let program = if inst.bundle.is_some() {
                out.restart.join("Contents/MacOS/elkaccounteditor")
            } else {
                out.restart.clone()
            };
            if app.restart {
                let args: Vec<String> = std::env::args().skip(1).collect();
                update::restart_soon(program, args);
            }
            HttpResult::json(
                json!({
                    "ok": true, "version": out.version, "restarting": app.restart,
                    "backups": out.backups.iter().map(|b| b.display().to_string()).collect::<Vec<_>>(),
                })
                .to_string(),
            )
        }
        Err(m) => HttpResult::bad_request(&m),
    }
}

/// Routes under `/api/update/`; `None` when the path is not one of them.
pub fn handle(app: &App, method: &str, path: &str, _q: &Query, body: &[u8]) -> Option<HttpResult> {
    let rest = path.strip_prefix("/api/update/")?;
    let (get, post) = (
        method.eq_ignore_ascii_case("GET"),
        method.eq_ignore_ascii_case("POST"),
    );
    let empty = |status| HttpResult {
        status,
        content_type: String::new(),
        body: Vec::new(),
    };
    Some(match rest {
        "info" if get => HttpResult::json(info(app).to_string()),
        "check" if post => check(app),
        "install" if post => install(app, body),
        "info" | "check" | "install" => empty(405),
        _ => empty(404),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dispatch;
    use crate::update::tests::{TestServer, temp};
    use elk_panel::Shared;

    fn app_in(d: &std::path::Path, feed: &str) -> App {
        let mut a = App::with_dir(
            Shared::with_backup_dir(d.join("backups")),
            &d.join("ElkEditor data"),
        );
        elk_library::Settings {
            history_keep: 20,
            update_url: Some(feed.to_string()),
            ..Default::default()
        }
        .save(&d.join("ElkEditor data"))
        .unwrap();
        a.exe = d.join("ElkAccountEditor.exe");
        a
    }

    fn post(a: &App, p: &str, b: &str) -> (u16, String) {
        let r = dispatch(a, "POST", p, b.as_bytes());
        (r.status, r.text())
    }

    #[test]
    fn url_precedence_env_then_settings_then_github() {
        let d = temp("urlprec");
        let a = app_in(&d, "http://127.0.0.1:1/settings");
        assert_eq!(
            feed_url(&a, Some("http://127.0.0.1:2/env")),
            "http://127.0.0.1:2/env"
        );
        assert_eq!(feed_url(&a, Some("  ")), "http://127.0.0.1:1/settings");
        assert_eq!(feed_url(&a, None), "http://127.0.0.1:1/settings");
        elk_library::Settings::default()
            .save(&d.join("ElkEditor data"))
            .unwrap();
        assert_eq!(feed_url(&a, None), update::DEFAULT_URL);
        assert!(update::DEFAULT_URL.contains("dumpdog/elk-account-editor/releases/latest"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn info_check_and_install_over_http() {
        let names = names();
        if names.is_empty() {
            return; // no build for this platform
        }
        let d = temp("routes");
        std::fs::write(d.join("ElkAccountEditor.exe"), "OLD EXE").unwrap();
        std::fs::write(d.join("README.txt"), "OLD README").unwrap();
        let s = TestServer::start();
        let zip = {
            use std::io::Write;
            let mut buf = Vec::new();
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("pkg/ElkAccountEditor.exe", o).unwrap();
            z.write_all(b"NEW EXE").unwrap();
            z.finish().unwrap();
            buf
        };
        let a = app_in(&d, &s.url("/latest"));
        // 404 before anything is published
        let (st, t) = post(&a, "/api/update/check", "");
        assert_eq!(st, 400);
        assert!(t.contains("HTTP 404"), "{t}");
        let r = serde_json::json!({"tag_name": "v99.1.0", "name": "99.1.0", "body": "notes here", "html_url": s.url("/rel"), "assets": [
            {"name": names[0], "browser_download_url": s.url("/dl/zip"), "size": zip.len()},
            {"name": "SHA256SUMS", "browser_download_url": s.url("/dl/sums"), "size": 10}]});
        s.put("/latest", 200, r.to_string());
        s.put("/dl/zip", 200, zip.clone());
        s.put(
            "/dl/sums",
            200,
            format!("{}  {}\n", update::sha256_hex(&zip), names[0]),
        );
        let (st, t) = post(&a, "/api/update/check", "");
        assert_eq!(st, 200, "{t}");
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(
            (
                v["newer"].clone(),
                v["latest"].clone(),
                v["notes"].clone(),
                v["canInstall"].clone()
            ),
            (
                json!(true),
                json!("99.1.0"),
                json!("notes here"),
                json!(true)
            )
        );
        let i: Value =
            serde_json::from_str(&dispatch(&a, "GET", "/api/update/info", b"").text()).unwrap();
        assert_eq!(i["version"], update::CURRENT);
        // no click, no install
        assert_eq!(post(&a, "/api/update/install", "{}").0, 400);
        assert_eq!(
            std::fs::read_to_string(d.join("ElkAccountEditor.exe")).unwrap(),
            "OLD EXE"
        );
        let (st, t) = post(&a, "/api/update/install", r#"{"confirm":true}"#);
        assert_eq!(st, 200, "{t}");
        assert_eq!(
            std::fs::read_to_string(d.join("ElkAccountEditor.exe")).unwrap(),
            "NEW EXE"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("ElkAccountEditor.exe.old")).unwrap(),
            "OLD EXE"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("README.txt")).unwrap(),
            "OLD README",
            "a file the zip does not carry stays"
        );
        assert!(
            d.join("ElkEditor data/library.sqlite").is_file(),
            "the library is untouched"
        );
        assert_eq!(dispatch(&a, "GET", "/api/update/check", b"").status, 405);
        assert_eq!(dispatch(&a, "GET", "/api/update/nope", b"").status, 404);
        let _ = std::fs::remove_dir_all(d);
    }
}
