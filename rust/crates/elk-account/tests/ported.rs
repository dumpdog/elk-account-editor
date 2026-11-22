//! Ports of the C# unit tests in port/tests/Elk.Tests for the units in this crate.

use std::path::PathBuf;

use elk_account::records::{ZoneFlag1, ZoneFlag2, ZoneRecord};
use elk_account::*;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn sample(name: &str) -> Option<String> {
    std::fs::read(root().join("sample").join(name))
        .ok()
        .map(|b| decode_latin1(&b))
}

fn testing(name: &str) -> Option<String> {
    std::fs::read(root().join("TESTING").join(name))
        .ok()
        .map(|b| decode_latin1(&b))
}

fn template() -> String {
    decode_latin1(&std::fs::read(root().join("port/src/Elk.Editor/template-m1g.txt")).unwrap())
}

// ---- AccountExportTests ----------------------------------------------------------------------------------------------

fn export_sample() -> String {
    let mut l: Vec<String> = vec!["\"Acct\",\"x\"".into(), "1".into(), "7".into()];
    for zn in [1, 161] {
        let mut z = ZoneRecord::default();
        z.set_function(3);
        z.set_flag1(17);
        z.set_name(&format!("Old {zn}"));
        z.bytes[3] = 9;
        l.push(zn.to_string());
        l.extend(z.bytes.iter().map(|b| b.to_string()));
    }
    l.insert(2, "2".into());
    l.remove(3); // count line "2" replaces the stray "7"
    l.extend(["0".into(), "0".into(), "\"tail\"".into()]);
    l.join("\r\n") + "\r\n"
}

#[test]
fn export_updates_existing_and_adds_new_in_order_keeping_crlf() {
    let spec = |zone, name: &str, def, ty, area, flags1, silent| ZoneSpec {
        zone,
        name: name.into(),
        definition: def,
        type_: ty,
        area,
        flags1,
        swinger_shutdown: false,
        silent_alarm: silent,
    };
    let specs = [
        spec(1, "Front Door", 1, 0, 2, ZoneFlag1::BYPASSABLE, true),
        spec(40, "Kitchen", 2, 3, 1, 0, false),
    ];
    let (text, _log) = AccountExport::apply_zones(&export_sample(), &specs).unwrap();
    assert!(text.contains("\r\n"));
    let lines: Vec<String> = text.split("\r\n").map(String::from).collect();
    let blk = AccountExport::find_zone_block(&lines).unwrap();
    assert_eq!(blk.count, 3);
    assert_eq!(lines[blk.count_line + 1], "1");
    assert_eq!(lines[blk.count_line + 1 + 33], "40");
    assert_eq!(lines[blk.count_line + 1 + 66], "161");
    let mut z1 = ZoneRecord::default();
    for b in 0..32 {
        z1.bytes[b] = lines[blk.count_line + 2 + b].parse().unwrap();
    }
    assert_eq!(z1.name(), "Front Door");
    assert_eq!(z1.bytes[3], 9); // unused byte preserved
    assert_eq!(z1.function(), 1);
    assert_eq!(z1.flag1(), 1);
    assert_eq!(ZoneFlag2::area_of(z1.flag2()), 2);
    assert_ne!(z1.flag2() & ZoneFlag2::SILENT_ALARM, 0);
    assert_eq!(lines[lines.len() - 2], "\"tail\""); // data after the block untouched
}

#[test]
fn export_missing_block_is_reported() {
    let spec = ZoneSpec {
        zone: 1,
        name: "x".into(),
        definition: 1,
        type_: 0,
        area: 1,
        flags1: 0,
        swinger_shutdown: false,
        silent_alarm: false,
    };
    assert!(matches!(
        AccountExport::apply_zones("\"a\"\r\n5\r\n", &[spec]),
        Err(Error::InvalidData(_))
    ));
}

// ---- AccountFileTests --------------------------------------------------------------------------------------------------

#[test]
fn real_sample_roundtrips_byte_for_byte_through_the_model() {
    let Some(text) = sample("ElkRPAcctSample Export") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    assert_eq!(f.zones.entries.len(), 64);
    assert_eq!(f.users.entries.len(), 8);
    assert_eq!(f.areas.entries.len(), 1);
    assert_eq!(f.keypads.entries.len(), 2);
    assert_eq!(f.phones.entries.len(), 1);
    assert_eq!(f.to_text(), text);
    AccountModel::from(&f).unwrap().apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text);
}

