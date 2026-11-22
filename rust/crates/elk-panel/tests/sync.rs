//! Ports of PanelSyncTests.cs and PanelResetTests: diff, apply and pull against the in-process simulated panel (consistent with ElkRP's code, not proof
//! about hardware), plus the progress counters.

mod common;

use common::*;
use elk_account::{AccountFile, AccountModel};
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::{DefaultRecords, PanelProgress, ZoneRecord};
use elk_panel::testkit;
use elk_panel::{PanelChange, PanelSync};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

fn communicator() -> AccountFile {
    sample_file("Sample Communicator")
}

#[test]
fn diff_of_identical_panel_and_file_has_no_changes() {
    let f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    let mut s = connect(&panel);
    let r = PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert!(r.changes.is_empty());
    assert!(r.counts.contains_key("zone"));
    assert_eq!(0, r.counts["zone"].1);
    assert!(r.counts["zone"].0 > 0);
    assert!(r.skipped.contains(&"user 201".to_string())); // special users have no panel item
}

#[test]
fn diff_apply_diff_writes_only_what_changed_and_converges() {
    let mut f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    // edit the file in memory: a zone name, the phone number, a zone report row, an area name and the keypad name
    let mut m = AccountModel::from(&f).unwrap();
    m.zones[0].name = "Edited Zone".into();
    m.phones[0].number = "5550000".into();
    m.comm.as_mut().unwrap().zones[0].codes[0] = 0;
    m.areas[0].name = "Edited Area".into();
    m.keypads[0].name = "Edited Pad".into();
    m.apply_to(&mut f).unwrap();

    let mut s = connect(&panel);
    let d1 = PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert_eq!(5, d1.changes.len());
    let mut labels: Vec<&str> = d1.changes.iter().map(|c| c.label.as_str()).collect();
    labels.sort();
    assert_eq!(vec!["area", "keypad", "telephone", "zone", "zone report codes"], labels);

    PanelSync::apply(&mut s, &d1.changes, None).unwrap();
    let d2 = PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert!(d2.changes.is_empty()); // read back: panel now equals the file
    let mut z = ZoneRecord::new();
    z.bytes.copy_from_slice(&f.zones.entries[0].bytes);
    assert_eq!("Edited Zone", z.name());
    assert_eq!(f.zones.entries[0].bytes, panel.record(5u8, f.zones.entries[0].number as u8).unwrap());
    assert_eq!(f.phones.entries[0].bytes, panel.record(8u8, 1).unwrap());
    assert_eq!(f.keypads.entries[0].bytes, panel.record(4u8, 1).unwrap());
}

#[test]
fn pull_overwrites_file_records_and_adds_only_used_ones() {
    let mut f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    testkit::add_pull_extras(&panel);

    let mut s = connect(&panel);
    let notes = PanelSync::pull(&mut s, &mut f, None).unwrap();
    assert!(!notes.is_empty());

    let model = AccountModel::from(&f).unwrap();
    assert_eq!("Panel Name", model.zones.iter().find(|z| z.n == 3).unwrap().name);
    assert!(f.zones.get(150).is_some());
    assert!(f.zones.get(151).is_none());
    assert!(f.users.get(50).is_some());
    assert!(f.users.get(51).is_none());
    assert_eq!("PULLED", model.users.iter().find(|x| x.n == 50).unwrap().name);
    assert!(f.zone_rcs.get(150).is_some()); // pulled zones get report-code rows
                                            // the result is a valid file again, and a diff against the same panel is clean
    let f2 = AccountFile::parse(&f.to_text()).unwrap();
    let d = PanelSync::diff(&mut s, &f2, false, false).unwrap();
    assert!(d.changes.is_empty());
}

#[test]
fn progress_estimates_match_the_transfers_actually_made() {
    let f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    let mut s = connect(&panel);
    let n = Arc::new(AtomicI32::new(0));
    let n2 = n.clone();
    s.on_transfer = Some(Box::new(move || {
        n2.fetch_add(1, Ordering::SeqCst);
    }));
    PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert_eq!(PanelSync::estimate_diff_transfers(&f, false), n.load(Ordering::SeqCst));
    n.store(0, Ordering::SeqCst);
    PanelSync::diff(&mut s, &f, true, false).unwrap();
    assert_eq!(PanelSync::estimate_diff_transfers(&f, true), n.load(Ordering::SeqCst));
    // a write is one transfer (two for a keypad)
    n.store(0, Ordering::SeqCst);
    let changes = vec![
        PanelChange { label: "zone".into(), number: 1, section: 5, data: vec![0; 32], is_keypad: false, part: 0 },
        PanelChange { label: "keypad".into(), number: 1, section: 4, data: vec![0; 148], is_keypad: true, part: 0 },
    ];
    PanelSync::apply(&mut s, &changes, None).unwrap();
    assert_eq!(PanelSync::write_transfers(&changes), n.load(Ordering::SeqCst));
    assert_eq!(3, n.load(Ordering::SeqCst));
}

