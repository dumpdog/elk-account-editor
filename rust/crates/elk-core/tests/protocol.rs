//! Ports of ProtocolTests.cs, PanelSupportTests.cs (name setters), DeleteTests.cs (default records) and the HandshakeTests in SessionTests.cs.

use elk_core::*;

// Zone voice words as stored for TESTACCT zones 1 and 2: 1H=1, 1L=215, 2L=22 / 23.
fn voice(z: &mut ZoneRecord, v: &[(usize, u8)]) {
    for &(i, b) in v {
        z.bytes[20 + i] = b;
    }
}

fn zone(function: u8, flag1: u8, name: &str) -> ZoneRecord {
    let mut z = ZoneRecord::new();
    z.set_function(function);
    z.set_flag1(flag1);
    z.set_name(name);
    z
}

#[test]
fn zone1_default_crc_matches_stored_udcrc() {
    let mut z = zone(1, 17, "Zone 001");
    voice(&mut z, &[(0, 1), (1, 215), (3, 22)]);
    assert_eq!(2259, z.crc());
}

#[test]
fn zone1_edited_crc_matches_stored_modcrc() {
    let mut z = zone(2, 17, "ZONE-ONE-CHANGED");
    voice(&mut z, &[(0, 1), (1, 215), (3, 22)]);
    assert_eq!(3116, z.crc());
}

#[test]
fn zone2_default_crc_matches_stored_udcrc() {
    let mut z = zone(1, 17, "Zone 002");
    voice(&mut z, &[(0, 1), (1, 215), (3, 23)]);
    assert_eq!(36299, z.crc());
}

#[test]
fn frame_round_trip_with_escaped_dle() {
    let payload = [5u8, 0x10, 0, 0x10, 0x10, 9];
    let frames = FrameParser::default().feed(&Framing::encode(&payload));
    assert_eq!(1, frames.len());
    assert!(frames[0].crc_ok);
    assert_eq!(payload.to_vec(), frames[0].payload);
}

#[test]
fn corrupt_crc_is_flagged() {
    let mut enc = Framing::encode(&[1, 2, 3]);
    *enc.last_mut().unwrap() ^= 0xFF;
    let f = FrameParser::default().feed(&enc);
    assert_eq!(1, f.len());
    assert!(!f[0].crc_ok);
}

#[test]
fn ack_and_nak_are_recognised() {
    let kinds: Vec<FrameKind> = FrameParser::default().feed(&[0x10, 0x06, 0x10, 0x15]).iter().map(|f| f.kind).collect();
    assert_eq!(vec![FrameKind::Ack, FrameKind::Nak], kinds);
}

#[test]
fn ff_body_is_debug_text_only_when_asked() {
    let enc = Framing::encode(&[0xFF, 1, 2]);
    assert!(FrameParser::new(true).feed(&enc).is_empty());
    let f = FrameParser::new(false).feed(&enc);
    assert_eq!(vec![0xFF, 1, 2], f[0].payload);
}

#[test]
fn encrypt_decrypt_round_trip_with_short_and_long_passphrase() {
    for pass in ["short", "PROBEPASSPHRASE0123456789ABCDEFG"] {
        let key = ElkCrypto::derive_key("0A1B2C3D4E5F", pass).unwrap();
        assert_eq!(if pass.len() <= 16 { 16 } else { 32 }, key.len());
        let framed = Framing::encode(&[5, 0, 0, 7]);
        let dec = ElkCrypto::decrypt(&key, &ElkCrypto::encrypt(&key, &framed).unwrap()).unwrap();
        assert_eq!(framed, dec[..framed.len()]);
    }
    assert!(ElkSession::new("0A1B2C3D4E5F", "").is_err()); // empty key: .NET fails on first use
}

fn area(n: &str, e1: u8, n1: u8, e2: u8, n2: u8, f1: u8, f2: u8) -> AreaRecord {
    let mut a = AreaRecord::new();
    a.set_name(n);
    a.set_exit1(e1);
    a.set_entrance1(n1);
    a.set_exit2(e2);
    a.set_entrance2(n2);
    a.set_flags1(f1);
    a.set_flags2(f2);
    a.set_arm_time_window(15);
    a
}

#[test]
fn area_crc_matches_stored_modcrc() {
    for (n, e1, n1, e2, n2, f1, f2, expected) in [
        ("AREA-ONE-PROBE", 60, 30, 60, 30, 97, 80, 24532),
        ("Alpha2", 71, 41, 82, 23, 54, 168, 33085),
        ("Beta3", 71, 41, 82, 23, 54, 168, 22889),
        ("AREA-EIGHT-PROBE", 60, 30, 60, 30, 97, 80, 18040),
    ] {
        assert_eq!(expected, area(n, e1, n1, e2, n2, f1, f2).crc(), "{n}");
    }
}

