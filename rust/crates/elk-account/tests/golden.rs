//! Golden tests: the fixtures in tests/fixtures are what the C# editor server (rust/original-build) answered for the same inputs.
//! model/ = POST /api/open, requests/ = POST /api/save bodies, saved/ = their answers, export/ = /api/zones/export and
//! /api/export-alarm-list, read/ = /api/zones/read (inputs in readinputs/).

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use elk_account::xml::Document;
use elk_account::*;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const SAMPLES: [&str; 4] = [
    "ElkRPAcctSample Export",
    "Sample Communicator",
    "Sample Zones Import",
    "Sample Zones Import Export",
];

fn sample_text(i: usize) -> String {
    decode_latin1(&fs::read(root().join("sample").join(SAMPLES[i])).unwrap())
}

#[test]
fn every_sample_file_saves_byte_identical() {
    let mut n = 0;
    for e in fs::read_dir(root().join("sample")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "csv") {
            continue;
        }
        let text = decode_latin1(&fs::read(&p).unwrap());
        let mut f = AccountFile::parse(&text).unwrap();
        assert_eq!(f.to_text(), text, "{p:?} untouched");
        let m = AccountModel::from(&f).unwrap();
        m.apply_to(&mut f).unwrap();
        assert_eq!(f.to_text(), text, "{p:?} through the model");
        // and through JSON, as the browser does
        let m2 = AccountModel::from_json(&m.to_json()).unwrap();
        assert_eq!(m2, m);
        let mut f2 = AccountFile::parse(&text).unwrap();
        m2.apply_to(&mut f2).unwrap();
        assert_eq!(f2.to_text(), text, "{p:?} through JSON");
        n += 1;
    }
    assert!(n >= 4);
}

#[test]
fn model_json_equals_the_csharp_server() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    for i in 0..4 {
        let want = fs::read_to_string(fixtures().join(format!("model/s{i}.json"))).unwrap();
        let f = AccountFile::parse(&sample_text(i)).unwrap();
        let got = format!(
            "{{\"file\":\"s{i}\",\"model\":{}}}",
            without_details(&AccountModel::from(&f).unwrap().to_json())
        );
        assert_eq!(got, want, "model json s{i}");
    }
}

#[test]
fn csharp_model_json_deserialises_and_reserialises_identically() {
    for i in 0..4 {
        let want = fs::read_to_string(fixtures().join(format!("model/s{i}.json"))).unwrap();
        let inner = want
            .strip_prefix(&format!("{{\"file\":\"s{i}\",\"model\":"))
            .unwrap()
            .strip_suffix('}')
            .unwrap();
        let m = AccountModel::from_json(inner).unwrap();
        assert_eq!(m.to_json(), inner);
    }
}

fn request_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(fixtures().join("requests"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v
}

fn sample_index(name: &str) -> usize {
    name[1..2].parse().unwrap()
}

#[test]
fn saves_equal_the_csharp_server() {
    let mut ok = 0;
    let mut errs = 0;
    for p in request_files() {
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let original = sample_text(sample_index(&stem));
        let body = fs::read_to_string(&p).unwrap();
        let result = (|| -> Result<String> {
            let model = AccountModel::from_json(&body)?;
            let mut f = AccountFile::parse(&original)?;
            model.apply_to(&mut f)?;
            Ok(f.to_text())
        })();
        let bin = fixtures().join(format!("saved/{stem}.bin"));
        let err = fixtures().join(format!("saved/{stem}.err"));
        if bin.exists() {
            let want = fs::read(&bin).unwrap();
            let got = encode_latin1(&result.unwrap_or_else(|e| panic!("{stem}: {e}")));
            assert!(got == want, "{stem}: saved bytes differ");
            ok += 1;
        } else {
            let want: String = serde_json::from_slice(&fs::read(&err).unwrap()).unwrap();
            assert_eq!(result.expect_err(&stem).message(), want, "{stem}");
            errs += 1;
        }
    }
    assert!(ok >= 16 && errs >= 40, "{ok} {errs}");
}

#[test]
fn unedited_save_equals_the_original_file() {
    for (i, name) in SAMPLES.iter().enumerate() {
        let want = fs::read(fixtures().join(format!("saved/s{i}_plain.bin"))).unwrap();
        assert_eq!(want, fs::read(root().join("sample").join(name)).unwrap());
    }
}

fn model_for(stem: &str) -> AccountModel {
    AccountModel::from_json(
        &fs::read_to_string(fixtures().join(format!("requests/{stem}.json"))).unwrap(),
    )
    .unwrap()
}

fn entries(xlsx: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(xlsx)).unwrap();
    let mut m = BTreeMap::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let mut b = Vec::new();
        e.read_to_end(&mut b).unwrap();
        m.insert(e.name().to_string(), b);
    }
    m
}

fn assert_same_package(got: &[u8], want: &[u8], what: &str) {
    let (g, w) = (entries(got), entries(want));
    assert_eq!(
        g.keys().collect::<Vec<_>>(),
        w.keys().collect::<Vec<_>>(),
        "{what}: parts"
    );
    for (k, v) in &w {
        assert!(
            g[k] == *v,
            "{what}: part {k} differs ({} vs {} bytes)",
            g[k].len(),
            v.len()
        );
    }
}