#[test]
fn pull_progress_reaches_exactly_its_total() {
    let mut f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    let mut s = connect(&panel);
    let pr = Arc::new(PanelProgress::new());
    pr.begin("Reading the panel", PanelSync::estimate_pull_transfers(&f));
    let p2 = pr.clone();
    s.on_transfer = Some(Box::new(move || p2.tick()));
    PanelSync::pull(&mut s, &mut f, Some(&pr)).unwrap();
    let snap = pr.snapshot();
    assert!(snap.active);
    assert_eq!(snap.total, snap.done);
    assert!(snap.done > 400);
    pr.end();
    pr.tick();
    assert_eq!(snap.done, pr.snapshot().done); // nothing counts once the operation has ended
}

#[test]
fn progress_total_grows_if_more_happens_than_expected() {
    let pr = PanelProgress::new();
    pr.tick();
    assert_eq!(0, pr.snapshot().done); // before begin: ignored
    pr.begin("x", 2);
    pr.tick();
    pr.tick();
    pr.tick();
    let s = pr.snapshot();
    assert_eq!((3, 3), (s.done, s.total));
    pr.next("y", 0);
    pr.tick();
    let s = pr.snapshot();
    assert_eq!(("y", 1, 0), (s.phase.as_str(), s.done, s.total)); // 0 = unknown total stays unknown
}

#[test]
fn reset_missing_only_touches_records_that_differ_from_default_and_are_not_in_the_file() {
    let f = AccountFile::parse(&template()).unwrap(); // 48 zones, 4 users, 1 area, 1 keypad
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed_defaults(&panel);
    testkit::add_reset_leftovers(&panel);

    let mut s = connect(&panel);
    let plain = PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert!(!plain.changes.iter().any(|c| c.label.ends_with("(reset)"))); // off by default

    let d = PanelSync::diff(&mut s, &f, true, false).unwrap();
    let mut reset: Vec<String> = d.changes.iter().filter(|c| c.label.ends_with("(reset)")).map(|c| format!("{} {}", c.label, c.number)).collect();
    reset.sort();
    assert_eq!(vec!["area (reset) 3", "telephone (reset) 2", "user (reset) 50", "zone (reset) 150"], reset);

    let only_reset: Vec<PanelChange> = d.changes.iter().filter(|c| c.label.ends_with("(reset)")).cloned().collect();
    PanelSync::apply(&mut s, &only_reset, None).unwrap();
    let after = PanelSync::diff(&mut s, &f, true, false).unwrap();
    assert!(!after.changes.iter().any(|c| c.label.ends_with("(reset)"))); // panel records outside the file are default again
    assert_eq!(DefaultRecords::zone(150), panel.record(5u8, 150).unwrap());
}

// ---- extras beyond the C# tests -------------------------------------------------------------------------------------------------------------------------

#[test]
fn local_code_is_only_sent_on_request_and_globals_are_compared() {
    let mut f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    let mut m = AccountModel::from(&f).unwrap();
    let g = m.globals.as_mut().unwrap();
    g[0] ^= 1;
    g[17] ^= 1; // the local programming code
    m.apply_to(&mut f).unwrap();
    let mut s = connect(&panel);
    let d = PanelSync::diff(&mut s, &f, false, false).unwrap();
    let c = d.changes.iter().find(|c| c.label == "globals").unwrap();
    let have = panel.record(7u8, 1).unwrap();
    assert_ne!(have[0], c.data[0]);
    assert_eq!(have[17], c.data[17]); // kept as the panel has it
    let d = PanelSync::diff(&mut s, &f, false, true).unwrap();
    let c = d.changes.iter().find(|c| c.label == "globals").unwrap();
    assert_ne!(have[17], c.data[17]);
    assert_eq!((0, 1), d.counts["globals"]);
}

#[test]
fn counts_are_ordered_by_label() {
    let f = communicator();
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, &f);
    let mut s = connect(&panel);
    let d = PanelSync::diff(&mut s, &f, true, false).unwrap();
    let keys: Vec<&String> = d.counts.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(sorted, keys);
    assert!(keys.iter().any(|k| k.as_str() == "zone (reset)"));
}
