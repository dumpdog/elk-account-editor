//! Elk account editor: a local web server on 127.0.0.1 that edits ElkRP account export files, shown in a native window
//! (or the browser with `--browser`). Port of `Elk.Editor/Program.cs`; the routes live in `server.rs`.
//!
//!   elkaccounteditor [--browser] [--no-open] [--data-dir <folder>] [export-file]

// Release builds on Windows are GUI-subsystem programs: no console window when double-clicked (see console.rs, ADR-017).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod console;
mod fallback;
mod library_routes;
mod server;
mod state;
mod update;
mod update_routes;

use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use elk_panel::Shared;
use state::App;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
struct Args {
    /// First argument not starting with `--`.
    file: Option<String>,
    browser: bool,
    no_open: bool,
    /// `--data-dir <folder>` or `--data-dir=<folder>`: the folder of the accounts library.
    data_dir: Option<String>,
}

fn parse_args(args: &[String]) -> Args {
    let mut a = Args {
        file: None,
        browser: false,
        no_open: false,
        data_dir: None,
    };
    let mut it = args.iter();
    while let Some(s) = it.next() {
        if s == "--data-dir" {
            a.data_dir = it.next().cloned();
        } else if let Some(v) = s.strip_prefix("--data-dir=") {
            a.data_dir = Some(v.to_string());
        } else if s == "--browser" {
            a.browser = true;
        } else if s == "--no-open" {
            a.no_open = true;
        } else if !s.starts_with("--") && a.file.is_none() {
            a.file = Some(s.clone());
        }
    }
    a
}

/// Binds 127.0.0.1 on `wanted`, or on any free port when that one is taken (a fixed port keeps the page's origin, and so its storage, stable).
fn bind(wanted: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], wanted)))
        .or_else(|_| TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))))
}

/// After an update the new program starts while the old one is still closing: wait (up to 10 s) for the usual port so the page keeps its address.
fn bind_after_restart(wanted: u16) -> std::io::Result<TcpListener> {
    for _ in 0..100 {
        if let Ok(l) = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], wanted))) {
            return Ok(l);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    bind(wanted)
}

fn port_from_env(value: Option<&str>) -> u16 {
    value.and_then(|v| v.trim().parse().ok()).unwrap_or(51980)
}

/// Opens `url` with the system's default handler.
fn open_in_browser(url: &str) -> bool {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().is_ok()
}

async fn route(State(state): State<Arc<App>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .to_string();
    let method = parts.method.to_string();
    let Ok(body) = axum::body::to_bytes(body, usize::MAX).await else {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("response");
    };
    let result =
        tokio::task::spawn_blocking(move || server::dispatch(&state, &method, &target, &body))
            .await;
    let r = result.unwrap_or_else(|_| elk_panel::HttpResult::server_error());
    let mut resp = Response::builder().status(r.status);
    if !r.content_type.is_empty() {
        resp = resp.header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&r.content_type).expect("content type"),
        );
    }
    resp.body(Body::from(r.body)).expect("response")
}

/// Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("a SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn main() {
    console::init();
    console::install_panic_hook();
    fallback::report_fallbacks();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv);

    let state = Arc::new(App::new(Shared::new(), args.data_dir.as_deref()));
    if let Some(f) = &args.file {
        let p = std::path::Path::new(f);
        if p.is_file()
            && let Ok(bytes) = std::fs::read(p)
        {
            let mut s = state.shared.session();
            s.original_text = Some(elk_account::decode_latin1(&bytes));
            s.source_name = p.file_name().map(|n| n.to_string_lossy().into_owned());
        }
    }

    update::cleanup_old(&update::Install::detect(&state.exe, state.library.dir()));
    let port = port_from_env(std::env::var("PORT").ok().as_deref());
    let listener = if std::env::var("ELKEDITOR_RESTART").is_ok() {
        bind_after_restart(port)
    } else {
        bind(port)
    }
    .expect("binding 127.0.0.1");
    listener
        .set_nonblocking(true)
        .expect("non-blocking listener");
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("local address").port()
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("async runtime");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let app = axum::Router::new()
        .fallback(route)
        .with_state(state.clone());
    let server = rt.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = stop_rx => {}
                    _ = shutdown_signal() => {}
                }
            })
            .await;
    });

    let mut windowed = false;
    if !args.browser {
        // The window runs on the main thread (macOS requires it).
        match elk_planner_window::run(&url) {
            Ok(()) => windowed = true,
            Err(e) => println!("Could not open a native window ({e}); using your browser instead."),
        }
    }
    if !windowed {
        println!("Elk account editor running at {url}  (Ctrl+C to quit)");
        if !args.no_open && !open_in_browser(&url) {
            println!("Open that address in your browser.");
        }
        let _ = rt.block_on(server);
    } else {
        let _ = stop_tx.send(());
        let _ = rt.block_on(server);
    }
    state.shared.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn args_pick_first_non_flag() {
        let a = parse_args(&v(&["--browser", "--no-open", "a.txt", "b.txt"]));
        assert_eq!(
            a,
            Args {
                file: Some("a.txt".into()),
                browser: true,
                no_open: true,
                data_dir: None
            }
        );
        assert_eq!(
            parse_args(&v(&[])),
            Args {
                file: None,
                browser: false,
                no_open: false,
                data_dir: None
            }
        );
        // the folder after --data-dir is not taken for the export file
        let a = parse_args(&v(&["--data-dir", "D:/data", "x.txt"]));
        assert_eq!(
            (a.file.as_deref(), a.data_dir.as_deref()),
            (Some("x.txt"), Some("D:/data"))
        );
        let a = parse_args(&v(&["--data-dir=/tmp/e", "--browser"]));
        assert_eq!((a.file, a.data_dir.as_deref()), (None, Some("/tmp/e")));
    }

    #[test]
    fn port_defaults() {
        assert_eq!(port_from_env(None), 51980);
        assert_eq!(port_from_env(Some("abc")), 51980);
        assert_eq!(port_from_env(Some("52001")), 52001);
    }

    #[test]
    fn busy_port_falls_back() {
        let first = bind(0).unwrap();
        let busy = first.local_addr().unwrap().port();
        let second = bind(busy).unwrap();
        assert_ne!(second.local_addr().unwrap().port(), busy);
    }
}