#[test]
fn model_edits_land_in_the_right_bytes() {
    let Some(text) = sample("ElkRPAcctSample Export") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let z = m.zones.iter_mut().find(|x| x.n == 2).unwrap();
    z.name = "Patio Door".into();
    z.flags = vec!["bypassable".into(), "chime".into(), "silent".into()];
    z.area = 3;
    z.definition = 1;
    m.zones.push(ZoneDto {
        n: 100,
        name: "New".into(),
        definition: 3,
        ..Default::default()
    });
    m.users.iter_mut().find(|u| u.n == 2).unwrap().code = "1357".into();
    m.areas[0].exit1 = 45;
    m.phones[0].number = "8005551212".into();
    m.apply_to(&mut f).unwrap();
    let f2 = AccountFile::parse(&f.to_text()).unwrap();
    let m2 = AccountModel::from(&f2).unwrap();
    let z2 = m2.zones.iter().find(|x| x.n == 2).unwrap();
    assert_eq!(z2.name, "Patio Door");
    assert_eq!(z2.area, 3);
    assert!(z2.flags.contains(&"chime".to_string()));
    assert!(z2.flags.contains(&"silent".to_string()));
    assert_eq!(m2.zones.len(), 65);
    assert_eq!(
        m2.zones.iter().find(|x| x.n > 48 && x.n < 161).unwrap().n,
        100
    );
    assert_eq!(m2.users.iter().find(|u| u.n == 2).unwrap().code, "1357");
    assert_eq!(m2.areas[0].exit1, 45);
    assert_eq!(m2.phones[0].number, "8005551212");
    assert_eq!(m.zone_groups, m2.zone_groups);
}

#[test]
fn wireless_groups_are_within_zones_17_to_160() {
    for (csv, ok) in [
        ("0,0,0,0,0,0,0,0,0,0,0,0,0", true),
        ("0,1,1,0,0,0,0,0,0,0,0,0,0", true),
        ("0,1,1,1,1,1,1,1,1,1,0,0,0", true),
        ("0,0,1,0,0,0,0,0,0,0,0,0,0", true),
        ("0,1,0,1,0,0,0,0,0,0,0,0,0", true),
        ("1,1,0,0,0,0,0,0,0,0,0,0,0", false),
        ("0,1,1,1,1,1,1,1,1,1,1,0,0", false),
    ] {
        let g: Vec<i32> = csv.split(',').map(|s| s.parse().unwrap()).collect();
        assert_eq!(
            AccountModel::validate_wireless_groups(&g).is_ok(),
            ok,
            "{csv}"
        );
    }
}

// ---- CommunicatorTests / TelephoneTests -------------------------------------------------------------------------------------

#[test]
fn communicator_export_roundtrips_and_decodes() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    assert!(f.has_comm());
    assert_eq!(f.area_rcs.entries.len(), 1);
    assert_eq!(f.user_rcs.entries.len(), 8);
    assert_eq!(f.zone_rcs.entries.len(), 65);
    assert_eq!(f.to_text(), text);
    let m = AccountModel::from(&f).unwrap();
    let c = m.comm.as_ref().unwrap();
    assert!(c.areas[0].codes.iter().all(|&x| x == 1));
    assert_eq!([c.users[0].close, c.users[0].open], [1, 1]);
    assert_eq!(c.zones[0].codes, vec![1, 0, 0, 0]);
    assert_eq!(c.system.ac_fail_delay, 15);
    assert!(c.system.codes.iter().all(|&x| x == 1));
    assert_eq!(c.keypads[0].codes, vec![1, 1, 1, 0, 0, 0]);
    m.apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text);
}

#[test]
fn switching_on_the_pulses_in_the_old_export_gives_the_communicator_export() {
    let (Some(before), Some(after)) = (
        sample("Sample Zones Import Export"),
        sample("Sample Communicator"),
    ) else {
        return;
    };
    let mut f = AccountFile::parse(&before).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let c = m.comm.as_mut().unwrap();
    for a in &mut c.areas {
        a.codes[..17].fill(1);
    }
    for u in &mut c.users {
        u.close = 1;
        u.open = 1;
    }
    c.system.codes[..18].fill(1);
    c.keypads[0].codes[..3].fill(1);
    m.apply_to(&mut f).unwrap();
    let got = f
        .to_text()
        .replace("\"Sample Import\"", "\"Sample Import F1 Com\"");
    assert_eq!(got, after);
}

#[test]
fn report_code_out_of_range_is_refused() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    m.comm.as_mut().unwrap().system.codes[0] = 256;
    assert!(matches!(m.apply_to(&mut f), Err(Error::Format(_))));
}

#[test]
fn ip_format_allows_one_attempt_and_area_codes_are_written() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let t = &mut m.phones[0];
    t.format = 6;
    t.attempts = 5;
    t.account_codes[0] = Some("000000".into());
    t.account_codes[1] = Some("009999".into());
    m.apply_to(&mut f).unwrap();
    let e = &f.phones.entries[0].bytes;
    assert_eq!(e[21] >> 4, 6);
    assert_eq!(e[22], 1);
    let back = AccountModel::from(&AccountFile::parse(&f.to_text()).unwrap())
        .unwrap()
        .phones
        .remove(0);
    assert_eq!(back.account_codes[0].as_deref(), Some("000000"));
    assert_eq!(back.account_codes[1].as_deref(), Some("009999"));
}

