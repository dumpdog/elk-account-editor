//! M1XEP setup: block layout, decode, change encoding, the read / write conversation against the simulated module, discovery.
//! All data is made up; nothing here talks to a real module.

use elk_core::simulated_panel::SimulatedPanel;
use elk_core::xep_config::*;
use elk_core::ElkSession;
use std::net::UdpSocket;
use std::time::Duration;

fn put(b: &mut [u8], at: usize, len: usize, s: &str) {
    for (i, slot) in b[at..at + len].iter_mut().enumerate() {
        *slot = s.as_bytes().get(i).copied().unwrap_or(0);
    }
}

/// A module configured the way a small installation might be, at the offsets ElkRP's `Show_Item` reads.
fn sample() -> XepBlocks {
    let mut b = XepBlocks::blank();
    put(&mut b.xep1, 0, 16, "Test XEP");
    b.xep1[16..20].copy_from_slice(&[192, 168, 7, 20]);
    b.xep1[20..24].copy_from_slice(&[255, 255, 255, 0]);
    b.xep1[24..28].copy_from_slice(&[192, 168, 7, 1]);
    b.xep1[28..30].copy_from_slice(&2101u16.to_be_bytes());
    b.xep1[30..32].copy_from_slice(&2601u16.to_be_bytes());
    b.xep1[32..36].copy_from_slice(&[192, 168, 7, 1]);
    b.xep1[36..40].copy_from_slice(&[8, 8, 8, 8]);
    b.xep1[40] = 0b0000_0100; // static, discovery on
    put(&mut b.xep2, 0, 16, "installer");
    put(&mut b.xep2, 16, 16, "pw-one");
    put(&mut b.xep2, 32, 16, "second");
    put(&mut b.xep2, 48, 16, "pw-two");
    put(&mut b.xep2, 256, 48, "members.example.test");
    put(&mut b.xep2, 448, 48, "smtp.example.test");
    b.xep2[496..498].copy_from_slice(&587u16.to_be_bytes());
    put(&mut b.xep2, 500, 48, "alarm@example.test");
    put(&mut b.xep2, 548, 48, "mailuser");
    put(&mut b.xep2, 596, 48, "mailsecret");
    put(&mut b.xep4, 104, 48, "errors@example.test");
    put(&mut b.xep4, 156, 48, "time.example.test");
    b.xep4[206] = 3;
    b.xep4[207] = 30;
    b.xep4[208] = 1;
    b.xep4[209] = 32; // (32 - 52) / 4 = -5 hours
    put(&mut b.xep5, 0, 48, "10.0.0.9");
    b.xep5[48..50].copy_from_slice(&4999u16.to_be_bytes());
    b.xep5[50] = 1;
    b.xep5[51] = 2;
    put(&mut b.xep5, 52, 16, "Kitchen");
    put(&mut b.xep5, 340, 16, "Radio");
    // central station 2: Sur-Gard, address, port, account 1234, supervised, heartbeat 30
    put(&mut b.xep6, 48, 48, "cs.example.test");
    b.xep6[385] = 1;
    b.xep6[392 + 5 + 3] = 0x12;
    b.xep6[392 + 5 + 4] = 0x34;
    b.xep6[561] = 0b10;
    b.xep4[32 + 2..32 + 4].copy_from_slice(&7700u16.to_be_bytes());
    b.xep4[97] = 30;
    let mut e = vec![0u8; EMAIL_LEN];
    put(&mut e, 0, 48, "owner@example.test");
    put(&mut e, 48, 255, "The alarm went off");
    b.emails[1] = Some(e);
    b
}

#[test]
fn lines_round_trip_and_reject_wrong_shapes() {
    let b = sample();
    let lines = b.to_lines();
    assert_eq!(lines.len(), FILE_FIXED_LINES + EMAIL_LEN + 1);
    assert_eq!(lines[FILE_FIXED_LINES - 1], "1", "the number of emails");
    assert_eq!(lines[FILE_FIXED_LINES], "2", "the slot number is 1-based");
    assert_eq!(XepBlocks::from_lines(&lines), Some(b.clone()));
    assert_eq!(XepBlocks::from_lines(&XepBlocks::blank().to_lines()), Some(XepBlocks::blank()));
    // a line too many, a missing line, a value above 255, a wrong email count
    let mut more = lines.clone();
    more.push("0".into());
    assert_eq!(XepBlocks::from_lines(&more), None);
    assert_eq!(XepBlocks::from_lines(&lines[..lines.len() - 1]), None);
    let mut big = lines.clone();
    big[5] = "256".into();
    assert_eq!(XepBlocks::from_lines(&big), None);
    let mut count = lines.clone();
    count[FILE_FIXED_LINES - 1] = "2".into();
    assert_eq!(XepBlocks::from_lines(&count), None);
}

