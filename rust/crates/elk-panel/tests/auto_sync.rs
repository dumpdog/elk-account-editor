//! Panel sync of the automation items against the simulated panel (consistent with ElkRP's code, not proof about hardware): compare, send,
//! read back, the zero-write rule for unedited items, the packet budget and the transfer estimates.

mod common;

use common::*;
use elk_account::{AccountFile, AccountModel, TextDto};
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::{AutoKind, PACKET_COUNT};
use elk_panel::auto_sync;
use elk_panel::testkit;
use elk_panel::PanelSync;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

fn file() -> AccountFile {
    let f = sample_file("ElkRPAcctSample Export");
    assert!(f.automation.is_some(), "the sample must have an automation block");
    f
}

fn seeded(f: &AccountFile) -> SimulatedPanel {
    let panel = SimulatedPanel::new(MAC, PASS, CODE);
    testkit::seed(&panel, f);
    testkit::seed_automation(&panel, f);
    panel
}

/// Write requests (opcode 128 and above) the panel has received.
fn writes(p: &SimulatedPanel) -> usize {
    p.received().iter().filter(|r| r[0] >= 128 && r[0] != 0xFF).count()
}

fn labels(r: &elk_panel::SyncResult) -> Vec<String> {
    let mut l: Vec<String> = r.changes.iter().map(|c| format!("{} {}", c.label, c.number)).collect();
    l.sort();
    l
}

#[test]
fn identical_panel_and_file_have_no_automation_changes_and_nothing_is_written() {
    let f = file();
    let panel = seeded(&f);
    let mut s = connect(&panel);
    let r = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert!(r.changes.is_empty(), "{:?}", labels(&r));
    for k in AutoKind::ALL {
        assert_eq!((k.count() as i32, 0), r.counts[k.label()], "{}", k.label());
    }
    assert_eq!((1, 0), r.counts["rule packets"]);
    assert_eq!((1, 0), r.counts["text packets"]);
    PanelSync::apply(&mut s, &r.changes, None).unwrap();
    assert_eq!(0, writes(&panel));
    // not asking for automation leaves it out entirely
    let r = PanelSync::diff(&mut s, &f, false, false).unwrap();
    assert!(!r.counts.contains_key("task"));
}