#[test]
fn format_above_7_is_refused() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    m.phones[0].format = 8;
    assert!(matches!(m.apply_to(&mut f), Err(Error::Format(_))));
}

// ---- DeleteTests -----------------------------------------------------------------------------------------------------

#[test]
fn without_prune_a_missing_record_is_kept_and_with_prune_it_is_removed() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let zones = m.zones.len();
    let rcs = m.comm.as_ref().unwrap().zones.len();
    m.zones.retain(|z| z.n != 5 && z.n != 6);
    m.comm
        .as_mut()
        .unwrap()
        .zones
        .retain(|z| z.n != 5 && z.n != 6);
    m.apply_to(&mut f).unwrap(); // Prune off: nothing is deleted
    assert_eq!(f.zones.entries.len(), zones);
    assert_eq!(f.zone_rcs.entries.len(), rcs);

    m.prune = true;
    m.apply_to(&mut f).unwrap();
    assert_eq!(f.zones.entries.len(), zones - 2);
    assert_eq!(f.zone_rcs.entries.len(), rcs - 2);
    assert!(f.zones.get(5).is_none());
    assert!(f.zone_rcs.get(6).is_none());
    let f2 = AccountFile::parse(&f.to_text()).unwrap(); // still a valid file with the right counts
    assert_eq!(f2.zones.entries.len(), zones - 2);
    assert!(f2.has_comm());
    assert_eq!(f2.zone_rcs.entries.len(), rcs - 2);
    assert_eq!(
        m.phones.len(),
        AccountModel::from(&f2).unwrap().phones.len()
    );
}

#[test]
fn prune_removes_users_phones_keypads_and_their_report_rows() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    let users = m.users.len();
    let urc = m.comm.as_ref().unwrap().users.len();
    let victim = m.users.iter().find(|u| u.n > 1).unwrap().n;
    m.users.retain(|u| u.n != victim);
    m.comm.as_mut().unwrap().users.retain(|u| u.n != victim);
    m.phones.clear();
    m.prune = true;
    m.apply_to(&mut f).unwrap();
    assert_eq!(f.users.entries.len(), users - 1);
    assert_eq!(f.user_rcs.entries.len(), urc - 1);
    assert!(f.phones.entries.is_empty());
    let f2 = AccountFile::parse(&f.to_text()).unwrap();
    assert!(f2.has_comm());
    assert!(f2.phones.entries.is_empty());
    assert_eq!(f2.users.entries.len(), users - 1);
}

#[test]
fn prune_refuses_to_leave_things_in_a_deleted_area_or_delete_required_items() {
    let Some(text) = sample("Sample Communicator") else {
        return;
    };
    let f = AccountFile::parse(&text).unwrap();
    let base = AccountModel::from(&f).unwrap();
    let mut m1 = base.clone();
    m1.prune = true;
    m1.areas.clear();
    assert!(matches!(
        m1.apply_to(&mut AccountFile::parse(&text).unwrap()),
        Err(Error::Format(_))
    ));
    let mut m2 = base.clone();
    m2.prune = true;
    m2.users.retain(|u| u.n != 1);
    assert!(matches!(
        m2.apply_to(&mut AccountFile::parse(&text).unwrap()),
        Err(Error::Format(_))
    ));
    let mut m3 = base;
    m3.prune = true;
    m3.keypads.clear();
    assert!(matches!(
        m3.apply_to(&mut AccountFile::parse(&text).unwrap()),
        Err(Error::Format(_))
    ));
}

// ---- NoPhoneAccountTests (TESTING folder, skipped when absent) ---------------------------------------------------------------

#[test]
fn no_telephones_account_finds_communicator_block() {
    let Some(text) = testing("RHTEST") else {
        return;
    };
    let f = AccountFile::parse(&text).unwrap();
    assert!(f.phones.entries.is_empty());
    assert!(f.has_comm());
    assert_eq!(f.area_rcs.entries.len(), 1);
    assert_eq!(f.user_rcs.entries.len(), 4);
    assert_eq!(f.zone_rcs.entries.len(), 16);
    assert_eq!(f.to_text(), text);
}

#[test]
fn no_telephones_account_communicator_edit_roundtrips() {
    let Some(text) = testing("RHTEST") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    m.comm.as_mut().unwrap().areas[0].codes[2] = 3;
    m.comm.as_mut().unwrap().system.codes[1] = 3;
    m.apply_to(&mut f).unwrap();
    let m2 = AccountModel::from(&AccountFile::parse(&f.to_text()).unwrap()).unwrap();
    assert_eq!(m2.comm.as_ref().unwrap().areas[0].codes[2], 3);
    assert_eq!(m2.comm.as_ref().unwrap().system.codes[1], 3);
    assert_eq!(m.zones.len(), m2.zones.len());
    assert!(m2.phones.is_empty());
}

