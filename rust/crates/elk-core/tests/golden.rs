//! Golden checks: expected values were produced by running the C# `Elk.Protocol` (files under tests/golden).

use elk_core::*;

fn golden(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn line_with<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines().find(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("no golden line {prefix}")).strip_prefix(prefix).unwrap().trim()
}

fn unhex(s: &str) -> Vec<u8> {
    hex_decode(s).unwrap()
}

#[test]
fn crc_and_frames_match_csharp() {
    let g = golden("misc.txt");
    for l in g.lines().filter(|l| l.starts_with("crc ")) {
        let p: Vec<&str> = l.split(' ').collect();
        let data = unhex(p[1]);
        assert_eq!(p[2].parse::<u16>().unwrap(), Crc16::compute(&data), "{l}");
    }
    assert_eq!(unhex(line_with(&g, "frame ")), Framing::encode(&[5, 0x10, 0, 0x10, 0x10, 9]));
    assert_eq!(unhex(line_with(&g, "frame0 ")), Framing::encode(&[]));
}

#[test]
fn aes_keys_and_ciphertext_match_csharp() {
    let g = golden("misc.txt");
    for l in g.lines().filter(|l| l.starts_with("key ")) {
        let p: Vec<&str> = l.split(' ').collect();
        assert_eq!(unhex(p[3]), ElkCrypto::derive_key(p[1], p[2]).unwrap(), "{l}");
    }
    assert_eq!(unhex(line_with(&g, "key2 ")), ElkCrypto::derive_key("00:40:9D:00:00:04", "testphrase").unwrap());
    let framed = Framing::encode(&[5, 0, 0, 7]);
    for l in g.lines().filter(|l| l.starts_with("enc ")) {
        let p: Vec<&str> = l.split(' ').collect();
        let key = ElkCrypto::derive_key("0A1B2C3D4E5F", p[1]).unwrap();
        assert_eq!(unhex(p[2]), ElkCrypto::encrypt(&key, &framed).unwrap(), "{l}");
    }
}

#[test]
fn identity_block_and_connect_match_csharp() {
    let g = golden("misc.txt");
    let mut n = 0;
    let mut rng = || {
        n += 1;
        if n == 1 {
            7
        } else {
            200
        }
    };
    assert_eq!(unhex(line_with(&g, "ident ")), Handshake::identity_block("246801", "31415926", &mut rng).unwrap());
    let mut n2 = 0;
    let mut rng2 = || {
        n2 += 1;
        if n2 == 1 {
            7
        } else {
            200
        }
    };
    assert_eq!(unhex(line_with(&g, "connect49 ")), Handshake::build_connect("246801", ConnectMode::Tls, Some("00000001"), Some(&mut rng2)).unwrap());
    assert_eq!(unhex(line_with(&g, "connect ")), Handshake::build_connect("246801", ConnectMode::Aes, None, None).unwrap());
}

#[test]
fn name_field_behaviour_matches_csharp() {
    let g = golden("misc.txt");
    let mut z = ZoneRecord::new();
    z.set_name("Front Door");
    assert_eq!(unhex(line_with(&g, "zname1 ")), z.bytes);
    z.set_name("A very long name that is cut");
    let l = line_with(&g, "zname2 ");
    assert_eq!(unhex(l.split(' ').next().unwrap()), z.bytes);
    assert_eq!("A very long name", z.name());
    z.set_name("caf\u{e9}\u{1F600}x");
    assert_eq!(unhex(line_with(&g, "zname3 ").split(' ').next().unwrap()), z.bytes);
    let mut a = AreaRecord::new();
    a.bytes[..16].copy_from_slice(b"Area 1          ");
    a.set_name("Hall");
    assert_eq!(unhex(line_with(&g, "aname ")), a.bytes);
}

#[test]
fn default_records_match_csharp_for_every_zone() {
    for l in golden("default_zones.txt").lines() {
        let (n, h) = l.split_once(' ').unwrap();
        assert_eq!(h.trim(), hex_upper(&DefaultRecords::zone(n.parse().unwrap())), "zone {n}");
    }
    let m = golden("default_misc.txt");
    for l in m.lines() {
        let p: Vec<&str> = l.split(' ').collect();
        let got = match p[0] {
            "user" => hex_upper(&DefaultRecords::user(p[1].parse().unwrap())),
            "area" => hex_upper(&DefaultRecords::area(p[1].parse().unwrap())),
            "keypad" => hex_upper(&DefaultRecords::keypad()),
            "phone" => hex_upper(&DefaultRecords::phone()),
            "arc" => hex_upper(&DefaultRecords::area_rc()),
            "urc" => hex_upper(&DefaultRecords::user_rc()),
            "zrc" => hex_upper(&DefaultRecords::zone_rc()),
            other => panic!("{other}"),
        };
        assert_eq!(p.last().unwrap().trim(), got, "{l}");
    }
}