#[test]
fn zone_exports_equal_the_csharp_server() {
    for i in 0..4 {
        for tag in ["plain", "edit"] {
            let stem = format!("s{i}_{tag}");
            let m = model_for(&stem);
            for (name, all) in [("", false), ("all", true)] {
                let csv =
                    fs::read(fixtures().join(format!("export/{stem}_csv{name}.csv"))).unwrap();
                assert!(
                    ZoneSheet::to_csv(&ZoneSheet::table(&m.zones, all)) == csv,
                    "{stem} csv{name}"
                );
                let x =
                    fs::read(fixtures().join(format!("export/{stem}_xlsx{name}.xlsx"))).unwrap();
                assert_same_package(
                    &ZoneSheet::to_xlsx(&ZoneSheet::table(&m.zones, all)).unwrap(),
                    &x,
                    &format!("{stem} xlsx{name}"),
                );
            }
        }
    }
}

/// The date the C# server wrote into B2 of an Alarm Zone List.
fn fixture_date(xlsx: &[u8]) -> chrono::NaiveDate {
    let e = entries(xlsx);
    let doc =
        Document::parse(std::str::from_utf8(&e["xl/worksheets/sheet1.xml"]).unwrap()).unwrap();
    let mut cells = Vec::new();
    doc.root.descendants(
        "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
        "c",
        &mut cells,
    );
    let b2 = cells
        .into_iter()
        .find(|c| c.attr("r") == Some("B2"))
        .unwrap();
    chrono::NaiveDate::parse_from_str(&b2.value(), "%Y-%m-%d").unwrap()
}

#[test]
fn alarm_zone_lists_equal_the_csharp_server() {
    for i in 0..4 {
        for tag in ["plain", "edit"] {
            let stem = format!("s{i}_{tag}");
            let m = model_for(&stem);
            let want = fs::read(fixtures().join(format!("export/{stem}_alarm.xlsx"))).unwrap();
            let got = AlarmZoneList::build(&m, None, Some(fixture_date(&want))).unwrap();
            assert_same_package(&got, &want, &stem);
            let want = fs::read(fixtures().join(format!("export/{stem}_alarmtitle.xlsx"))).unwrap();
            let got =
                AlarmZoneList::build(&m, Some("My Site & Co"), Some(fixture_date(&want))).unwrap();
            assert_same_package(&got, &want, &format!("{stem} title"));
        }
    }
}

#[test]
fn zone_reads_equal_the_csharp_server() {
    let mut n = 0;
    for e in fs::read_dir(fixtures().join("readinputs")).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let data = fs::read(&p).unwrap();
        let got = ZoneSheet::read(&data, &name);
        let found: Vec<PathBuf> = fs::read_dir(fixtures().join("read"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|q| {
                q.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{name}."))
            })
            .collect();
        assert_eq!(found.len(), 1, "{name}");
        let fname = found[0].file_name().unwrap().to_string_lossy().to_string();
        let want = fs::read_to_string(&found[0]).unwrap();
        if fname.contains(".200.") {
            assert_eq!(
                got.unwrap_or_else(|e| panic!("{name}: {e}")).to_json(),
                want,
                "{name}"
            );
        } else {
            let msg = got.expect_err(&name);
            assert_eq!(
                serde_json::to_string(msg.message()).unwrap(),
                want,
                "{name}"
            );
        }
        n += 1;
    }
    assert!(n >= 15);
}

#[test]
fn opening_a_non_account_file_gives_the_csharp_message() {
    let want = fs::read_to_string(fixtures().join("read/open_bad.400.txt")).unwrap();
    let e = AccountFile::parse("hello\r\nworld\r\n").unwrap_err();
    assert_eq!(serde_json::to_string(e.message()).unwrap(), want);
}

#[allow(dead_code)]
fn exists(p: &Path) -> bool {
    p.exists()
}

#[test]
fn cli_export_zones_equals_the_csharp_cli() {
    let csv = fs::read_to_string(root().join("sample/sample-zones.csv")).unwrap();
    let (specs, errors) = ZoneCsv::parse(&csv);
    assert!(errors.is_empty());
    let (text, log) = AccountExport::apply_zones(&sample_text(0), &specs).unwrap();
    let want = fs::read(fixtures().join("cli/export-zones.out")).unwrap();
    assert!(encode_latin1(&text) == want, "export-zones output differs");
    let want_log = fs::read_to_string(fixtures().join("cli/export-zones.log")).unwrap();
    let want_log: Vec<&str> = want_log.trim_start_matches('\u{feff}').lines().collect();
    assert_eq!(log, &want_log[..log.len()]);
}

#[test]
fn automation_meta_members_equal_the_csharp_server() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    let want = fs::read_to_string(fixtures().join("meta.json")).unwrap();
    use elk_account::json::to_string as j;
    let tail = format!(
        ",\"words\":{},\"voiceMessages\":{},\"outputNames\":{},\"eventNames\":{},\"customTypes\":{},\"lightTypes\":{},\"lightFormats\":{}}}",
        j(AutomationMeta::words()),
        j(AutomationMeta::voice_message_names()),
        j(AutomationMeta::output_names()),
        j(rules::event_names()),
        j(&AutomationMeta::CUSTOM_TYPES[..]),
        j(&AutomationMeta::LIGHT_TYPES[..]),
        j(&AutomationMeta::LIGHT_FORMATS[..]),
    );
    assert!(want.ends_with(&tail), "meta tail differs");
}

/// The Rust model has one member the C# server did not write (`details`, the Account Details page, ADR-010). It is the last
/// member of the model JSON and holds no nested objects, so the C# oracle's bytes are the Rust bytes with it cut off.
fn without_details(json: &str) -> String {
    match json.find(",\"details\":{") {
        Some(at) => {
            let end = json[at..].find('}').unwrap() + at + 1;
            format!("{}{}", &json[..at], &json[end..])
        }
        None => json.to_string(),
    }
}