#[test]
fn edited_items_are_compared_written_and_read_back() {
    let mut f = file();
    let panel = seeded(&f);
    let mut m = AccountModel::from(&f).unwrap();
    {
        let a = m.automation.as_mut().unwrap();
        a.outputs.iter_mut().find(|o| o.n == 1).unwrap().name = "Edit Out".into();
        a.outputs.iter_mut().find(|o| o.n == 40).unwrap().name = "Edit Out 40".into();
        a.tasks.iter_mut().find(|t| t.n == 3).unwrap().name = "Edit Task".into();
        a.counters[2] = Some("Edit Ctr".into());
        a.thermostats[0] = Some("Edit Therm".into());
        a.customs.iter_mut().find(|c| c.n == 14).unwrap().value = 77;
        a.customs.iter_mut().find(|c| c.n == 2).unwrap().value = 15;
        a.lights.iter_mut().find(|l| l.n == 5).unwrap().name = "Edit Light".into();
        a.lights.iter_mut().find(|l| l.n == 256).unwrap().name = "Last Light".into();
        a.voice_messages.iter_mut().find(|v| v.n == 2).unwrap().words[0] = 9;
        a.voice_messages.iter_mut().find(|v| v.n == 115).unwrap().words[1] = 3;
        a.texts[0].text = "Happy Birthday to Mr. Smith, the best customer ever".into();
        a.texts.push(TextDto { index: 0, text: "Second".into(), packets: 0, used_by_rules: 0 });
        a.rules.remove(1);
    }
    m.apply_to(&mut f).unwrap();

    let mut s = connect(&panel);
    let d1 = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    let l = labels(&d1);
    for want in
        ["output 1", "output 40", "task 3", "counter 3", "thermostat 1", "custom setting 2", "alarm duration 2", "light 5", "light 256", "voice message 2", "voice message 115"]
    {
        assert!(l.contains(&want.to_string()), "missing {want} in {l:?}");
    }
    assert!(d1.changes.iter().any(|c| c.label == "rule packets") && d1.changes.iter().any(|c| c.label == "text packets"));
    assert_eq!(0, writes(&panel)); // comparing writes nothing

    PanelSync::apply(&mut s, &d1.changes, None).unwrap();
    assert_eq!(auto_sync::write_transfers(&d1.changes) as usize, writes(&panel));
    let d2 = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert!(d2.changes.is_empty(), "{:?}", labels(&d2));

    // the panel holds exactly the file's bytes
    let a = f.automation.as_ref().unwrap();
    assert_eq!(a.outputs[0], panel.auto_item(2, 1, 1).unwrap());
    assert_eq!(a.outputs[39], panel.auto_item(2, 1, 40).unwrap());
    assert_eq!(a.tasks[2], panel.auto_item(2, 3, 3).unwrap());
    assert_eq!(a.counters[2], panel.auto_item(2, 2, 3).unwrap());
    assert_eq!(a.counters[64], panel.auto_item(2, 2, 65).unwrap());
    assert_eq!(a.customs[13], panel.auto_item(2, 0, 14).unwrap());
    assert_eq!(a.customs[1], panel.auto_item(2, 0, 2).unwrap());
    assert_eq!(a.lights[255], panel.auto_item(14, 0, 256).unwrap());
    assert_eq!(a.voice[114], panel.auto_item(13, 0, 323).unwrap());
    assert_eq!(a.packets, panel.packets());

    // reading the panel into another copy of the file gives the same automation data (comments of moved rules are cleared by the read)
    let mut g = file();
    let notes = PanelSync::pull_with(&mut s, &mut g, None, true).unwrap();
    assert!(notes.iter().any(|n| n.starts_with("automation items:")), "{notes:?}");
    let (mut want, mut got) = (AccountModel::from(&f).unwrap().automation.unwrap(), AccountModel::from(&g).unwrap().automation.unwrap());
    for r in want.rules.iter_mut().chain(got.rules.iter_mut()) {
        r.comment.clear();
    }
    assert_eq!(want, got);
    // and a file made that way has no differences left
    let r = PanelSync::diff_with(&mut s, &g, false, false, true).unwrap();
    assert!(r.changes.is_empty(), "{:?}", labels(&r));
}

#[test]
fn only_what_changed_is_written() {
    let mut f = file();
    let panel = seeded(&f);
    let mut m = AccountModel::from(&f).unwrap();
    m.automation.as_mut().unwrap().tasks.iter_mut().find(|t| t.n == 7).unwrap().name = "Only This".into();
    m.apply_to(&mut f).unwrap();
    let mut s = connect(&panel);
    let d = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert_eq!(vec!["task 7".to_string()], labels(&d));
    PanelSync::apply(&mut s, &d.changes, None).unwrap();
    assert_eq!(1, writes(&panel));
    let w = panel.received().into_iter().find(|r| r[0] >= 128).unwrap();
    assert_eq!(vec![130, 3, 0, 7], w[..4].to_vec()); // section 2 write, part 3 (tasks), item 7
    assert_eq!(32, w.len());
}

#[test]
fn an_eight_character_last_text_is_compared_correctly() {
    // a text of 8 characters ends in an all-zero packet, which ElkRP's own scan cannot tell from the end marker; compare knows the count
    let mut f = file();
    let panel = seeded(&f);
    let mut m = AccountModel::from(&f).unwrap();
    m.automation.as_mut().unwrap().texts.push(TextDto { index: 0, text: "ABCDEFGH".into(), packets: 0, used_by_rules: 0 });
    m.apply_to(&mut f).unwrap();
    let mut s = connect(&panel);
    let d = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert_eq!(vec!["text packets 6".to_string()], labels(&d).into_iter().filter(|l| l.starts_with("text")).collect::<Vec<_>>());
    PanelSync::apply(&mut s, &d.changes, None).unwrap();
    let d = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert!(d.changes.is_empty());
}