#[test]
fn hex_round_trip_checks_sizes() {
    let b = sample();
    let h = b.to_hex();
    assert_eq!(XepBlocks::from_hex(&h).unwrap(), b);
    let mut bad = h.clone();
    bad.xep1.push_str("00");
    assert!(XepBlocks::from_hex(&bad).is_err());
    let mut bad = h;
    bad.emails.pop();
    assert!(XepBlocks::from_hex(&bad).is_err());
}

#[test]
fn decode_reads_the_fields_at_elkrps_offsets() {
    let s = XepSettings::decode(&sample());
    assert_eq!(s.name, "Test XEP");
    assert!(!s.dhcp && s.discovery && !s.dns_auto && !s.no_passwords && !s.m1_cloud);
    assert_eq!((s.ip.as_str(), s.mask.as_str(), s.gateway.as_str()), ("192.168.7.20", "255.255.255.0", "192.168.7.1"));
    assert_eq!((s.non_secure_port, s.secure_port), (2101, 2601));
    assert_eq!((s.dns1.as_str(), s.dns2.as_str()), ("192.168.7.1", "8.8.8.8"));
    assert_eq!(s.logins.len(), 8);
    assert_eq!((s.logins[0].user.as_str(), s.logins[0].password.as_str()), ("installer", "pw-one"));
    assert_eq!(s.logins[1].user, "second");
    assert_eq!((s.smtp_server.as_str(), s.smtp_port, s.mail_from.as_str()), ("smtp.example.test", 587, "alarm@example.test"));
    assert_eq!((s.mail_user.as_str(), s.mail_password.as_str()), ("mailuser", "mailsecret"));
    assert_eq!(s.emails.len(), 16);
    assert_eq!((s.emails[1].slot, s.emails[1].address.as_str(), s.emails[1].message.as_str()), (2, "owner@example.test", "The alarm went off"));
    assert_eq!(s.emails[0].address, "");
    assert_eq!((s.ddns_url.as_str(), s.ddns_error_email.as_str()), ("members.example.test", "errors@example.test"));
    assert_eq!((s.time_server.as_str(), s.time_hour, s.time_minute, s.time_notify_errors, s.time_zone), ("time.example.test", 3, 30, true, -5.0));
    assert_eq!((s.audio_ip.as_str(), s.audio_port, s.audio_tcp, s.audio_model), ("10.0.0.9", 4999, true, 2));
    assert_eq!((s.audio_zone_names[0].as_str(), s.audio_source_names[0].as_str()), ("Kitchen", "Radio"));
    let cs = &s.reports[1];
    assert_eq!((cs.cs, cs.receiver_type, cs.receiver_name.as_str(), cs.address.as_str(), cs.port), (2, 1, "DSC Sur-Gard", "cs.example.test", 7700));
    assert_eq!((cs.account.as_str(), cs.supervise, cs.heartbeat_seconds, cs.encrypt), ("1234", true, 30, false));
    assert_eq!(s.reports[0].account, "");
    // ElkRP shows 2601 / 25 when the module holds 0
    let blank = XepSettings::decode(&XepBlocks::blank());
    assert_eq!((blank.secure_port, blank.smtp_port), (2601, 25));
}

/// Offsets of the bytes that differ, per block.
fn diff(a: &XepBlocks, b: &XepBlocks) -> Vec<(&'static str, usize)> {
    let mut v = Vec::new();
    for (n, x, y) in [("1", &a.xep1, &b.xep1), ("2", &a.xep2, &b.xep2), ("4", &a.xep4, &b.xep4), ("5", &a.xep5, &b.xep5), ("6", &a.xep6, &b.xep6)] {
        for i in 0..x.len() {
            if x[i] != y[i] {
                v.push((n, i));
            }
        }
    }
    v
}

