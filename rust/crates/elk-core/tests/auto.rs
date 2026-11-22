//! Automation items on the (simulated) panel: item layout, 16-bit item numbers, batched reads, rule / text packets, and the backup of them.

use elk_core::simulated_panel::SimulatedPanel;
use elk_core::*;
use std::sync::Arc;

fn open(panel: &SimulatedPanel) -> ElkSession {
    let mut s = ElkSession::plain();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login("246801").unwrap();
    s
}

#[test]
fn item_layout_follows_the_elkrp_tables() {
    use AutoKind::*;
    assert_eq!((2, 3, 32), Task.locate(32));
    assert_eq!((2, 2, 64), Counter.locate(64));
    assert_eq!((2, 2, 65), Thermostat.locate(1));
    assert_eq!((2, 0, 12), AlarmDuration.locate(12));
    assert_eq!((2, 0, 13), CustomSetting.locate(1));
    assert_eq!((2, 1, 64), Output.locate(64));
    assert_eq!(14, Light.locate(256).0);
    assert_eq!((13, 323), (Voice.locate(115).0, Voice.locate(115).2));
    assert_eq!((209, 12), (Voice.locate(1).2, Voice.len() as u16));
    assert_eq!(None, Task.needs_firmware(1));
    assert_eq!(Some((4, 4)), Output.needs_firmware(33));
    assert_eq!(None, Output.needs_firmware(32));
    assert!(firmware_at_least("5.2.9", 4, 4) && firmware_at_least("4.4.0", 4, 4) && !firmware_at_least("4.3.9", 4, 4) && !firmware_at_least("", 4, 0));
    for k in AutoKind::ALL {
        assert_eq!(Some(k), AutoKind::from_label(k.label()));
    }
}

#[test]
fn items_round_trip_with_batched_reads_and_16_bit_numbers() {
    let panel = SimulatedPanel::new_plain("246801");
    let mut s = open(&panel);
    // light 256 crosses the 8-bit boundary in the request (item 256 = 01 00); lights are read 4 at a time
    let l255 = vec![7u8; 28];
    let l256 = vec![8u8; 28];
    AutoKind::Light.write_one(&mut s, 255, &l255).unwrap();
    AutoKind::Light.write_one(&mut s, 256, &l256).unwrap();
    let r = AutoKind::Light.read_range(&mut s, 253, 256).unwrap();
    assert_eq!(vec![vec![0u8; 28], vec![0u8; 28], l255.clone(), l256.clone()], r);
    // voice messages: 115 items, 16 per request, item numbers 209..323
    for n in [1usize, 48, 115] {
        AutoKind::Voice.write_one(&mut s, n, &[n as u8; 12]).unwrap();
    }
    let v = AutoKind::Voice.read_range(&mut s, 1, 115).unwrap();
    assert_eq!(115, v.len());
    assert_eq!(vec![115u8; 12], v[114]);
    assert_eq!(vec![48u8; 12], v[47]);
    assert_eq!(Some(vec![115u8; 12]), panel.auto_item(13, 0, 323));
    // the parts of section 2 are different records
    AutoKind::Output.write_one(&mut s, 1, &[1; 28]).unwrap();
    AutoKind::Task.write_one(&mut s, 1, &[3; 28]).unwrap();
    AutoKind::Counter.write_one(&mut s, 1, &[2; 12]).unwrap();
    AutoKind::AlarmDuration.write_one(&mut s, 1, &[4; 16]).unwrap();
    assert_eq!(vec![vec![1u8; 28]], AutoKind::Output.read_range(&mut s, 1, 1).unwrap());
    assert_eq!(vec![vec![3u8; 28]], AutoKind::Task.read_range(&mut s, 1, 1).unwrap());
    assert_eq!(vec![vec![2u8; 12]], AutoKind::Counter.read_range(&mut s, 1, 1).unwrap());
    assert_eq!(vec![vec![4u8; 16]], AutoKind::AlarmDuration.read_range(&mut s, 1, 1).unwrap());
    assert!(AutoKind::Task.write_one(&mut s, 1, &[0; 27]).is_err()); // wrong size is refused before anything is sent
}

