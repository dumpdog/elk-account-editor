//! The editor's HTTP routes (port of `Elk.Editor/Program.cs`). Every handler is a plain blocking function from request parts to
//! an [`HttpResult`]; the axum glue in `main.rs` only moves bytes. The panel and live routes are delegated to `elk-panel`.

use elk_account::{
    AccountFile, AccountModel, AlarmZoneList, AutomationMeta, Error, ModelMeta, ZoneSheet,
    decode_latin1, encode_latin1,
};
use elk_panel::{
    HttpResult, Query, Shared, handle as panel_handle, json_syntax_check, parse_model_body,
    template_text,
};

use crate::library_routes;
use crate::state::App;
use crate::update_routes;

/// The page, embedded at build time (served unchanged).
pub const INDEX_HTML: &str = include_str!("../assets/index.html");

const JSON: &str = "application/json; charset=utf-8";
const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// `Results.File(bytes, contentType)`.
fn file(bytes: Vec<u8>, content_type: &str) -> HttpResult {
    HttpResult {
        status: 200,
        content_type: content_type.to_string(),
        body: bytes,
    }
}

fn json(body: String) -> HttpResult {
    HttpResult::json(body)
}

fn empty(status: u16) -> HttpResult {
    HttpResult {
        status,
        content_type: String::new(),
        body: Vec::new(),
    }
}

/// The exception kinds Program.cs turns into a 400 (`InvalidDataException`, `FormatException`, and where listed `ArgumentOutOfRangeException`).
fn bad_or_500(e: &Error, out_of_range: bool) -> HttpResult {
    match e {
        Error::InvalidData(m) | Error::Format(m) => HttpResult::bad_request(m),
        Error::OutOfRange(m) if out_of_range => HttpResult::bad_request(m),
        _ => HttpResult::server_error(),
    }
}

/// `{"file":<name or null>,"model":<model>}`.
fn file_and_model(name: &Option<String>, model_json: &str) -> String {
    format!(
        "{{\"file\":{},\"model\":{}}}",
        elk_account::json::to_string(name),
        model_json
    )
}

fn model_of(text: &str) -> elk_account::Result<AccountModel> {
    AccountModel::from(&AccountFile::parse(text)?)
}

fn meta_json() -> String {
    use elk_account::json::to_string as j;
    format!(
        concat!(
            "{{\"zoneFlags\":{},\"userFlags\":{},\"areaFlags\":{},\"phoneFlags\":{},",
            "\"zoneDefinitions\":{},\"zoneTypes\":{},\"busModels\":{},\"zoneExpanderSwitches\":{},",
            "\"areaRcLabels\":{},\"systemRcLabels\":{},\"zoneRcLabels\":{},\"userRcLabels\":{},",
            "\"words\":{},\"voiceMessages\":{},\"outputNames\":{},\"eventNames\":{},",
            "\"customTypes\":{},\"lightTypes\":{},\"lightFormats\":{},\"eventWords\":{}}}"
        ),
        j(&ModelMeta::ZONE_FLAGS[..]),
        j(&ModelMeta::USER_FLAGS[..]),
        j(&ModelMeta::AREA_FLAGS[..]),
        j(&ModelMeta::PHONE_FLAGS[..]),
        j(&ModelMeta::ZONE_DEFINITIONS[..]),
        j(&ModelMeta::ZONE_TYPES[..]),
        elk_planner_window::data_bus_plan::models_json(),
        elk_planner_window::data_bus_plan::zone_expander_switches_json(),
        j(&ModelMeta::AREA_RC_LABELS[..]),
        j(&ModelMeta::SYSTEM_RC_LABELS[..]),
        j(&ModelMeta::ZONE_RC_LABELS[..]),
        j(&ModelMeta::USER_RC_LABELS[..]),
        j(AutomationMeta::words()),
        j(AutomationMeta::voice_message_names()),
        j(AutomationMeta::output_names()),
        j(elk_account::rules::event_names()),
        j(&AutomationMeta::CUSTOM_TYPES[..]),
        j(&AutomationMeta::LIGHT_TYPES[..]),
        j(&AutomationMeta::LIGHT_FORMATS[..]),
        elk_account::english::event_words_json(),
    )
}

fn api_new(state: &Shared) -> HttpResult {
    let text = template_text();
    let name = Some("New account".to_string());
    let model = match model_of(&text) {
        Ok(m) => m,
        Err(_) => return HttpResult::server_error(),
    };
    {
        let mut s = state.session();
        s.original_text = Some(text);
        s.source_name = name.clone();
    }
    json(file_and_model(&name, &model.to_json()))
}

