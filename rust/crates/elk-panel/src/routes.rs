//! Request and response logic of the panel and live routes of `Program.cs`, as framework-free functions.
//!
//! Every handler has the shape `fn(&Shared, body: &[u8], &Query) -> HttpResult`, so any HTTP server can call it (the editor's axum server calls them from
//! `spawn_blocking`). [`Shared`] is internally synchronised: share it as `Arc<Shared>`. Status codes, JSON member names and order, and error texts follow
//! the C# (`Results.Json` with the web defaults, `Results.BadRequest(string)` = a JSON string body).

use crate::auto_sync;
use crate::error::{Kind, PanelError, Result};
use crate::jsonerr::{self, Span};
use crate::jsonw;
use crate::live::LivePanel;
use crate::sync::{PanelSync, Settings};
use chrono::{Local, NaiveDate, NaiveDateTime};
use elk_account::{AccountFile, AccountModel};
use elk_core::{BusModule, ElkError, LiveSnapshot, PanelAccess, PanelBackup, PanelClock, PanelControl, PanelFile, PanelLog, PanelModules, PanelProgress, ProgressSnapshot};
use serde_json::Value;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const JSON_UTF8: &str = "application/json; charset=utf-8";
const TEMPLATE_M1G: &[u8] = include_bytes!("template-m1g.txt");

/// The response of a handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResult {
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type` header value (empty for an empty 500, like ASP.NET).
    pub content_type: String,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl HttpResult {
    /// 200 with a JSON body that is already serialised.
    pub fn json(body: String) -> Self {
        HttpResult { status: 200, content_type: JSON_UTF8.into(), body: body.into_bytes() }
    }
    /// `Results.BadRequest(message)`: 400 with the message as a JSON string.
    pub fn bad_request(message: &str) -> Self {
        let mut s = String::new();
        jsonw::push_str_relaxed(&mut s, message);
        HttpResult { status: 400, content_type: JSON_UTF8.into(), body: s.into_bytes() }
    }
    /// An unhandled exception: 500 with no body.
    pub fn server_error() -> Self {
        HttpResult { status: 500, content_type: String::new(), body: Vec::new() }
    }
    /// The body as text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// The query string of a request (percent-decoded).
#[derive(Debug, Clone, Default)]
pub struct Query(Vec<(String, String)>);

impl Query {
    /// No parameters.
    pub fn new() -> Self {
        Query(Vec::new())
    }
    /// Parses `a=b&c=d` (a leading `?` is allowed; `+` is a space).
    pub fn parse(q: &str) -> Self {
        let q = q.strip_prefix('?').unwrap_or(q);
        let mut v = Vec::new();
        for part in q.split('&').filter(|p| !p.is_empty()) {
            let (k, val) = part.split_once('=').unwrap_or((part, ""));
            v.push((pct_decode(k), pct_decode(val)));
        }
        Query(v)
    }
    /// The first value of `key` (names are case-insensitive in ASP.NET).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The open file the panel routes work on (the `originalText` / `sourceName` locals of `Program.cs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The open account file as text (Latin-1 decoded), `None` until a file is opened.
    pub original_text: Option<String>,
    /// Its display name (`?name=` of `/api/open`, the file name, `New account`, `From panel`).
    pub source_name: Option<String>,
}

