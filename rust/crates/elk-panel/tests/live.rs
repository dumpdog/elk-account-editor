//! LivePanel (the shared session) and the route helpers, against the simulated panel. The byte-exact answers of the routes are in golden.rs.

mod common;

use common::*;
use elk_core::simulated_panel::SimulatedPanel;
use elk_panel::routes::{default_backup_dir, parse_datetime};
use elk_panel::{handle, testkit, Kind, LivePanel, Query, Settings, Shared};
use std::time::Duration;

fn settings(p: &SimulatedPanel) -> Settings {
    Settings { host: "127.0.0.1".into(), port: p.port() as i32, code: CODE.into(), plain: true, mac: None, passphrase: None, tls: false, serial: None }
}

fn aes_settings(p: &SimulatedPanel) -> Settings {
    Settings { plain: false, mac: Some(MAC.into()), passphrase: Some(PASS.into()), ..settings(p) }
}

fn logins(p: &SimulatedPanel) -> usize {
    p.received().iter().filter(|r| r.len() >= 4 && r[0] == 0x7F && r[3] == 1).count()
}

fn live() -> LivePanel {
    let mut l = LivePanel::new();
    l.retry_delay = Duration::from_millis(5);
    l.enroll_wait = Duration::ZERO;
    l
}

#[test]
fn connect_poll_and_disconnect() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    assert!(!l.connected());
    assert!(l.poll().unwrap().is_none());
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    assert!(l.connected());
    assert_eq!(Some("127.0.0.1".to_string()), l.host());
    assert_eq!(Some("5.2.9".to_string()), l.firmware());
    assert_eq!(Some("31415926".to_string()), l.serial());
    let snap = l.poll().unwrap().unwrap();
    assert_eq!(8, snap.areas.len());
    assert_eq!(208, snap.zones.len());
    l.disconnect();
    assert!(!l.connected());
    assert_eq!(None, l.firmware());
    assert_eq!(Some("127.0.0.1".to_string()), l.host(), "the host stays after a disconnect, like _settings in the C#");
}

#[test]
fn aes_session_works_too() {
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    let l = live();
    l.connect(&aes_settings(&panel), &mut |_| {}).unwrap();
    assert!(l.poll().unwrap().is_some());
}

#[test]
fn use_session_borrows_the_open_session_for_the_same_panel() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    assert_eq!(1, logins(&panel));
    let fw = l.use_session(&settings(&panel), &mut |_| {}, |s| Ok(s.panel.as_ref().unwrap().firmware.clone())).unwrap();
    assert_eq!("5.2.9", fw);
    assert_eq!(1, logins(&panel), "no second login");
    assert!(l.connected());
}

#[test]
fn use_session_opens_and_closes_its_own_session_when_nothing_is_open() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    let fw = l.use_session(&settings(&panel), &mut |_| {}, |s| Ok(s.panel.as_ref().unwrap().serial_hex.clone())).unwrap();
    assert_eq!("31415926", fw);
    assert_eq!(1, logins(&panel));
    assert!(!l.connected());
}

#[test]
fn use_session_for_another_port_does_not_touch_the_open_session() {
    let a = SimulatedPanel::new_plain(CODE);
    let b = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&a), &mut |_| {}).unwrap();
    l.use_session(&settings(&b), &mut |_| {}, |_| Ok(())).unwrap();
    assert_eq!(1, logins(&b));
    assert!(l.connected());
    assert!(l.poll().unwrap().is_some());
}

#[test]
fn use_open_without_a_session_says_so() {
    let l = live();
    let e = l.use_open(|_| Ok(())).unwrap_err();
    assert_eq!(Kind::InvalidOperation, e.kind());
    assert_eq!("Not connected. Connect on the Status tab first.", e.message());
    let e = l.change_access_code("123456").unwrap_err();
    assert_eq!("Not connected. Connect on the Live tab first.", e.message());
    let e = l.enroll(&mut |_| {}).unwrap_err();
    assert_eq!("Not connected. Connect on the Status tab first.", e.message());
}

#[test]
fn a_broken_link_closes_the_session_and_keeps_the_reason() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    // a request the simulated panel never answers makes the borrowed call time out: that is a link error
    let e = l
        .use_open(|s| {
            s.timeout = Duration::from_millis(100);
            s.send(&[0x7B, 0, 0, 0, 0, 0, 0])?; // not a request the simulator answers
            Ok(s.receive(0x7B)?)
        })
        .unwrap_err();
    assert_eq!(Kind::Timeout, e.kind());
    assert!(!l.connected());
    assert_eq!(Some(e.message()), l.error());
    // the next connect clears the error
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    assert_eq!(None, l.error());
}

