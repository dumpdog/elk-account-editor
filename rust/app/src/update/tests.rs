use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtomicOrdering};

static N: AtomicU32 = AtomicU32::new(0);

pub(crate) fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "elk-update-test-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, AtomicOrdering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn v(s: &str) -> Version {
    parse_version(s).unwrap()
}

#[test]
fn versions_parse_and_compare() {
    assert_eq!(v("v1.2.3").nums, [1, 2, 3]);
    assert_eq!(v("1.2").nums, [1, 2, 0]);
    assert_eq!(v("0.2.0+build5").pre, None);
    assert!(parse_version("").is_none());
    assert!(parse_version("latest").is_none());
    assert!(parse_version("1.2.3.4").is_none());
    assert!(parse_version("1..3").is_none());
    assert_eq!(is_newer("0.1.0", "v0.2.0"), Some(true));
    assert_eq!(is_newer("0.2.0", "0.2.0"), Some(false));
    assert_eq!(
        is_newer("0.10.0", "0.9.9"),
        Some(false),
        "numbers, not text"
    );
    assert_eq!(is_newer("0.2.0", "1.0.0"), Some(true));
    assert_eq!(is_newer("0.2.0", "0.2.1-rc.1"), Some(true));
    assert_eq!(
        is_newer("0.2.1", "0.2.1-rc.1"),
        Some(false),
        "a pre-release is older than its release"
    );
    assert_eq!(is_newer("0.2.1-rc.1", "0.2.1"), Some(true));
    assert_eq!(is_newer("1.0.0-alpha.2", "1.0.0-alpha.10"), Some(true));
    assert_eq!(is_newer("1.0.0-alpha", "1.0.0-alpha.1"), Some(true));
    assert_eq!(is_newer("1.0.0-1", "1.0.0-alpha"), Some(true));
    assert_eq!(is_newer("0.2.0", "nightly"), None);
    // the version of this build is a real version
    assert!(parse_version(CURRENT).is_some());
}

fn release_json(tag: &str, base: &str, with_sums: bool, size: u64) -> String {
    let mut assets = vec![
        serde_json::json!({"name": "ElkAccountEditor-win-x64.zip", "browser_download_url": format!("{base}/dl/ElkAccountEditor-win-x64.zip"), "size": size}),
        serde_json::json!({"name": "ElkAccountEditor-mac-arm64.zip", "browser_download_url": format!("{base}/dl/ElkAccountEditor-mac-arm64.zip"), "size": size}),
        serde_json::json!({"name": "ElkAccountEditor-mac-universal2.zip", "browser_download_url": format!("{base}/dl/ElkAccountEditor-mac-universal2.zip"), "size": size}),
    ];
    if with_sums {
        assets.push(serde_json::json!({"name": "SHA256SUMS", "browser_download_url": format!("{base}/dl/SHA256SUMS"), "size": 300}));
    }
    serde_json::json!({"tag_name": tag, "name": format!("Elk editor {tag}"), "body": "- new things\n- fixes", "html_url": format!("{base}/release"), "assets": assets}).to_string()
}

#[test]
fn feed_parsing_and_platform_assets() {
    let r = parse_release(&release_json("v0.3.0", "http://x", true, 5)).unwrap();
    assert_eq!(
        (r.tag.as_str(), r.name.as_str(), r.assets.len()),
        ("v0.3.0", "Elk editor v0.3.0", 4)
    );
    assert_eq!(r.notes, "- new things\n- fixes");
    assert!(parse_release("not json").is_err());
    assert!(parse_release("{}").is_err());
    let w = asset_names("windows", "x86_64");
    assert_eq!(
        pick_asset(&w, &r.assets).unwrap().name,
        "ElkAccountEditor-win-x64.zip"
    );
    let arm = asset_names("macos", "aarch64");
    assert_eq!(
        pick_asset(&arm, &r.assets).unwrap().name,
        "ElkAccountEditor-mac-arm64.zip"
    );
    let intel = asset_names("macos", "x86_64");
    assert_eq!(
        pick_asset(&intel, &r.assets).unwrap().name,
        "ElkAccountEditor-mac-universal2.zip"
    );
    // an Apple chip build falls back to the universal zip when the release has no arm64 zip
    let only_uni: Vec<Asset> = r
        .assets
        .iter()
        .filter(|a| a.name.contains("universal2"))
        .cloned()
        .collect();
    assert_eq!(
        pick_asset(&arm, &only_uni).unwrap().name,
        "ElkAccountEditor-mac-universal2.zip"
    );
    assert!(asset_names("linux", "x86_64").is_empty());
}

