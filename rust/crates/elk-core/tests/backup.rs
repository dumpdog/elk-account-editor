//! PanelBackup and PanelProgress against the simulated panel (the C# has these exercised through PanelSyncTests, which is not in this crate's scope).

use elk_core::simulated_panel::SimulatedPanel;
use elk_core::*;
use std::sync::Arc;

fn populated() -> SimulatedPanel {
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
        for n in from..=to {
            let data = match sec {
                1 => DefaultRecords::user(n),
                8 => DefaultRecords::phone(),
                9 => DefaultRecords::area_rc(),
                10 => DefaultRecords::user_rc(),
                12 => DefaultRecords::zone_rc(),
                _ => vec![sec as u8, n as u8, 0, 9],
            };
            panel.set_record(sec as u8, n as u8, data);
        }
    }
    for part in 0..3u8 {
        panel.set_record(Section::WirelessZone, part + 1, vec![part; 5]);
    }
    panel
}

fn open(panel: &SimulatedPanel) -> ElkSession {
    let mut s = ElkSession::plain();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login("246801").unwrap();
    s
}

#[test]
fn full_backup_reads_every_record_and_counts_its_transfers() {
    let panel = populated();
    let mut s = open(&panel);
    let pr = Arc::new(PanelProgress::new());
    pr.begin("Reading the panel", PanelBackup::estimate_transfers());
    let p2 = pr.clone();
    s.on_transfer = Some(Box::new(move || p2.tick()));
    let f = PanelBackup::read(&mut s).unwrap();
    assert_eq!(208, f.zones.len());
    assert_eq!(8, f.areas.len());
    assert_eq!(16, f.keypads.len());
    assert_eq!(BLOCKS.iter().map(|b| (b.2 - b.1 + 1) as usize).sum::<usize>() + 3, f.blocks.len());
    assert_eq!("5.2.9", f.firmware.as_deref().unwrap());
    assert_eq!("31415926", f.serial.as_deref().unwrap());
    assert_eq!("Zone 001", f.zones[0].name);
    assert_eq!("Area 3", f.areas[2].name);
    let snap = pr.snapshot();
    assert_eq!(PanelBackup::estimate_transfers(), snap.done);
    assert_eq!(snap.done, snap.total); // the estimate was exact
    assert_eq!(f.blocks.last().unwrap().part, 2);
    assert_eq!("wireless receiver", PanelBackup::block_label(15));
    assert_eq!("section 99", PanelBackup::block_label(99));
}

#[test]
fn update_compares_by_crc_and_bytes_and_writes_only_what_differs() {
    let panel = populated();
    let mut s = open(&panel);
    let mut f = PanelBackup::read(&mut s).unwrap();
    assert_eq!(0, PanelBackup::update(&mut s, &f, false, None).unwrap());
    assert_eq!(PanelBackup::estimate_transfers_for(&f) as usize, f.zones.len() + f.areas.len() + 2 * f.keypads.len() + f.blocks.len());

    f.zones[4].name = "Renamed".into();
    f.areas[1].exit1 = 99;
    f.keypads[2].name = "NEWPAD".into();
    f.blocks[10].hex = hex_upper(&DefaultRecords::user(77));
    let mut log: Vec<String> = Vec::new();
    {
        let mut cb = |m: &str| log.push(m.to_string());
        assert_eq!(4, PanelBackup::update(&mut s, &f, false, Some(&mut cb)).unwrap());
    }
    assert_eq!(vec!["zone 5: 'Renamed' differs from panel", "area 2: 'Area 2' differs from panel", "keypad 3: 'NEWPAD' differs from panel", "user 11 differs from panel",], log);
    assert_eq!(DefaultRecords::zone(5), panel.record(Section::Zone, 5).unwrap()); // compare only: nothing written
    assert_eq!(4, PanelBackup::update(&mut s, &f, true, None).unwrap());
    assert_eq!("Renamed", s.read_zone(5).unwrap().name());
    assert_eq!(99, s.read_area(2).unwrap().exit1());
    assert_eq!("NEWPAD", s.read_keypad(3).unwrap().name());
    assert_eq!(DefaultRecords::user(77), panel.record(1u8, 11).unwrap());
    assert_eq!(0, PanelBackup::update(&mut s, &f, false, None).unwrap());
}

#[test]
fn backup_file_with_a_bad_hex_is_an_error() {
    let panel = populated();
    let mut s = open(&panel);
    let mut f = PanelBackup::read(&mut s).unwrap();
    f.zones[0].hex = "XYZ".into();
    assert!(PanelBackup::update(&mut s, &f, false, None).is_err());
    f.zones[0].hex = "00".repeat(33);
    assert!(PanelBackup::update(&mut s, &f, false, None).is_err());
}

#[test]
fn progress_counts_phases_and_grows_its_total() {
    let pr = PanelProgress::new();
    pr.tick();
    assert_eq!(0, pr.snapshot().done); // before begin: ignored
    assert!(!pr.snapshot().active);
    pr.begin("Reading", 2);
    pr.note("hello");
    pr.tick();
    pr.tick();
    pr.tick();
    let s = pr.snapshot();
    assert_eq!(("Reading", "hello", 3, 3, true), (s.phase.as_str(), s.note.as_str(), s.done, s.total, s.active));
    pr.next("Writing", 0);
    pr.tick();
    pr.add_total(5);
    let s = pr.snapshot();
    assert_eq!(("Writing", "", 1, 5), (s.phase.as_str(), s.note.as_str(), s.done, s.total));
    pr.end();
    let s = pr.snapshot();
    assert!(!s.active);
    assert_eq!(0.0, s.seconds);
}