/// Everything the panel and live routes share. Hold it as `Arc<Shared>`; all methods take `&self`.
pub struct Shared {
    /// The open file (the other, non-panel routes of the server read and write it too).
    pub session: Mutex<Session>,
    /// The shared panel session.
    pub live: LivePanel,
    /// Progress of the running panel operation (polled by `/api/panel/progress`).
    pub progress: Arc<PanelProgress>,
    /// Where backups are kept (`Documents\ElkEditor backups`).
    pub backup_dir: PathBuf,
    /// Pause before the status read that follows a remote-control command (C#: 600 ms).
    pub control_delay: Duration,
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

/// `Documents\ElkEditor backups` (`Environment.SpecialFolder.MyDocuments`; on Linux .NET returns the home folder).
/// On macOS this is `~/Documents/ElkEditor backups`, as the page and the Mac README say (the C# build used the home folder there).
pub fn default_backup_dir() -> PathBuf {
    let docs = if cfg!(any(windows, target_os = "macos")) { dirs::document_dir() } else { dirs::home_dir() };
    docs.unwrap_or_else(|| PathBuf::from(".")).join("ElkEditor backups")
}

impl Shared {
    /// State with the default backup folder.
    pub fn new() -> Self {
        Self::with_backup_dir(default_backup_dir())
    }
    /// State with a given backup folder.
    pub fn with_backup_dir(dir: PathBuf) -> Self {
        Shared {
            session: Mutex::new(Session::default()),
            live: LivePanel::new(),
            progress: Arc::new(PanelProgress::new()),
            backup_dir: dir,
            control_delay: Duration::from_millis(600),
        }
    }
    /// The open-file state.
    pub fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `ApplicationStopping`: closes the panel session politely.
    pub fn shutdown(&self) {
        self.live.disconnect();
    }
    fn backup_dir(&self) -> std::io::Result<&Path> {
        std::fs::create_dir_all(&self.backup_dir)?;
        Ok(&self.backup_dir)
    }
    fn save_backup(&self, f: &PanelFile, why: &str) -> Result<PathBuf> {
        let dir = self.backup_dir()?;
        let p = dir.join(format!("panel-{}-{}-{why}.json", f.serial.as_deref().unwrap_or("unknown"), Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::write(&p, f.to_json())?;
        Ok(p)
    }
}

/// The blank M1G 5.3 account (`Template()` in `Program.cs`) as Latin-1 text.
pub fn template_text() -> String {
    elk_account::decode_latin1(TEMPLATE_M1G)
}

// ---- small JSON writer -------------------------------------------------------------------------------------------------------------------------------

struct Obj {
    s: String,
    n: usize,
}

impl Obj {
    fn new() -> Self {
        Obj { s: "{".into(), n: 0 }
    }
    fn key(&mut self, k: &str) {
        if self.n > 0 {
            self.s.push(',');
        }
        self.n += 1;
        jsonw::push_str(&mut self.s, k);
        self.s.push(':');
    }
    fn raw(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        self.s.push_str(v);
        self
    }
    fn str(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        jsonw::push_str(&mut self.s, v);
        self
    }
    fn opt(mut self, k: &str, v: Option<&str>) -> Self {
        self.key(k);
        match v {
            Some(v) => jsonw::push_str(&mut self.s, v),
            None => self.s.push_str("null"),
        }
        self
    }
    fn bool(mut self, k: &str, v: bool) -> Self {
        self.key(k);
        self.s.push_str(if v { "true" } else { "false" });
        self
    }
    fn int(mut self, k: &str, v: i64) -> Self {
        self.key(k);
        self.s.push_str(&v.to_string());
        self
    }
    fn f64(mut self, k: &str, v: f64) -> Self {
        self.key(k);
        jsonw::push_f64(&mut self.s, v);
        self
    }
    fn end(mut self) -> String {
        self.s.push('}');
        self.s
    }
}

fn arr(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

fn str_arr(items: &[String]) -> String {
    arr(items.iter().map(|s| {
        let mut o = String::new();
        jsonw::push_str(&mut o, s);
        o
    }))
}

fn module_json(m: &BusModule) -> String {
    Obj::new()
        .str("kind", &m.kind)
        .int("address", m.address as i64)
        .str("model", &m.model)
        .str("hardware", &m.hardware)
        .str("boot", &m.boot)
        .str("firmware", &m.firmware)
        .bool("responding", m.responding)
        .end()
}

fn snapshot_json(s: &LiveSnapshot) -> String {
    let areas = arr(s.areas.iter().map(|a| {
        Obj::new()
            .int("area", a.area as i64)
            .bool("ready", a.ready)
            .bool("readyToForceArm", a.ready_to_force_arm)
            .int("armed", a.armed as i64)
            .str("armedName", &a.armed_name)
            .bool("alarm", a.alarm)
            .end()
    }));
    let zones = arr(s.zones.iter().map(|z| {
        Obj::new()
            .int("zone", z.zone as i64)
            .str("state", &z.state)
            .bool("bypassed", z.bypassed)
            .bool("forceArmed", z.force_armed)
            .str("physical", &z.physical)
            .f64("volts", z.volts)
            .end()
    }));
    Obj::new().raw("areas", &areas).raw("zones", &zones).raw("system", &str_arr(&s.system)).end()
}

/// `ProgressSnapshot` as `/api/panel/progress` serves it.
pub fn progress_json(p: &ProgressSnapshot) -> String {
    Obj::new().bool("active", p.active).str("phase", &p.phase).str("note", &p.note).int("done", p.done as i64).int("total", p.total as i64).f64("seconds", p.seconds).end()
}

pub(crate) fn now_hms() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn now_full() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn fmt_full(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---- request parsing (System.Text.Json web defaults: names ignore case, numbers may be quoted) ------------------------------------------------------------

struct Req {
    ty: &'static str,
    map: serde_json::Map<String, Value>,
    spans: Vec<Span>,
    errors: RefCell<Vec<(usize, String)>>,
}

/// `JsonSerializer.DeserializeAsync<T>` for the record `ty`: `Err` is a `JsonException` message, `Ok(None)` is a JSON `null` body.
fn parse_req(body: &[u8], ty: &'static str) -> std::result::Result<Option<Req>, String> {
    let (spans, root_start, root_end) = jsonerr::scan(body)?;
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(body);
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Null) => Ok(None),
        Ok(Value::Object(map)) => Ok(Some(Req { ty, map, spans, errors: RefCell::new(Vec::new()) })),
        Ok(Value::Array(_)) => Err(format!("The JSON value could not be converted to {ty}. Path: $ | LineNumber: 0 | BytePositionInLine: {}.", root_start + 1)),
        Ok(_) => Err(format!("The JSON value could not be converted to {ty}. Path: $ | LineNumber: 0 | BytePositionInLine: {root_end}.")),
        Err(e) => Err(e.to_string()),
    }
}

impl Req {
    fn find(&self, name: &str) -> Option<&Value> {
        self.map.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }
    /// Notes a "could not be converted" error (the first one in document order is the one reported) and returns nothing to use.
    fn fail(&self, name: &str, ty: Option<&str>) {
        let sp = self.spans.iter().rev().find(|s| s.key.eq_ignore_ascii_case(name));
        let (key, start, pos) = match (sp, self.find(name)) {
            (Some(s), Some(Value::Object(_) | Value::Array(_))) => (s.key.as_str(), s.start, s.start + 1),
            (Some(s), _) => (s.key.as_str(), s.start, s.end),
            _ => (name, 0, 0),
        };
        let m = format!("The JSON value could not be converted to {}. Path: $.{key} | LineNumber: 0 | BytePositionInLine: {pos}.", ty.unwrap_or(self.ty));
        self.errors.borrow_mut().push((start, m));
    }
    fn string(&self, name: &str) -> Option<String> {
        match self.find(name) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => {
                self.fail(name, None);
                None
            }
        }
    }
    fn int(&self, name: &str) -> i32 {
        let v = match self.find(name) {
            None => Some(0),
            Some(Value::Number(n)) => n.as_i64().and_then(|v| i32::try_from(v).ok()),
            Some(Value::String(s)) => s.trim().parse::<i32>().ok(), // AllowReadingFromString
            Some(_) => None,
        };
        v.unwrap_or_else(|| {
            self.fail(name, None);
            0
        })
    }
    fn bool(&self, name: &str) -> bool {
        match self.find(name) {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                self.fail(name, None);
                false
            }
        }
    }
    /// `Err` with the `JsonException` message of the first member (in document order) that could not be converted.
    fn check(&self) -> std::result::Result<(), String> {
        match self.errors.borrow().iter().min_by_key(|e| e.0) {
            Some((_, m)) => Err(m.clone()),
            None => Ok(()),
        }
    }
}

/// `PanelRequest` and `RestoreRequest` (the restore adds `File` and `Apply`).
pub(crate) struct PanelReq {
    pub(crate) host: Option<String>,
    port: i32,
    pub(crate) code: Option<String>,
    plain: bool,
    mac: Option<String>,
    passphrase: Option<String>,
    model: Option<AccountModel>,
    reset_missing: bool,
    send_local_code: bool,
    tls: bool,
    serial: Option<String>,
    file: Option<String>,
    apply: bool,
    /// Rust only: also compare / send / read / back up the automation items (`"automation": true` in the request).
    automation: bool,
    /// Rust only (pull): how many rules the page has marked disabled; ElkRP refuses to receive rules then.
    disabled_rules: i32,
}

/// Checks the JSON syntax of a request body the way System.Text.Json reports it (`Err` is the `JsonException` message).
pub fn json_syntax_check(body: &[u8]) -> std::result::Result<(), String> {
    jsonerr::scan(body).map(|_| ())
}

/// `JsonSerializer.DeserializeAsync<AccountModel>(body)`: `Ok(None)` is a JSON `null` body, `Err` a `JsonException` message
/// (syntax errors use the .NET wording; type errors inside the model use serde's text plus `Path: $`).
pub fn parse_model_body(body: &[u8]) -> std::result::Result<Option<AccountModel>, String> {
    jsonerr::scan(body)?;
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(body);
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Null) => Ok(None),
        Ok(v @ Value::Object(_)) => serde_json::from_value::<AccountModel>(v).map(Some).map_err(|e| format!("{e} Path: $")),
        Ok(_) => Err("The JSON value could not be converted to Elk.Protocol.AccountModel. Path: $ | LineNumber: 0 | BytePositionInLine: 0.".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn parse_panel_req(body: &[u8], ty: &'static str, with_model: bool) -> std::result::Result<Option<PanelReq>, String> {
    let Some(r) = parse_req(body, ty)? else {
        return Ok(None);
    };
    let model = if with_model {
        match r.find("model") {
            None | Some(Value::Null) => None,
            Some(v @ Value::Object(_)) => match serde_json::from_value::<AccountModel>(v.clone()) {
                Ok(m) => Some(m),
                Err(e) => {
                    let start = r.spans.iter().rev().find(|s| s.key.eq_ignore_ascii_case("model")).map_or(0, |s| s.start);
                    r.errors.borrow_mut().push((start, format!("{e} Path: $.model")));
                    None
                }
            },
            Some(_) => {
                r.fail("model", Some("Elk.Protocol.AccountModel"));
                None
            }
        }
    } else {
        None
    };
    let pr = PanelReq {
        host: r.string("host"),
        port: r.int("port"),
        code: r.string("code"),
        plain: r.bool("plain"),
        mac: r.string("mac"),
        passphrase: r.string("passphrase"),
        model,
        reset_missing: ty == "PanelRequest" && r.bool("resetMissing"),
        send_local_code: ty == "PanelRequest" && r.bool("sendLocalCode"),
        tls: r.bool("tls"),
        serial: r.string("serial"),
        file: if ty == "RestoreRequest" { r.string("file") } else { None },
        apply: ty == "RestoreRequest" && r.bool("apply"),
        automation: ty == "PanelRequest" && r.bool("automation"),
        disabled_rules: if ty == "PanelRequest" { r.int("disabledRules") } else { 0 },
    };
    r.check()?;
    Ok(Some(pr))
}

pub(crate) fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|v| v.trim().is_empty())
}

