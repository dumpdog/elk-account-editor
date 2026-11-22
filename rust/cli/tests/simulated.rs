//! Panel commands against elk-core's simulated panel (plain port).

use elk_account::AccountFile;
use elk_core::simulated_panel::SimulatedPanel;

const CODE: &str = "123456";

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

fn conn(p: &SimulatedPanel) -> Vec<String> {
    [
        "--host",
        "127.0.0.1",
        "--port",
        &p.port().to_string(),
        "--plain",
        "--code",
        CODE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn with(p: &SimulatedPanel, cmd: &[&str]) -> (i32, String, String) {
    let mut v: Vec<&str> = cmd.to_vec();
    let c = conn(p);
    v.extend(c.iter().map(String::as_str));
    run(&v)
}

fn seeded() -> SimulatedPanel {
    let p = SimulatedPanel::new_plain(CODE);
    let f = AccountFile::parse(&elk_account::decode_latin1(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sample/ElkRPAcctSample Export"
        ))
        .unwrap(),
    ))
    .unwrap();
    elk_panel::testkit::seed(&p, &f);
    p
}

#[test]
fn raw_read_prints_the_reply() {
    let p = seeded();
    let (code, out, err) = with(&p, &["raw-read", "--section", "10", "--item", "1"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("32 bytes: ") || out.contains(" bytes: "),
        "{out}"
    );
}

#[test]
fn download_then_upload_dry_run_finds_nothing_to_change() {
    let p = seeded();
    let file = std::env::temp_dir().join("elk-cli-sim-download.json");
    let file_s = file.to_string_lossy().into_owned();
    let (code, out, err) = with(&p, &["download", "--out", &file_s]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("saved "), "{out}");
    let (code, out, err) = with(&p, &["upload", "--in", &file_s]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("0 record(s) would change. Re-run with --yes to write them."),
        "{out}"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn upload_export_dry_run_reports_matches() {
    let p = seeded();
    let export = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../sample/ElkRPAcctSample Export"
    );
    let (code, out, err) = with(&p, &["upload-export", "--in", export]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("already match") && out.contains("dry run; add --yes to write"),
        "{out}"
    );
}

#[test]
fn status_modules_and_control_run() {
    let p = seeded();
    let (code, out, err) = with(&p, &["status"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("area 1: ") && out.contains("system: "),
        "{out}"
    );
    // the simulated panel has no bus module records, so the scan reports that the panel did not answer
    let (code, _, err) = with(&p, &["modules"]);
    assert_eq!(code, 1);
    assert!(err.contains("No reply"), "{err}");
    let (code, out, err) = with(&p, &["control", "--do", "silence", "--area", "1"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("reply: "), "{out}");
}

#[test]
fn wrong_access_code_is_an_error() {
    let p = seeded();
    let port = p.port().to_string();
    let (code, _, err) = run(&[
        "status",
        "--host",
        "127.0.0.1",
        "--port",
        &port,
        "--plain",
        "--code",
        "000000",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("error:"), "{err}");
}

#[test]
fn automation_download_and_export_dry_run_read_only() {
    let p = seeded();
    // seed the automation items from the same export
    let f = AccountFile::parse(&elk_account::decode_latin1(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sample/ElkRPAcctSample Export"
        ))
        .unwrap(),
    ))
    .unwrap();
    elk_panel::testkit::seed_automation(&p, &f);
    let file = std::env::temp_dir().join("elk-cli-sim-download-auto.json");
    let file_s = file.to_string_lossy().into_owned();
    let (code, out, err) = with(&p, &["download", "--automation", "--out", &file_s]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("automation blocks"), "{out}");
    let (code, out, err) = with(&p, &["upload", "--in", &file_s]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("0 record(s) would change"), "{out}");
    let export = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../sample/ElkRPAcctSample Export"
    );
    let (code, out, err) = with(&p, &["upload-export", "--in", export, "--automation"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("task") && out.contains("rule packets"),
        "{out}"
    );
    assert!(!out.contains(" differs"), "{out}");
    assert!(
        p.received().iter().all(|r| r[0] < 128 || r[0] == 0xFF),
        "a dry run must not write"
    );
    let _ = std::fs::remove_file(&file);
}
