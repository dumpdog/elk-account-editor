//! Ports of SessionTests.cs, LiveTests.cs, ModulesTests.cs and ClockLogTests.cs against the simulated panel.

use chrono::{NaiveDate, NaiveDateTime};
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::*;
use std::time::Duration;

const MAC: &str = "0A1B2C3D4E5F";
const PASS: &str = "PROBEPASSPHRASE0123456789ABCDEFG";

fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap()
}

fn start(split: bool) -> (SimulatedPanel, ElkSession) {
    let panel = SimulatedPanel::new(MAC, PASS, "246801");
    panel.set_split_writes(split);
    let mut z = ZoneRecord::new();
    z.set_function(2);
    z.set_flag1(17);
    z.set_name("ZONE-ONE-CHANGED");
    z.bytes[20] = 1;
    z.bytes[21] = 215;
    z.bytes[23] = 22; // voice words as stored for sample zone 1
    panel.set_record(Section::Zone, 1, z.bytes.to_vec());
    let mut a = AreaRecord::new();
    a.set_name("Alpha2");
    a.set_exit1(71);
    a.set_entrance1(41);
    a.set_exit2(82);
    a.set_entrance2(23);
    a.set_flags1(54);
    a.set_flags2(168);
    a.set_arm_time_window(15);
    panel.set_record(Section::Area, 2, a.bytes.to_vec());
    (panel, ElkSession::new(MAC, PASS).unwrap())
}

fn roundtrip(split: bool) {
    let (panel, mut s) = start(split);
    s.connect("127.0.0.1", panel.port()).unwrap();
    let info = s.login("246801").unwrap();
    assert_eq!("5.2.9", info.firmware);

    let mut zone = s.read_zone(1).unwrap();
    assert_eq!("ZONE-ONE-CHANGED", zone.name());
    assert_eq!(3116, zone.crc());

    let area = s.read_area(2).unwrap();
    assert_eq!("Alpha2", area.name());
    assert_eq!(33085, area.crc());

    zone.set_name("RENAMED");
    zone.set_flag1(ZoneFlag1::CHIME.0);
    s.write_record(Section::Zone, 1, &zone.bytes, 0).unwrap();
    assert_eq!("RENAMED", s.read_zone(1).unwrap().name());
    assert_eq!(zone.crc(), s.query_crc(Section::Zone, 1).unwrap());

    s.keep_alive().unwrap();
    s.disconnect().unwrap();
}

#[test]
fn login_read_write_crc_roundtrip() {
    roundtrip(false);
}

#[test]
fn login_read_write_crc_roundtrip_split_writes() {
    roundtrip(true);
}

#[test]
fn plain_session_roundtrip() {
    let panel = SimulatedPanel::new_plain("246801");
    let mut z = ZoneRecord::new();
    z.set_name("PLAIN");
    panel.set_record(Section::Zone, 3, z.bytes.to_vec());
    let mut s = ElkSession::plain();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login("246801").unwrap();
    assert_eq!("PLAIN", s.read_zone(3).unwrap().name());
}

#[test]
fn wrong_access_code_is_rejected() {
    let (panel, mut s) = start(false);
    s.connect("127.0.0.1", panel.port()).unwrap();
    assert!(matches!(s.login("000000"), Err(ElkError::Unauthorized(_))));
}

#[test]
fn wrong_passphrase_never_gets_a_reply() {
    let panel = SimulatedPanel::new(MAC, PASS, "246801");
    let mut s = ElkSession::new(MAC, "different passphrase").unwrap();
    s.timeout = Duration::from_millis(400);
    s.connect("127.0.0.1", panel.port()).unwrap();
    assert!(s.login("246801").is_err());
}

#[test]
fn not_connected_session_errors() {
    let mut s = ElkSession::plain();
    assert!(matches!(s.send(&[1]), Err(ElkError::InvalidOperation(_))));
    assert!(ElkSession::tls("123").is_err());
    assert!(ElkSession::tls("0001ZZ31").is_err());
    assert!(ElkSession::tls("00000001").is_ok());
}