pub(crate) fn settings_of(pr: &PanelReq) -> Settings {
    Settings {
        host: pr.host.as_deref().unwrap_or("").trim().to_string(),
        port: pr.port,
        code: pr.code.as_deref().unwrap_or("").trim().to_string(),
        plain: pr.plain,
        mac: pr.mac.clone(),
        passphrase: pr.passphrase.clone(),
        tls: pr.tls,
        serial: pr.serial.as_deref().map(|s| s.trim().to_string()),
    }
}

// ---- catch lists ---------------------------------------------------------------------------------------------------------------------------------------

/// `catch (TimeoutException or IOException or SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException)`.
pub(crate) const PANEL_CATCH: [Kind; 5] = [Kind::Timeout, Kind::Io, Kind::Unauthorized, Kind::Argument, Kind::InvalidData];
/// Link failures plus `InvalidOperationException` (the "open" live routes).
const LINK_CATCH: [Kind; 4] = [Kind::InvalidOperation, Kind::Timeout, Kind::Io, Kind::InvalidData];

fn caught(e: &PanelError, kinds: &[Kind]) -> bool {
    let k = e.kind();
    kinds.iter().any(|x| *x == k || (*x == Kind::Argument && k == Kind::OutOfRange))
}

fn log_suffix(log: &[String]) -> String {
    if log.is_empty() {
        String::new()
    } else {
        format!("\n{}", log.join("\n"))
    }
}

/// A caught exception becomes `400 prefix + message (+ log)`, anything else the empty 500.
pub(crate) fn fail(e: &PanelError, kinds: &[Kind], prefix: &str, log: Option<&[String]>) -> HttpResult {
    if caught(e, kinds) {
        HttpResult::bad_request(&format!("{prefix}{}{}", e.message(), log.map(log_suffix).unwrap_or_default()))
    } else {
        HttpResult::server_error()
    }
}

/// `LinkFail`: an `InvalidOperationException` is shown as is, the rest gets the `what` prefix.
fn link_fail(e: &PanelError, what: &str) -> HttpResult {
    if e.kind() == Kind::InvalidOperation {
        HttpResult::bad_request(&e.message())
    } else {
        HttpResult::bad_request(&format!("{what}: {}", e.message()))
    }
}

