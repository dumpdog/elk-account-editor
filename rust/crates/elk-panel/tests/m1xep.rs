//! The `/api/m1xep/*` routes against the simulated panel and module. All data is made up.

mod common;

use common::*;
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::xep_config::{XepBlocks, XepSettings, EMAIL_LEN};
use elk_panel::{handle, HttpResult, Query, Shared};
use serde_json::{json, Value};

fn put(b: &mut [u8], at: usize, len: usize, s: &str) {
    for (i, slot) in b[at..at + len].iter_mut().enumerate() {
        *slot = s.as_bytes().get(i).copied().unwrap_or(0);
    }
}

fn blocks() -> XepBlocks {
    let mut b = XepBlocks::blank();
    put(&mut b.xep1, 0, 16, "Test XEP");
    b.xep1[16..20].copy_from_slice(&[192, 168, 7, 20]);
    b.xep1[20..24].copy_from_slice(&[255, 255, 255, 0]);
    b.xep1[24..28].copy_from_slice(&[192, 168, 7, 1]);
    b.xep1[28..30].copy_from_slice(&2101u16.to_be_bytes());
    b.xep1[30..32].copy_from_slice(&2601u16.to_be_bytes());
    put(&mut b.xep2, 0, 16, "installer");
    put(&mut b.xep2, 16, 16, "pw-one");
    put(&mut b.xep4, 156, 48, "time.example.test");
    let mut e = vec![0u8; EMAIL_LEN];
    put(&mut e, 0, 48, "owner@example.test");
    put(&mut e, 48, 255, "The alarm went off");
    b.emails[1] = Some(e);
    b
}

struct Rig {
    panel: SimulatedPanel,
    state: Shared,
    dir: std::path::PathBuf,
}

fn rig() -> Rig {
    let panel = SimulatedPanel::new_plain(CODE);
    panel.with_xep(|x| {
        x.enabled = true;
        x.blocks = blocks();
    });
    let dir = temp_dir("m1xep");
    Rig { state: Shared::with_backup_dir(dir.clone()), panel, dir }
}

impl Rig {
    fn conn(&self) -> Value {
        json!({ "host": "127.0.0.1", "port": self.panel.port(), "code": CODE, "plain": true })
    }
    fn post(&self, path: &str, body: &Value) -> HttpResult {
        handle(&self.state, "POST", path, body.to_string().as_bytes(), &Query::new()).expect("route")
    }
    fn write(&self, settings: &XepSettings, extra: Value) -> HttpResult {
        let mut b = self.conn();
        b["settings"] = serde_json::to_value(settings).unwrap();
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        self.post("/api/m1xep/write", &b)
    }
    fn writes_received(&self) -> usize {
        self.panel.received().iter().filter(|r| r[0] == 0x75 && r[1] >= 0x80).count()
    }
    fn held(&self) -> XepSettings {
        XepSettings::decode(&self.panel.with_xep(|x| x.blocks.clone()))
    }
}

fn body(r: &HttpResult) -> Value {
    serde_json::from_slice(&r.body).unwrap_or_else(|_| Value::String(r.text()))
}

#[test]
fn read_route_answers_the_decoded_settings() {
    let r = rig();
    let res = r.post("/api/m1xep/read", &r.conn());
    assert_eq!(res.status, 200, "{}", res.text());
    let v = body(&res);
    assert_eq!(v["module"]["firmware"], "1.3.20");
    assert_eq!(v["settings"]["name"], "Test XEP");
    assert_eq!(v["settings"]["ip"], "192.168.7.20");
    assert_eq!(v["settings"]["nonSecurePort"], 2101);
    assert_eq!(v["settings"]["logins"][0]["password"], "pw-one");
    assert_eq!(v["settings"]["emails"][1]["address"], "owner@example.test");
    assert_eq!(v["settings"]["reports"].as_array().unwrap().len(), 8);
    assert_eq!(r.writes_received(), 0);
}