#[test]
fn unchanged_settings_change_nothing_and_unknown_bytes_survive() {
    let mut b = sample();
    b.xep1[41] = 0x5A; // a byte this code has no field for
    b.xep2[498] = 7;
    let s = XepSettings::decode(&b);
    let (out, changes) = XepSettings::apply(&b, &s, &s).unwrap();
    assert!(changes.is_empty());
    assert_eq!(out, b);
    // a change elsewhere leaves them alone
    let mut n = s.clone();
    n.name = "Renamed".into();
    let (out, changes) = XepSettings::apply(&b, &s, &n).unwrap();
    assert_eq!(out.xep1[41], 0x5A);
    assert_eq!(out.xep2[498], 7);
    assert!(diff(&b, &out).iter().all(|d| d.0 == "1" && d.1 < 16));
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].tab.as_str(), changes[0].field.as_str(), changes[0].identity), ("TCP/IP Settings", "Device Name", false));
}

#[test]
fn each_change_touches_exactly_its_bytes() {
    let b = sample();
    let s = XepSettings::decode(&b);
    let one = |f: &dyn Fn(&mut XepSettings)| {
        let mut n = s.clone();
        f(&mut n);
        XepSettings::apply(&b, &s, &n).unwrap()
    };
    let (o, c) = one(&|n| n.ip = "192.168.7.21".into());
    assert_eq!(diff(&b, &o), vec![("1", 19)]);
    assert!(c[0].identity);
    let (o, _) = one(&|n| n.non_secure_port = 0);
    assert_eq!(diff(&b, &o), vec![("1", 28), ("1", 29)]);
    let (o, _) = one(&|n| n.dhcp = true);
    assert_eq!(diff(&b, &o), vec![("1", 40)]);
    assert_eq!(o.xep1[40], 0b0000_0101);
    let (o, c) = one(&|n| n.logins[2].password = "abc".into());
    assert_eq!(diff(&b, &o).first(), Some(&("2", 80)));
    assert_eq!((c[0].field.as_str(), c[0].new.as_str(), c[0].identity), ("Password 3", "***", true), "passwords are masked in the diff");
    let (o, c) = one(&|n| n.smtp_server = "mail.example.test".into());
    assert!(diff(&b, &o).iter().all(|d| d.0 == "2" && (448..496).contains(&d.1)));
    assert!(!c[0].identity);
    let (o, _) = one(&|n| n.smtp_port = 25);
    assert_eq!(diff(&b, &o), vec![("2", 496), ("2", 497)]);
    let (o, _) = one(&|n| n.time_hour = 4);
    assert_eq!(diff(&b, &o), vec![("4", 206)]);
    let (o, _) = one(&|n| n.time_notify_errors = false);
    assert_eq!(diff(&b, &o), vec![("4", 208)]);
    let (o, _) = one(&|n| n.audio_ip = "audio.example.test".into());
    assert!(diff(&b, &o).iter().all(|d| d.0 == "5" && (d.1 < 48 || d.1 == 50)));
    assert_eq!(o.xep5[50], 0b11, "a host name sets the URL bit and keeps the TCP bit");
    let (o, _) = one(&|n| n.audio_zone_names[1] = "Den".into());
    assert!(diff(&b, &o).iter().all(|d| d.0 == "5" && (68..84).contains(&d.1)));
    let (o, _) = one(&|n| n.emails[0] = EmailItem { slot: 1, address: "a@b.test".into(), message: "hi".into() });
    assert_eq!(diff(&b, &o), vec![]);
    assert_eq!(&o.emails[0].as_ref().unwrap()[..8], b"a@b.test");
    let (o, _) = one(&|n| n.emails[1] = EmailItem { slot: 2, ..Default::default() });
    assert_eq!(o.emails[1], None, "an empty address removes the email");
}