#[test]
fn no_telephones_account_can_add_a_telephone() {
    let Some(text) = testing("RHTEST") else {
        return;
    };
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    assert!(m.has_phones);
    assert!(m.phones.is_empty());
    let mut codes = vec![Some("000000".to_string()); 8];
    codes[0] = Some("009999".into());
    m.phones.push(PhoneDto {
        n: 1,
        name: "Central".into(),
        number: "5551234".into(),
        format: 6,
        attempts: 3,
        account_codes: codes,
        flags: vec!["areaEvents".into(), "globalEvents".into()],
    });
    m.comm.as_mut().unwrap().areas[0].codes[0] = 1;
    m.apply_to(&mut f).unwrap();
    let text2 = f.to_text();
    assert_eq!(text.split('\n').count() + 89, text2.split('\n').count()); // one phone = 1 id line + 88 bytes
    let m2 = AccountModel::from(&AccountFile::parse(&text2).unwrap()).unwrap();
    assert_eq!(m2.phones.len(), 1);
    let p = &m2.phones[0];
    assert_eq!(
        (p.name.as_str(), p.number.as_str(), p.format, p.attempts),
        ("Central", "5551234", 6, 1)
    );
    assert_eq!(p.account_codes[0].as_deref(), Some("009999"));
    assert_eq!(p.account_codes[1].as_deref(), Some("000000"));
    let mut fl = p.flags.clone();
    fl.sort();
    assert_eq!(fl, ["areaEvents", "globalEvents"]);
    assert_eq!(m2.comm.as_ref().unwrap().areas[0].codes[0], 1);
    assert_eq!(m2.comm.as_ref().unwrap().users.len(), 4);
    assert_eq!(m2.comm.as_ref().unwrap().zones.len(), 16);
    assert_eq!(m.zones.len(), m2.zones.len());
}

// ---- GlobalsTests / WirelessTests -----------------------------------------------------------------------------------------

#[test]
fn globals_edit_round_trips_and_unchanged_is_byte_identical() {
    let text = template();
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    assert_eq!(m.globals.as_ref().unwrap().len(), 48);
    m.apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text); // untouched = identical
    m.globals.as_mut().unwrap()[16] = 7;
    m.globals.as_mut().unwrap()[0] |= 8;
    m.apply_to(&mut f).unwrap();
    let g = AccountFile::parse(&f.to_text())
        .unwrap()
        .globals()
        .unwrap()
        .unwrap();
    assert_eq!(g[16], 7);
    assert!(g[0] & 8 != 0);
}

#[test]
fn wireless_template_has_three_blocks_and_unchanged_apply_is_identical() {
    let text = template();
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    assert_eq!(m.wireless.as_ref().unwrap().len(), 3);
    assert!(m.wireless.as_ref().unwrap().iter().all(|b| b.len() == 260));
    m.apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text);
    m.wireless.as_mut().unwrap()[0][4 + 16] |= 1;
    m.wireless.as_mut().unwrap()[0][116 + 3 * 16 + 2] = 0x35;
    m.apply_to(&mut f).unwrap();
    let w = AccountFile::parse(&f.to_text())
        .unwrap()
        .wireless()
        .unwrap()
        .unwrap();
    assert_eq!(w[0][20] & 1, 1);
    assert_eq!(w[0][166], 0x35);
}

#[test]
fn wireless_real_exports_decode_as_documented() {
    let (Some(a), Some(b)) = (testing("ROUND5/R5-B Wireless"), testing("ROUND5/R7-B")) else {
        return;
    };
    let wa = &AccountFile::parse(&a).unwrap().wireless().unwrap().unwrap()[0];
    let wb = &AccountFile::parse(&b).unwrap().wireless().unwrap().unwrap()[0];
    assert_eq!(wa[2], 16);
    assert_eq!((wa[4 + 16], wb[4 + 16]), (41, 49));
    assert_eq!(wa[20] & 1, 1);
    assert!(wa[20] & 8 != 0 && wa[20] & 16 == 0);
    assert!(wb[20] & 8 == 0 && wb[20] & 16 != 0);
    assert_eq!(
        [0, 1, 2, 3].map(|i| wa[116 + 3 * (16 + i) + 2]),
        [51, 52, 53, 53]
    );
}

// ---- DeleteTests: default records live in elk-core; the zone default is checked through the shared voice/record helpers ------

// ---- ZoneImportTests ---------------------------------------------------------------------------------------------------