#[test]
fn checksums_must_match_exactly() {
    let data = b"the zip bytes";
    let h = sha256_hex(data);
    assert_eq!(h.len(), 64);
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let sums = parse_sums(&format!(
        "{h}  a.zip\r\n{}  *dir/b.zip\n# comment\nnot a line\n{}  short\n",
        "AB".repeat(32),
        "z".repeat(10)
    ));
    assert_eq!(sums.len(), 2);
    assert!(verify(data, "a.zip", &sums).is_ok());
    let e = verify(b"other bytes", "a.zip", &sums).unwrap_err();
    assert!(e.contains("does not match") && e.contains("refused"), "{e}");
    let e = verify(data, "missing.zip", &sums).unwrap_err();
    assert!(e.contains("no entry"), "{e}");
    assert_eq!(
        sums["b.zip"],
        "ab".repeat(32),
        "upper case hex is accepted, names lose their folder"
    );
}

#[test]
fn only_https_or_this_computer() {
    assert!(check_url("https://api.github.com/repos/x/y/releases/latest").is_ok());
    assert!(check_url("http://127.0.0.1:8080/x").is_ok());
    assert!(check_url("http://localhost/x").is_ok());
    assert!(check_url("http://[::1]:80/x").is_ok());
    assert!(check_url("http://example.com/x").is_err());
    assert!(check_url("http://127.0.0.1.evil.com/x").is_err());
    assert!(check_url("ftp://x").is_err());
    assert!(check_url("file:///etc/passwd").is_err());
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (n, b) in files {
            z.start_file(*n, o).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }
    buf
}

#[test]
fn unpacking_refuses_paths_that_leave_the_folder() {
    let d = temp("unpack");
    let ok = zip_of(&[("top/a.txt", b"a"), ("top/sub/b.txt", b"b")]);
    unpack_zip(&ok, &d.join("out")).unwrap();
    assert_eq!(std::fs::read(d.join("out/top/sub/b.txt")).unwrap(), b"b");
    assert_eq!(payload_root(&d.join("out")), d.join("out/top"));
    let bad = zip_of(&[("../evil.txt", b"x")]);
    assert!(
        unpack_zip(&bad, &d.join("out2"))
            .unwrap_err()
            .contains("unsafe")
    );
    assert!(!d.join("evil.txt").exists());
    assert!(unpack_zip(b"not a zip", &d.join("out3")).is_err());
    let _ = std::fs::remove_dir_all(d);
}