#[test]
fn a_dropped_connection_makes_poll_return_none_with_the_error_set() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    drop(panel);
    assert!(l.poll().unwrap().is_none());
    assert!(!l.connected());
    assert!(l.error().is_some());
}

#[test]
fn errors_that_are_not_link_errors_keep_the_session() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    let e = l.use_open(|_| Err::<(), _>(elk_panel::PanelError::Elk(elk_core::ElkError::InvalidOperation("nope".into())))).unwrap_err();
    assert_eq!("nope", e.message());
    assert!(l.connected());
    assert_eq!(None, l.error());
}

#[test]
fn changing_the_access_code_is_remembered_for_the_next_login() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    assert!(l.change_access_code("135790").unwrap());
    assert_eq!("135790", panel.access_code());
    // enrollment re-logs in when the link broke; with the remembered code it succeeds. The simulator does not drop the link, so check the session still works.
    assert!(l.poll().unwrap().is_some());
}

#[test]
fn enroll_on_the_simulator_reports_the_missing_answers() {
    let panel = SimulatedPanel::new_plain(CODE);
    testkit::seed(&panel, &sample_file("Sample Communicator"));
    let mut l = live();
    l.enroll_wait = Duration::from_millis(1);
    l.connect(&settings(&panel), &mut |_| {}).unwrap();
    // the simulator answers neither the enrollment nor the module scan: enroll() errors at the scan with a timeout and leaves the session open
    let e = l.enroll(&mut |_| {}).unwrap_err();
    assert_eq!(Kind::Timeout, e.kind());
    assert!(l.connected());
}

#[test]
fn connect_failure_is_reported_after_the_retries_with_a_log_line_each() {
    let l = live();
    let mut log = Vec::new();
    let st = Settings { host: "127.0.0.1".into(), port: 1, code: CODE.into(), plain: true, mac: None, passphrase: None, tls: false, serial: None };
    let e = l.connect(&st, &mut |m| log.push(m.to_string())).unwrap_err();
    assert!(e.is_io());
    assert_eq!(5, log.len());
    assert_eq!("login attempt 1 failed (SocketException); retrying in 0 s", log[0]);
    assert!(!l.connected());
    assert_eq!(None, l.host(), "settings are only remembered after a successful connect");
}

#[test]
fn a_rejected_code_is_not_retried() {
    let panel = SimulatedPanel::new_plain(CODE);
    let l = live();
    let mut log = Vec::new();
    let st = Settings { code: "000000".into(), ..settings(&panel) };
    let e = l.connect(&st, &mut |m| log.push(m.to_string())).unwrap_err();
    assert_eq!("Panel rejected the RP access code.", e.to_string());
    assert!(log.is_empty());
}

// ---- helpers --------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn query_parsing() {
    let q = Query::parse("?max=5&name=a%20b+c&flag");
    assert_eq!(Some("5"), q.get("max"));
    assert_eq!(Some("5"), q.get("MAX"));
    assert_eq!(Some("a b c"), q.get("name"));
    assert_eq!(Some(""), q.get("flag"));
    assert_eq!(None, q.get("zz"));
}

#[test]
fn datetime_formats() {
    let d = |y, mo, da, h, mi, s| chrono::NaiveDate::from_ymd_opt(y, mo, da).unwrap().and_hms_opt(h, mi, s).unwrap();
    assert_eq!(Some(d(2026, 1, 2, 3, 4, 5)), parse_datetime("2026-01-02 03:04:05"));
    assert_eq!(Some(d(2026, 1, 2, 3, 4, 5)), parse_datetime(" 2026-01-02T03:04:05 "));
    assert_eq!(Some(d(2026, 3, 4, 5, 6, 0)), parse_datetime("2026-03-04T05:06"));
    assert_eq!(Some(d(2026, 12, 31, 23, 59, 30)), parse_datetime("12/31/2026 11:59:30 PM"));
    assert_eq!(Some(d(2026, 12, 31, 0, 0, 0)), parse_datetime("2026-12-31"));
    assert_eq!(None, parse_datetime("soon"));
    assert_eq!(None, parse_datetime(""));
}

#[test]
fn bad_request_uses_the_relaxed_json_string_encoding() {
    let r = elk_panel::HttpResult::bad_request("it's <b> & \"q\" + `t`\r\n\\ é 😀");
    assert_eq!(400, r.status);
    assert_eq!("application/json; charset=utf-8", r.content_type);
    assert_eq!("\"it's <b> & \\\"q\\\" + `t`\\r\\n\\\\ é \\uD83D\\uDE00\"", r.text());
}

