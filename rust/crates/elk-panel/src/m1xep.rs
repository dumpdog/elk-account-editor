//! The M1XEP settings routes (`/api/m1xep/*`): read the module's own setup, read the copy that is stored in the open account file, discover modules on the
//! network, and (behind a diff, a backup and two confirmations) write changed settings. A Rust-only feature, see `rust/M1XEP-NOTES.md` and ADR-013.
//!
//! The conversation with the module is in `elk_core::xep_config`; this file is the request / response and file handling around it.

use crate::error::{Kind, PanelError};
use crate::live::LivePanel;
use crate::routes::{blank, fail, now_hms, parse_panel_req, settings_of, HttpResult, PanelReq, Shared};
use chrono::Local;
use elk_account::AccountFile;
use elk_core::xep_config::{discover, XepBlocks, XepChange, XepHex, XepInfo, XepModule, XepSettings, DISCOVERY_PORT};
use elk_core::ElkError;
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

/// Link failures plus `InvalidOperation` (old firmware) and the argument errors the validation raises.
const XEP_CATCH: [Kind; 6] = [Kind::Timeout, Kind::Io, Kind::Unauthorized, Kind::Argument, Kind::InvalidData, Kind::InvalidOperation];

/// How long the module may take to acknowledge a block (ElkRP: "may take up to a minute").
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// Routes a request. `None` when the path is not one of the `/api/m1xep/*` routes.
pub fn handle(state: &Shared, method: &str, path: &str, body: &[u8]) -> Option<HttpResult> {
    let post = method.eq_ignore_ascii_case("POST");
    let get = method.eq_ignore_ascii_case("GET");
    Some(match (path, get, post) {
        ("/api/m1xep/read", _, true) => handle_read(state, body),
        ("/api/m1xep/file", _, true) => handle_file(state),
        ("/api/m1xep/find", _, true) => handle_find(body),
        ("/api/m1xep/write", _, true) => handle_write(state, body),
        ("/api/m1xep/backups", true, _) => handle_backups(state),
        ("/api/m1xep/load", _, true) => handle_load(state, body),
        _ => return None,
    })
}

fn json_ok(v: Value) -> HttpResult {
    HttpResult::json(v.to_string())
}

fn module_json(i: &XepInfo) -> Value {
    json!({ "firmware": i.firmware, "hardware": i.hardware, "boot": i.boot, "appVersion": i.app_version })
}

fn panel_request(body: &[u8]) -> std::result::Result<PanelReq, HttpResult> {
    match parse_panel_req(body, "PanelRequest", false) {
        Ok(Some(p)) if !blank(&p.host) && !blank(&p.code) => Ok(p),
        Ok(_) => Err(HttpResult::bad_request("Host and RP access code are needed.")),
        Err(m) => Err(HttpResult::bad_request(&m)),
    }
}

fn run<T>(live: &LivePanel, pr: &PanelReq, log: &mut Vec<String>, work: impl FnOnce(&mut elk_core::ElkSession) -> crate::error::Result<T>) -> crate::error::Result<T> {
    live.use_session(&settings_of(pr), &mut |m| log.push(m.to_string()), work)
}

/// `POST /api/m1xep/read`: the module's settings, read only (`75 06`, `75 01`, `75 02`, `75 04`, `75 07`, `75 08` and `75 03 n`).
fn handle_read(state: &Shared, body: &[u8]) -> HttpResult {
    let pr = match panel_request(body) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let mut log = Vec::new();
    match run(&state.live, &pr, &mut log, |s| Ok(XepModule::read(s)?)) {
        Ok((info, blocks)) => json_ok(json!({ "module": module_json(&info), "settings": XepSettings::decode(&blocks), "at": now_hms() })),
        Err(e) => fail(&e, &XEP_CATCH, "Could not read the M1XEP: ", Some(&log)),
    }
}