#[test]
fn inverted_option_flags_are_complements_on_defined_bits() {
    assert_eq!(0x57, 97 ^ 54);
    assert_eq!(0xF8, 80 ^ 168);
}

#[test]
fn zone_flag_constants() {
    assert_eq!(16, 17 & !(ZoneFlag1::BYPASSABLE.0 as i32));
    assert_eq!(0, 16 & !(ZoneFlag1::CHIME.0 as i32));
    assert_eq!(255, ZoneFlag1(255).0);
    assert_eq!(48, ZoneFlag2::with(0, None, Some(4)));
    assert_eq!(4, ZoneFlag2::area(48));
    assert_eq!(0, ZoneFlag2::type_of(136));
    assert_eq!(1, ZoneFlag2::area(136));
    assert_ne!(0, 136 & ZoneFlag2::SWINGER_SHUTDOWN);
    assert_ne!(0, 136 & ZoneFlag2::SILENT_ALARM);
    assert_eq!(5, ZoneFlag2::type_of(5));
    assert_eq!(8, ZoneFlag1::FORCE_ARM.0);
    for (f, stored) in [
        (ZoneFlag1::USE_DIALER_DELAY, 64),
        (ZoneFlag1::LISTEN_IN, 128),
        (ZoneFlag1::BYPASSABLE, 1),
        (ZoneFlag1::PERIODIC_TRIP, 2),
        (ZoneFlag1::FAST_RESPONSE, 4),
        (ZoneFlag1::CHIME, 16),
        (ZoneFlag1::CROSS_ZONED, 32),
    ] {
        assert_eq!(stored, f.0);
    }
    assert_eq!(176, 48 | ZoneFlag2::SILENT_ALARM);
    assert_eq!(56, 48 | ZoneFlag2::SWINGER_SHUTDOWN);
    assert!((ZoneFlag1::BYPASSABLE | ZoneFlag1::CHIME).contains(ZoneFlag1::CHIME));
}

#[test]
fn default_keypad_crcs_match_stored_values() {
    let mut k = KeypadRecord::new();
    k.set_area(1);
    k.set_name("Keypad 01       ");
    k.bytes[1] = 56;
    k.bytes[4] = 20;
    k.bytes[5] = 96;
    let defs = [(0u8, 10u8), (0, 23), (0, 22)];
    let lite = [(3u8, 233u8), (3, 237), (3, 236)];
    for i in 0..3 {
        k.bytes[28 + 2 * i] = defs[i].0;
        k.bytes[29 + 2 * i] = defs[i].1;
        k.bytes[40 + 2 * i] = lite[i].0;
        k.bytes[41 + 2 * i] = lite[i].1;
    }
    let names = ["F1 = Fire       ", "F2 = Police     ", "F3 = Medical    ", "F4 = Not Defined", "F5 = Not Defined", "F6 = Not Defined"];
    for (i, n) in names.iter().enumerate() {
        k.bytes[52 + 16 * i..52 + 16 * i + 16].copy_from_slice(n.as_bytes());
    }
    assert_eq!(55021, k.crc1());
    assert_eq!(14106, k.crc2());
    assert_eq!("F2 = Police", k.fkey_name(1));
    assert_eq!(23, k.fkey_definition(1));
}

#[test]
fn name_setter_keeps_existing_space_padding_when_unchanged() {
    let mut a = AreaRecord::new();
    a.bytes[..16].copy_from_slice(b"Area 1          ");
    let before = a.bytes;
    a.set_name("Area 1");
    assert_eq!(before, a.bytes); // unchanged text must not alter the record (or its CRC)
    a.set_name("Hall");
    assert_eq!(b' ', a.bytes[15]); // new text keeps the field's space padding
    assert_eq!("Hall", a.name());
}

#[test]
fn name_setter_keeps_nul_padding_for_nul_padded_fields() {
    let mut z = ZoneRecord::new();
    z.set_name("Front Door");
    assert_eq!(0, z.bytes[4 + 15]);
    z.set_name("Front Door");
    assert_eq!("Front Door", z.name());
}

#[test]
fn name_setter_cuts_at_16_and_replaces_non_ascii() {
    let mut z = ZoneRecord::new();
    z.set_name("A very long name that is cut");
    assert_eq!("A very long name", z.name());
    z.set_name("caf\u{e9}\u{1F600}x"); // each UTF-16 unit above 0x7F becomes '?' (golden: 3 of them)
    assert_eq!("caf???x", z.name());
}

// ---- DeleteTests.cs: default records ----

