//! Offline commands compared with the output of the C# `Elk.Cli` (captured in `tests/fixtures`, line ends normalised).

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sample(name: &str) -> String {
    root()
        .join("../../sample")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(root().join("tests/fixtures").join(name))
        .unwrap()
        .replace("\r\n", "\n")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = elk_cli::run(&a, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap().replace("\r\n", "\n"),
        String::from_utf8(err).unwrap(),
    )
}

const ZONE: &str = "0A0341004C6976696E6720526F6F6D0000000000000000000000000000000000";

#[test]
fn crc_frame_decode() {
    assert_eq!(
        run(&["crc", "010203"]),
        (0, fixture("crc.txt"), String::new())
    );
    assert_eq!(run(&["crc", "01 02 03"]).1, fixture("crc.txt"));
    assert_eq!(
        run(&["frame", "0A0B0C"]),
        (0, fixture("frame.txt"), String::new())
    );
    assert_eq!(
        run(&["decode", "zone", ZONE]),
        (0, fixture("decode_zone.txt"), String::new())
    );
    assert_eq!(
        run(&[
            "decode",
            "area",
            "536D69746820486F757365686F6C64002D1E3C1E53500F00"
        ])
        .1,
        fixture("decode_area.txt")
    );
    assert_eq!(
        run(&["decode", "keypad", "0138000014604672"]).1,
        fixture("decode_keypad.txt")
    );
    let (code, _, err) = run(&["decode", "bogus", "00"]);
    assert_eq!((code, err.trim()), (1, "zone|area|keypad"));
}

#[test]
fn inspect_matches_csharp() {
    assert_eq!(
        run(&["inspect", "--in", &sample("ElkRPAcctSample Export")]).1,
        fixture("inspect.txt")
    );
    assert_eq!(
        run(&["inspect", "--in", &sample("Sample Communicator"), "--all"]).1,
        fixture("inspect_all.txt")
    );
}

#[test]
fn export_zones_matches_csharp() {
    let out = std::env::temp_dir().join("elk-cli-export-zones-test.out");
    let out_s = out.to_string_lossy().into_owned();
    let (code, stdout, _) = run(&[
        "export-zones",
        "--in",
        &sample("ElkRPAcctSample Export"),
        "--csv",
        &sample("sample-zones.csv"),
        "--out",
        &out_s,
    ]);
    assert_eq!(code, 0);
    assert_eq!(stdout.replace(&out_s, "OUT"), fixture("export_zones.txt"));
    let expected = std::fs::read(root().join("tests/fixtures/export_zones.out")).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn export_zones_refuses_same_file() {
    let f = sample("ElkRPAcctSample Export");
    let (code, _, err) = run(&[
        "export-zones",
        "--in",
        &f,
        "--csv",
        &sample("sample-zones.csv"),
        "--out",
        &f,
    ]);
    assert_eq!(
        (code, err.trim()),
        (2, "--out must be a different file from --in.")
    );
}

#[test]
fn export_alarm_list_writes_a_workbook() {
    let out = std::env::temp_dir().join("elk-cli-alarm-list-test.xlsx");
    let out_s = out.to_string_lossy().into_owned();
    let (code, stdout, _) = run(&[
        "export-alarm-list",
        "--in",
        &sample("ElkRPAcctSample Export"),
        "--out",
        &out_s,
        "--title",
        "Site X",
    ]);
    assert_eq!(
        (code, stdout.trim()),
        (0, format!("wrote {out_s}").as_str())
    );
    assert_eq!(&std::fs::read(&out).unwrap()[..2], b"PK");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn usage_and_unknown_command() {
    assert_eq!(
        run(&[]),
        (
            1,
            "usage: elk crc|frame|decode|download|upload ...\n".to_string(),
            String::new()
        )
    );
    assert_eq!(run(&["nope"]).0, 1);
    assert_eq!(
        run(&[
            "raw-write",
            "--section",
            "10",
            "--item",
            "3",
            "--data",
            "0102AB"
        ])
        .1,
        fixture("raw_write_dry.txt")
    );
    let (code, _, err) = run(&["inspect"]);
    assert_eq!((code, err.trim()), (1, "error: missing --in"));
}
