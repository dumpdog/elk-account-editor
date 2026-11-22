//! The panel routes with `"automation": true` against the simulated panel: compare answers the usual counts / changes / skipped shape and writes nothing,
//! send writes a backup (with the automation items in it) before the first write, and without the flag the answers have no automation rows.

mod common;

use common::*;
use elk_account::{AccountFile, AccountModel};
use elk_core::simulated_panel::SimulatedPanel;
use elk_panel::{handle, testkit, Query, Shared};
use serde_json::{json, Value};
use std::time::Duration;

fn writes(p: &SimulatedPanel) -> usize {
    p.received().iter().filter(|r| r[0] >= 128 && r[0] != 0xFF).count()
}

fn setup() -> (Shared, SimulatedPanel, AccountFile, std::path::PathBuf) {
    let text = sample("ElkRPAcctSample Export");
    let f = AccountFile::parse(&text).unwrap();
    let panel = SimulatedPanel::new_plain(CODE);
    testkit::seed(&panel, &f);
    testkit::seed_automation(&panel, &f);
    let dir = temp_dir("autoroutes");
    let mut shared = Shared::with_backup_dir(dir.join("b"));
    shared.live.retry_delay = Duration::from_millis(10);
    {
        let mut s = shared.session();
        s.original_text = Some(text);
        s.source_name = Some("sample".into());
    }
    (shared, panel, f, dir)
}

fn request(panel: &SimulatedPanel, model: &AccountModel, automation: Option<bool>) -> Vec<u8> {
    let mut v = json!({ "host": "127.0.0.1", "port": panel.port(), "code": CODE, "plain": true, "model": serde_json::from_str::<Value>(&model.to_json()).unwrap() });
    if let Some(a) = automation {
        v["automation"] = json!(a);
    }
    serde_json::to_vec(&v).unwrap()
}

fn call(shared: &Shared, path: &str, body: &[u8]) -> Value {
    let r = handle(shared, "POST", path, body, &Query::new()).unwrap();
    assert_eq!(200, r.status, "{}", r.text());
    serde_json::from_str(&r.text()).unwrap()
}