#[test]
fn refusals() {
    let b = sample();
    let s = XepSettings::decode(&b);
    let bad = |f: &dyn Fn(&mut XepSettings)| {
        let mut n = s.clone();
        f(&mut n);
        XepSettings::apply(&b, &s, &n).unwrap_err().to_string()
    };
    assert!(bad(&|n| n.mask = "255.0.255.0".into()).contains("Subnet Mask"));
    assert!(bad(&|n| n.mask = "0.0.0.0".into()).contains("Subnet Mask"));
    assert!(bad(&|n| n.mask = "255.255.255.255".into()).contains("Subnet Mask"));
    assert!(bad(&|n| n.gateway = "10.0.0.1".into()).contains("same subnet"));
    assert!(bad(&|n| n.gateway = "0.0.0.0".into()).contains("Default Gateway"));
    assert!(bad(&|n| n.ip = "192.168.7".into()).contains("IP Address"));
    assert!(bad(&|n| n.secure_port = 0).contains("Secure Port"));
    assert!(bad(&|n| n.secure_port = 70000).contains("Secure Port"));
    assert!(bad(&|n| n.non_secure_port = 2601).contains("same number"));
    assert!(bad(&|n| n.name = "x".repeat(17)).contains("at most 16"));
    assert!(bad(&|n| n.name = "caf\u{e9}".into()).contains("printable ASCII"));
    assert!(bad(&|n| n.time_hour = 24).contains("hour"));
    assert!(bad(&|n| n.emails[3] = EmailItem { slot: 4, address: String::new(), message: "orphan".into() }).contains("needs an email address"));
    assert!(bad(&|n| n.reports[0].address = "x".into()).contains("read only"));
    assert!(bad(&|n| n.m1_cloud = true).contains("read only"));
    assert!(bad(&|n| n.time_zone = 1.0).contains("time zone"));
    assert!(bad(&|n| n.logins.pop().map(|_| ()).unwrap_or(())).contains("incomplete"));
    // static values cannot be edited while DHCP is on
    let mut dhcp_b = b.clone();
    dhcp_b.xep1[40] |= 1;
    let ds = XepSettings::decode(&dhcp_b);
    let mut n = ds.clone();
    n.ip = "192.168.7.99".into();
    assert!(XepSettings::apply(&dhcp_b, &ds, &n).unwrap_err().to_string().contains("DHCP off"));
    // a bad value that is not changed does not block an unrelated edit (ElkRP does not re-validate what you did not touch)
    let mut odd = b.clone();
    odd.xep1[20..24].copy_from_slice(&[255, 0, 255, 0]);
    let os = XepSettings::decode(&odd);
    let mut n = os.clone();
    n.name = "ok".into();
    assert!(XepSettings::apply(&odd, &os, &n).is_ok());
}

fn session(p: &SimulatedPanel) -> ElkSession {
    let mut s = ElkSession::plain();
    s.connect("127.0.0.1", p.port()).unwrap();
    s.login("246801").unwrap();
    s
}

fn panel_with(b: XepBlocks) -> SimulatedPanel {
    let p = SimulatedPanel::new_plain("246801");
    p.with_xep(|x| {
        x.enabled = true;
        x.blocks = b;
    });
    p
}

#[test]
fn read_returns_what_the_module_holds_and_sends_only_reads() {
    let b = sample();
    let p = panel_with(b.clone());
    let mut s = session(&p);
    let (info, got) = XepModule::read(&mut s).unwrap();
    assert_eq!(got, b);
    assert_eq!((info.firmware.as_str(), info.app_version), ("1.3.20", 0x01_0314));
    let sent: Vec<Vec<u8>> = p.received().into_iter().filter(|r| r[0] == 0x75).collect();
    assert_eq!(sent.len(), 1 + 5 + 16);
    assert!(sent.iter().all(|r| r[1] < 0x80), "a read never sends a write sub-command");
    assert_eq!(sent[0], vec![0x75, 6]);
    assert_eq!(sent[1], vec![0x75, 1]);
    assert_eq!(sent[6], vec![0x75, 3, 0]);
    assert_eq!(sent[21], vec![0x75, 3, 15]);
}

#[test]
fn read_refuses_old_firmware_and_reports_a_silent_module() {
    let p = panel_with(sample());
    p.with_xep(|x| x.app_version = 0x01_0212);
    let mut s = session(&p);
    let e = XepModule::read(&mut s).unwrap_err().to_string();
    assert!(e.contains("1.2.18") && e.contains("1.3.19"), "{e}");
    // no module behind the panel (a plain panel answers nothing to 0x75)
    let q = SimulatedPanel::new_plain("246801");
    let mut s = session(&q);
    s.timeout = Duration::from_millis(150);
    assert!(XepModule::read(&mut s).unwrap_err().is_timeout());
}