#[test]
fn read_route_errors() {
    let r = rig();
    assert_eq!(r.post("/api/m1xep/read", &json!({})).status, 400);
    r.panel.with_xep(|x| x.app_version = 0x01_0100);
    let res = r.post("/api/m1xep/read", &r.conn());
    assert_eq!(res.status, 400);
    assert!(res.text().contains("1.1.0") && res.text().contains("1.3.19"), "{}", res.text());
    assert!(handle(&r.state, "GET", "/api/m1xep/read", b"", &Query::new()).is_none(), "read is POST only");
}

#[test]
fn dry_run_shows_the_diff_and_sends_nothing() {
    let r = rig();
    let mut s = r.held();
    s.name = "Garage".into();
    s.ip = "192.168.7.21".into();
    s.logins[0].password = "new-pw".into();
    let res = r.write(&s, json!({}));
    assert_eq!(res.status, 200, "{}", res.text());
    let v = body(&res);
    assert_eq!(v["applied"], false);
    assert_eq!(v["identityChanges"], true);
    let fields: Vec<&str> = v["changes"].as_array().unwrap().iter().map(|c| c["field"].as_str().unwrap()).collect();
    assert_eq!(fields, vec!["Device Name", "IP Address", "Password 1"]);
    assert_eq!(v["changes"][2]["new"], "******", "passwords are masked");
    assert_eq!(r.writes_received(), 0);
    assert_eq!(r.held().name, "Test XEP");
}

#[test]
fn write_needs_the_confirmation_and_the_separate_identity_warning() {
    let r = rig();
    let mut s = r.held();
    s.name = "Garage".into();
    let res = r.write(&s, json!({ "apply": true }));
    assert_eq!(res.status, 400);
    assert!(res.text().contains("confirmation"), "{}", res.text());
    assert_eq!(r.writes_received(), 0);
    assert!(std::fs::read_dir(&r.dir).unwrap().next().is_none(), "no backup without a write");

    let mut s = r.held();
    s.secure_port = 2602;
    let res = r.write(&s, json!({ "apply": true, "confirm": true }));
    assert_eq!(res.status, 400);
    assert!(res.text().contains("lock you out") && res.text().contains("Secure Port"), "{}", res.text());
    assert_eq!(r.writes_received(), 0);
    assert!(std::fs::read_dir(&r.dir).unwrap().next().is_none(), "no backup when refused");
}

#[test]
fn write_saves_a_backup_first_sends_the_changed_block_and_checks_it() {
    let r = rig();
    let before = r.panel.with_xep(|x| x.blocks.clone());
    let mut s = r.held();
    s.name = "Garage".into();
    s.time_server = "pool.example.test".into();
    let res = r.write(&s, json!({ "apply": true, "confirm": true }));
    assert_eq!(res.status, 200, "{}", res.text());
    let v = body(&res);
    assert_eq!(v["applied"], true);
    assert_eq!(v["verified"], true, "{}", v["verifyNote"]);
    assert_eq!(v["rebootRequired"], true);
    assert_eq!(v["sent"].as_array().unwrap().len(), 2, "blocks 1 and 4 only");
    assert_eq!(r.held().name, "Garage");
    assert_eq!(r.held().time_server, "pool.example.test");
    // the backup holds what the module had before
    let name = v["backup"]["name"].as_str().unwrap();
    assert!(name.starts_with("m1xep-31415926-") && name.ends_with("-before-write.json"), "{name}");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(r.dir.join(name)).unwrap()).unwrap();
    assert_eq!(doc["settings"]["name"], "Test XEP");
    assert_eq!(doc["kind"], "m1xep-backup");
    // the first request of the write conversation is a read, the write comes after the backup existed
    let list = body(&handle(&r.state, "GET", "/api/m1xep/backups", b"", &Query::new()).unwrap());
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], name);
    // loading the backup gives the old settings back (and sends nothing)
    let n = r.writes_received();
    let loaded = body(&r.post("/api/m1xep/load", &json!({ "file": name })));
    assert_eq!(loaded["settings"]["name"], "Test XEP");
    assert_eq!(r.writes_received(), n);
    // putting the backup back through the same route restores the module exactly
    let old: XepSettings = serde_json::from_value(loaded["settings"].clone()).unwrap();
    let res = r.write(&old, json!({ "apply": true, "confirm": true }));
    assert_eq!(res.status, 200, "{}", res.text());
    assert_eq!(r.panel.with_xep(|x| x.blocks.clone()), before);
}

