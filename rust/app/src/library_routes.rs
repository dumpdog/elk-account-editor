//! `/api/library/*` (Rust editor only, ADR-015): the accounts library. Thin glue between the HTTP shapes the page uses and `elk-library`.
//! Errors answer 400 with the message as a JSON string (like the other routes), storage failures 500 with the message.

use elk_account::{AccountFile, AccountModel, decode_latin1};
use elk_library::{
    AccountEdit, DEFAULT_HISTORY_KEEP, Error, Library, MAX_HISTORY_KEEP, NewAccount, PanelSettings,
};
use elk_panel::{HttpResult, Query, Shared, parse_model_body};
use serde_json::{Value, json};

use crate::state::App;

fn fail(e: Error) -> HttpResult {
    match e {
        Error::Db(_) | Error::Io(_) | Error::Other(_) => HttpResult {
            status: 500,
            content_type: "application/json; charset=utf-8".into(),
            body: serde_json::to_vec(&e.to_string()).unwrap_or_default(),
        },
        _ => HttpResult::bad_request(&e.to_string()),
    }
}

fn ok(v: Value) -> HttpResult {
    HttpResult::json(v.to_string())
}

fn bytes(b: Vec<u8>) -> HttpResult {
    HttpResult {
        status: 200,
        content_type: "application/octet-stream".into(),
        body: b,
    }
}

fn body_json(body: &[u8]) -> Result<Value, HttpResult> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| HttpResult::bad_request("The request is not a JSON object."))
}