#[test]
fn default_voice_words() {
    for (zone, prefix) in [
        (1, vec![471u16, 22]),
        (2, vec![471, 23]),
        (3, vec![471, 24]),
        (12, vec![471, 33]),
        (20, vec![471, 41]),
        (21, vec![471, 41, 22]),
        (40, vec![471, 43]),
        (100, vec![471, 22, 49]),
        (105, vec![471, 22, 49, 26]),
        (110, vec![471, 22, 31]),
        (186, vec![471, 22, 47, 27]),
        (200, vec![471, 23, 49]),
        (208, vec![471, 23, 49, 29]),
    ] {
        let w = ZoneVoice::words(zone).unwrap();
        assert_eq!(&w[..prefix.len()], &prefix[..], "zone {zone}");
        assert!(w[prefix.len()..].iter().all(|&x| x == 0), "zone {zone}");
    }
}

#[test]
fn record_voice_matches_stored_zone1_bytes() {
    let mut z = ZoneRecord::default();
    z.set_function(1);
    z.set_flag1(17);
    z.set_name("Zone 001");
    ZoneVoice::apply(&mut z, 1).unwrap();
    assert_eq!(z.crc(), 2259); // stored UDCRC of sample zone 1 (voice words 471, 22)
}

#[test]
fn csv_builds_record_with_flags_type_area_and_voice() {
    let (zones, errors) = ZoneCsv::parse("Zone,Name,Definition,Type,Area,Attributes\n186,Back Door,1,5,4,\"Bypassable|Enable Chime|Silent alarm\"\n");
    assert!(errors.is_empty());
    assert_eq!(zones.len(), 1);
    let z = zones[0].to_record().unwrap();
    assert_eq!(z.name(), "Back Door");
    assert_eq!(z.function(), 1);
    assert_eq!(z.flag1(), ZoneFlag1::BYPASSABLE | ZoneFlag1::CHIME);
    assert_eq!(ZoneFlag2::type_of(z.flag2()), 5);
    assert_eq!(ZoneFlag2::area_of(z.flag2()), 4);
    assert_ne!(z.flag2() & ZoneFlag2::SILENT_ALARM, 0);
    assert_eq!(z.flag2() & ZoneFlag2::SWINGER_SHUTDOWN, 0);
    assert_eq!(z.voice(), [471, 22, 47, 27, 0, 0]);
}

#[test]
fn csv_reports_bad_rows_with_line_numbers() {
    let (zones, errors) = ZoneCsv::parse(
        "Zone,Name,Definition,Type,Area,Attributes\n1,A,1,0,1,\n1,B,1,0,1,\n2,TOO-LONG-NAME-OVER-16,1,0,1,\n3,C,1,9,1,\n4,D,1,0,1,Nonsense\n209,E,1,0,1,\n",
    );
    assert_eq!(zones.len(), 1);
    assert_eq!(errors.len(), 5);
    assert!(errors
        .iter()
        .any(|e| e.starts_with("line 3") && e.contains("twice")));
}

// ---- ZoneSheetTests ---------------------------------------------------------------------------------------------------------

fn row(n: i32, name: &str, def: Option<i32>, ty: Option<i32>, area: Option<i32>) -> ZoneSheetRow {
    ZoneSheetRow {
        n,
        name: Some(name.into()),
        definition: def,
        type_: ty,
        area,
    }
}

#[test]
fn sheet_csv_with_header_any_column_order() {
    let r = ZoneSheet::read(
        b"Name,Zone,Area,Type,Definition\r\nFront Door,1,Area 1,Normally Closed,Burglar Entry/Exit 1\r\nGarage,2,2,0,03\r\n",
        "z.csv",
    )
    .unwrap();
    assert!(r.warnings.is_empty());
    assert_eq!(r.rows[0], row(1, "Front Door", Some(1), Some(1), Some(1)));
    assert_eq!(r.rows[1], row(2, "Garage", Some(3), Some(0), Some(2)));
}

#[test]
fn sheet_names_only_is_numbered_from_zone_1() {
    let r = ZoneSheet::read(b"Name\nFront Door\nBack Door\n\nKitchen Window\n", "z.csv").unwrap();
    assert_eq!(r.rows.iter().map(|x| x.n).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(r
        .rows
        .iter()
        .all(|x| x.definition.is_none() && x.type_.is_none() && x.area.is_none()));
    assert!(r.warnings.iter().any(|w| w.contains("no Zone column")));
}

#[test]
fn sheet_headerless_uses_paste_box_order_and_tabs() {
    let r = ZoneSheet::read(b"5\tPatio\t1\t0\t1\n6\tShed\t8\t2\t3\n", "z.txt").unwrap();
    assert_eq!(r.rows[0], row(5, "Patio", Some(1), Some(0), Some(1)));
    assert_eq!(r.rows[1], row(6, "Shed", Some(8), Some(2), Some(3)));
}

#[test]
fn sheet_quoted_cells_semicolons_and_bom() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"Zone;Name\n1;\"Door, front\"\n2;\"He said \"\"hi\"\"\"\n");
    let r = ZoneSheet::read(&bytes, "z.csv").unwrap();
    assert_eq!(r.rows[0].name.as_deref(), Some("Door, front"));
    assert_eq!(r.rows[1].name.as_deref(), Some("He said \"hi\""));
}