#[test]
fn identity_changes_go_through_only_with_the_warning_ticked() {
    let r = rig();
    let mut s = r.held();
    s.secure_port = 2602;
    s.non_secure_port = 0;
    let res = r.write(&s, json!({ "apply": true, "confirm": true, "allowIdentity": true }));
    assert_eq!(res.status, 200, "{}", res.text());
    assert_eq!(body(&res)["identityChanges"], true);
    assert_eq!((r.held().secure_port, r.held().non_secure_port), (2602, 0));
}

#[test]
fn invalid_values_are_refused_before_anything_is_sent() {
    let r = rig();
    for (f, text) in [
        (Box::new(|s: &mut XepSettings| s.mask = "255.0.255.0".into()) as Box<dyn Fn(&mut XepSettings)>, "Subnet Mask"),
        (Box::new(|s| s.gateway = "10.1.1.1".into()), "same subnet"),
        (Box::new(|s| s.non_secure_port = s.secure_port), "same number"),
        (Box::new(|s| s.name = "x".repeat(40)), "at most 16"),
    ] {
        let mut s = r.held();
        f(&mut s);
        let res = r.write(&s, json!({ "apply": true, "confirm": true, "allowIdentity": true }));
        assert_eq!(res.status, 400);
        assert!(res.text().contains(text), "{}", res.text());
    }
    assert_eq!(r.write(&r.held(), json!({})).status, 200);
    let res = r.post("/api/m1xep/write", &r.conn());
    assert_eq!(res.status, 400);
    assert_eq!(r.writes_received(), 0);
}

#[test]
fn nothing_to_change_sends_nothing() {
    let r = rig();
    let res = r.write(&r.held(), json!({ "apply": true, "confirm": true }));
    let v = body(&res);
    assert_eq!((res.status, &v["applied"]), (200, &json!(false)));
    assert!(v["note"].as_str().unwrap().contains("Nothing differs"));
    assert_eq!(r.writes_received(), 0);
}

#[test]
fn load_refuses_other_files() {
    let r = rig();
    assert_eq!(r.post("/api/m1xep/load", &json!({ "file": "panel-1-2-manual.json" })).status, 400);
    assert_eq!(r.post("/api/m1xep/load", &json!({ "file": "m1xep-nothing.json" })).status, 400);
    std::fs::write(r.dir.join("m1xep-broken.json"), "{not json").unwrap();
    assert_eq!(r.post("/api/m1xep/load", &json!({ "file": "..\\..\\m1xep-broken.json" })).status, 400, "path parts are dropped");
}

#[test]
fn file_route_reads_the_block_of_the_open_account() {
    let r = rig();
    let res = body(&r.post("/api/m1xep/file", &json!({})));
    assert_eq!(res["found"], false);
    assert!(res["note"].as_str().unwrap().contains("No account file"));
    let text = sample("ElkRPAcctSample Export");
    r.state.session().original_text = Some(text.clone());
    let res = body(&r.post("/api/m1xep/file", &json!({})));
    assert_eq!(res["found"], true, "{res}");
    assert_eq!(res["settings"]["name"], "");
    // an export whose M1XEP block holds the made-up settings
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.split(eol).map(str::to_string).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let tail = elk_account::AccountFile::parse(&text).unwrap().lines_after_automation().unwrap().len();
    lines.truncate(lines.len() - tail);
    lines.extend(blocks().to_lines());
    let edited = lines.join(eol) + eol;
    r.state.session().original_text = Some(edited);
    let res = body(&r.post("/api/m1xep/file", &json!({})));
    assert_eq!(res["found"], true, "{res}");
    assert_eq!(res["settings"]["name"], "Test XEP");
    assert_eq!(res["settings"]["emails"][1]["address"], "owner@example.test");
}

#[test]
fn find_validates_its_input() {
    let r = rig();
    let res = r.post("/api/m1xep/find", &json!({ "host": "not an address" }));
    assert_eq!(res.status, 400);
}