/// elk-core words some `ArgumentOutOfRangeException` messages differently from .NET (`new ArgumentOutOfRangeException()` has the framework's stock text);
/// put the .NET text back where a handler returns it to the page.
fn fix_range(e: PanelError) -> PanelError {
    match e {
        PanelError::Elk(ElkError::OutOfRange(m)) => PanelError::Elk(ElkError::OutOfRange(match m.as_str() {
            "zone or area out of range" | "area or level out of range" | "area out of range" => "Specified argument was out of the range of valid values.".into(),
            "the panel keeps years 2000 to 2099" => "the panel keeps years 2000 to 2099 (Parameter 't')".into(),
            _ => m,
        })),
        other => other,
    }
}

fn tracked(state: &Shared, run: impl FnOnce() -> HttpResult) -> HttpResult {
    let r = run();
    state.progress.end();
    r
}

fn with_transfer_ticks<T>(state: &Shared, s: &mut elk_core::ElkSession, run: impl FnOnce(&mut elk_core::ElkSession) -> T) -> T {
    let p = state.progress.clone();
    s.on_transfer = Some(Box::new(move || p.tick()));
    let r = run(s);
    s.on_transfer = None;
    r
}

// ---- /api/panel/progress -------------------------------------------------------------------------------------------------------------------------------

/// `GET /api/panel/progress`.
pub fn handle_panel_progress(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    HttpResult::json(progress_json(&state.progress.snapshot()))
}

// ---- /api/panel/compare and /send ----------------------------------------------------------------------------------------------------------------------

/// `POST /api/panel/compare`: compares the edited model with the panel; nothing is written.
pub fn handle_panel_compare(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    tracked(state, || panel_call(state, body, false))
}

/// `POST /api/panel/send`: writes what differs (after saving a backup), reads back and reports what still differs.
pub fn handle_panel_send(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    tracked(state, || panel_call(state, body, true))
}

fn panel_call(state: &Shared, body: &[u8], send: bool) -> HttpResult {
    let Some(original) = state.session().original_text.clone() else {
        return HttpResult::bad_request("Open a file first.");
    };
    let pr = match parse_panel_req(body, "PanelRequest", true) {
        Ok(p) => p,
        Err(m) => return HttpResult::bad_request(&m),
    };
    let pr = match pr {
        Some(p) if p.model.is_some() && !blank(&p.host) && !blank(&p.code) => p,
        _ => return HttpResult::bad_request("Host, RP access code and the edited data are needed."),
    };
    let mut f = match AccountFile::parse(&original).and_then(|mut f| pr.model.as_ref().expect("checked").apply_to(&mut f).map(|_| f)) {
        Ok(f) => f,
        Err(e) => return account_400(&e),
    };
    // ElkRP skips rules marked Disabled when it sends (the editor keeps the flag in the model; the file cannot hold it)
    let mut dropped = 0;
    if pr.automation {
        let off = elk_account::rules::disabled_indexes(&pr.model.as_ref().expect("checked").automation.as_ref().map(|a| a.rules.clone()).unwrap_or_default());
        if !off.is_empty() {
            if let Some(a) = f.automation.as_mut() {
                dropped = elk_account::rules::drop_disabled(a, &off);
            }
        }
    }
    let mut log: Vec<String> = Vec::new();
    let progress = state.progress.clone();
    progress.begin("Connecting to the panel", 0);
    let (reset, local, auto) = (pr.reset_missing, pr.send_local_code, pr.automation);
    let r = state.live.use_session(
        &settings_of(&pr),
        &mut |m| {
            log.push(m.to_string());
            progress.note(m);
        },
        |s| {
            with_transfer_ticks(state, s, |s| {
                let fw = s.panel.as_ref().map(|p| p.firmware.clone()).unwrap_or_default();
                let extra = if auto { auto_sync::estimate_diff(&f, &fw) } else { 0 };
                state.progress.next("Comparing with the panel", PanelSync::estimate_diff_transfers(&f, reset) + extra);
                let mut d = PanelSync::diff_with(s, &f, reset, local, auto)?;
                let mut written = 0;
                let mut backup_path: Option<String> = None;
                if send {
                    if !d.changes.is_empty() {
                        let backup_total = PanelBackup::estimate_transfers() + if auto { elk_core::estimate_backup_transfers() } else { 0 };
                        state.progress.next("Saving a backup of the panel first", backup_total);
                        let backup = if auto { PanelBackup::read_with_automation(s)? } else { PanelBackup::read(s)? };
                        backup_path = Some(state.save_backup(&backup, "before-send")?.to_string_lossy().into_owned());
                        // a failed backup stops the send
                    }
                    state.progress.next(&format!("Writing {} record(s) to the panel", d.changes.len()), PanelSync::write_transfers(&d.changes));
                    PanelSync::apply(s, &d.changes, None)?;
                    written = d.changes.len();
                    state.progress.next("Reading back to check", PanelSync::estimate_diff_transfers(&f, reset) + extra);
                    d = PanelSync::diff_with(s, &f, reset, local, auto)?; // read back: anything still different is reported
                }
                let panel = s.panel.as_ref();
                Ok((panel.map(|p| p.firmware.clone()), panel.map(|p| p.serial_hex.clone()), written, backup_path, d))
            })
        },
    );
    match r {
        Ok((firmware, serial, written, backup_path, mut d)) => {
            if dropped > 0 {
                d.skipped.push(format!("{dropped} disabled rule(s) are not sent (ElkRP does not send disabled rules; the panel keeps the enabled ones only)"));
            }
            let counts = arr(d.counts.iter().map(|(k, v)| Obj::new().str("label", k).int("same", v.0 as i64).int("differ", v.1 as i64).end()));
            let changes = arr(d.changes.iter().map(|c| Obj::new().str("label", &c.label).int("number", c.number as i64).end()));
            HttpResult::json(
                Obj::new()
                    .opt("firmware", firmware.as_deref())
                    .opt("serial", serial.as_deref())
                    .int("written", written as i64)
                    .raw("log", &str_arr(&log))
                    .opt("backupPath", backup_path.as_deref())
                    .raw("counts", &counts)
                    .raw("changes", &changes)
                    .raw("skipped", &str_arr(&d.skipped))
                    .end(),
            )
        }
        Err(e) => fail(&e, &PANEL_CATCH, "Could not talk to the panel: ", Some(&log)),
    }
}