#[test]
fn sheet_bad_rows_are_skipped_with_a_warning_and_long_names_cut() {
    let r = ZoneSheet::read(
        b"Zone,Name,Definition\n0,Zero,1\n209,Too high,1\nabc,Text,1\n7,A name that is much too long,Nonsense\n",
        "z.csv",
    )
    .unwrap();
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.rows[0].n, 7);
    assert_eq!(r.rows[0].name.as_deref(), Some("A name that is m"));
    assert!(r.rows[0].definition.is_none());
    assert_eq!(r.warnings.len(), 5); // three skipped rows, the cut name, the unknown definition
}

#[test]
fn sheet_duplicate_zone_is_reported() {
    let r = ZoneSheet::read(b"Zone,Name\n3,One\n3,Two\n", "z.csv").unwrap();
    assert!(r
        .warnings
        .iter()
        .any(|w| w.contains("Zone 3 appears 2 times")));
}

#[test]
fn sheet_definition_lookup() {
    let names = &ModelMeta::ZONE_DEFINITIONS;
    for (s, want) in [
        ("1", Some(1)),
        ("01", Some(1)),
        ("Burglar Entry/Exit 1", Some(1)),
        ("burglar  entry/exit 2", Some(2)),
        ("03 = Burglar Perimeter Instant", Some(3)),
        ("Burglar Perimeter", Some(3)),
        ("Fire Alarm", Some(10)),
        ("99", None),
        ("Burglar", None),
    ] {
        assert_eq!(ZoneSheet::lookup(s, names), want, "{s}");
    }
}

fn zones() -> Vec<ZoneDto> {
    vec![
        ZoneDto {
            n: 1,
            name: "Front Door".into(),
            definition: 1,
            type_: 1,
            area: 1,
            ..Default::default()
        },
        ZoneDto {
            n: 2,
            name: "Unused".into(),
            definition: 0,
            ..Default::default()
        },
        ZoneDto {
            n: 3,
            name: "Hall, \"main\"".into(),
            definition: 4,
            type_: 0,
            area: 2,
            ..Default::default()
        },
    ]
}

#[test]
fn sheet_export_skips_disabled_unless_asked() {
    assert_eq!(ZoneSheet::table(&zones(), false).len(), 3); // header + 2
    assert_eq!(ZoneSheet::table(&zones(), true).len(), 4);
    assert_eq!(
        ZoneSheet::table(&zones(), false)[1],
        [
            "1",
            "Front Door",
            "Burglar Entry/Exit 1",
            "Normally Closed",
            "Area 1"
        ]
    );
}

#[test]
fn sheet_csv_export_reads_back_identically() {
    let back = ZoneSheet::read(
        &ZoneSheet::to_csv(&ZoneSheet::table(&zones(), false)),
        "z.csv",
    )
    .unwrap();
    assert!(back.warnings.is_empty());
    assert_eq!(
        back.rows[0],
        row(1, "Front Door", Some(1), Some(1), Some(1))
    );
    assert_eq!(
        back.rows[1],
        row(3, "Hall, \"main\"", Some(4), Some(0), Some(2))
    );
}