#[test]
fn write_sends_only_the_changed_blocks_and_the_simulator_ends_up_as_asked() {
    let b = sample();
    let p = panel_with(b.clone());
    let mut s = session(&p);
    let (_, held) = XepModule::read(&mut s).unwrap();
    let old = XepSettings::decode(&held);
    let mut new = old.clone();
    new.name = "Garage".into();
    new.time_hour = 5;
    new.emails[4] = EmailItem { slot: 5, address: "x@y.test".into(), message: "m".into() };
    new.emails[1] = EmailItem { slot: 2, ..Default::default() };
    let (to, changes) = XepSettings::apply(&held, &old, &new).unwrap();
    assert!(changes.iter().all(|c| !c.identity));
    let before = p.received().len();
    let sent = XepModule::write(&mut s, &held, &to, Duration::from_secs(5)).unwrap();
    assert_eq!(sent, vec!["network settings and flags (block 1)", "ports, time server and report settings (block 4)", "email 2", "email 5"]);
    let writes: Vec<Vec<u8>> = p.received()[before..].to_vec();
    assert_eq!(writes.iter().map(|w| (w[0], w[1])).collect::<Vec<_>>(), vec![(0x75, 0x81), (0x75, 0x84), (0x75, 0x83), (0x75, 0x83)]);
    assert_eq!(writes[0].len(), 2 + XEP1_LEN);
    assert_eq!(writes[2][2], 1, "email slot 2 is index 1 on the wire");
    assert_eq!(writes[2].len(), 3 + EMAIL_LEN);
    assert!(writes[2][3..].iter().all(|v| *v == 0), "removing an email sends zeros");
    let now = p.with_xep(|x| x.blocks.clone());
    assert_eq!(now, to);
    // nothing to send, nothing sent
    let before = p.received().len();
    assert!(XepModule::write(&mut s, &to, &to, Duration::from_secs(5)).unwrap().is_empty());
    assert_eq!(p.received().len(), before);
}

#[test]
fn a_write_the_module_does_not_acknowledge_is_an_error() {
    let b = sample();
    let p = panel_with(b.clone());
    p.with_xep(|x| x.ignore_write = Some(0x81));
    let mut s = session(&p);
    let mut to = b.clone();
    to.xep1[0] = b'Z';
    to.xep4[206] = 9;
    let e = XepModule::write(&mut s, &b, &to, Duration::from_millis(200)).unwrap_err();
    assert!(e.is_timeout(), "{e}");
    let writes: Vec<Vec<u8>> = p.received().into_iter().filter(|r| r[0] == 0x75 && r[1] >= 0x80).collect();
    assert_eq!(writes.len(), 1, "after the first block fails nothing else is sent");
}

#[test]
fn discovery_decodes_the_reply_and_sends_the_request() {
    let responder = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = responder.local_addr().unwrap();
    responder.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let t = std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        let (n, from) = responder.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"XEPID");
        let mut r = vec![0u8; 33];
        r[..5].copy_from_slice(b"M1XEP");
        r[5..11].copy_from_slice(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]);
        r[11..15].copy_from_slice(&[192, 168, 7, 20]);
        r[15..17].copy_from_slice(&2601u16.to_be_bytes());
        r[17..17 + 8].copy_from_slice(b"Test XEP");
        responder.send_to(&r, from).unwrap();
        responder.send_to(&r, from).unwrap(); // a repeat is listed once
        responder.send_to(b"junk", from).unwrap();
    });
    let found = discover(addr, false, Duration::from_millis(600)).unwrap();
    t.join().unwrap();
    assert_eq!(found, vec![FoundXep { name: "Test XEP".into(), mac: "00-1A-2B-3C-4D-5E".into(), ip: "192.168.7.20".into(), secure_port: 2601 }]);
    assert_eq!(parse_discovery_reply(b"M1XEP"), None);
    assert_eq!(parse_discovery_reply(&[b'X'; 40]), None);
}