#[test]
fn live_decoding_matches_csharp() {
    let g = golden("live.txt");
    let lines: Vec<&str> = g.lines().collect();
    // areas
    let ra = unhex(lines[0].strip_prefix("areas ").unwrap());
    for (i, a) in PanelLive::decode_areas(&ra).iter().enumerate() {
        let want = format!("{} {} {} {} {} {}", a.area, a.ready, a.ready_to_force_arm, a.armed, a.armed_name.to_lowercase(), a.alarm);
        assert_eq!(lines[1 + i], want);
    }
    // zones
    let zl = lines[9].strip_prefix("zones ").unwrap();
    let (zs, vs) = zl.split_once(' ').unwrap();
    let zones = PanelLive::decode_zones(&unhex(zs), Some(&unhex(vs)));
    assert_eq!(208, zones.len());
    for (i, z) in zones.iter().enumerate() {
        let want = format!("{}|{}|{}|{}|{}|{}", z.zone, z.state, z.bypassed, z.force_armed, z.physical, z.volts);
        assert_eq!(lines[10 + i], want);
    }
    let after = 10 + 208;
    let sy = unhex(lines[after].strip_prefix("sys ").unwrap());
    assert_eq!(lines[after + 1], PanelLive::decode_system(&sy).join("|"));
}

#[test]
fn modules_decoding_matches_csharp() {
    let g = golden("modules.txt");
    for l in g.lines() {
        let p: Vec<&str> = l.splitn(2, ' ').collect();
        match p[0] {
            "enr" => {
                let (r, want) = p[1].split_once(" => ").unwrap();
                let got: Vec<String> = PanelModules::decode_enrolled(&unhex(r)).iter().map(|x| format!("{},{},{}", x.0, x.1, x.2)).collect();
                assert_eq!(want, got.join("|"));
            }
            "model" => {
                let w: Vec<&str> = p[1].split(' ').collect();
                // kind may contain spaces: the last two tokens are hardware and model
                let model = w[w.len() - 1];
                let hw: i32 = w[w.len() - 2].parse().unwrap();
                let kind = w[..w.len() - 2].join(" ");
                assert_eq!(model, PanelModules::model_for(&kind, hw), "{l}");
            }
            "ver" => {
                let (r, want) = p[1].split_once(' ').unwrap();
                let m = PanelModules::decode_versions("Keypad", 3, Some(&unhex(r)));
                let got = format!("{}|{}|{}|{}|{}|{}|{}", m.kind, m.address, m.model, m.hardware, m.boot, m.firmware, m.responding).to_lowercase();
                assert_eq!(want, got);
            }
            _ => panic!("{l}"),
        }
    }
}

#[test]
fn clock_and_access_requests_match_csharp() {
    use chrono::NaiveDate;
    let g = golden("clocklog.txt");
    let t = NaiveDate::from_ymd_opt(2099, 12, 31).unwrap().and_hms_opt(23, 59, 59).unwrap();
    assert_eq!(unhex(line_with(&g, "set ")), PanelClock::set_request(t));
    assert_eq!(unhex(line_with(&g, "acc ")), PanelAccess::change_request("987650").unwrap());
}

fn sample_file() -> PanelFile {
    PanelFile {
        firmware: Some("5.3.44".into()),
        serial: Some("00000001".into()),
        zones: vec![
            ZoneJson { number: 1, name: "Zone \"q\" 'a' <b> & + `t` \\ / \u{e9} \u{1F600} \t".into(), function: 2, flag1: 17, flag2: 0, hex: hex_upper(&DefaultRecords::zone(1)) },
            ZoneJson { number: 2, name: "Zone 002".into(), function: 1, flag1: 17, flag2: 48, hex: hex_upper(&DefaultRecords::zone(2)) },
        ],
        areas: vec![AreaJson {
            number: 1,
            name: "Area 1".into(),
            exit1: 60,
            entrance1: 30,
            exit2: 60,
            entrance2: 30,
            flags1: 97,
            flags2: 80,
            arm_time_window: 15,
            hex: hex_upper(&DefaultRecords::area(1)),
        }],
        keypads: vec![KeypadJson { number: 1, name: "Keypad 1".into(), area: 1, hex: hex_upper(&DefaultRecords::keypad()) }],
        blocks: vec![BlockJson { section: 1, item: 1, hex: hex_upper(&DefaultRecords::user(1)), part: 0 }, BlockJson { section: 15, item: 1, hex: "0102".into(), part: 2 }],
        automation: vec![],
    }
}

fn native(s: &str) -> String {
    // the golden files were written on Windows (\r\n, as .NET's Environment.NewLine)
    let n = s.replace("\r\n", "\n");
    if cfg!(windows) {
        n.replace('\n', "\r\n")
    } else {
        n
    }
}

#[test]
fn panel_file_json_is_byte_identical_to_csharp() {
    assert_eq!(native(&golden("panelfile.json")), sample_file().to_json());
    assert_eq!(native(&golden("panelfile_empty.json")), PanelFile::default().to_json());
}

#[test]
fn panel_file_json_round_trips() {
    let f = sample_file();
    assert_eq!(f, PanelFile::from_json(&f.to_json()).unwrap());
    assert_eq!(PanelFile::default(), PanelFile::from_json(&PanelFile::default().to_json()).unwrap());
    assert!(PanelFile::from_json("not json").is_err());
    assert!(PanelFile::from_json("null").is_err());
}