fn api_model(state: &Shared) -> HttpResult {
    let (text, name) = {
        let s = state.session();
        (s.original_text.clone(), s.source_name.clone())
    };
    match text {
        None => empty(204),
        Some(t) => match model_of(&t) {
            Ok(m) => json(file_and_model(&name, &m.to_json())),
            Err(_) => HttpResult::server_error(),
        },
    }
}

fn api_open(state: &Shared, body: &[u8], q: &Query) -> HttpResult {
    let text = decode_latin1(body);
    match model_of(&text) {
        Ok(model) => {
            let name = q.get("name").map(str::to_string);
            let out = file_and_model(&name, &model.to_json());
            let mut s = state.session();
            s.original_text = Some(text);
            s.source_name = name;
            json(out)
        }
        Err(e) => bad_or_500(&e, false),
    }
}

/// The request body as a model: `Err` is the 400 answer (`JsonException` text, or "empty request" for a JSON null).
fn model_from_request(body: &[u8]) -> Result<AccountModel, HttpResult> {
    match parse_model_body(body) {
        Ok(Some(m)) => Ok(m),
        Ok(None) => Err(HttpResult::bad_request("empty request")),
        Err(m) => Err(HttpResult::bad_request(&m)),
    }
}

fn api_save(state: &Shared, body: &[u8]) -> HttpResult {
    let Some(original) = state.session().original_text.clone() else {
        return HttpResult::bad_request("Open a file first.");
    };
    let model = match model_from_request(body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let result = AccountFile::parse(&original).and_then(|mut f| model.apply_to(&mut f).map(|()| f));
    match result {
        Ok(f) => file(encode_latin1(&f.to_text()), "application/octet-stream"),
        Err(e) => bad_or_500(&e, true),
    }
}

fn api_zones_read(body: &[u8], q: &Query) -> HttpResult {
    match ZoneSheet::read(body, q.get("name").unwrap_or("")) {
        Ok(r) => json(r.to_json()),
        Err(e) => bad_or_500(&e, false),
    }
}

fn api_zones_export(body: &[u8], q: &Query) -> HttpResult {
    let model = match model_from_request(body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let table = ZoneSheet::table(&model.zones, q.get("all") == Some("1"));
    if q.get("format") == Some("csv") {
        return file(ZoneSheet::to_csv(&table), "text/csv");
    }
    match ZoneSheet::to_xlsx(&table) {
        Ok(b) => file(b, XLSX),
        Err(e) => bad_or_500(&e, false),
    }
}

fn api_export_alarm_list(body: &[u8], q: &Query) -> HttpResult {
    let model = match model_from_request(body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let title = q.get("title").filter(|t| !t.is_empty());
    match AlarmZoneList::build(&model, title, None) {
        Ok(b) => file(b, XLSX),
        Err(e) => bad_or_500(&e, false),
    }
}

fn api_databus_check(body: &[u8]) -> HttpResult {
    if let Err(m) = json_syntax_check(body) {
        return HttpResult::bad_request(&m);
    }
    match elk_planner_window::check_json(&String::from_utf8_lossy(body)) {
        Ok(b) => json(b),
        Err(m) => HttpResult {
            status: 400,
            content_type: JSON.to_string(),
            body: m.into_bytes(),
        },
    }
}

/// `/api/rules/english` (Rust editor only): applies the page's model to the opened file, exactly as a save does, and answers the
/// ElkRP wording of every rule with the packet budget. Nothing is stored. A rule that cannot be written gets the save's 400 text.
fn api_rules_english(state: &Shared, body: &[u8]) -> HttpResult {
    let Some(original) = state.session().original_text.clone() else {
        return HttpResult::bad_request("Open a file first.");
    };
    let model = match model_from_request(body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let result = AccountFile::parse(&original).and_then(|mut f| model.apply_to(&mut f).map(|()| f));
    match result {
        Ok(f) => match elk_account::english::english_report(&f) {
            Some(r) => json(elk_account::json::to_string(&r)),
            None => HttpResult::bad_request(
                "this file has no automation section (outputs, tasks, rules ...)",
            ),
        },
        Err(e) => bad_or_500(&e, true),
    }
}

/// Splits `/path?query` into the lower-cased path (ASP.NET routing ignores case) and the query.
pub fn split_target(target: &str) -> (String, Query) {
    match target.split_once('?') {
        Some((p, q)) => (p.to_ascii_lowercase(), Query::parse(q)),
        None => (target.to_ascii_lowercase(), Query::new()),
    }
}

/// Answers one request. Unknown paths get an empty 404, a wrong method on a known editor path an empty 405.
pub fn dispatch(app: &App, method: &str, target: &str, body: &[u8]) -> HttpResult {
    let state = &app.shared;
    let (path, q) = split_target(target);
    let get = method.eq_ignore_ascii_case("GET");
    let post = method.eq_ignore_ascii_case("POST");
    if let Some(r) = panel_handle(state, method, &path, body, &q) {
        return r;
    }
    if let Some(r) = library_routes::handle(app, method, &path, &q, body) {
        return r;
    }
    if let Some(r) = update_routes::handle(app, method, &path, &q, body) {
        return r;
    }
    let allowed = match path.as_str() {
        "/" | "/api/meta" | "/api/model" => Some(get),
        "/api/new"
        | "/api/open"
        | "/api/save"
        | "/api/rules/english"
        | "/api/zones/read"
        | "/api/zones/export"
        | "/api/export-alarm-list"
        | "/api/databus/check" => Some(post),
        _ => None,
    };
    match allowed {
        None => return empty(404),
        Some(false) => return empty(405),
        Some(true) => {}
    }
    match path.as_str() {
        "/" => HttpResult {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            body: INDEX_HTML.as_bytes().to_vec(),
        },
        "/api/meta" => json(meta_json()),
        "/api/model" => api_model(state),
        "/api/new" => api_new(state),
        "/api/open" => api_open(state, body, &q),
        "/api/save" => api_save(state, body),
        "/api/rules/english" => api_rules_english(state, body),
        "/api/zones/read" => api_zones_read(body, &q),
        "/api/zones/export" => api_zones_export(body, &q),
        "/api/export-alarm-list" => api_export_alarm_list(body, &q),
        "/api/databus/check" => api_databus_check(body),
        _ => empty(404),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn shared() -> App {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "elk-app-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        App::with_dir(
            Shared::with_backup_dir(dir.join("backups")),
            &dir.join("data"),
        )
    }

    fn sample() -> Vec<u8> {
        let p =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sample/ElkRPAcctSample Export");
        std::fs::read(p).expect("sample file")
    }

    #[test]
    fn target_split_lowercases_path_only() {
        let (p, q) = split_target("/API/Open?name=A%20B.txt");
        assert_eq!(p, "/api/open");
        assert_eq!(q.get("name"), Some("A B.txt"));
    }

    #[test]
    fn unknown_path_404_wrong_method_405() {
        let s = shared();
        assert_eq!(dispatch(&s, "GET", "/nope", b"").status, 404);
        assert_eq!(dispatch(&s, "GET", "/api/open", b"").status, 405);
        assert_eq!(dispatch(&s, "POST", "/api/meta", b"").status, 405);
    }

    #[test]
    fn index_is_html() {
        let r = dispatch(&shared(), "GET", "/", b"");
        assert_eq!(r.content_type, "text/html; charset=utf-8");
        assert!(r.text().starts_with("<!DOCTYPE"));
    }

    #[test]
    fn meta_keys_in_order() {
        let t = meta_json();
        let mut at = 0;
        for k in [
            "zoneFlags",
            "userFlags",
            "areaFlags",
            "phoneFlags",
            "zoneDefinitions",
            "zoneTypes",
            "busModels",
            "zoneExpanderSwitches",
            "areaRcLabels",
            "systemRcLabels",
            "zoneRcLabels",
            "userRcLabels",
            "words",
            "voiceMessages",
            "outputNames",
            "eventNames",
            "customTypes",
            "lightTypes",
            "lightFormats",
        ] {
            let i = t[at..]
                .find(&format!("\"{k}\":"))
                .unwrap_or_else(|| panic!("{k} missing or out of order"));
            at += i;
        }
    }

    #[test]
    fn model_before_open_is_204_then_open_save_round_trips() {
        let s = shared();
        assert_eq!(dispatch(&s, "GET", "/api/model", b"").status, 204);
        let data = sample();
        let r = dispatch(&s, "POST", "/api/open?name=x.txt", &data);
        assert_eq!(r.status, 200);
        assert!(r.text().starts_with("{\"file\":\"x.txt\",\"model\":"));
        let m = dispatch(&s, "GET", "/api/model", b"");
        assert_eq!(m.body, r.body);
        let text = r.text();
        let model = &text[text.find("\"model\":").unwrap() + 8..text.len() - 1];
        let saved = dispatch(&s, "POST", "/api/save", model.as_bytes());
        assert_eq!(saved.status, 200);
        assert_eq!(saved.content_type, "application/octet-stream");
        assert_eq!(saved.body, data);
    }

    #[test]
    fn rules_english_route() {
        // needs the event table (ElkRP wording), which the public source tree does not include
        if !elk_account::rules::event_table_present() {
            return;
        }
        let s = shared();
        let r = dispatch(&s, "POST", "/api/rules/english", b"{}");
        assert_eq!(
            (r.status, r.text().as_str()),
            (400, "\"Open a file first.\"")
        );
        let data = sample();
        let opened = dispatch(&s, "POST", "/api/open?name=x.txt", &data);
        let text = opened.text();
        let model = &text[text.find("\"model\":").unwrap() + 8..text.len() - 1];
        let r = dispatch(&s, "POST", "/api/rules/english", model.as_bytes());
        assert_eq!(r.status, 200);
        let t = r.text();
        assert!(
            t.starts_with("{\"rules\":[[\"WHENEVER ANY BURGLAR ALARM, ANY AREA TURNS ON\""),
            "{t}"
        );
        assert!(t.contains("\"THEN TURN Strobe Light (Out 3) OFF\""));
        assert!(
            t.ends_with("\"rulePacketsUsed\":12,\"textPacketsUsed\":4,\"percent\":3}"),
            "{t}"
        );
        // a rule ElkRP could not read is refused with the reason (and a plain GET is a 405)
        let bad = model.replacen(
            "\"packets\":[[89,1,3,235,26,3,0,0],",
            "\"packets\":[[89,1,3,235,26,99,0,0],",
            1,
        );
        assert_ne!(bad, model);
        let r = dispatch(&s, "POST", "/api/rules/english", bad.as_bytes());
        assert_eq!(r.status, 400);
        assert!(r.text().contains("output must be 1-64"), "{}", r.text());
        assert_eq!(dispatch(&s, "GET", "/api/rules/english", b"").status, 405);
        assert!(
            dispatch(&s, "GET", "/api/meta", b"")
                .text()
                .contains("\"1001\":[\"ANY\",\"ANY\",\"TURNS ON\"")
        );
    }

    #[test]
    fn disabled_rules_do_not_change_the_saved_file() {
        let s = shared();
        let data = sample();
        let opened = dispatch(&s, "POST", "/api/open?name=x.txt", &data);
        let text = opened.text();
        let model = &text[text.find("\"model\":").unwrap() + 8..text.len() - 1];
        let flagged = model.replacen(
            "\"packets\":[[89,1,3,235,26,3,0,0],",
            "\"disabled\":true,\"packets\":[[89,1,3,235,26,3,0,0],",
            1,
        );
        assert_ne!(flagged, model);
        // the export file cannot hold the flag: every rule is saved, the bytes are those of the unedited file
        let saved = dispatch(&s, "POST", "/api/save", flagged.as_bytes());
        assert_eq!(saved.status, 200);
        assert_eq!(saved.body, data);
        let r = dispatch(&s, "POST", "/api/rules/english", flagged.as_bytes());
        assert_eq!(r.status, 200);
        assert!(r.text().ends_with("\"percent\":3}"), "{}", r.text());
    }

    #[test]
    fn save_without_open_and_garbage() {
        let s = shared();
        let r = dispatch(&s, "POST", "/api/save", b"{}");
        assert_eq!(
            (r.status, r.text().as_str()),
            (400, "\"Open a file first.\"")
        );
        let r = dispatch(&s, "POST", "/api/open", b"garbage");
        assert_eq!(r.status, 400);
        let r = dispatch(&s, "POST", "/api/new", b"");
        assert_eq!(r.status, 200);
        let r = dispatch(&s, "POST", "/api/save", b"null");
        assert_eq!(r.text(), "\"empty request\"");
        let r = dispatch(&s, "POST", "/api/save", b"{not json");
        assert_eq!(r.status, 400);
    }

    #[test]
    fn databus_bad_json_is_400() {
        let r = dispatch(&shared(), "POST", "/api/databus/check", b"{not json");
        assert_eq!(r.status, 400);
        assert_eq!(r.content_type, JSON);
    }

    #[test]
    fn m1xep_routes_are_wired() {
        // (the module conversation itself is tested in elk-panel against a simulated module)
        let s = shared();
        let r = dispatch(&s, "POST", "/api/m1xep/read", b"{}");
        assert_eq!(
            (r.status, r.text().as_str()),
            (400, "\"Host and RP access code are needed.\"")
        );
        let r = dispatch(&s, "POST", "/api/m1xep/write", b"{}");
        assert_eq!(r.status, 400);
        let r = dispatch(&s, "POST", "/api/m1xep/find", b"{\"host\":\"nope\"}");
        assert_eq!(r.status, 400);
        let r = dispatch(&s, "POST", "/API/M1XEP/File", b"");
        assert_eq!(
            (r.status, r.text().contains("\"found\":false")),
            (200, true)
        );
        assert_eq!(dispatch(&s, "GET", "/api/m1xep/read", b"").status, 404);
    }
}