#[test]
fn keypad_two_part_read_and_write() {
    let panel = SimulatedPanel::new("0A1B2C3D4E5F", "pw", "246801");
    let mut k = KeypadRecord::new();
    k.set_area(8);
    k.set_name("KEYPAD-LAST-PRB");
    k.bytes[52] = b'F';
    panel.set_record(Section::Keypad, 16, k.bytes.to_vec());
    let mut s = ElkSession::new("0A1B2C3D4E5F", "pw").unwrap();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login("246801").unwrap();
    let mut got = s.read_keypad(16).unwrap();
    assert_eq!(k.bytes, got.bytes);
    got.set_name("CHANGED");
    got.bytes[147] = 0x5A;
    s.write_keypad(16, &got).unwrap();
    let again = s.read_keypad(16).unwrap();
    assert_eq!("CHANGED", again.name());
    assert_eq!(0x5A, again.bytes[147]);
    assert_eq!(got.crc1(), again.crc1());
    assert_eq!(got.crc2(), again.crc2());
}

// ---- LiveTests.cs ----

const LMAC: &str = "00:40:9D:00:00:04";
const LPASS: &str = "testphrase";
const CODE: &str = "246801";

#[test]
fn decodes_the_replies_seen_on_the_real_panel() {
    // area reply captured from the spare panel: area 1 not ready, areas 2-8 ready (0x04), bytes 4-7 = 00 10 00 00
    let mut a = [0u8; 68];
    a[0] = 0x7F;
    a[3] = 6;
    a[5] = 0x10;
    for i in 2..=8 {
        a[8 * i] = 0x04;
    }
    let areas = PanelLive::decode_areas(&a);
    assert_eq!(8, areas.len());
    assert!(!areas[0].ready);
    assert!(areas[1].ready);
    assert_eq!("Disarmed", areas[1].armed_name);
    // bits 0-1 = 3, bits 2-4 = 0x08, bits 5-7 = 1
    let mut b = [0u8; 68];
    b[8] = 0x20 | 0x08 | 3;
    let one = PanelLive::decode_areas(&b).remove(0);
    assert!(one.alarm);
    assert!(one.ready_to_force_arm);
    assert_eq!("Armed Away", one.armed_name);
    // zones 1-16 = 0x09 (open, violated), voltage byte 0x89 = 13.7 V
    let mut z = [0u8; 212];
    let mut v = [0u8; 212];
    for i in 1..=16 {
        z[i + 3] = 0x09;
        v[i + 3] = 0x89;
    }
    z[20] = 0x20 | 0x02 | 4; // zone 17: EOL resistor, trouble, bypassed
    let zones = PanelLive::decode_zones(&z, Some(&v));
    assert_eq!(208, zones.len());
    assert_eq!("Violated", zones[0].state);
    assert_eq!("Open (+)", zones[0].physical);
    assert!((zones[0].volts - 13.7).abs() < 1e-3);
    assert_eq!("Trouble", zones[16].state);
    assert!(zones[16].bypassed);
    assert_eq!("EOLR (=)", zones[16].physical);
    assert_eq!("Normal", zones[100].state);
    // system: byte 4 + 18 = A.C. failure
    let mut s = [0u8; 56];
    s[4 + 18] = 1;
    s[4 + 19] = 2;
    assert_eq!(vec!["A.C. Failure", "Box Tamper"], PanelLive::decode_system(&s));
}

fn open_live() -> (SimulatedPanel, ElkSession) {
    let panel = SimulatedPanel::new(LMAC, LPASS, CODE);
    let mut s = ElkSession::new(LMAC, LPASS).unwrap();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login(CODE).unwrap();
    (panel, s)
}

#[test]
fn reads_a_snapshot_over_an_open_session_and_stays_open() {
    let (_panel, mut s) = open_live();
    let one = PanelLive::read(&mut s).unwrap();
    let two = PanelLive::read(&mut s).unwrap(); // the same session answers repeated polls
    assert_eq!(8, one.areas.len());
    assert_eq!("Armed Away", one.areas[2].armed_name);
    assert!(one.system.contains(&"A.C. Failure".to_string()));
    assert_eq!("Violated", one.zones[0].state);
    assert!((one.zones[0].volts - 13.7).abs() < 1e-3);
    assert_eq!(one.zones.len(), two.zones.len());
}