#[test]
fn sheet_xlsx_export_reads_back_identically_and_is_a_valid_package() {
    let bytes = ZoneSheet::to_xlsx(&ZoneSheet::table(&zones(), true)).unwrap();
    let z = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
    assert!(z.file_names().any(|n| n == "xl/worksheets/sheet1.xml"));
    let back = ZoneSheet::read(&bytes, "z.xlsx").unwrap();
    assert!(back.warnings.is_empty());
    assert_eq!(back.rows.iter().map(|r| r.n).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(back.rows[2].name.as_deref(), Some("Hall, \"main\""));
    assert_eq!(back.rows[1].definition, Some(0));
}

/// A workbook laid out the way Excel saves it: shared strings, numbers as plain values, cells that skip a column, a second sheet.
#[test]
fn sheet_reads_an_excel_style_workbook() {
    use std::io::Write;
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let mut add = |p: &str, x: String| {
        zw.start_file(p, opts).unwrap();
        zw.write_all(x.as_bytes()).unwrap();
    };
    let ns = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"";
    add("xl/workbook.xml", format!("<workbook {ns} xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Notes\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Zones\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>"));
    add("xl/_rels/workbook.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Target=\"worksheets/sheet2.xml\"/></Relationships>".into());
    add("xl/sharedStrings.xml", format!("<sst {ns}><si><t>Zone</t></si><si><t>Name</t></si><si><r><t>Front </t></r><r><t>Door</t></r></si><si><t>Type</t></si><si><t>Normally Open</t></si><si><t>Ignore me</t></si></sst>"));
    add("xl/worksheets/sheet1.xml", format!("<worksheet {ns}><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>5</v></c></row></sheetData></worksheet>"));
    // columns A (Zone), B (Name), D (Type): column C is empty and missing from the XML
    add("xl/worksheets/sheet2.xml", format!("<worksheet {ns}><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"D1\" t=\"s\"><v>3</v></c></row>\
<row r=\"2\"><c r=\"A2\"><v>12</v></c><c r=\"B2\" t=\"s\"><v>2</v></c><c r=\"D2\" t=\"s\"><v>4</v></c></row></sheetData></worksheet>"));
    let bytes = zw.finish().unwrap().into_inner();
    let r = ZoneSheet::read(&bytes, "book.xlsx").unwrap();
    assert!(r.warnings.is_empty());
    assert_eq!(r.rows, vec![row(12, "Front Door", None, Some(2), None)]);
}

#[test]
fn sheet_old_xls_and_empty_files_give_clear_errors() {
    let xls = [0xD0, 0xCF, 0x11, 0xE0, 0, 0, 0, 0];
    let e = ZoneSheet::read(&xls, "old.xls").unwrap_err();
    assert!(e.message().contains("Save As"));
    let r = ZoneSheet::read(&[], "e.csv").unwrap();
    assert_eq!(r.warnings.len(), 1);
    assert!(r.warnings[0].contains("no rows"));
}

// ---- AlarmZoneListTests --------------------------------------------------------------------------------------------------------

mod alarm {
    use super::*;
    use elk_account::xml::{Document, Element};
    use std::collections::HashMap;
    use std::io::Read;

    const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

    fn model(zones: &[(i32, &str, i32, i32)]) -> AccountModel {
        let mut m = AccountModel {
            account: "Test Site".into(),
            ..Default::default()
        };
        for z in zones {
            m.zones.push(ZoneDto {
                n: z.0,
                name: z.1.into(),
                definition: z.2,
                area: z.3,
                ..Default::default()
            });
        }
        m
    }

    fn entry(xlsx: &[u8], name: &str) -> Document {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(xlsx)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        Document::parse(&s).unwrap()
    }

    struct Sheet {
        sheet: Document,
        book: Document,
    }

    impl Sheet {
        fn new(xlsx: &[u8]) -> Sheet {
            Sheet {
                sheet: entry(xlsx, "xl/worksheets/sheet1.xml"),
                book: entry(xlsx, "xl/workbook.xml"),
            }
        }
        fn cells(&self) -> HashMap<String, &Element> {
            let mut v = Vec::new();
            self.sheet.root.descendants(NS, "c", &mut v);
            v.into_iter()
                .map(|c| (c.attr("r").unwrap().to_string(), c))
                .collect()
        }
        fn rows(&self) -> Vec<&Element> {
            let mut v = Vec::new();
            self.sheet.root.descendants(NS, "row", &mut v);
            v
        }
    }

    fn text(c: &HashMap<String, &Element>, r: &str) -> String {
        match c.get(r) {
            None => String::new(),
            Some(e) if e.attr("t") == Some("inlineStr") => e.value(),
            Some(e) => e.child(NS, "v").map(|v| v.value()).unwrap_or_default(),
        }
    }

    fn row_of_zone(c: &HashMap<String, &Element>, zone: i32) -> usize {
        c.iter()
            .filter(|(k, v)| {
                k.starts_with('A')
                    && v.attr("t").is_none()
                    && v.child(NS, "v")
                        .is_some_and(|x| x.value() == zone.to_string())
                    && k[1..].parse::<usize>().unwrap() >= 7
            })
            .map(|(k, _)| k[1..].parse::<usize>().unwrap())
            .min()
            .unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
    }

    #[test]
    fn fills_name_definition_and_area_and_clears_the_sample_project() {
        let bytes = AlarmZoneList::build(
            &model(&[
                (1, "Smoke Detectors", 10, 1),
                (2, "Sump", 0, 1),
                (4, "Rec Room Water", 25, 2),
                (33, "Foyer Motion", 4, 1),
            ]),
            None,
            date(2026, 11, 19),
        )
        .unwrap();
        let s = Sheet::new(&bytes);
        let c = s.cells();
        assert_eq!(text(&c, "B1"), "Test Site");
        assert_eq!(text(&c, "B2"), "2026-11-19");
        assert_eq!(text(&c, "B7"), "Smoke Detectors");
        assert_eq!(text(&c, "E7"), "10");
        assert_eq!(text(&c, "F7"), "1");
        assert_eq!(text(&c, "B8"), "Sump");
        assert_eq!(text(&c, "E8"), "");
        assert_eq!(text(&c, "F8"), ""); // named but disabled: no type or area
        assert_eq!(text(&c, "E10"), "25");
        assert_eq!(text(&c, "F10"), "2");
        assert_eq!(text(&c, "D10"), ""); // power source was sample data
        assert_eq!(text(&c, "B11"), "");
        assert_eq!(text(&c, "C11"), ""); // zone 5 not in the account
        let r33 = row_of_zone(&c, 33);
        assert_eq!(text(&c, &format!("B{r33}")), "Foyer Motion");
        assert_eq!(text(&c, &format!("E{r33}")), "04");
        assert_eq!(text(&c, "F27"), "7"); // header fixed: shared string 7 = Area
        assert!(!s.sheet.to_string_with_decl().contains("Location: SMC"));
        let r = s
            .rows()
            .into_iter()
            .find(|x| x.attr("r") == Some("63"))
            .unwrap();
        assert_eq!(r.attr("hidden"), Some("1"));
        assert_eq!(text(&c, "B63"), "");
    }

    #[test]
    fn template_is_kept_intact_and_print_area_follows_the_last_row() {
        let bytes = AlarmZoneList::build(&model(&[(1, "A", 10, 1)]), None, None).unwrap();
        let names = |b: &[u8]| {
            let z = zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
            let mut n: Vec<String> = z.file_names().map(String::from).collect();
            n.sort();
            n
        };
        assert_eq!(names(AlarmZoneList::template_bytes()), names(&bytes));
        let s = Sheet::new(&bytes);
        assert!(s.book.root.value().contains("$A$1:$M$172"));
        let mut dim = Vec::new();
        s.sheet.root.descendants(NS, "dimension", &mut dim);
        assert_eq!(dim[0].attr("ref"), Some("A1:N172"));
        assert!(s
            .rows()
            .iter()
            .all(|r| r.attr("r").unwrap().parse::<i32>().unwrap() <= 172));
        let mut merges = Vec::new();
        s.sheet.root.descendants(NS, "mergeCell", &mut merges);
        for m in merges {
            let first = m.attr("ref").unwrap().split(':').next().unwrap();
            let n: i32 = first
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap();
            assert!(n <= 172);
        }
    }

    #[test]
    fn zones_above_128_add_expander_blocks() {
        let bytes = AlarmZoneList::build(
            &model(&[
                (130, "Shed Door", 3, 1),
                (150, "Barn Motion", 4, 1),
                (208, "Last", 3, 1),
            ]),
            None,
            None,
        )
        .unwrap();
        let s = Sheet::new(&bytes);
        let c = s.cells();
        assert_eq!(text(&c, &format!("B{}", row_of_zone(&c, 130))), "Shed Door");
        assert_eq!(text(&c, &format!("E{}", row_of_zone(&c, 130))), "03");
        assert_eq!(
            text(&c, &format!("B{}", row_of_zone(&c, 150))),
            "Barn Motion"
        );
        assert_eq!(text(&c, &format!("B{}", row_of_zone(&c, 208))), "Last");
        let nums: Vec<i32> = s
            .rows()
            .iter()
            .map(|r| r.attr("r").unwrap().parse().unwrap())
            .collect();
        let mut sorted = nums.clone();
        sorted.sort();
        assert_eq!(nums, sorted);
        sorted.dedup();
        assert_eq!(nums.len(), sorted.len());
        let mut brk = Vec::new();
        s.sheet.root.descendants(NS, "brk", &mut brk);
        let ids: Vec<i32> = brk
            .iter()
            .map(|b| b.attr("id").unwrap().parse().unwrap())
            .collect();
        let mut sid = ids.clone();
        sid.sort();
        assert_eq!(ids, sid);
        assert!(s
            .book
            .root
            .value()
            .contains(&format!("$M${}", nums.iter().max().unwrap())));
        for row in s.rows() {
            let cols: Vec<i32> = row
                .elements()
                .filter(|x| x.is(NS, "c"))
                .map(|x| {
                    x.attr("r")
                        .unwrap()
                        .chars()
                        .take_while(|ch| ch.is_alphabetic())
                        .fold(0, |a, ch| a * 26 + ch as i32 - 'A' as i32 + 1)
                })
                .collect();
            let mut sc = cols.clone();
            sc.sort();
            assert_eq!(cols, sc);
        }
    }

    #[test]
    fn builds_from_a_real_export_when_samples_exist() {
        let Some(text_) = sample("ElkRPAcctSample Export") else {
            return;
        };
        let m = AccountModel::from(&AccountFile::parse(&text_).unwrap()).unwrap();
        let bytes = AlarmZoneList::build(&m, None, None).unwrap();
        let s = Sheet::new(&bytes);
        let c = s.cells();
        for z in m.zones.iter().filter(|z| z.n <= 128 && z.definition != 0) {
            assert_eq!(
                text(&c, &format!("E{}", row_of_zone(&c, z.n))),
                format!("{:02}", z.definition)
            );
        }
    }
}