fn write(p: &Path, s: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, s).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn swap_keeps_a_backup_and_rolls_back_when_a_later_item_fails() {
    let d = temp("swap");
    for (n, old, new) in [("one", "old1", "new1"), ("two", "old2", "new2")] {
        write(&d.join(n), old);
        write(&d.join("staged").join(n), new);
    }
    let items: Vec<Item> = ["one", "two"]
        .iter()
        .map(|n| Item {
            target: d.join(n),
            new: d.join("staged").join(n),
            backup: suffixed(&d.join(n), ".old"),
        })
        .collect();
    swap_all(&items).unwrap();
    assert_eq!(
        (read(&d.join("one")), read(&d.join("two"))),
        ("new1".into(), "new2".into())
    );
    assert_eq!(
        (read(&d.join("one.old")), read(&d.join("two.old"))),
        ("old1".into(), "old2".into())
    );

    // second run: the new file of the second item is gone, so the first replacement must be undone
    write(&d.join("staged/one"), "newer1");
    let _ = std::fs::remove_file(d.join("staged/two"));
    let e = swap_all(&items).unwrap_err();
    assert!(e.contains("put back"), "{e}");
    assert_eq!(
        (read(&d.join("one")), read(&d.join("two"))),
        ("new1".into(), "new2".into()),
        "both are as they were"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn swap_replaces_a_whole_app_bundle_and_keeps_the_old_one() {
    let d = temp("bundle");
    let app = d.join("Elk Account Editor.app");
    write(&app.join("Contents/MacOS/elkaccounteditor"), "old binary");
    write(&d.join("README.txt"), "old readme");
    write(&d.join("ElkEditor data/library.sqlite"), "precious");
    let inst = Install::detect(
        &app.join("Contents/MacOS/elkaccounteditor"),
        Some(&d.join("ElkEditor data")),
    );
    assert_eq!(inst.bundle.as_deref(), Some(app.as_path()));
    assert_eq!(inst.base(), d);
    let zip = zip_of(&[
        (
            "ElkAccountEditor-mac-arm64/Elk Account Editor.app/Contents/MacOS/elkaccounteditor",
            b"new binary",
        ),
        (
            "ElkAccountEditor-mac-arm64/Elk Account Editor.app/Contents/Info.plist",
            b"plist",
        ),
        ("ElkAccountEditor-mac-arm64/README.txt", b"new readme"),
        (
            "ElkAccountEditor-mac-arm64/Start in browser.command",
            b"cmd",
        ),
    ]);
    let out = install_zip(&zip, &inst, "9.9.9").unwrap();
    assert_eq!(out.restart, app);
    assert_eq!(
        read(&app.join("Contents/MacOS/elkaccounteditor")),
        "new binary"
    );
    assert_eq!(
        read(&d.join("Elk Account Editor.app.previous/Contents/MacOS/elkaccounteditor")),
        "old binary"
    );
    assert_eq!(read(&d.join("README.txt")), "new readme");
    assert!(
        !d.join("Start in browser.command").exists(),
        "files that were not there are not added"
    );
    assert_eq!(read(&d.join("ElkEditor data/library.sqlite")), "precious");
    assert!(!inst.staging().exists());
    // a second update replaces the earlier backup
    let zip2 = zip_of(&[(
        "p/Elk Account Editor.app/Contents/MacOS/elkaccounteditor",
        b"newer binary",
    )]);
    install_zip(&zip2, &inst, "9.9.10").unwrap();
    assert_eq!(
        read(&d.join("Elk Account Editor.app.previous/Contents/MacOS/elkaccounteditor")),
        "new binary"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_update_never_touches_the_data_folder() {
    let d = temp("guard");
    // the program sits INSIDE the data folder (a silly --data-dir): refused
    write(&d.join("data/prog.exe"), "old");
    let inst = Install {
        exe: d.join("data/prog.exe"),
        bundle: None,
        data_dir: Some(d.join("data")),
    };
    let zip = zip_of(&[("p/ElkAccountEditor.exe", b"new")]);
    let e = install_zip(&zip, &inst, "9.9.9").unwrap_err();
    assert!(e.contains("library data folder"), "{e}");
    assert_eq!(read(&d.join("data/prog.exe")), "old");
    let _ = std::fs::remove_dir_all(d);
}

// ---- a local release server ------------------------------------------------------------------------------------------

type Routes = std::sync::Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

pub(crate) struct TestServer {
    pub base: String,
    routes: Routes,
    stop: std::sync::Arc<AtomicBool>,
    pub hits: std::sync::Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    pub fn start() -> TestServer {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let routes: Routes = Default::default();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let hits: std::sync::Arc<Mutex<Vec<String>>> = Default::default();
        let (r2, s2, h2) = (routes.clone(), stop.clone(), hits.clone());
        std::thread::spawn(move || {
            for c in l.incoming() {
                if s2.load(AtomicOrdering::SeqCst) {
                    break;
                }
                let Ok(mut c) = c else { continue };
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                h2.lock().unwrap().push(path.clone());
                let (status, body) = r2
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"not found".to_vec()));
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = c.write_all(head.as_bytes());
                let _ = c.write_all(&body);
            }
        });
        TestServer {
            base,
            routes,
            stop,
            hits,
        }
    }
    pub fn put(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (status, body.into()));
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::SeqCst);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
    }
}