#[test]
fn compare_and_send_with_automation_follow_the_usual_shape() {
    let (shared, panel, f, dir) = setup();
    let mut m = AccountModel::from(&f).unwrap();
    m.automation.as_mut().unwrap().outputs.iter_mut().find(|o| o.n == 2).unwrap().name = "Route Edit".into();

    // without the flag: no automation rows at all, nothing to send
    let r = call(&shared, "/api/panel/compare", &request(&panel, &m, None));
    assert!(!r["counts"].as_array().unwrap().iter().any(|c| c["label"] == "output"));
    assert_eq!(0, r["changes"].as_array().unwrap().len());

    // with the flag: one output differs, nothing is written by comparing
    let r = call(&shared, "/api/panel/compare", &request(&panel, &m, Some(true)));
    assert_eq!(0, r["written"]);
    assert_eq!(vec![json!({"label": "output", "number": 2})], r["changes"].as_array().unwrap().clone());
    let out = r["counts"].as_array().unwrap().iter().find(|c| c["label"] == "output").unwrap().clone();
    assert_eq!((63, 1), (out["same"].as_i64().unwrap(), out["differ"].as_i64().unwrap()));
    assert!(r["skipped"].as_array().unwrap().iter().any(|s| s == "user 201") || r["skipped"].is_array());
    assert_eq!(0, writes(&panel));

    // send: a backup first (it has the automation items), one write, read back clean
    let r = call(&shared, "/api/panel/send", &request(&panel, &m, Some(true)));
    assert_eq!(1, r["written"]);
    assert_eq!(0, r["changes"].as_array().unwrap().len());
    let backup = std::fs::read_to_string(r["backupPath"].as_str().unwrap()).unwrap();
    assert!(backup.contains("\"AutomationBlocks\""));
    assert_eq!(1, writes(&panel));
    assert_eq!(m.automation.as_ref().unwrap().outputs[1].name, "Route Edit");
    let have = panel.auto_item(2, 1, 2).unwrap();
    assert_eq!(b"Route Edit", &have[..10].iter().map(|b| b & 0x7F).collect::<Vec<u8>>()[..]);

    // sending the same thing again writes nothing and makes no backup
    let r = call(&shared, "/api/panel/send", &request(&panel, &m, Some(true)));
    assert_eq!(0, r["written"]);
    assert!(r["backupPath"].is_null());
    assert_eq!(1, writes(&panel));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pull_with_automation_reads_the_items_into_the_model() {
    let (shared, panel, f, dir) = setup();
    panel.set_auto_item(2, 3, 9, {
        let mut t = vec![0u8; 28];
        t[0] = 0x80 | b'P';
        t[1..7].copy_from_slice(b"ulled\0");
        t
    });
    let m = AccountModel::from(&f).unwrap();
    let r = call(&shared, "/api/panel/pull", &request(&panel, &m, Some(true)));
    let notes: Vec<String> = r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert!(notes.iter().any(|n| n.starts_with("automation items:")), "{notes:?}");
    let tasks = r["model"]["automation"]["tasks"].as_array().unwrap();
    let t9 = tasks.iter().find(|t| t["n"] == 9).unwrap();
    assert_eq!("Pulled", t9["name"]);
    // without the flag the automation items are left as they were in the file
    let r = call(&shared, "/api/panel/pull", &request(&panel, &m, None));
    let notes: Vec<String> = r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert!(!notes.iter().any(|n| n.starts_with("automation")));
    let _ = std::fs::remove_dir_all(dir);
}

/// The rule packets on the simulated panel up to the end marker.
fn rules_on(panel: &SimulatedPanel) -> Vec<[u8; 8]> {
    panel.packets().into_iter().take_while(|p| p[0] != 0).collect()
}

#[test]
fn disabled_rules_are_not_sent_and_come_back_when_enabled() {
    let (shared, panel, f, dir) = setup();
    let all = rules_on(&panel);
    let mut m = AccountModel::from(&f).unwrap();
    let a = m.automation.as_mut().unwrap();
    assert!(a.rules.len() >= 3);
    let gone = a.rules[1].packets.len();
    a.rules[1].disabled = true;

    // compare says the rules would change and names the disabled rule; nothing is written
    let r = call(&shared, "/api/panel/compare", &request(&panel, &m, Some(true)));
    assert_eq!(0, writes(&panel));
    assert_eq!(vec![json!({"label": "rule packets", "number": (all.len() - gone) as i64})], r["changes"].as_array().unwrap().clone());
    assert!(r["skipped"].as_array().unwrap().iter().any(|s| s.as_str().unwrap().starts_with("1 disabled rule(s) are not sent")), "{r}");

    // send: the panel now holds the enabled rules only, in order, followed by the end marker; the texts are untouched
    let texts_before: Vec<[u8; 8]> = panel.packets()[528 - f.automation.as_ref().unwrap().text_packets_used..].to_vec();
    let r = call(&shared, "/api/panel/send", &request(&panel, &m, Some(true)));
    assert_eq!(1, r["written"]);
    assert_eq!(0, r["changes"].as_array().unwrap().len());
    let now = rules_on(&panel);
    assert_eq!(all.len() - gone, now.len());
    let first = m.automation.as_ref().unwrap().rules[0].packets.len();
    assert_eq!(all[..first], now[..first]);
    assert_eq!(all[first + gone..], now[first..]);
    assert_eq!(texts_before[..], panel.packets()[528 - texts_before.len()..]);

    // sending again writes nothing
    let w = writes(&panel);
    let r = call(&shared, "/api/panel/send", &request(&panel, &m, Some(true)));
    assert_eq!(0, r["written"]);
    assert_eq!(w, writes(&panel));

    // enabled again: the rule is written back
    m.automation.as_mut().unwrap().rules[1].disabled = false;
    let r = call(&shared, "/api/panel/send", &request(&panel, &m, Some(true)));
    assert_eq!(1, r["written"]);
    assert_eq!(all, rules_on(&panel));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn receiving_is_refused_while_a_rule_is_disabled() {
    let (shared, panel, f, dir) = setup();
    let m = AccountModel::from(&f).unwrap();
    let mut body: Value = serde_json::from_slice(&request(&panel, &m, Some(true))).unwrap();
    body["disabledRules"] = json!(2);
    let r = handle(&shared, "POST", "/api/panel/pull", &serde_json::to_vec(&body).unwrap(), &Query::new()).unwrap();
    assert_eq!(400, r.status);
    assert_eq!("\"Rules cannot be received while some are disabled. To receive Rules, you must first enable all of them.\"", r.text());
    // without automation (rules are not received) or with none disabled the pull goes ahead
    body["disabledRules"] = json!(0);
    let r = handle(&shared, "POST", "/api/panel/pull", &serde_json::to_vec(&body).unwrap(), &Query::new()).unwrap();
    assert_eq!(200, r.status, "{}", r.text());
    let _ = std::fs::remove_dir_all(dir);
}