/// `catch (InvalidDataException or FormatException or ArgumentOutOfRangeException)` around the account-file parse / apply.
fn account_400(e: &elk_account::Error) -> HttpResult {
    match e {
        elk_account::Error::InvalidData(_) | elk_account::Error::Format(_) | elk_account::Error::OutOfRange(_) => HttpResult::bad_request(e.message()),
        _ => HttpResult::server_error(),
    }
}

// ---- backups -------------------------------------------------------------------------------------------------------------------------------------------

/// `GET /api/panel/backups`: the saved backups, newest first.
pub fn handle_panel_backups(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    let Ok(dir) = state.backup_dir() else {
        return HttpResult::server_error();
    };
    let mut files: Vec<(std::time::SystemTime, String, u64)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return HttpResult::server_error();
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if !(lower.starts_with("panel-") && lower.ends_with(".json")) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        if !md.is_file() {
            continue;
        }
        files.push((md.modified().unwrap_or(std::time::UNIX_EPOCH), name, md.len()));
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0)); // newest first, stable
    HttpResult::json(arr(files.iter().map(|(t, name, len)| {
        let when: chrono::DateTime<Local> = (*t).into();
        Obj::new().str("name", name).str("when", &when.format("%Y-%m-%d %H:%M:%S").to_string()).int("kb", (*len / 1024) as i64).end()
    })))
}

/// `POST /api/panel/backup/delete` (Rust only, ADR-018): `{"name": "panel-....json"}` removes one saved backup. Only a plain `panel-*.json` file name inside the
/// backup folder is accepted (no path parts), so nothing else can be deleted through this route.
pub fn handle_panel_backup_delete(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    let name = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| v.get("name").and_then(|n| n.as_str().map(str::to_string))).unwrap_or_default();
    let lower = name.to_lowercase();
    if name.is_empty() || name.contains(['/', '\\']) || !(lower.starts_with("panel-") && lower.ends_with(".json")) {
        return HttpResult::bad_request("That is not a saved panel backup.");
    }
    let Ok(dir) = state.backup_dir() else {
        return HttpResult::server_error();
    };
    let path = dir.join(&name);
    if !path.is_file() {
        return HttpResult::bad_request("That backup file was not found.");
    }
    match std::fs::remove_file(&path) {
        Ok(()) => HttpResult::json("{\"ok\":true}".to_string()),
        Err(e) => HttpResult::bad_request(&format!("The backup could not be deleted: {e}")),
    }
}

/// `POST /api/panel/backups/open` (Rust only, ADR-018): shows the backup folder in the system file manager.
pub fn handle_panel_backups_open(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    let Ok(dir) = state.backup_dir() else {
        return HttpResult::server_error();
    };
    let prog = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(prog).arg(dir).spawn() {
        Ok(_) => HttpResult::json(format!("{{\"path\":{}}}", serde_json::to_string(&dir.display().to_string()).unwrap_or_default())),
        Err(e) => HttpResult::bad_request(&format!("The folder could not be opened: {e}")),
    }
}