fn arm_refused(status: u8) {
    let (panel, mut s) = open_live();
    panel.set_area_status(2, status);
    let e = PanelControl::arm_checked(&mut s, 2, 1, false).unwrap_err();
    assert!(matches!(e, ElkError::InvalidOperation(_)));
    let m = e.to_string();
    assert!(m.contains("not ready to arm"), "{m}");
    assert!(m.contains("Nothing was sent"));
    assert!(m.contains("force arm"));
    assert!(m.contains("Open or violated zones: 1, 2"));
    PanelLive::read(&mut s).unwrap(); // let the panel finish with the session
    assert!(panel.arm_requests().is_empty());
}

#[test]
fn arm_is_refused_unless_the_area_is_fully_ready_not_ready() {
    arm_refused(0x00);
}

#[test]
fn arm_is_refused_unless_the_area_is_fully_ready_force_arm_ready() {
    arm_refused(0x08);
}

#[test]
fn arm_is_sent_when_the_area_is_ready_and_force_skips_the_check() {
    let (panel, mut s) = open_live();
    PanelControl::arm_checked(&mut s, 2, 2, false).unwrap(); // area 2 is ready in the canned status
    panel.set_area_status(4, 0x08);
    PanelControl::arm_checked(&mut s, 4, 1, true).unwrap(); // explicit force: no check
    PanelLive::read(&mut s).unwrap();
    assert_eq!(vec![vec![0x7F, 0, 0x40, 0, 2, 2], vec![0x7F, 0, 0x40, 0, 4, 1]], panel.arm_requests());
}

#[test]
fn disarm_does_not_need_a_ready_area() {
    let (panel, mut s) = open_live();
    panel.set_area_status(1, 0x00);
    PanelControl::arm_checked(&mut s, 1, 0, false).unwrap();
    PanelLive::read(&mut s).unwrap();
    let r = panel.arm_requests();
    assert_eq!(1, r.len());
    assert_eq!(0, r[0][5]);
}

#[test]
fn control_argument_ranges_are_checked() {
    let (_panel, mut s) = open_live();
    assert!(matches!(PanelControl::arm(&mut s, 0, 1), Err(ElkError::OutOfRange(_))));
    assert!(matches!(PanelControl::arm(&mut s, 1, 7), Err(ElkError::OutOfRange(_))));
    assert!(matches!(PanelControl::toggle_bypass(&mut s, 209, 1), Err(ElkError::OutOfRange(_))));
    assert!(matches!(PanelControl::silence(&mut s, 9), Err(ElkError::OutOfRange(_))));
}

#[test]
fn silence_and_bypass_send_the_traced_bytes() {
    let (panel, mut s) = open_live();
    s.timeout = Duration::from_millis(150);
    PanelControl::silence(&mut s, 3).unwrap();
    PanelControl::toggle_bypass(&mut s, 5, 1).unwrap();
    let got = panel.received();
    assert!(got.contains(&vec![0xFF, 0, 0, 10, 3]));
    assert!(got.contains(&vec![0xFF, 0, 0, 9, 0, 5, 1, 0]));
}

// ---- ModulesTests.cs ----

#[test]
fn decodes_the_enrolled_bitmaps_seen_on_the_real_panel() {
    // 88-byte reply to 7F 00 00 04 from the spare panel: header, FF FF at 4-5, and a 1 at bytes 30, 55 and 63
    let mut r = [0u8; 88];
    r[0] = 0x7F;
    r[3] = 4;
    r[4] = 0xFF;
    r[5] = 0xFF;
    r[30] = 1;
    r[55] = 1;
    r[63] = 1;
    assert_eq!(vec![("Keypad", 1u8, 1)], PanelModules::decode_enrolled(&r));
    // keypad 9 and 16 (second byte of the group), input expander 3, output expander 13, serial expander 2
    r[63] = 0;
    r[62] = 0x81;
    r[67] = 0x04;
    r[71 - 1] = 0x10;
    r[79] = 0x02;
    let d: Vec<String> = PanelModules::decode_enrolled(&r).iter().map(|x| format!("{} {}", x.0, x.2)).collect();
    assert_eq!(vec!["Keypad 9", "Keypad 16", "Input expander 3", "Output expander 13", "Serial port expander 2"], d);
}