/// `POST /api/m1xep/file`: the M1XEP Setup block stored in the open account file (what ElkRP would send with its Send button).
fn handle_file(state: &Shared) -> HttpResult {
    let text = state.session().original_text.clone();
    let Some(text) = text else {
        return json_ok(json!({ "found": false, "note": "No account file is open." }));
    };
    let Ok(f) = AccountFile::parse(&text) else {
        return json_ok(json!({ "found": false, "note": "The open file could not be read." }));
    };
    let Some(tail) = f.lines_after_automation() else {
        return json_ok(json!({ "found": false, "note": "This file has no M1XEP Setup block (account software older than 5.0, or the automation section was not found)." }));
    };
    match XepBlocks::from_lines(tail) {
        Some(b) => json_ok(json!({ "found": true, "settings": XepSettings::decode(&b) })),
        None => json_ok(json!({ "found": false, "note": format!("The {} lines after the automation section do not have the shape of an M1XEP Setup block.", tail.len()) })),
    }
}

/// `POST /api/m1xep/find` `{host}`: ElkRP's "Find": the ASCII request `XEPID` on UDP 2362, to the host (or the broadcast address when none is given).
fn handle_find(body: &[u8]) -> HttpResult {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let host = v.get("host").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (target, broadcast) = if host.is_empty() {
        (SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT), true)
    } else {
        match host.parse::<Ipv4Addr>() {
            Ok(ip) => (SocketAddr::new(IpAddr::V4(ip), DISCOVERY_PORT), false),
            Err(_) => return HttpResult::bad_request("Give the module's IP address (like 192.168.1.20), or leave the address empty to search the whole network."),
        }
    };
    match discover(target, broadcast, Duration::from_millis(2500)) {
        Ok(list) => json_ok(json!({ "found": list })),
        Err(e) => HttpResult::bad_request(&format!("The search could not run: {e}")),
    }
}

fn backup_dir(state: &Shared) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(&state.backup_dir)?;
    Ok(state.backup_dir.clone())
}

/// Saves what the module held before a write: `m1xep-<serial>-<time>-before-write.json` in the backup folder.
fn save_backup(state: &Shared, serial: &str, info: &XepInfo, blocks: &XepBlocks, why: &str) -> std::io::Result<PathBuf> {
    let p = backup_dir(state)?.join(format!("m1xep-{serial}-{}-{why}.json", Local::now().format("%Y%m%d-%H%M%S")));
    let doc = json!({
        "kind": "m1xep-backup",
        "version": 1,
        "savedAt": Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "serial": serial,
        "module": module_json(info),
        "blocks": blocks.to_hex(),
        "settings": XepSettings::decode(blocks),
    });
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap_or_default())?;
    Ok(p)
}

/// `GET /api/m1xep/backups`: the saved module backups, newest first.
fn handle_backups(state: &Shared) -> HttpResult {
    let Ok(dir) = backup_dir(state) else {
        return HttpResult::server_error();
    };
    let mut files: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let ok = name.to_lowercase().starts_with("m1xep-") && name.to_lowercase().ends_with(".json") && e.metadata().is_ok_and(|m| m.is_file());
                    ok.then(|| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), name))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    json_ok(Value::Array(
        files
            .iter()
            .map(|(t, n)| {
                let when: chrono::DateTime<Local> = (*t).into();
                json!({ "name": n, "when": when.format("%Y-%m-%d %H:%M:%S").to_string() })
            })
            .collect(),
    ))
}

/// `POST /api/m1xep/load` `{file}`: the settings of a saved backup, to review (and, through the write route, to put back). Sends nothing.
fn handle_load(state: &Shared, body: &[u8]) -> HttpResult {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = v.get("file").and_then(Value::as_str).unwrap_or("");
    let name = name.rsplit(['/', '\\']).next().unwrap_or("");
    if !name.to_lowercase().starts_with("m1xep-") || !name.to_lowercase().ends_with(".json") {
        return HttpResult::bad_request("That is not an M1XEP backup file.");
    }
    let Ok(dir) = backup_dir(state) else {
        return HttpResult::server_error();
    };
    let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
        return HttpResult::bad_request("That backup file was not found.");
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let parsed: Option<(XepHex, Value)> = serde_json::from_str::<Value>(text).ok().and_then(|d| {
        let h: XepHex = serde_json::from_value(d.get("blocks")?.clone()).ok()?;
        Some((h, d))
    });
    let Some((hexes, doc)) = parsed else {
        return HttpResult::bad_request("That file is not a valid M1XEP backup.");
    };
    match XepBlocks::from_hex(&hexes) {
        Ok(b) => json_ok(json!({ "settings": XepSettings::decode(&b), "savedAt": doc.get("savedAt"), "module": doc.get("module") })),
        Err(e) => HttpResult::bad_request(&format!("That file is not a valid M1XEP backup: {e}")),
    }
}

