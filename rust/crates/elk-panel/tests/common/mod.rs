//! Shared helpers of the integration tests.
#![allow(dead_code)]

use elk_account::{decode_latin1, AccountFile};
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::ElkSession;
use std::path::PathBuf;

pub const MAC: &str = "00:40:9D:00:00:01";
pub const PASS: &str = "testphrase";
pub const CODE: &str = "246801";

/// The repository root (`Elk-rust`), three levels above the crate.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A file of the repository's `sample` folder, decoded as Latin-1 (like the C# `Sample()` helper).
pub fn sample(name: &str) -> String {
    let p = repo_root().join("sample").join(name);
    decode_latin1(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
}

pub fn sample_file(name: &str) -> AccountFile {
    AccountFile::parse(&sample(name)).unwrap()
}

pub fn template() -> String {
    elk_panel::template_text()
}

pub fn connect(p: &SimulatedPanel) -> ElkSession {
    let mut s = ElkSession::new(MAC, PASS).unwrap();
    s.connect("127.0.0.1", p.port()).unwrap();
    s.login(CODE).unwrap();
    s
}

/// A fresh unique folder under the system temp folder.
pub fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("elk-tmp-{}-{}-{tag}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