/// `POST /api/panel/backup`: reads the whole panel and saves it as `panel-<serial>-<time>-manual.json`.
pub fn handle_panel_backup(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    tracked(state, || {
        // (not inside a try in the C#: a malformed body is an unhandled JsonException, HTTP 500)
        let pr = match parse_panel_req(body, "PanelRequest", false) {
            Ok(p) => p,
            Err(_) => return HttpResult::server_error(),
        };
        let pr = match pr {
            Some(p) if !blank(&p.host) && !blank(&p.code) => p,
            _ => return HttpResult::bad_request("Host and RP access code are needed."),
        };
        let progress = state.progress.clone();
        progress.begin("Connecting to the panel", 0);
        let mut log: Vec<String> = Vec::new();
        let r = state
            .live
            .use_session(
                &settings_of(&pr),
                &mut |m| {
                    log.push(m.to_string());
                    progress.note(m);
                },
                |s| {
                    with_transfer_ticks(state, s, |s| {
                        if pr.automation {
                            state.progress.next("Reading the whole panel", PanelBackup::estimate_transfers_with_automation());
                            Ok(PanelBackup::read_with_automation(s)?)
                        } else {
                            state.progress.next("Reading the whole panel", PanelBackup::estimate_transfers());
                            Ok(PanelBackup::read(s)?)
                        }
                    })
                },
            )
            .and_then(|f| {
                let p = state.save_backup(&f, "manual")?;
                Ok((p, f))
            });
        match r {
            Ok((p, f)) => HttpResult::json(
                Obj::new()
                    .str("path", &p.to_string_lossy())
                    .str("name", &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                    .int("zones", f.zones.len() as i64)
                    .int("blocks", f.blocks.len() as i64)
                    .end(),
            ),
            Err(e) => fail(&e, &PANEL_CATCH, "Could not read the panel: ", None),
        }
    })
}

/// `Path.GetFileName`.
fn file_name(p: &str) -> &str {
    let seps: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    p.rsplit(seps).next().unwrap_or("")
}

/// `POST /api/panel/restore`: `file` must be one of the saved backups (looked up by name in the backup folder). `apply: false` only lists what would change.
pub fn handle_panel_restore(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    tracked(state, || {
        let pr = match parse_panel_req(body, "RestoreRequest", false) {
            Ok(p) => p,
            Err(_) => return HttpResult::server_error(),
        };
        let pr = match pr {
            Some(p) if !blank(&p.host) && !blank(&p.code) && !blank(&p.file) => p,
            _ => return HttpResult::bad_request("Host, RP access code and a backup file are needed."),
        };
        let Ok(dir) = state.backup_dir() else {
            return HttpResult::server_error();
        };
        let path = dir.join(file_name(pr.file.as_deref().unwrap_or("")));
        if !path.is_file() {
            return HttpResult::bad_request("That backup file was not found.");
        }
        let mut log: Vec<String> = Vec::new();
        let mut changes: Vec<String> = Vec::new();
        let mut run = || -> Result<(i32, i32, PanelFile)> {
            let bytes = std::fs::read(&path)?;
            let text = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes)).into_owned();
            let backup = PanelFile::from_json(&text)?;
            let progress = state.progress.clone();
            progress.begin("Connecting to the panel", 0);
            let apply = pr.apply;
            let (n, left) = state.live.use_session(
                &settings_of(&pr),
                &mut |m| {
                    log.push(m.to_string());
                    progress.note(m);
                },
                |s| {
                    with_transfer_ticks(state, s, |s| {
                        state
                            .progress
                            .next(if apply { "Restoring the panel from the backup" } else { "Comparing the panel with the backup" }, PanelBackup::estimate_transfers_for(&backup));
                        let n = PanelBackup::update(s, &backup, apply, Some(&mut |c: &str| changes.push(c.to_string())))?;
                        if apply {
                            state.progress.next("Reading back to check", PanelBackup::estimate_transfers_for(&backup));
                        }
                        let left = if apply { PanelBackup::update(s, &backup, false, None)? } else { n }; // read back after a restore
                        Ok((n, left))
                    })
                },
            )?;
            Ok((n, left, backup))
        };
        match run() {
            Ok((n, left, backup)) => HttpResult::json(
                Obj::new()
                    .int("differ", n as i64)
                    .int("written", if pr.apply { n as i64 } else { 0 })
                    .int("remaining", left as i64)
                    .raw("changes", &str_arr(&changes))
                    .opt("backupFirmware", backup.firmware.as_deref())
                    .opt("backupSerial", backup.serial.as_deref())
                    .end(),
            ),
            Err(e) => fail(&e, &[Kind::Timeout, Kind::Io, Kind::Unauthorized, Kind::Argument, Kind::InvalidData, Kind::Json, Kind::Format], "Restore failed: ", None),
        }
    })
}

// ---- /api/panel/pull -----------------------------------------------------------------------------------------------------------------------------------

/// What a successful pull answers with.
struct PullOut {
    model_json: String,
    name: Option<String>,
    notes: Vec<String>,
    firmware: Option<String>,
    serial: Option<String>,
}

/// `POST /api/panel/pull`: reads the panel into the open file (a blank account when nothing is open) and returns the new model.
pub fn handle_panel_pull(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    tracked(state, || {
        let pr = match parse_panel_req(body, "PanelRequest", false) {
            Ok(p) => p,
            Err(m) => return HttpResult::bad_request(&m),
        };
        let pr = match pr {
            Some(p) if !blank(&p.host) && !blank(&p.code) => p,
            _ => return HttpResult::bad_request("Host and RP access code are needed."),
        };
        if pr.automation && pr.disabled_rules > 0 {
            return HttpResult::bad_request("Rules cannot be received while some are disabled. To receive Rules, you must first enable all of them.");
        }
        let mut log: Vec<String> = Vec::new();
        let run = |log: &mut Vec<String>| -> Result<PullOut> {
            let (original, name) = {
                let mut ses = state.session();
                if ses.original_text.is_none() {
                    // nothing open: read into a blank account
                    ses.original_text = Some(template_text());
                    ses.source_name = Some("From panel".into());
                }
                (ses.original_text.clone().expect("set"), ses.source_name.clone())
            };
            let mut f = AccountFile::parse(&original)?;
            let progress = state.progress.clone();
            progress.begin("Connecting to the panel", 0);
            let (notes, fw, serial) = state.live.use_session(
                &settings_of(&pr),
                &mut |m| {
                    log.push(m.to_string());
                    progress.note(m);
                },
                |s| {
                    let notes = with_transfer_ticks(state, s, |s| {
                        state.progress.next("Reading the panel", PanelSync::estimate_pull_transfers(&f));
                        PanelSync::pull_with(s, &mut f, Some(&state.progress), pr.automation)
                    })?;
                    let panel = s.panel.as_ref();
                    Ok((notes, panel.map(|p| p.firmware.clone()), panel.map(|p| p.serial_hex.clone())))
                },
            )?;
            let text = f.to_text();
            let model = AccountModel::from(&AccountFile::parse(&text)?)?;
            state.session().original_text = Some(text);
            Ok(PullOut { model_json: model.to_json(), name, notes, firmware: fw, serial })
        };
        match run(&mut log) {
            Ok(o) => HttpResult::json(
                Obj::new()
                    .opt("file", o.name.as_deref())
                    .raw("model", &o.model_json)
                    .raw("notes", &str_arr(&o.notes))
                    .opt("firmware", o.firmware.as_deref())
                    .opt("serial", o.serial.as_deref())
                    .end(),
            ),
            Err(e) => fail(&e, &[Kind::Timeout, Kind::Io, Kind::Unauthorized, Kind::Argument, Kind::InvalidData, Kind::Format], "Could not read the panel: ", Some(&log)),
        }
    })
}

// ---- live connection -----------------------------------------------------------------------------------------------------------------------------------

fn live_state(state: &Shared, snap: Option<&LiveSnapshot>) -> HttpResult {
    let l = &state.live;
    HttpResult::json(
        Obj::new()
            .bool("connected", l.connected())
            .opt("host", l.host().as_deref())
            .opt("error", l.error().as_deref())
            .opt("firmware", l.firmware().as_deref())
            .opt("serial", l.serial().as_deref())
            .raw("snapshot", &snap.map(snapshot_json).unwrap_or_else(|| "null".into()))
            .str("at", &now_hms())
            .end(),
    )
}