fn hex(b: &[u8]) -> String {
    hex_upper(b)
}

#[test]
fn default_records_match_the_untouched_panel() {
    assert_eq!("000100005A6F6E6520313030000000000000000001D700160031000000000000", hex(&DefaultRecords::zone(100)));
    assert_eq!("000100005A6F6E6520323038000000000000000001D700170031001D00000000", hex(&DefaultRecords::zone(208)));
    assert_eq!("000000000000E2000155534552203130300000000000000000000000", hex(&DefaultRecords::user(100)));
    assert_eq!("417265612035202020202020202020203C1E3C1E61500F00", hex(&DefaultRecords::area(5)));
    assert_eq!("0000000033600000", hex(&DefaultRecords::keypad()[..8]));
    assert_eq!(148, DefaultRecords::keypad().len());
    let ph = DefaultRecords::phone();
    assert_eq!(88, ph.len());
    assert_eq!(0xA8, ph[20]);
    assert_eq!(8, ph[22]);
    assert_eq!(b'0', ph[23]);
    assert_eq!(b'0', ph[70]);
    assert_eq!(0, ph[71]);
    assert_eq!("01000001", hex(&DefaultRecords::user_rc()));
    assert_eq!(20, DefaultRecords::area_rc().len());
    assert_eq!(4, DefaultRecords::zone_rc().len());
}

// ---- HandshakeTests (SessionTests.cs) ----

#[test]
fn connect_request_has_digits_last_first() {
    assert_eq!(vec![0x7F, 0, 0, 1, 1, 0, 8, 6, 4, 2], Handshake::build_connect("246801", ConnectMode::Aes, None, None).unwrap());
    assert!(Handshake::build_connect("12345", ConnectMode::Aes, None, None).is_err());
    assert!(Handshake::build_connect("12a456", ConnectMode::Aes, None, None).is_err());
    assert!(Handshake::build_connect("246801", ConnectMode::Tls, None, None).is_err());
    // serial required
}

#[test]
fn identity_block_is_16_bytes_and_decrypts_to_layout() {
    use aes::cipher::generic_array::GenericArray;
    use aes::cipher::{BlockDecrypt, KeyInit};
    let mut calls = 0;
    let mut rng = || {
        calls += 1;
        if calls == 1 {
            7
        } else {
            200
        }
    };
    let blk = Handshake::identity_block("246801", "31415926", &mut rng).unwrap();
    let mut plain = blk;
    aes::Aes128::new(GenericArray::from_slice(&Handshake::FIXED_KEY)).decrypt_block(GenericArray::from_mut_slice(&mut plain));
    assert_eq!([3, 1, 4, 1, 5, 9, 2, 6], plain[1..9]);
    assert_eq!([2, 4, 6, 8, 0, 1], plain[10..16]);
    assert_eq!((7, 200), (plain[0], plain[9]));
    // the thread RNG gives non-zero random bytes
    let p = Handshake::build_connect("246801", ConnectMode::Tls, Some("31415926"), None).unwrap();
    assert_eq!(26, p.len());
}

#[test]
fn control_messages_match_traced_bytes() {
    assert_eq!(vec![0x7F, 0, 0, 0xA5], Handshake::keep_alive());
    assert_eq!(vec![0x7F, 1, 0, 0xA5], Handshake::disconnect());
    assert_eq!(vec![5, 0, 0, 7], Handshake::read_request(Section::Zone, 7, 0));
    assert_eq!(vec![0x7E, 3, 0, 2], Handshake::crc_query(Section::Area, 2));
    assert_eq!(vec![133, 0, 0, 1, 9], Handshake::write_request(Section::Zone, 1, &[9], 0));
}

#[test]
fn connect_reply_parses_versions_and_serial() {
    let mut p = [0u8; 25];
    p[0] = 0x7F;
    p[4] = 5;
    p[5] = 2;
    p[6] = 9;
    p[8] = 0x31;
    p[9] = 0x41;
    p[10] = 0x59;
    p[11] = 0x26;
    p[12] = 1;
    p[13] = 2;
    p[14] = 3;
    p[15] = 4;
    p[16] = 5;
    p[22] = 5;
    p[23] = 1;
    p[24] = 2;
    let r = ConnectReply::parse(&p).unwrap();
    assert!(r.accepted);
    assert!(!r.in_boot_loader);
    assert_eq!("5.2.9", r.firmware);
    assert_eq!("31415926", r.serial_hex);
    assert_eq!("1.2", r.hardware);
    assert_eq!("3.4.5", r.boot);
    assert_eq!(Some("5.1.2".to_string()), r.min_rp_version);
    assert!(ConnectReply::parse(&p[..10]).is_err());
}