/// A program folder with an old program, the library data and a server that offers `tag` with `payload` as the release zip.
fn setup(tag: &str, zip: &[u8], sums_for: Option<&[u8]>) -> (PathBuf, Install, TestServer) {
    let d = temp("http");
    write(&d.join("ElkAccountEditor.exe"), "OLD EXE");
    write(&d.join("elk.exe"), "OLD CLI");
    write(&d.join("README.txt"), "OLD README");
    write(&d.join("ElkEditor data/library.sqlite"), "PRECIOUS");
    let inst = Install::detect(
        &d.join("ElkAccountEditor.exe"),
        Some(&d.join("ElkEditor data")),
    );
    let s = TestServer::start();
    s.put(
        "/latest",
        200,
        release_json(tag, &s.base, sums_for.is_some(), zip.len() as u64),
    );
    for n in [
        "ElkAccountEditor-win-x64.zip",
        "ElkAccountEditor-mac-arm64.zip",
        "ElkAccountEditor-mac-universal2.zip",
    ] {
        s.put(&format!("/dl/{n}"), 200, zip.to_vec());
    }
    if let Some(src) = sums_for {
        let h = sha256_hex(src);
        let text: String = [
            "ElkAccountEditor-win-x64.zip",
            "ElkAccountEditor-mac-arm64.zip",
            "ElkAccountEditor-mac-universal2.zip",
        ]
        .iter()
        .map(|n| format!("{h}  {n}\n"))
        .collect();
        s.put("/dl/SHA256SUMS", 200, text);
    }
    (d, inst, s)
}

fn release_zip() -> Vec<u8> {
    zip_of(&[
        ("ElkAccountEditor-win-x64/ElkAccountEditor.exe", b"NEW EXE"),
        ("ElkAccountEditor-win-x64/elk.exe", b"NEW CLI"),
        ("ElkAccountEditor-win-x64/README.txt", b"NEW README"),
    ])
}

const NAMES: [&str; 1] = ["ElkAccountEditor-win-x64.zip"];