/// `POST /api/live/connect`: opens the shared session and returns the first status.
pub fn handle_live_connect(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    let pr = match parse_panel_req(body, "PanelRequest", false) {
        Ok(p) => p,
        Err(m) => return HttpResult::bad_request(&m),
    };
    let pr = match pr {
        Some(p) if !blank(&p.host) && !blank(&p.code) => p,
        _ => return HttpResult::bad_request("Host and RP access code are needed."),
    };
    let mut log: Vec<String> = Vec::new();
    if let Err(e) = state.live.connect(&settings_of(&pr), &mut |m| log.push(m.to_string())) {
        return fail(&PanelError::Elk(e), &PANEL_CATCH, "Could not connect: ", Some(&log));
    }
    match state.live.poll() {
        Ok(snap) => live_state(state, snap.as_ref()),
        Err(_) => HttpResult::server_error(),
    }
}

/// `POST /api/live/control`: bypass toggle, arm / disarm, silence on the open session.
pub fn handle_live_control(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    let parsed = parse_req(body, "LiveControl").and_then(|r| match r {
        None => Ok(None),
        Some(r) => {
            let v = (r.string("kind"), r.int("area"), r.int("zone"), r.int("level"), r.bool("force"));
            r.check().map(|_| Some(v))
        }
    });
    let c = match parsed {
        Ok(c) => c,
        Err(m) => return HttpResult::bad_request(&m),
    };
    let Some((kind, area, zone, level, force)) = c else {
        return HttpResult::bad_request("Nothing to do.");
    };
    let r = state.live.use_open(|s| {
        match kind.as_deref() {
            Some("bypass") => PanelControl::toggle_bypass(s, zone, area)?,
            Some("arm") => PanelControl::arm_checked(s, area, level, force)?,
            Some("silence") => PanelControl::silence(s, area)?,
            _ => return Err(PanelError::Elk(ElkError::Argument("Unknown command.".into()))),
        };
        Ok(())
    });
    match r.map_err(fix_range) {
        Ok(()) => {
            std::thread::sleep(state.control_delay);
            match state.live.poll() {
                Ok(snap) => live_state(state, snap.as_ref()),
                Err(_) => HttpResult::server_error(),
            }
        }
        Err(e) if e.kind() == Kind::InvalidOperation => HttpResult::bad_request(&e.message()),
        Err(e) => fail(&e, &[Kind::Timeout, Kind::Io, Kind::Argument], "The panel did not accept that: ", None),
    }
}

/// `POST /api/live/modules`: the installed bus devices.
pub fn handle_live_modules(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    match state.live.use_open(|s| Ok(PanelModules::scan(s)?)) {
        Ok(list) => HttpResult::json(Obj::new().raw("modules", &arr(list.iter().map(module_json))).str("note", "").str("at", &now_hms()).end()),
        Err(e) if e.kind() == Kind::InvalidOperation => HttpResult::bad_request(&e.message()),
        Err(e) => fail(&e, &[Kind::Timeout, Kind::Io, Kind::InvalidData], "Could not read the devices: ", None),
    }
}

/// `POST /api/live/enroll`: starts bus module enrollment, waits, and lists the devices.
pub fn handle_live_enroll(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    let mut log: Vec<String> = Vec::new();
    match state.live.enroll(&mut |m| log.push(m.to_string())) {
        Ok((note, list)) => HttpResult::json(Obj::new().raw("modules", &arr(list.iter().map(module_json))).str("note", &note).str("at", &now_hms()).end()),
        Err(e) if e.kind() == Kind::InvalidOperation => HttpResult::bad_request(&e.message()),
        Err(e) => fail(&e, &[Kind::Timeout, Kind::Io, Kind::InvalidData, Kind::Unauthorized], "Enrollment did not finish: ", Some(&log)),
    }
}

/// `POST /api/live/time`: the panel clock and the computer's.
pub fn handle_live_time(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    match state.live.use_open(|s| Ok(PanelClock::read(s)?)) {
        Ok(t) => HttpResult::json(Obj::new().opt("panel", t.map(fmt_full).as_deref()).str("computer", &now_full()).end()),
        Err(e) if caught(&e, &LINK_CATCH) => link_fail(&e, "Could not read the panel clock"),
        Err(_) => HttpResult::server_error(),
    }
}

/// `DateTime.TryParse(text, InvariantCulture, DateTimeStyles.None)` for the formats a browser or a person types.
pub fn parse_datetime(text: &str) -> Option<NaiveDateTime> {
    let t = text.trim();
    const DT: [&str; 12] = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%m/%d/%Y %H:%M:%S%.f",
        "%m/%d/%Y %H:%M",
        "%m/%d/%Y %I:%M:%S %p",
        "%m/%d/%Y %I:%M %p",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%B %d, %Y %H:%M:%S",
        "%d %B %Y %H:%M:%S",
    ];
    for f in DT {
        if let Ok(v) = NaiveDateTime::parse_from_str(t, f) {
            return Some(v);
        }
    }
    for f in ["%Y-%m-%d", "%m/%d/%Y", "%Y/%m/%d", "%B %d, %Y", "%d %B %Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, f) {
            return d.and_hms_opt(0, 0, 0);
        }
    }
    None
}

