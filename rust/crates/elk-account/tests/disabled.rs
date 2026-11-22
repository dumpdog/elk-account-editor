//! Editor-side rule Disable flag (ADR-014): never written to the export file, and a send leaves disabled rule groups out.

use std::fs;
use std::path::PathBuf;

use elk_account::rules::{disabled_indexes, drop_disabled, group};
use elk_account::*;

fn sample_text() -> String {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sample/ElkRPAcctSample Export");
    decode_latin1(&fs::read(p).unwrap())
}

#[test]
fn flag_is_not_in_the_file_and_not_in_unflagged_json() {
    let text = sample_text();
    let f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    assert!(!m.to_json().contains("\"disabled\""));
    let rules = &mut m.automation.as_mut().unwrap().rules;
    assert!(rules.len() >= 2);
    rules[1].disabled = true;
    assert!(m.to_json().contains("\"disabled\":true"));
    // saving a model with disabled rules gives the same bytes as the original (the file cannot hold the flag)
    let mut g = AccountFile::parse(&text).unwrap();
    m.apply_to(&mut g).unwrap();
    assert_eq!(g.to_text(), text);
    // the flag survives a JSON round trip of the model (what Undo and the draft do)
    let again: AccountModel = serde_json::from_str(&m.to_json()).unwrap();
    assert!(again.automation.unwrap().rules[1].disabled);
}

#[test]
fn dropping_a_group_compacts_the_area_and_keeps_the_texts() {
    let f = AccountFile::parse(&sample_text()).unwrap();
    let a = f.automation.clone().unwrap();
    let groups = group(&a);
    assert!(groups.len() >= 3, "sample has {} rules", groups.len());
    let mut b = a.clone();
    let removed = drop_disabled(&mut b, &[1usize].into_iter().collect());
    assert_eq!(removed, 1);
    assert_eq!(b.rule_packets_used, a.rule_packets_used - groups[1].1);
    let g2 = group(&b);
    assert_eq!(g2.len(), groups.len() - 1);
    // the first rule is unchanged, the third now follows it
    let (s0, c0) = groups[0];
    assert_eq!(b.packets[s0..s0 + c0], a.packets[s0..s0 + c0]);
    let (s2, c2) = groups[2];
    assert_eq!(b.packets[c0..c0 + c2], a.packets[s2..s2 + c2]);
    assert_eq!(b.comments[1], a.comments[2]);
    // end marker: the packet after the last rule is zero, texts untouched
    assert_eq!(b.packets[b.rule_packets_used][0], 0);
    let t = 528 - a.text_packets_used;
    assert_eq!(b.packets[t..], a.packets[t..]);
    assert_eq!(b.text_packets_used, a.text_packets_used);
    // nothing to drop: unchanged
    let mut c = a.clone();
    assert_eq!(drop_disabled(&mut c, &Default::default()), 0);
    assert_eq!(c, a);
}

#[test]
fn indexes_come_from_the_flags() {
    let f = AccountFile::parse(&sample_text()).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let rules = &mut m.automation.as_mut().unwrap().rules;
    rules[0].disabled = true;
    rules[2].disabled = true;
    let d = disabled_indexes(rules);
    assert_eq!(d.len(), 2);
    assert!(d.contains(&0) && d.contains(&2));
}