fn int(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(|x| match x {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

fn panel_of(v: &Value) -> Option<PanelSettings> {
    v.as_object()?;
    serde_json::from_value::<PanelSettings>(v.clone()).ok()
}

fn q_id(q: &Query, key: &str) -> Option<i64> {
    q.get(key).and_then(|s| s.trim().parse().ok())
}

/// The model of a stored file with its disabled-rule flags applied (what `/api/open` returns, plus `library`).
fn open_json(
    app: &App,
    lib: &Library,
    id: i64,
    version: Option<i64>,
) -> Result<HttpResult, HttpResult> {
    let acc = lib.get_account(id).map_err(fail)?;
    let content = match version {
        Some(v) => lib.version_content(id, v).map_err(fail)?,
        None => acc.content.clone(),
    };
    let text = decode_latin1(&content);
    let mut model = AccountFile::parse(&text)
        .and_then(|f| AccountModel::from(&f))
        .map_err(|e| {
            HttpResult::bad_request(&format!("The stored file cannot be read: {}", e.message()))
        })?;
    if let Some(a) = model.automation.as_mut() {
        for &i in &acc.disabled_rules {
            if let Some(r) = a.rules.get_mut(i) {
                r.disabled = true;
            }
        }
    }
    let name = acc.summary.name.clone();
    {
        let mut s = app.shared.session();
        s.original_text = Some(text);
        s.source_name = Some(name.clone());
    }
    let panel = acc.panel.as_ref().map(|p| json!(p)).unwrap_or(Value::Null);
    let lib_json = json!({
        "id": acc.summary.id, "name": name, "folderId": acc.summary.folder_id, "notes": acc.summary.notes,
        "tags": acc.summary.tags, "updated": acc.summary.updated, "panel": panel,
    });
    Ok(HttpResult::json(format!(
        "{{\"file\":{},\"model\":{},\"library\":{}}}",
        elk_account::json::to_string(&Some(name)),
        model.to_json(),
        lib_json
    )))
}

/// The edited file, as `/api/save` makes it, and the model's disabled rules.
fn saved_bytes(shared: &Shared, model_body: &[u8]) -> Result<(Vec<u8>, Vec<usize>), HttpResult> {
    let Some(original) = shared.session().original_text.clone() else {
        return Err(HttpResult::bad_request("Open a file first."));
    };
    let model = match parse_model_body(model_body) {
        Ok(Some(m)) => m,
        Ok(None) => return Err(HttpResult::bad_request("empty request")),
        Err(m) => return Err(HttpResult::bad_request(&m)),
    };
    let off = model
        .automation
        .as_ref()
        .map(|a| {
            let mut v: Vec<usize> = elk_account::rules::disabled_indexes(&a.rules)
                .into_iter()
                .collect();
            v.sort_unstable();
            v
        })
        .unwrap_or_default();
    let f = AccountFile::parse(&original)
        .and_then(|mut f| model.apply_to(&mut f).map(|()| f))
        .map_err(|e| HttpResult::bad_request(e.message()))?;
    Ok((elk_account::encode_latin1(&f.to_text()), off))
}

fn list(app: &App, q: &Query) -> HttpResult {
    let (settings, warn) = app.library.settings();
    let loc = app.library.location.as_ref();
    let lib = match &app.library.lib {
        Ok(l) => l,
        Err(why) => {
            return ok(
                json!({"available": false, "error": why, "dataDir": loc.map(|l| l.dir.display().to_string())}),
            );
        }
    };
    match lib.list(q.get("q")) {
        Ok(l) => ok(json!({
            "available": true,
            "dataDir": lib.dir().display().to_string(),
            "note": loc.map(|l| l.note.clone()),
            "fellBack": loc.is_some_and(|l| l.fell_back),
            "warning": warn,
            "historyKeep": settings.history_keep,
            "folders": l.folders, "accounts": l.accounts,
        })),
        Err(e) => fail(e),
    }
}

fn save(app: &App, lib: &Library, body: &[u8]) -> HttpResult {
    let req = match body_json(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let Some(model) = req.get("model").filter(|m| m.is_object()) else {
        return HttpResult::bad_request("The edited data are needed.");
    };
    let (content, off) =
        match saved_bytes(&app.shared, &serde_json::to_vec(model).unwrap_or_default()) {
            Ok(x) => x,
            Err(r) => return r,
        };
    let keep = app.library.settings().0.history_keep;
    let panel = req.get("panel").and_then(panel_of);
    let notes = text(&req, "notes");
    let tags = text(&req, "tags");
    if let Some(id) = int(&req, "id").filter(|&i| i > 0) {
        // details first (a name clash is refused before the file changes)
        let edit = AccountEdit {
            name: text(&req, "name"),
            folder_id: int(&req, "folderId"),
            notes,
            tags,
            panel: req.get("panel").map(|_| panel),
        };
        if let Err(e) = lib.edit_account(id, edit) {
            return fail(e);
        }
        return match lib.update_content(id, &content, &off, keep) {
            Ok(o) => {
                let a = lib.get_account(id).map(|a| a.summary);
                match a {
                    Ok(s) => ok(
                        json!({"id": id, "created": false, "changed": o.changed, "versions": o.versions_kept, "name": s.name, "updated": s.updated}),
                    ),
                    Err(e) => fail(e),
                }
            }
            Err(e) => fail(e),
        };
    }
    let new = NewAccount {
        name: text(&req, "name").unwrap_or_default(),
        folder_id: int(&req, "folderId").unwrap_or(0),
        notes: notes.unwrap_or_default(),
        tags: tags.unwrap_or_default(),
        content,
        disabled_rules: off,
        panel,
    };
    match lib.add_account(new) {
        Ok(id) => match lib.get_account(id) {
            Ok(a) => ok(
                json!({"id": id, "created": true, "changed": true, "versions": 0, "name": a.summary.name, "updated": a.summary.updated}),
            ),
            Err(e) => fail(e),
        },
        Err(e) => fail(e),
    }
}

fn import(lib: &Library, q: &Query, body: &[u8]) -> HttpResult {
    let t = decode_latin1(body);
    if let Err(e) = AccountFile::parse(&t).and_then(|f| AccountModel::from(&f)) {
        return HttpResult::bad_request(&format!(
            "That is not an ElkRP account export file: {}",
            e.message()
        ));
    }
    let raw = q.get("name").unwrap_or("").trim();
    let name = if raw.is_empty() {
        "Imported account"
    } else {
        raw
    };
    let folder = q_id(q, "folder").unwrap_or(0);
    let name = match lib.free_name(folder, name) {
        Ok(n) => n,
        Err(e) => return fail(e),
    };
    // the bytes as received, not the decoded text
    let new = NewAccount {
        name,
        folder_id: folder,
        content: body.to_vec(),
        ..Default::default()
    };
    match lib.add_account(new) {
        Ok(id) => ok(json!({"id": id})),
        Err(e) => fail(e),
    }
}

fn settings_json(app: &App) -> Value {
    let (s, warn) = app.library.settings();
    json!({
        "historyKeep": s.history_keep, "maxHistoryKeep": MAX_HISTORY_KEEP, "defaultHistoryKeep": DEFAULT_HISTORY_KEEP,
        "updateUrl": s.update_url, "autoSavePanel": s.auto_save_panel, "warning": warn,
        "dataDir": app.library.dir().map(|d| d.display().to_string()),
    })
}

/// Routes under `/api/library/`; `None` when the path is not one of them.
pub fn handle(app: &App, method: &str, path: &str, q: &Query, body: &[u8]) -> Option<HttpResult> {
    let rest = path.strip_prefix("/api/library/")?;
    let get = method.eq_ignore_ascii_case("GET");
    let post = method.eq_ignore_ascii_case("POST");
    let known_get = [
        "list",
        "account",
        "export",
        "history",
        "history/export",
        "settings",
    ];
    let known_post = [
        "folder/create",
        "folder/rename",
        "folder/move",
        "folder/delete",
        "open",
        "save",
        "import",
        "account/update",
        "account/delete",
        "history/restore",
        "backup",
        "settings",
    ];
    let known = known_get.contains(&rest) || known_post.contains(&rest);
    if !known {
        return Some(HttpResult {
            status: 404,
            content_type: String::new(),
            body: Vec::new(),
        });
    }
    if !(get && known_get.contains(&rest) || post && known_post.contains(&rest)) {
        return Some(HttpResult {
            status: 405,
            content_type: String::new(),
            body: Vec::new(),
        });
    }
    if rest == "list" {
        return Some(list(app, q));
    }
    if rest == "settings" && get {
        return Some(ok(settings_json(app)));
    }
    let lib = match &app.library.lib {
        Ok(l) => l,
        Err(why) => {
            return Some(HttpResult::bad_request(&format!(
                "The library is not available: {why}"
            )));
        }
    };
    Some(route(app, lib, rest, q, body))
}

fn route(app: &App, lib: &Library, rest: &str, q: &Query, body: &[u8]) -> HttpResult {
    let (settings, _) = app.library.settings();
    match rest {
        "open" => match q_id(q, "id") {
            Some(id) => open_json(app, lib, id, None).unwrap_or_else(|r| r),
            None => HttpResult::bad_request("Which account?"),
        },
        "save" => save(app, lib, body),
        "import" => import(lib, q, body),
        "account" => match q_id(q, "id").map(|i| lib.get_account(i)) {
            Some(Ok(a)) => ok(json!({
                "id": a.summary.id, "name": a.summary.name, "folderId": a.summary.folder_id, "notes": a.summary.notes,
                "tags": a.summary.tags, "updated": a.summary.updated, "panel": a.panel,
            })),
            Some(Err(e)) => fail(e),
            None => HttpResult::bad_request("Which account?"),
        },
        "export" => match q_id(q, "id").map(|i| lib.get_account(i)) {
            Some(Ok(a)) => bytes(a.content),
            Some(Err(e)) => fail(e),
            None => HttpResult::bad_request("Which account?"),
        },
        "history" => match q_id(q, "id").map(|i| lib.history(i)) {
            Some(Ok(h)) => ok(json!(h)),
            Some(Err(e)) => fail(e),
            None => HttpResult::bad_request("Which account?"),
        },
        "history/export" => match (q_id(q, "id"), q_id(q, "version")) {
            (Some(i), Some(v)) => lib.version_content(i, v).map(bytes).unwrap_or_else(fail),
            _ => HttpResult::bad_request("Which version?"),
        },
        "backup" => {
            let dest = lib.dir().join("backups");
            match lib.backup(&dest) {
                Ok(p) => ok(json!({"path": p.display().to_string()})),
                Err(e) => fail(e),
            }
        }
        "settings" => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let mut s = settings.clone();
            if let Some(k) = int(&v, "historyKeep") {
                if !(0..=MAX_HISTORY_KEEP as i64).contains(&k) {
                    return HttpResult::bad_request(&format!(
                        "Versions to keep must be 0 to {MAX_HISTORY_KEEP}."
                    ));
                }
                s.history_keep = k as u32;
            }
            if let Some(b) = v.get("autoSavePanel").and_then(Value::as_bool) {
                s.auto_save_panel = b;
            }
            match s.save(lib.dir()) {
                Ok(()) => ok(settings_json(app)),
                Err(e) => fail(e),
            }
        }
        _ => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            mutate(lib, rest, &v, settings.history_keep)
        }
    }
}

fn mutate(lib: &Library, rest: &str, v: &Value, keep: u32) -> HttpResult {
    let done = |r: elk_library::Result<()>| match r {
        Ok(()) => ok(json!({"ok": true})),
        Err(e) => fail(e),
    };
    let id = int(v, "id");
    match rest {
        "folder/create" => match lib.create_folder(
            int(v, "parentId").unwrap_or(0),
            &text(v, "name").unwrap_or_default(),
        ) {
            Ok(id) => ok(json!({"id": id})),
            Err(e) => fail(e),
        },
        "folder/rename" => {
            done(lib.rename_folder(id.unwrap_or(-1), &text(v, "name").unwrap_or_default()))
        }
        "folder/move" => done(lib.move_folder(id.unwrap_or(-1), int(v, "parentId").unwrap_or(0))),
        "folder/delete" => done(
            lib.delete_folder(
                id.unwrap_or(-1),
                v.get("withContent")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
        "account/delete" => done(lib.delete_account(id.unwrap_or(-1))),
        "account/update" => done(lib.edit_account(
            id.unwrap_or(-1),
            AccountEdit {
                name: text(v, "name"),
                folder_id: int(v, "folderId"),
                notes: text(v, "notes"),
                tags: text(v, "tags"),
                panel: v.get("panel").map(panel_of),
            },
        )),
        "history/restore" => {
            done(lib.restore_version(id.unwrap_or(-1), int(v, "versionId").unwrap_or(-1), keep))
        }
        _ => HttpResult {
            status: 404,
            content_type: String::new(),
            body: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dispatch;
    use std::path::PathBuf;

    fn app() -> App {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "elk-lib-routes-{}-{}",
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

    fn call(a: &App, m: &str, target: &str, body: &[u8]) -> (u16, String) {
        let r = dispatch(a, m, target, body);
        (r.status, r.text())
    }

    fn json_of(a: &App, m: &str, target: &str, body: &[u8]) -> Value {
        let (s, t) = call(a, m, target, body);
        assert_eq!(s, 200, "{target}: {t}");
        serde_json::from_str(&t).unwrap()
    }

    /// The model of the account that is open now.
    fn model_now(a: &App) -> Value {
        let v = json_of(a, "GET", "/api/model", b"");
        v["model"].clone()
    }

    #[test]
    fn folders_import_open_save_history_export_round_trip() {
        let a = app();
        let l = json_of(&a, "GET", "/api/library/list", b"");
        assert_eq!(l["available"], true);
        assert!(l["dataDir"].as_str().unwrap().ends_with("data"));
        assert_eq!(l["historyKeep"], 20);

        let f = json_of(
            &a,
            "POST",
            "/api/library/folder/create",
            br#"{"parentId":0,"name":"Customers"}"#,
        )["id"]
            .as_i64()
            .unwrap();
        let sub = json_of(
            &a,
            "POST",
            "/api/library/folder/create",
            format!(r#"{{"parentId":{f},"name":"Smith"}}"#).as_bytes(),
        )["id"]
            .as_i64()
            .unwrap();
        assert_eq!(
            call(
                &a,
                "POST",
                "/api/library/folder/create",
                format!(r#"{{"parentId":{f},"name":"smith"}}"#).as_bytes()
            )
            .0,
            400
        );

        // import the sample byte for byte, into a folder
        let data = sample();
        let id = json_of(
            &a,
            "POST",
            &format!("/api/library/import?name=Sample&folder={sub}"),
            &data,
        )["id"]
            .as_i64()
            .unwrap();
        let again = json_of(
            &a,
            "POST",
            &format!("/api/library/import?name=Sample&folder={sub}"),
            &data,
        )["id"]
            .as_i64()
            .unwrap();
        assert_ne!(id, again);
        let r = dispatch(&a, "GET", &format!("/api/library/export?id={id}"), b"");
        assert!(r.status == 200 && r.body == data);
        assert_eq!(
            call(&a, "POST", "/api/library/import?name=x", b"not an account").0,
            400
        );

        // open it like File > Open does
        let o = json_of(&a, "POST", &format!("/api/library/open?id={id}"), b"");
        assert_eq!(o["file"], "Sample");
        assert_eq!(o["library"]["folderId"], sub);
        let opened = dispatch(&a, "POST", "/api/open?name=Sample", &data);
        let plain: Value = serde_json::from_str(&opened.text()).unwrap();
        assert_eq!(
            o["model"], plain["model"],
            "the library opens a file exactly as Open does"
        );
        let n_rules = o["model"]["automation"]["rules"].as_array().unwrap().len();
        assert!(n_rules >= 2);
        // (the plain open replaced the session; open from the library again for the editing steps)
        json_of(&a, "POST", &format!("/api/library/open?id={id}"), b"");

        // an unedited save changes nothing and makes no version
        let mut model = model_now(&a);
        let body = json!({"id": id, "model": model}).to_string();
        let s = json_of(&a, "POST", "/api/library/save", body.as_bytes());
        assert_eq!(
            (
                s["changed"].clone(),
                s["created"].clone(),
                s["versions"].clone()
            ),
            (json!(false), json!(false), json!(0))
        );
        let r = dispatch(&a, "GET", &format!("/api/library/export?id={id}"), b"");
        assert!(r.body == data);

        // edit, disable a rule, save: a version appears and the flag is kept
        model["zones"][0]["name"] = json!("Edited zone");
        model["automation"]["rules"][1]["disabled"] = json!(true);
        let body = json!({"id": id, "model": model, "notes": "edited"}).to_string();
        let s = json_of(&a, "POST", "/api/library/save", body.as_bytes());
        assert_eq!(
            (s["changed"].clone(), s["versions"].clone()),
            (json!(true), json!(1))
        );
        let edited = dispatch(&a, "GET", &format!("/api/library/export?id={id}"), b"").body;
        assert!(edited != data);
        let o = json_of(&a, "POST", &format!("/api/library/open?id={id}"), b"");
        assert_eq!(o["model"]["zones"][0]["name"], "Edited zone");
        assert_eq!(o["model"]["automation"]["rules"][1]["disabled"], true);
        assert!(
            o["model"]["automation"]["rules"][0]
                .get("disabled")
                .is_none()
        );
        assert_eq!(o["library"]["notes"], "edited");

        // history: the first version is the file as imported; restoring it brings the original bytes back
        let h = json_of(&a, "GET", &format!("/api/library/history?id={id}"), b"");
        let v = h[0]["id"].as_i64().unwrap();
        let old = dispatch(
            &a,
            "GET",
            &format!("/api/library/history/export?id={id}&version={v}"),
            b"",
        );
        assert!(old.body == data);
        json_of(
            &a,
            "POST",
            "/api/library/history/restore",
            format!(r#"{{"id":{id},"versionId":{v}}}"#).as_bytes(),
        );
        assert_eq!(
            dispatch(&a, "GET", &format!("/api/library/export?id={id}"), b"").body,
            data
        );
        let h = json_of(&a, "GET", &format!("/api/library/history?id={id}"), b"");
        assert_eq!(
            h.as_array().unwrap().len(),
            2,
            "the restored state is itself kept"
        );

        // save as a new account from what is open
        let body = json!({"name": "Copy", "folderId": 0, "model": model_now(&a)}).to_string();
        let s = json_of(&a, "POST", "/api/library/save", body.as_bytes());
        assert_eq!(s["created"], true);
        let l = json_of(&a, "GET", "/api/library/list?q=copy", b"");
        assert_eq!(l["accounts"].as_array().unwrap().len(), 1);

        // move, rename, delete
        let moved = format!(r#"{{"id":{id},"folderId":{f},"name":"Smith house"}}"#);
        json_of(&a, "POST", "/api/library/account/update", moved.as_bytes());
        let (s, t) = call(
            &a,
            "POST",
            "/api/library/folder/delete",
            format!(r#"{{"id":{f}}}"#).as_bytes(),
        );
        assert_eq!(s, 400);
        assert!(t.contains("not empty"), "{t}");
        json_of(
            &a,
            "POST",
            "/api/library/folder/delete",
            format!(r#"{{"id":{f},"withContent":true}}"#).as_bytes(),
        );
        let l = json_of(&a, "GET", "/api/library/list", b"");
        assert_eq!(
            l["accounts"].as_array().unwrap().len(),
            1,
            "only the copy in the top level is left"
        );
        assert!(l["folders"].as_array().unwrap().is_empty());
    }

    #[test]
    fn panel_settings_are_kept_without_the_code() {
        let a = app();
        let data = sample();
        dispatch(&a, "POST", "/api/open?name=S", &data);
        let body = json!({"name": "With panel", "model": model_now(&a),
            "panel": {"host": "192.0.2.5", "port": 2601, "tls": true, "serial": "00000001", "code": "TOPSECRET", "passphrase": "SECRETPASS"}}).to_string();
        let id = json_of(&a, "POST", "/api/library/save", body.as_bytes())["id"]
            .as_i64()
            .unwrap();
        let o = json_of(&a, "POST", &format!("/api/library/open?id={id}"), b"");
        assert_eq!(o["library"]["panel"]["host"], "192.0.2.5");
        assert!(o["library"]["panel"]["code"].is_null());
        assert!(o["library"]["panel"]["passphrase"].is_null());
        let raw = std::fs::read(a.library.dir().unwrap().join("library.sqlite")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("TOPSECRET"));
        // the user ticked "remember code"
        let upd = json!({"id": id, "panel": {"host": "192.0.2.5", "port": 2601, "rememberCode": true, "code": "TOPSECRET"}}).to_string();
        json_of(&a, "POST", "/api/library/account/update", upd.as_bytes());
        let o = json_of(&a, "POST", &format!("/api/library/open?id={id}"), b"");
        assert_eq!(o["library"]["panel"]["code"], "TOPSECRET");
    }

    #[test]
    fn backup_settings_and_route_shapes() {
        let a = app();
        let b = json_of(&a, "POST", "/api/library/backup", b"");
        let p = PathBuf::from(b["path"].as_str().unwrap());
        assert!(p.is_file() && p.extension().unwrap() == "zip");
        let s = json_of(&a, "POST", "/api/library/settings", br#"{"historyKeep":3}"#);
        assert_eq!(s["historyKeep"], 3);
        assert_eq!(
            json_of(&a, "GET", "/api/library/list", b"")["historyKeep"],
            3
        );
        assert_eq!(
            call(
                &a,
                "POST",
                "/api/library/settings",
                br#"{"historyKeep":9999}"#
            )
            .0,
            400
        );
        // ADR-018: autosave after a panel connection is on by default and can be switched off (and keeps the other settings)
        assert_eq!(
            json_of(&a, "GET", "/api/library/settings", b"")["autoSavePanel"],
            true
        );
        let s = json_of(
            &a,
            "POST",
            "/api/library/settings",
            br#"{"autoSavePanel":false}"#,
        );
        assert_eq!(
            (s["autoSavePanel"].clone(), s["historyKeep"].clone()),
            (json!(false), json!(3))
        );
        assert_eq!(
            json_of(&a, "GET", "/api/library/settings", b"")["autoSavePanel"],
            false
        );
        assert_eq!(call(&a, "POST", "/api/library/list", b"").0, 405);
        assert_eq!(call(&a, "GET", "/api/library/nope", b"").0, 404);
        assert_eq!(call(&a, "GET", "/API/Library/List", b"").0, 200);
        assert_eq!(call(&a, "POST", "/api/library/open", b"").0, 400);
        assert_eq!(call(&a, "POST", "/api/library/open?id=99", b"").0, 400);
        assert_eq!(call(&a, "POST", "/api/library/save", b"{}").0, 400);
    }

    #[test]
    fn an_unavailable_library_is_reported_and_the_editor_goes_on() {
        let dir = std::env::temp_dir().join(format!("elk-lib-routes-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("blocker"), b"x").unwrap();
        let a = App {
            shared: Shared::with_backup_dir(dir.join("b")),
            library: crate::state::LibState::unavailable("no place for the data".into()),
            exe: PathBuf::from("x"),
            restart: false,
        };
        let v = json_of(&a, "GET", "/api/library/list", b"");
        assert_eq!(v["available"], false);
        assert_eq!(v["error"], "no place for the data");
        let (s, t) = call(&a, "POST", "/api/library/folder/create", br#"{"name":"x"}"#);
        assert_eq!(s, 400);
        assert!(t.contains("not available"));
        assert_eq!(call(&a, "GET", "/api/meta", b"").0, 200);
        let _ = std::fs::remove_dir_all(dir);
    }
}
