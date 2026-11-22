//! Golden tests: every response of the panel and live routes equals, byte for byte, what the C# editor answered for the same request against the same
//! simulated panel. The scenarios are in `tests/fixtures/scenarios.json`, the C# answers in `tests/fixtures/expected` (made by `tests/fixtures/regen.ps1`).
//! Times, the panel's backup folder and the backup time stamps are replaced by placeholders on both sides before comparing.

mod common;

use common::*;
use elk_account::{decode_latin1, AccountFile};
use elk_core::simulated_panel::SimulatedPanel;
use elk_panel::{handle, testkit, Query, Shared};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Replaces what differs between runs: clock times, the backup folder and the time stamp in backup file names.
fn normalize(text: &str) -> String {
    // The OS error text of a refused connection differs per platform (the fixtures were recorded on Windows).
    let text = text.replace("No connection could be made because the target machine actively refused it.", "@REFUSED@").replace("Connection refused", "@REFUSED@");
    let text = text.as_str();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        // "at":"HH:mm:ss"
        if let Some(r) = rest.strip_prefix("\"at\":\"") {
            if r.len() >= 9 && r.as_bytes()[8] == b'"' {
                out.push_str("\"at\":\"@T@\"");
                rest = &r[9..];
                continue;
            }
        }
        // "computer":"yyyy-MM-dd HH:mm:ss"
        if let Some(r) = rest.strip_prefix("\"computer\":\"") {
            if r.len() >= 20 && r.as_bytes()[19] == b'"' {
                out.push_str("\"computer\":\"@NOW@\"");
                rest = &r[20..];
                continue;
            }
        }
        // a backup path: everything before `panel-` is the folder
        for key in ["\"path\":\"", "\"backupPath\":\""] {
            if let Some(r) = rest.strip_prefix(key) {
                if let Some(i) = r.find("panel-") {
                    out.push_str(key);
                    out.push_str("@DIR@");
                    rest = &r[i..];
                    break;
                }
            }
        }
        // panel-<serial>-yyyyMMdd-HHmmss-
        if let Some(r) = rest.strip_prefix("panel-") {
            if let Some(d) = r.find('-') {
                let t = &r[d + 1..];
                if t.len() >= 16 && t.as_bytes()[8] == b'-' && t.as_bytes()[15] == b'-' && t[..8].bytes().all(|b| b.is_ascii_digit()) {
                    out.push_str(&format!("panel-{}-@TS@", &r[..d]));
                    rest = &t[15..];
                    continue;
                }
            }
        }
        let mut chars = rest.chars();
        out.push(chars.next().unwrap());
        rest = chars.as_str();
    }
    out
}