#[test]
fn an_inconsistent_rule_text_budget_is_refused_before_anything_is_read_or_written() {
    let mut f = file();
    let panel = seeded(&f);
    {
        let a = f.automation.as_mut().unwrap();
        a.rule_packets_used = 300;
        a.text_packets_used = 228; // 528: no packet left for the end marker
    }
    let mut s = connect(&panel);
    let e = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap_err();
    assert!(e.message().contains("end marker"), "{}", e.message());
    assert_eq!(0, writes(&panel));

    // a rule packet that starts with two zero bytes would be read by the panel as the end of the rules
    let mut f = file();
    f.automation.as_mut().unwrap().packets[3] = [0; 8];
    let e = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap_err();
    assert!(e.message().contains("two zero bytes"), "{}", e.message());
    assert_eq!(0, writes(&panel));
}

#[test]
fn a_file_without_automation_is_skipped_and_an_unknown_change_is_refused() {
    let mut f = file();
    f.automation = None;
    let panel = seeded(&f);
    let mut s = connect(&panel);
    let d = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    assert!(d.skipped.iter().any(|x| x.starts_with("automation items")));
    let bad = elk_panel::PanelChange { label: "task".into(), number: 99, section: 2, data: vec![0; 28], is_keypad: false, part: 3 };
    assert!(PanelSync::apply(&mut s, &[bad], None).is_err());
    let bad = elk_panel::PanelChange { label: "task".into(), number: 1, section: 2, data: vec![0; 27], is_keypad: false, part: 3 };
    assert!(PanelSync::apply(&mut s, &[bad], None).is_err());
    assert_eq!(0, writes(&panel));
}

#[test]
fn the_transfer_estimates_match_the_transfers_made() {
    let mut f = file();
    let panel = seeded(&f);
    let mut m = AccountModel::from(&f).unwrap();
    m.automation.as_mut().unwrap().texts[0].text = "Changed text that needs more than one packet of eight bytes".into();
    m.automation.as_mut().unwrap().lights.iter_mut().find(|l| l.n == 9).unwrap().name = "L9".into();
    m.apply_to(&mut f).unwrap();
    let mut s = connect(&panel);
    let n = Arc::new(AtomicI32::new(0));
    let n2 = n.clone();
    s.on_transfer = Some(Box::new(move || {
        n2.fetch_add(1, Ordering::SeqCst);
    }));
    let d = PanelSync::diff_with(&mut s, &f, false, false, true).unwrap();
    let fw = s.panel.as_ref().unwrap().firmware.clone();
    assert_eq!(PanelSync::estimate_diff_transfers(&f, false) + auto_sync::estimate_diff(&f, &fw), n.load(Ordering::SeqCst));
    n.store(0, Ordering::SeqCst);
    PanelSync::apply(&mut s, &d.changes, None).unwrap();
    assert_eq!(PanelSync::write_transfers(&d.changes), n.load(Ordering::SeqCst));
    // the pull estimate is a lower bound for a small account (the rule / text scan reads a couple of requests each)
    n.store(0, Ordering::SeqCst);
    let mut g = file();
    PanelSync::pull_with(&mut s, &mut g, None, true).unwrap();
    assert!(n.load(Ordering::SeqCst) >= auto_sync::estimate_pull(&fw) - 4, "{} vs {}", n.load(Ordering::SeqCst), auto_sync::estimate_pull(&fw));
}

#[test]
fn pull_keeps_rule_comments_only_for_unchanged_packets() {
    let f = file();
    let panel = seeded(&f);
    // the panel differs in the 3rd packet only
    let mut pk = panel.packets();
    pk[2][3] ^= 1;
    panel.set_packets(0, &pk);
    let mut g = file();
    g.automation.as_mut().unwrap().comments[0] = "first".into();
    g.automation.as_mut().unwrap().comments[2] = "third".into();
    let mut s = connect(&panel);
    PanelSync::pull_with(&mut s, &mut g, None, true).unwrap();
    let a = g.automation.as_ref().unwrap();
    assert_eq!("first", a.comments[0]);
    assert_eq!("", a.comments[2]);
    assert_eq!(f.automation.as_ref().unwrap().rule_packets_used, a.rule_packets_used);
    assert!(a.rule_packets_used + a.text_packets_used < PACKET_COUNT);
}