/// The result of a write that went through.
struct Written {
    backup: PathBuf,
    sent: Vec<String>,
    verified: bool,
    verify_note: String,
}

/// `POST /api/m1xep/write`: `{host, code, ..., settings, apply, confirm, allowIdentity}`.
///
/// Always reads the module first and works out the changes against what it holds now (never against what the page believes). With `apply` false it
/// only answers the diff. With `apply` true it needs `confirm`, refuses identity changes (IP, mask, gateway, DHCP, DNS, ports, user names and passwords)
/// unless `allowIdentity`, saves the current settings to a backup file, sends only the blocks that differ, then reads again and compares.
fn handle_write(state: &Shared, body: &[u8]) -> HttpResult {
    let pr = match panel_request(body) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(sv) = v.get("settings").filter(|s| s.is_object()) else {
        return HttpResult::bad_request("No settings were sent.");
    };
    let new: XepSettings = match serde_json::from_value(sv.clone()) {
        Ok(s) => s,
        Err(e) => return HttpResult::bad_request(&format!("The settings could not be read: {e}")),
    };
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (apply, confirm, allow_identity) = (flag("apply"), flag("confirm"), flag("allowIdentity"));
    let mut log = Vec::new();
    let r = run(&state.live, &pr, &mut log, |s| {
        let (info, held) = XepModule::read(s)?;
        let old = XepSettings::decode(&held);
        let (to, changes) = XepSettings::apply(&held, &old, &new)?;
        let identity = changes.iter().any(|c| c.identity);
        if !apply || changes.is_empty() {
            return Ok((info, changes, identity, None));
        }
        if !confirm {
            return Err(PanelError::Elk(ElkError::Argument("Tick the confirmation before sending anything to the module.".into())));
        }
        if identity && !allow_identity {
            let names: Vec<&str> = changes.iter().filter(|c| c.identity).map(|c| c.field.as_str()).collect();
            return Err(PanelError::Elk(ElkError::Argument(format!(
                "These changes decide how the module is reached or who may log in: {}. A wrong value can lock you out of the panel's network connection. Tick the separate warning to allow them.",
                names.join(", ")
            ))));
        }
        let serial = s.panel.as_ref().map_or("unknown".to_string(), |p| p.serial_hex.clone());
        let backup = save_backup(state, &serial, &info, &held, "before-write")?;
        let sent = XepModule::write(s, &held, &to, WRITE_TIMEOUT)?;
        // the module may drop the link while it reconfigures itself after a network change: then it cannot be checked
        let (verified, verify_note) = match XepModule::read(s) {
            Ok((_, now)) if now == to => (true, "Read back from the module: identical to what was sent.".to_string()),
            Ok(_) => (false, "Read back from the module: it holds something different from what was sent.".to_string()),
            Err(e) => (false, format!("Could not read back to check ({e}). If the network settings changed this is normal while the module restarts.")),
        };
        Ok((info, changes, identity, Some(Written { backup, sent, verified, verify_note })))
    });
    match r {
        Ok((info, changes, identity, written)) => {
            let mut out = json!({
                "module": module_json(&info),
                "changes": changes_json(&changes),
                "identityChanges": identity,
                "applied": written.is_some(),
                "at": now_hms(),
            });
            if let Some(w) = written {
                out["backup"] = json!({ "path": w.backup.to_string_lossy(), "name": w.backup.file_name().map(|n| n.to_string_lossy().into_owned()) });
                out["sent"] = json!(w.sent);
                out["verified"] = json!(w.verified);
                out["verifyNote"] = json!(w.verify_note);
                out["rebootRequired"] = json!(true);
                out["rebootNote"] = json!("The module applies the new setup only after it is rebooted (ElkRP: Account Details > M1XEP Setup > Reboot, or power-cycle the module). Rebooting drops the editor's connection to the panel.");
            } else if apply {
                out["note"] = json!("Nothing differs from what the module holds: nothing was sent.");
            }
            json_ok(out)
        }
        Err(e) => fail(&e, &XEP_CATCH, "The M1XEP settings were not sent: ", Some(&log)),
    }
}

fn changes_json(c: &[XepChange]) -> Value {
    serde_json::to_value(c).unwrap_or(Value::Null)
}