fn run_scenario(name: &str) {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("scenarios.json")).unwrap()).unwrap();
    let sc = manifest["scenarios"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("no scenario {name}"));
    let sample = sc["sample"].as_str().unwrap();
    let text = if sample == "@template" { template() } else { sample_text(sample) };
    let f = AccountFile::parse(&text).unwrap();
    let panel = SimulatedPanel::new_plain(CODE);
    testkit::setup(sc["setup"].as_str().unwrap(), &panel, &f).unwrap();

    let dir = temp_dir(name);
    let mut shared = Shared::with_backup_dir(dir.join("ElkEditor backups"));
    shared.live.retry_delay = Duration::from_millis(10);
    shared.live.enroll_wait = Duration::ZERO;
    shared.control_delay = Duration::ZERO;

    let exp = fixtures().join("expected");
    let mut last = String::new();
    for st in sc["steps"].as_array().unwrap() {
        let id = st["id"].as_str().unwrap();
        let tag = format!("{name}__{id}");
        match st["kind"].as_str().unwrap() {
            "open" => {
                let mut s = shared.session();
                s.original_text = Some(text.clone());
                s.source_name = Some(name.to_string());
                continue;
            }
            "new" => {
                let mut s = shared.session();
                s.original_text = Some(template());
                s.source_name = Some("New account".into());
                continue;
            }
            _ => {}
        }
        let body = if let Some(r) = st.get("request").and_then(|r| r.as_str()) {
            std::fs::read_to_string(fixtures().join("requests").join(format!("{r}.json"))).unwrap()
        } else {
            st.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string()
        };
        let body = body.replace("@PORT@", &panel.port().to_string()).replace("@LASTBACKUP@", &last);
        // Path.GetFileName splits on '\' only on Windows; elsewhere the same path trick is written with '/' (the product keeps the C# behaviour).
        let body = if cfg!(windows) { body } else { body.replace(r"..\\..\\", "../../") };
        let path_full = st["path"].as_str().unwrap();
        let (path, q) = path_full.split_once('?').unwrap_or((path_full, ""));
        let r = handle(&shared, st["method"].as_str().unwrap(), path, body.as_bytes(), &Query::parse(q)).unwrap_or_else(|| panic!("{tag}: not routed"));

        let want_status = std::fs::read_to_string(exp.join(format!("{tag}.status"))).unwrap_or_else(|e| panic!("{tag}: no expected status ({e}); run regen.ps1"));
        let mut lines = want_status.lines();
        let status: u16 = lines.next().unwrap().parse().unwrap();
        let ctype = lines.next().unwrap_or("");
        assert_eq!(status, r.status, "{tag}: status");
        assert_eq!(ctype, r.content_type, "{tag}: content type");
        let want_body = String::from_utf8(std::fs::read(exp.join(format!("{tag}.body"))).unwrap()).unwrap();
        let mut got = r.text();
        got = got.replace("retrying in 0 s", "retrying in 5 s"); // the test runs with a 10 ms pause instead of 5 s
        got = without_details(&got);
        assert_eq!(normalize(&want_body), normalize(&got), "{tag}: body");

        if let Some(key) = st.get("file").and_then(|k| k.as_str()) {
            if r.status == 200 {
                let j: Value = serde_json::from_str(&r.text()).unwrap();
                let p = PathBuf::from(j[key].as_str().unwrap());
                assert!(p.starts_with(&dir), "{tag}: backup written outside the configured folder");
                let want_file = std::fs::read(exp.join(format!("{tag}.file"))).unwrap();
                // The backup uses Environment.NewLine like the C# (CRLF on Windows, LF elsewhere); the fixtures were recorded on Windows.
                let want_file = if cfg!(windows) { want_file } else { String::from_utf8_lossy(&want_file).replace("\r\n", "\n").into_bytes() };
                assert_eq!(String::from_utf8_lossy(&want_file), String::from_utf8_lossy(&std::fs::read(&p).unwrap()), "{tag}: backup file");
                last = p.file_name().unwrap().to_string_lossy().into_owned();
            }
        }
    }
    shared.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

fn sample_text(name: &str) -> String {
    decode_latin1(&std::fs::read(repo_root().join("sample").join(name)).unwrap())
}

#[test]
fn comm_sync() {
    run_scenario("comm_sync");
}

#[test]
fn template_reset() {
    run_scenario("template_reset");
}

#[test]
fn pull_extras() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    run_scenario("pull_extras");
}

#[test]
fn pull_blank() {
    run_scenario("pull_blank");
}

#[test]
fn errors() {
    run_scenario("errors");
}

#[test]
fn unreachable() {
    run_scenario("unreachable");
}

#[test]
fn live_session() {
    run_scenario("live_session");
}

#[test]
fn normalize_replaces_times_and_folders() {
    let s = r#"{"path":"C:\\x\\ElkEditor backups\\panel-31415926-20261119-215650-manual.json","name":"panel-31415926-20261119-215650-manual.json","at":"21:56:50","computer":"2026-11-19 21:56:50"}"#;
    assert_eq!(r#"{"path":"@DIR@panel-31415926-@TS@-manual.json","name":"panel-31415926-@TS@-manual.json","at":"@T@","computer":"@NOW@"}"#, normalize(s));
}

/// The Rust model JSON has one member the C# oracle did not write (`details`, the Account Details page, ADR-010).
fn without_details(json: &str) -> String {
    match json.find(",\"details\":{") {
        Some(at) => {
            let end = json[at..].find('}').unwrap() + at + 1;
            format!("{}{}", &json[..at], &json[end..])
        }
        None => json.to_string(),
    }
}