#[test]
fn decodes_version_replies_and_models() {
    // keypad reply seen on the spare panel: hardware 1.6, boot 1.0.14, firmware 1.0.20
    let mut r = [0u8; 24];
    r[0] = 0x7C;
    r[2] = 1;
    r[3] = 1;
    r[8] = 1;
    r[9] = 6;
    r[10] = 1;
    r[11] = 0;
    r[12] = 14;
    r[13] = 1;
    r[14] = 0;
    r[15] = 20;
    let m = PanelModules::decode_versions("Keypad", 1, Some(&r));
    assert_eq!(("M1KP2/KP3", "1.6", "1.0.14", "1.0.20", true), (m.model.as_str(), m.hardware.as_str(), m.boot.as_str(), m.firmware.as_str(), m.responding));
    let missing = PanelModules::decode_versions("Input expander", 4, None);
    assert!(!missing.responding);
    assert_eq!("Missing or not working", missing.firmware);
    assert_eq!("M1XRFTWM", PanelModules::model_for("Input expander", 1800));
    assert_eq!("M1XEP", PanelModules::model_for("Ethernet expander", 0));
}

#[test]
fn enroll_reports_what_the_panel_answers() {
    let (_panel, mut s) = open_live();
    s.timeout = Duration::from_millis(150);
    // the simulator does not answer 7F 00 00 11
    assert_eq!("the panel sent no answer", PanelModules::enroll(&mut s).unwrap());
}

// ---- ClockLogTests.cs ----

#[test]
fn set_request_is_bcd_with_sunday_as_one() {
    let t = dt(2026, 11, 12, 19, 45, 30); // a Thursday: 5
    assert_eq!(vec![0xFF, 0, 0, 3, 0x30, 0x45, 0x19, 5, 0x12, 0x11, 0x26], PanelClock::set_request(t));
    assert_eq!(1, PanelClock::set_request(dt(2026, 11, 15, 0, 0, 0))[7]); // a Sunday
}

#[test]
fn clock_reply_decodes_and_a_zero_day_or_month_becomes_one() {
    let mut r = vec![0x7F, 0, 0, 3, 0x30, 0x45, 0x19, 5, 0x12, 0x11, 0x26];
    assert_eq!(Some(dt(2026, 11, 12, 19, 45, 30)), PanelClock::decode(&r));
    r[8] = 0;
    r[9] = 0;
    assert_eq!(Some(dt(2026, 1, 1, 19, 45, 30)), PanelClock::decode(&r));
    r[6] = 0x99;
    assert_eq!(None, PanelClock::decode(&r));
    assert_eq!(None, PanelClock::decode(&[0u8; 5]));
}

#[test]
fn access_code_request_sends_the_digits_last_first() {
    assert_eq!(vec![0x7F, 0, 0, 5, 1, 0, 8, 6, 4, 2], PanelAccess::change_request("246801").unwrap());
    assert_eq!(vec![0x7F, 0, 0, 5, 6, 5, 4, 3, 2, 1], PanelAccess::change_request("123456").unwrap());
    for bad in ["", "12345", "1234567", "12a456", "12 456", "\u{661}\u{662}\u{663}\u{664}\u{665}\u{666}"] {
        assert!(matches!(PanelAccess::change_request(bad), Err(ElkError::Argument(_))), "{bad:?}");
    }
}

/// Packs an entry the way the panel does (see PanelLog's description).
fn pack(t: NaiveDateTime, area: i32, ev: i32, item: i32) -> Vec<u8> {
    use chrono::{Datelike, Timelike};
    let (y, mo, d, h, mi) = (t.year() - 2000, t.month() as i32, t.day() as i32, t.hour() as i32, t.minute() as i32);
    let dow = t.weekday().num_days_from_sunday() as i32 + 1;
    let (a, i) = (area - 1, item - 1);
    vec![
        ((y << 3) | (mo >> 1)) as u8,
        (((mo & 1) << 7) | (d << 2) | (h >> 3)) as u8,
        (((h & 7) << 5) | (mi >> 1)) as u8,
        (((mi & 1) << 7) | (dow << 4) | (a << 1)) as u8,
        (ev >> 4) as u8,
        (((ev & 15) << 4) | (i >> 5)) as u8,
        ((i & 31) << 3) as u8,
        1,
    ]
}