#[test]
fn default_backup_folder_name() {
    assert_eq!("ElkEditor backups", default_backup_dir().file_name().unwrap().to_string_lossy());
}

#[test]
fn backups_list_is_newest_first_and_only_panel_json_files() {
    let dir = temp_dir("backups");
    let shared = Shared::with_backup_dir(dir.join("b"));
    // an empty (new) folder is created and lists nothing
    let r = handle(&shared, "GET", "/api/panel/backups", b"", &Query::new()).unwrap();
    assert_eq!((200, "[]".to_string()), (r.status, r.text()));
    let b = dir.join("b");
    std::fs::write(b.join("panel-1-old.json"), vec![b'x'; 2048]).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(b.join("panel-2-new.json"), "{}").unwrap();
    std::fs::write(b.join("notes.txt"), "x").unwrap();
    std::fs::write(b.join("panel-3.txt"), "x").unwrap();
    let r = handle(&shared, "GET", "/api/panel/backups", b"", &Query::new()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&r.text()).unwrap();
    let a = v.as_array().unwrap();
    assert_eq!(2, a.len());
    assert_eq!("panel-2-new.json", a[0]["name"]);
    assert_eq!(0, a[0]["kb"]);
    assert_eq!("panel-1-old.json", a[1]["name"]);
    assert_eq!(2, a[1]["kb"]);
    assert_eq!(19, a[0]["when"].as_str().unwrap().len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn backup_delete_only_removes_panel_json_files_in_the_folder() {
    let dir = temp_dir("bkdel");
    let shared = Shared::with_backup_dir(dir.join("b"));
    let b = dir.join("b");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("panel-1-old.json"), "{}").unwrap();
    std::fs::write(b.join("notes.txt"), "x").unwrap();
    std::fs::write(dir.join("panel-out.json"), "{}").unwrap();
    let del = |n: &str| handle(&shared, "POST", "/api/panel/backup/delete", format!("{{\"name\":\"{n}\"}}").as_bytes(), &Query::new()).unwrap();
    assert_eq!(400, del("notes.txt").status);
    assert_eq!(400, del("..\\\\panel-out.json").status);
    assert_eq!(400, del("../panel-out.json").status);
    assert_eq!(400, del("panel-nothere.json").status);
    assert!(b.join("notes.txt").exists() && dir.join("panel-out.json").exists());
    assert_eq!(200, del("panel-1-old.json").status);
    assert!(!b.join("panel-1-old.json").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unknown_routes_are_not_handled() {
    let shared = Shared::with_backup_dir(temp_dir("x"));
    assert!(handle(&shared, "GET", "/api/model", b"", &Query::new()).is_none());
    assert!(handle(&shared, "GET", "/api/panel/compare", b"", &Query::new()).is_none());
}

#[test]
fn idle_status_and_progress() {
    let shared = Shared::with_backup_dir(temp_dir("y"));
    let r = handle(&shared, "GET", "/api/live/status", b"", &Query::new()).unwrap();
    assert!(r.text().starts_with("{\"connected\":false,\"host\":null,\"error\":null,\"firmware\":null,\"serial\":null,\"snapshot\":null,\"at\":\""));
    let r = handle(&shared, "GET", "/api/panel/progress", b"", &Query::new()).unwrap();
    assert_eq!("{\"active\":false,\"phase\":\"\",\"note\":\"\",\"done\":0,\"total\":0,\"seconds\":0}", r.text());
}

#[test]
fn progress_is_cleared_after_every_tracked_route() {
    let panel = SimulatedPanel::new_plain(CODE);
    testkit::seed(&panel, &sample_file("Sample Communicator"));
    let dir = temp_dir("p");
    let shared = Shared::with_backup_dir(dir.join("b"));
    let body = format!("{{\"host\":\"127.0.0.1\",\"port\":{},\"code\":\"246801\",\"plain\":true}}", panel.port());
    let r = handle(&shared, "POST", "/api/panel/pull", body.as_bytes(), &Query::new()).unwrap();
    assert_eq!(200, r.status);
    let p = shared.progress.snapshot();
    assert!(!p.active);
    assert!(p.done > 400);
    assert_eq!(p.total, p.done);
    // the file the pull read into is now the open file
    assert_eq!(Some("From panel".to_string()), shared.session().source_name);
    assert!(shared.session().original_text.is_some());
    let _ = std::fs::remove_dir_all(dir);
}
