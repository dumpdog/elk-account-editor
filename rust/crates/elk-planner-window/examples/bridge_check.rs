//! Opens the native window on a tiny local test page and checks the `window.external` bridge end to end with fake dialogs.
//! Run: `cargo run --example bridge_check`. Exits 0 when every step passed, 1 on a failure, 2 on timeout.

use elk_planner_window::{run_with_dialogs, Dialogs};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>bridge</title><body>bridge test<script>
const NATIVE = !!(window.external && typeof window.external.sendMessage === 'function');
const got = [];
const send = o => window.external.sendMessage(JSON.stringify(o));
const report = (ok, info) => fetch('/report', { method: 'POST', body: JSON.stringify({ ok, info }) });
if (!NATIVE) report(false, 'NATIVE is false');
else {
  window.external.receiveMessage(raw => {
    const m = JSON.parse(raw); got.push(m);
    if (got.length === 1) send({ cmd: 'saveFile', purpose: 'plan', ext: '.json', name: 'plan', b64: btoa('{"x":1}') });
    else if (got.length === 2) send({ cmd: 'openFile', purpose: 'zones' });
    else if (got.length === 3) send({ cmd: 'nonsense' }), send({ cmd: 'saveFile' });
    else report(true, got);
  });
  send({ cmd: 'openFile', purpose: 'plan' });
}
</script>"#;

struct Fake {
    dir: PathBuf,
    opens: AtomicUsize,
}

impl Dialogs for Fake {
    fn pick_open(&self, _title: &str) -> Option<PathBuf> {
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            let p = self.dir.join("in.json");
            std::fs::write(&p, b"hello").unwrap();
            Some(p)
        } else {
            None // second open: cancelled
        }
    }
    fn pick_save(&self, _title: &str, _dir: Option<&Path>, _suggested: &str) -> Option<PathBuf> {
        Some(self.dir.join("out")) // no extension: the host must add ".json"
    }
}

fn main() {
    let dir = std::env::temp_dir().join(format!("elk-bridge-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let d2 = dir.clone();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let mut s = s;
            let mut buf = vec![0u8; 65536];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            if req.starts_with("POST /report") {
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("");
                println!("report: {}", body);
                let saved = std::fs::read(d2.join("out.json")).ok();
                let ok = body.starts_with("{\"ok\":true")
                    && saved.as_deref() == Some(b"{\"x\":1}".as_slice())
                    && body.contains("\"cmd\":\"opened\"")
                    && body.contains("aGVsbG8=")
                    && body.contains("\"cmd\":\"saved\"")
                    && body.contains("\"cmd\":\"cancelled\"")
                    && body.contains("\"cmd\":\"error\"");
                println!(
                    "saved file: {:?}; verdict: {}",
                    saved.map(|b| String::from_utf8_lossy(&b).into_owned()),
                    if ok { "PASS" } else { "FAIL" }
                );
                let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                std::process::exit(if ok { 0 } else { 1 });
            }
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", PAGE.len(), PAGE);
        }
    });
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(40));
        println!("timeout");
        std::process::exit(2);
    });
    if let Err(e) = run_with_dialogs(
        &url,
        Box::new(Fake {
            dir,
            opens: AtomicUsize::new(0),
        }),
    ) {
        println!("window failed: {}", e);
        std::process::exit(1);
    }
}