#[test]
fn check_reports_a_newer_release_with_notes_and_the_right_zip() {
    let z = release_zip();
    let (d, _inst, s) = setup("v99.0.0", &z, Some(&z));
    let c = check(&s.url("/latest"), &NAMES).unwrap();
    assert!(c.newer);
    assert_eq!((c.current.as_str(), c.latest.as_str()), (CURRENT, "99.0.0"));
    assert_eq!(c.release.notes, "- new things\n- fixes");
    assert_eq!(c.asset.unwrap().name, "ElkAccountEditor-win-x64.zip");
    assert!(c.has_sums);
    // the same version, or an older one, is not an update
    s.put(
        "/latest",
        200,
        release_json(&format!("v{CURRENT}"), &s.base, true, 1),
    );
    assert!(!check(&s.url("/latest"), &NAMES).unwrap().newer);
    s.put("/latest", 200, release_json("v0.0.1", &s.base, true, 1));
    assert!(!check(&s.url("/latest"), &NAMES).unwrap().newer);
    s.put("/latest", 200, release_json("nightly", &s.base, true, 1));
    assert!(
        check(&s.url("/latest"), &NAMES)
            .unwrap_err()
            .contains("not a version")
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_missing_or_private_repository_fails_gracefully() {
    let s = TestServer::start();
    let e = check(&s.url("/nothing-here"), &NAMES).unwrap_err();
    assert!(
        e.contains("HTTP 404") && e.contains("not public yet"),
        "{e}"
    );
    s.put("/limited", 403, "rate limit");
    assert!(
        check(&s.url("/limited"), &NAMES)
            .unwrap_err()
            .contains("403")
    );
    s.put("/junk", 200, "<html>");
    assert!(
        check(&s.url("/junk"), &NAMES)
            .unwrap_err()
            .contains("not valid JSON")
    );
    // nothing listening
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let e = check(&format!("http://127.0.0.1:{port}/x"), &NAMES).unwrap_err();
    assert!(e.starts_with("Could not check for updates"), "{e}");
    assert!(
        check("http://example.com/x", &NAMES)
            .unwrap_err()
            .contains("https://")
    );
}

#[test]
fn install_downloads_verifies_swaps_and_leaves_the_data_alone() {
    let z = release_zip();
    let (d, inst, s) = setup("v99.0.0", &z, Some(&z));
    let out = install(&s.url("/latest"), &NAMES, &inst).unwrap();
    assert_eq!(out.version, "99.0.0");
    assert_eq!(out.restart, d.join("ElkAccountEditor.exe"));
    assert_eq!(
        (
            read(&d.join("ElkAccountEditor.exe")),
            read(&d.join("elk.exe")),
            read(&d.join("README.txt"))
        ),
        ("NEW EXE".into(), "NEW CLI".into(), "NEW README".into())
    );
    assert_eq!(read(&d.join("ElkAccountEditor.exe.old")), "OLD EXE");
    assert_eq!(read(&d.join("ElkEditor data/library.sqlite")), "PRECIOUS");
    assert!(!inst.staging().exists());
    // the next start tidies the backups of the previous version
    cleanup_old(&inst);
    assert!(!d.join("ElkAccountEditor.exe.old").exists() && !d.join("README.txt.old").exists());
    assert!(d.join("ElkAccountEditor.exe").is_file());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_bad_checksum_installs_nothing() {
    let z = release_zip();
    let tampered = zip_of(&[("ElkAccountEditor-win-x64/ElkAccountEditor.exe", b"EVIL EXE")]);
    // SHA256SUMS lists the real zip, the server delivers another one
    let (d, inst, s) = setup("v99.0.0", &z, Some(&z));
    for n in [
        "ElkAccountEditor-win-x64.zip",
        "ElkAccountEditor-mac-arm64.zip",
        "ElkAccountEditor-mac-universal2.zip",
    ] {
        s.put(&format!("/dl/{n}"), 200, tampered.clone());
    }
    let e = install(&s.url("/latest"), &NAMES, &inst).unwrap_err();
    assert!(
        e.contains("does not match") && e.contains("nothing was installed"),
        "{e}"
    );
    assert_eq!(read(&d.join("ElkAccountEditor.exe")), "OLD EXE");
    assert!(!d.join("ElkAccountEditor.exe.old").exists());
    assert!(!inst.staging().exists(), "the zip was not even unpacked");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn no_checksum_file_or_no_download_for_this_computer_installs_nothing() {
    let z = release_zip();
    let (d, inst, s) = setup("v99.0.0", &z, None);
    let e = install(&s.url("/latest"), &NAMES, &inst).unwrap_err();
    assert!(
        e.contains("SHA256SUMS") && e.contains("nothing was installed"),
        "{e}"
    );
    assert!(
        !s.hits.lock().unwrap().iter().any(|h| h.starts_with("/dl/")),
        "nothing was downloaded"
    );
    let e = install(&s.url("/latest"), &["ElkAccountEditor-other.zip"], &inst).unwrap_err();
    assert!(e.contains("no download for this computer"), "{e}");
    // not newer: refused as well
    s.put(
        "/latest",
        200,
        release_json(&format!("v{CURRENT}"), &s.base, true, 1),
    );
    assert!(
        install(&s.url("/latest"), &NAMES, &inst)
            .unwrap_err()
            .contains("already have")
    );
    assert_eq!(read(&d.join("ElkAccountEditor.exe")), "OLD EXE");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_zip_without_the_program_rolls_back_and_cleans_up() {
    let z = zip_of(&[("p/README.txt", b"NEW README")]);
    let (d, inst, s) = setup("v99.0.0", &z, Some(&z));
    let e = install(&s.url("/latest"), &NAMES, &inst).unwrap_err();
    assert!(e.contains("no program file"), "{e}");
    assert_eq!(read(&d.join("README.txt")), "OLD README");
    assert!(!inst.staging().exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_read_only_folder_is_explained() {
    let d = temp("ro");
    let missing = d.join("does-not-exist");
    let e = check_writable(&missing).unwrap_err();
    assert!(
        e.contains("cannot be written") && e.contains("by hand"),
        "{e}"
    );
    assert!(check_writable(&d).is_ok());
    let _ = std::fs::remove_dir_all(d);
}