#[test]
fn rule_and_text_packets_follow_the_end_marker_layout() {
    let panel = SimulatedPanel::new_plain("246801");
    let mut s = open(&panel);
    let rules: Vec<Packet> = (0..20u8).map(|i| [i + 2, 1, 2, 3, 4, 5, 6, 7]).collect();
    let texts: Vec<Packet> = vec![*b"Hello Wo", *b"rld\0\0\0\0\0", *b"Second\0\0"];
    // stale data between the rules and the texts must not matter
    panel.set_packets(100, &[[9; 8]; 5]);
    write_rules(&mut s, &rules).unwrap();
    write_texts(&mut s, &texts).unwrap();
    assert_eq!(rules, read_rules(&mut s).unwrap());
    assert_eq!(texts, read_texts(&mut s).unwrap());
    assert!(rules_match(&mut s, &rules).unwrap());
    assert!(texts_match(&mut s, &texts).unwrap());
    assert!(!rules_match(&mut s, &rules[..19]).unwrap()); // the 20th rule packet is not an end marker
    assert!(!texts_match(&mut s, &texts[1..]).unwrap());
    let all = panel.packets();
    assert_eq!([0; 8], all[20]); // end marker of the rules
    assert_eq!([0; 8], all[PACKET_COUNT - 4]); // end marker of the texts
    assert_eq!(texts, all[PACKET_COUNT - 3..].to_vec());
    assert_eq!(2, write_packet_requests(21));
    assert_eq!(2, read_packet_requests(0, 21));
    assert_eq!(1, read_packet_requests(PACKET_COUNT - 4, 4));
    // a full area is refused: no room for the end marker
    assert!(write_rules(&mut s, &vec![[1; 8]; PACKET_COUNT]).is_err());
    assert!(write_texts(&mut s, &vec![[1; 8]; PACKET_COUNT]).is_err());
}

#[test]
fn a_backup_with_automation_reads_restores_and_round_trips_its_json() {
    let panel = SimulatedPanel::new_plain("246801");
    for n in 1..=208u8 {
        panel.set_record(Section::Zone, n, DefaultRecords::zone(n as i32));
    }
    for n in 1..=8u8 {
        panel.set_record(Section::Area, n, DefaultRecords::area(n as i32));
    }
    for n in 1..=16u8 {
        panel.set_record(Section::Keypad, n, DefaultRecords::keypad());
    }
    for (sec, from, to) in BLOCKS {
        let len = match sec {
            1 => 28,
            8 => 88,
            7 => 48,
            9 => 20,
            10 | 12 => 4,
            11 => 24,
            _ => 96,
        };
        for n in from..=to {
            panel.set_record(sec as u8, n as u8, vec![0; len]);
        }
    }
    for part in 0..3u8 {
        panel.set_record(Section::WirelessZone, part + 1, vec![part; 260]);
    }
    panel.set_auto_item(2, 1, 5, vec![5; 28]);
    panel.set_auto_item(14, 0, 77, vec![6; 28]);
    panel.set_packets(0, &[[2, 1, 0, 0, 0, 0, 0, 0]]);
    let mut s = open(&panel);
    let pr = Arc::new(PanelProgress::new());
    pr.begin("x", PanelBackup::estimate_transfers_with_automation());
    let p2 = pr.clone();
    s.on_transfer = Some(Box::new(move || p2.tick()));
    let f = PanelBackup::read_with_automation(&mut s).unwrap();
    s.on_transfer = None;
    let snap = pr.snapshot();
    assert_eq!(snap.total, snap.done);
    assert_eq!(32 + 64 + 16 + 12 + 20 + 64 + 256 + 115 + 1, f.automation.len());
    assert!(f.automation.iter().any(|b| b.kind == "output" && b.number == 5 && b.hex == "05".repeat(28)));
    assert!(f.automation.iter().any(|b| b.kind == "packets" && b.hex.len() == 528 * 16));
    let json = f.to_json();
    assert!(json.contains("\"AutomationBlocks\""));
    let back = PanelFile::from_json(&json).unwrap();
    assert_eq!(f, back);
    // a backup without automation does not write the member (the C# format is unchanged)
    assert!(!PanelBackup::read(&mut s).unwrap().to_json().contains("AutomationBlocks"));

    // change the panel, compare, restore, compare again
    panel.set_auto_item(2, 1, 5, vec![9; 28]);
    panel.set_packets(3, &[[7; 8]]);
    assert_eq!(2, PanelBackup::update(&mut s, &back, false, None).unwrap());
    assert_eq!(2, PanelBackup::update(&mut s, &back, true, None).unwrap());
    assert_eq!(0, PanelBackup::update(&mut s, &back, false, None).unwrap());
    assert_eq!(Some(vec![5u8; 28]), panel.auto_item(2, 1, 5));
    assert_eq!([0; 8], panel.packets()[3]);
    // an unknown kind in a backup file is refused
    let mut bad = back.clone();
    bad.automation.push(AutoBlockJson { kind: "gizmo".into(), number: 1, hex: "00".into() });
    assert!(PanelBackup::update(&mut s, &bad, false, None).is_err());
}