#[test]
fn log_entry_round_trips_through_the_packing() {
    let t = dt(2026, 11, 12, 19, 45, 0);
    let e = PanelLog::decode_entry(&pack(t, 3, 1367, 17), 0, 1).unwrap();
    assert_eq!(Some(t), e.time);
    assert_eq!((3, 1367, 17), (e.area, e.event, e.item));
    assert_eq!(None, PanelLog::decode_entry(&[0u8; 8], 0, 1)); // all zero: empty
    assert!(PanelLog::decode_entry(&pack(t, 1, 4095, 1), 0, 1).unwrap().end_of_log);
}

#[test]
fn end_of_log_marker_in_the_middle_is_skipped_and_an_empty_slot_ends_the_list() {
    let t = dt(2026, 11, 16, 22, 41, 0);
    let mut r = vec![0x7B, 5, 0x01, 0xED];
    r.extend(pack(t, 1, 1363, 1));
    r.extend(pack(t, 1, 4095, 1));
    r.extend(pack(t - chrono::Duration::days(5), 2, 1174, 1));
    r.extend([0u8; 8]);
    r.extend(pack(t, 1, 1111, 1));
    let (entries, next, ended) = PanelLog::decode_reply(&r, 1, i32::MAX);
    assert_eq!(vec![1363, 1174], entries.iter().map(|e| e.event).collect::<Vec<_>>());
    assert_eq!(vec![1, 2], entries.iter().map(|e| e.index).collect::<Vec<_>>());
    assert!(ended);
    assert_eq!(0x01ED, next);
    assert_eq!(1, PanelLog::decode_reply(&r, 1, 1).0.len()); // only the first slot is looked at
}

#[test]
fn clock_can_be_read_and_set_over_a_session() {
    let (panel, mut s) = {
        let panel = SimulatedPanel::new("00:40:9D:11:22:33", "elk", CODE);
        let mut s = ElkSession::new("00:40:9D:11:22:33", "elk").unwrap();
        s.connect("127.0.0.1", panel.port()).unwrap();
        s.login(CODE).unwrap();
        (panel, s)
    };
    assert_eq!(Some(panel.clock()), PanelClock::read(&mut s).unwrap());
    let want = dt(2027, 3, 9, 6, 7, 8);
    assert!(PanelClock::set(&mut s, want).unwrap());
    assert_eq!(want, panel.clock());
    assert_eq!(Some(want), PanelClock::read(&mut s).unwrap());
    assert!(matches!(PanelClock::set(&mut s, dt(1999, 1, 1, 0, 0, 0)), Err(ElkError::OutOfRange(_))));
}

#[test]
fn access_code_change_is_acknowledged_and_the_new_code_is_kept() {
    let panel = SimulatedPanel::new("00:40:9D:11:22:33", "elk", CODE);
    let mut s = ElkSession::new("00:40:9D:11:22:33", "elk").unwrap();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login(CODE).unwrap();
    assert!(PanelAccess::change(&mut s, "135790").unwrap());
    assert_eq!("135790", panel.access_code());
}

#[test]
fn log_is_read_newest_first_in_blocks_of_twenty() {
    let panel = SimulatedPanel::new("00:40:9D:11:22:33", "elk", CODE);
    let t0 = dt(2026, 11, 12, 10, 0, 0);
    for i in 0..45 {
        panel.push_log_record(pack(t0 - chrono::Duration::minutes(i as i64), 1 + i % 8, 100 + i, 1 + i));
    }
    let mut s = ElkSession::new("00:40:9D:11:22:33", "elk").unwrap();
    s.connect("127.0.0.1", panel.port()).unwrap();
    s.login(CODE).unwrap();
    let all = PanelLog::read(&mut s, 100).unwrap();
    assert_eq!(45, all.len());
    assert_eq!(vec![1, 2, 3], all.iter().take(3).map(|e| e.index).collect::<Vec<_>>());
    assert_eq!(100, all[0].event);
    assert_eq!(Some(t0 - chrono::Duration::minutes(44)), all[44].time);
    assert_eq!(7, PanelLog::read(&mut s, 7).unwrap().len()); // a smaller request stops early
}
