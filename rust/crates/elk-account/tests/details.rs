//! Account Details (ADR-010): the fields of ElkRP's Account Details page that the export file holds.

use std::fs;
use std::path::PathBuf;

use elk_account::*;

fn sample() -> String {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sample/ElkRPAcctSample Export");
    decode_latin1(&fs::read(p).unwrap())
}

#[test]
fn sample_details_are_read() {
    let f = AccountFile::parse(&sample()).unwrap();
    let d = f.details().unwrap();
    assert_eq!(d.name, "The Smiths");
    assert_eq!(d.address1, "1234 Main Ave.");
    assert_eq!(d.city, "Anywhere");
    assert_eq!(d.state, "NC");
    assert_eq!(d.zip, "28000-0000");
    assert_eq!(d.port, "2601");
    assert_eq!(d.hardware, "0.11");
    assert_eq!(d.boot, "3.2.2");
    assert_eq!(d.voice_list, "0.8");
    assert_eq!(d.installed, "2004-01-01");
    assert_eq!(d.serial, "00000000");
    assert_eq!(d.rp_code, "246801");
    assert_eq!((d.network_adapter, d.non_secure), (0, 0));
}

#[test]
fn unedited_details_keep_every_byte() {
    let text = sample();
    let mut f = AccountFile::parse(&text).unwrap();
    let m = AccountModel::from(&f).unwrap();
    let m2 = AccountModel::from_json(&m.to_json()).unwrap();
    m2.apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text);
}

#[test]
fn edited_details_change_only_their_fields() {
    let text = sample();
    let mut f = AccountFile::parse(&text).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    {
        let d = m.details.as_mut().unwrap();
        d.name = "Jones".into();
        d.notes = "line one\nline two".into();
        d.email = "a@b.c".into();
        d.network_adapter = 2;
        d.non_secure = 2;
        d.serial = "ignored".into();
    }
    m.apply_to(&mut f).unwrap();
    let out = f.to_text();
    let g = AccountFile::parse(&out).unwrap();
    let d = g.details().unwrap();
    assert_eq!(d.name, "Jones");
    assert_eq!(d.notes, "line one\nline two");
    assert_eq!(d.email, "a@b.c");
    assert_eq!((d.network_adapter, d.non_secure), (2, 2));
    assert_eq!(d.serial, "00000000");
    assert_eq!(d.address1, "1234 Main Ave.");
    // everything after the head is identical
    let tail = |t: &str| t[t.find("\r\n8\r\n1\r\n").unwrap()..].to_string();
    assert_eq!(tail(&out), tail(&text));
    assert!(out.contains("\"M1G\",\"5.0.12\",\"4.3.6\",\"0.11\",\"3.2.2\",\"0.8\",\"2004-01-01\",\"line one\r\nline two\",0\r\n"));
    // and the account still opens with the same sections
    assert_eq!(AccountModel::from(&g).unwrap().zones.len(), m.zones.len());
}

#[test]
fn bad_details_are_refused() {
    let mut f = AccountFile::parse(&sample()).unwrap();
    let mut m = AccountModel::from(&f).unwrap();
    m.details.as_mut().unwrap().name = "a\"b".into();
    assert!(m.apply_to(&mut f).is_err());
    m.details.as_mut().unwrap().name = "x".repeat(51);
    assert!(m.apply_to(&mut f).is_err());
    m.details.as_mut().unwrap().name = "ok".into();
    m.details.as_mut().unwrap().port = "26x1".into();
    assert!(m.apply_to(&mut f).is_err());
}