/// `POST /api/live/settime`: sets the panel clock (`useComputerTime`, or `time`).
pub fn handle_live_settime(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    let parsed = parse_req(body, "SetTimeRequest").and_then(|r| match r {
        None => Ok(None),
        Some(r) => {
            let v = (r.bool("useComputerTime"), r.string("time"));
            r.check().map(|_| Some(v))
        }
    });
    let r = match parsed {
        Ok(r) => r,
        Err(m) => return HttpResult::bad_request(&m),
    };
    const GIVE: &str = "Give a date and time, or use the computer's.";
    let Some((use_computer, time)) = r else {
        return HttpResult::bad_request(GIVE);
    };
    let when = if use_computer {
        Local::now().naive_local()
    } else {
        match time.as_deref().and_then(parse_datetime) {
            Some(t) => t,
            None => return HttpResult::bad_request(GIVE),
        }
    };
    match state
        .live
        .use_open(|s| {
            let acked = PanelClock::set(s, when)?;
            Ok((acked, PanelClock::read(s)?))
        })
        .map_err(fix_range)
    {
        Ok((acked, now)) => {
            HttpResult::json(Obj::new().bool("acknowledged", acked).str("sent", &fmt_full(when)).opt("panel", now.map(fmt_full).as_deref()).str("computer", &now_full()).end())
        }
        Err(e) if caught(&e, &[Kind::InvalidOperation, Kind::OutOfRange, Kind::Timeout, Kind::Io, Kind::InvalidData]) => link_fail(&e, "Could not set the panel clock"),
        Err(_) => HttpResult::server_error(),
    }
}

/// `POST /api/live/log?max=N`: the newest events (default 20, at most 512).
pub fn handle_live_log(state: &Shared, _body: &[u8], q: &Query) -> HttpResult {
    let max = q.get("max").and_then(|m| m.trim().parse::<i32>().ok()).map_or(20, |m| m.clamp(1, PanelLog::CAPACITY));
    match state.live.use_open(|s| Ok(PanelLog::read(s, max)?)) {
        Ok(list) => HttpResult::json(
            Obj::new()
                .raw(
                    "entries",
                    &arr(list.iter().map(|e| {
                        Obj::new()
                            .int("n", e.index as i64)
                            .opt("time", e.time.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).as_deref())
                            .int("area", e.area as i64)
                            .int("ev", e.event as i64)
                            .int("item", e.item as i64)
                            .end()
                    })),
                )
                .str("at", &now_hms())
                .end(),
        ),
        Err(e) if caught(&e, &LINK_CATCH) => link_fail(&e, "Could not read the log"),
        Err(_) => HttpResult::server_error(),
    }
}

/// `POST /api/live/accesscode`: changes the RP access code on the panel.
pub fn handle_live_accesscode(state: &Shared, body: &[u8], _q: &Query) -> HttpResult {
    let parsed = parse_req(body, "AccessCodeRequest").and_then(|r| match r {
        None => Ok(None),
        Some(r) => {
            let v = r.string("newCode");
            r.check().map(|_| Some(v))
        }
    });
    let r = match parsed {
        Ok(r) => r,
        Err(m) => return HttpResult::bad_request(&m),
    };
    let code = match r {
        Some(c) if PanelAccess::is_valid(c.as_deref().unwrap_or("")) => c.unwrap_or_default(),
        _ => return HttpResult::bad_request("The RP access code is exactly six digits."),
    };
    match state.live.change_access_code(&code) {
        Ok(true) => HttpResult::json(Obj::new().bool("changed", true).end()),
        Ok(false) => HttpResult::bad_request("The panel did not acknowledge the change, so the code is probably unchanged. Nothing was stored."),
        Err(e) if caught(&e, &LINK_CATCH) => link_fail(&e, "Could not change the code"),
        Err(_) => HttpResult::server_error(),
    }
}

/// `POST /api/live/disconnect`.
pub fn handle_live_disconnect(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    state.live.disconnect();
    live_state(state, None)
}

/// `GET /api/live/status`: the connection state and, when connected, a fresh status snapshot.
pub fn handle_live_status(state: &Shared, _body: &[u8], _q: &Query) -> HttpResult {
    let snap = if state.live.connected() {
        match state.live.poll() {
            Ok(s) => s,
            Err(_) => return HttpResult::server_error(),
        }
    } else {
        None
    };
    live_state(state, snap.as_ref())
}

/// Routes a request to the handler above. `None` when the method and path are not one of the panel or live routes.
pub fn handle(state: &Shared, method: &str, path: &str, body: &[u8], query: &Query) -> Option<HttpResult> {
    if let Some(r) = crate::m1xep::handle(state, method, path, body) {
        return Some(r);
    }
    let get = method.eq_ignore_ascii_case("GET");
    let post = method.eq_ignore_ascii_case("POST");
    Some(match (path, get, post) {
        ("/api/panel/progress", true, _) => handle_panel_progress(state, body, query),
        ("/api/panel/backups", true, _) => handle_panel_backups(state, body, query),
        ("/api/panel/backups/open", _, true) => handle_panel_backups_open(state, body, query),
        ("/api/panel/backup/delete", _, true) => handle_panel_backup_delete(state, body, query),
        ("/api/live/status", true, _) => handle_live_status(state, body, query),
        ("/api/panel/compare", _, true) => handle_panel_compare(state, body, query),
        ("/api/panel/send", _, true) => handle_panel_send(state, body, query),
        ("/api/panel/pull", _, true) => handle_panel_pull(state, body, query),
        ("/api/panel/backup", _, true) => handle_panel_backup(state, body, query),
        ("/api/panel/restore", _, true) => handle_panel_restore(state, body, query),
        ("/api/live/connect", _, true) => handle_live_connect(state, body, query),
        ("/api/live/control", _, true) => handle_live_control(state, body, query),
        ("/api/live/modules", _, true) => handle_live_modules(state, body, query),
        ("/api/live/enroll", _, true) => handle_live_enroll(state, body, query),
        ("/api/live/time", _, true) => handle_live_time(state, body, query),
        ("/api/live/settime", _, true) => handle_live_settime(state, body, query),
        ("/api/live/log", _, true) => handle_live_log(state, body, query),
        ("/api/live/accesscode", _, true) => handle_live_accesscode(state, body, query),
        ("/api/live/disconnect", _, true) => handle_live_disconnect(state, body, query),
        _ => return None,
    })
}
