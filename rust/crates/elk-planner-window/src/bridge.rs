//! The page <-> host message protocol of `NativeWindow.cs`, free of any GUI so it can be unit tested.
//!
//! The page sends `{cmd:'openFile', purpose?}` or `{cmd:'saveFile', purpose?, ext?, name, b64}` and gets back
//! `opened`, `saved`, `cancelled` or `error`. Unknown commands get no reply (as in the C#).

use crate::jsonw;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The file dialogs. `RfdDialogs` is the real one; tests use a fake.
pub trait Dialogs {
    /// Shows an "open file" dialog. `None` means the user cancelled.
    fn pick_open(&self, title: &str) -> Option<PathBuf>;
    /// Shows a "save file" dialog starting in `dir` (when known) with `suggested` as the file name. `None` means cancelled.
    fn pick_save(&self, title: &str, dir: Option<&Path>, suggested: &str) -> Option<PathBuf>;
}

/// What the host sends back to the page.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// No reply at all (unknown command).
    None,
    Cancelled,
    Opened {
        purpose: Option<String>,
        name: String,
        b64: String,
    },
    Saved {
        purpose: Option<String>,
        path: String,
    },
    Error {
        message: String,
    },
}

impl Reply {
    /// The JSON text the page receives, written like `JsonSerializer.Serialize` of the C# anonymous objects
    /// (same member order, `purpose` as `null` when absent). `None` for `Reply::None`.
    pub fn to_json(&self) -> Option<String> {
        let mut o = String::new();
        let opt = |o: &mut String, v: &Option<String>| match v {
            Some(s) => jsonw::push_str(o, s),
            None => o.push_str("null"),
        };
        match self {
            Reply::None => return None,
            Reply::Cancelled => o.push_str("{\"cmd\":\"cancelled\"}"),
            Reply::Opened { purpose, name, b64 } => {
                o.push_str("{\"cmd\":\"opened\",\"purpose\":");
                opt(&mut o, purpose);
                o.push_str(",\"name\":");
                jsonw::push_str(&mut o, name);
                o.push_str(",\"b64\":");
                jsonw::push_str(&mut o, b64);
                o.push('}');
            }
            Reply::Saved { purpose, path } => {
                o.push_str("{\"cmd\":\"saved\",\"purpose\":");
                opt(&mut o, purpose);
                o.push_str(",\"path\":");
                jsonw::push_str(&mut o, path);
                o.push('}');
            }
            Reply::Error { message } => {
                o.push_str("{\"cmd\":\"error\",\"message\":");
                jsonw::push_str(&mut o, message);
                o.push('}');
            }
        }
        Some(o)
    }
}

const MISSING: &str = "The given key was not present in the dictionary.";
const WRONG_KIND: &str = "The requested operation requires an element of a different type.";

/// Member that must exist (`GetProperty`). Names are case-sensitive, as with `JsonElement`.
fn required<'a>(root: &'a Value, name: &str) -> Result<&'a Value, String> {
    root.as_object()
        .ok_or_else(|| WRONG_KIND.to_string())?
        .get(name)
        .ok_or_else(|| MISSING.to_string())
}

/// `JsonElement.GetString()`: null gives `None`, any other non-string throws.
fn get_string(v: &Value) -> Result<Option<String>, String> {
    match v {
        Value::String(s) => Ok(Some(s.clone())),
        Value::Null => Ok(None),
        _ => Err(WRONG_KIND.to_string()),
    }
}

/// `TryGetProperty(name, ...) ? GetString() : null`.
fn optional_string(root: &Value, name: &str) -> Result<Option<String>, String> {
    match root.get(name) {
        Some(v) => get_string(v),
        None => Ok(None),
    }
}

fn open_title(purpose: Option<&str>) -> &'static str {
    match purpose {
        Some("zones") => "Open a zone list (Excel .xlsx or CSV)",
        Some("plan") => "Open a data bus plan (.json)",
        _ => "Open an ElkRP Export Account file",
    }
}

fn save_title(purpose: Option<&str>, name: &str) -> String {
    let lead = match purpose {
        Some("zones") => "Save the zone list (suggested name: ",
        Some("plan") => "Save the data bus plan (suggested name: ",
        _ => "Save edited export (suggested name: ",
    };
    format!("{}{})", lead, name)
}

/// Handles one message from the page. Never panics on bad input: problems come back as `Reply::Error`.
pub fn handle_message(json: &str, dialogs: &dyn Dialogs) -> Reply {
    match handle(json, dialogs) {
        Ok(r) => r,
        Err(message) => Reply::Error { message },
    }
}

fn handle(json: &str, dialogs: &dyn Dialogs) -> Result<Reply, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let cmd = get_string(required(&root, "cmd")?)?;
    match cmd.as_deref() {
        Some("openFile") => open_file(&root, dialogs),
        Some("saveFile") => save_file(&root, dialogs),
        _ => Ok(Reply::None),
    }
}

fn open_file(root: &Value, dialogs: &dyn Dialogs) -> Result<Reply, String> {
    let purpose = optional_string(root, "purpose")?;
    let Some(picked) = dialogs.pick_open(open_title(purpose.as_deref())) else {
        return Ok(Reply::Cancelled);
    };
    if picked.as_os_str().is_empty() {
        return Ok(Reply::Cancelled);
    }
    let bytes = std::fs::read(&picked).map_err(|e| e.to_string())?;
    Ok(Reply::Opened {
        purpose,
        name: picked
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        b64: STANDARD.encode(bytes),
    })
}

fn save_file(root: &Value, dialogs: &dyn Dialogs) -> Result<Reply, String> {
    let name = get_string(required(root, "name")?)?.unwrap_or_else(|| "export".to_string());
    let dir = dirs::document_dir();
    let purpose = optional_string(root, "purpose")?;
    let ext = optional_string(root, "ext")?;
    let Some(mut path) = dialogs.pick_save(
        &save_title(purpose.as_deref(), &name),
        dir.as_deref(),
        &name,
    ) else {
        return Ok(Reply::Cancelled);
    };
    if path.as_os_str().is_empty() {
        return Ok(Reply::Cancelled);
    }
    // the dialog may not add the extension
    if let Some(ext) = ext.as_deref().filter(|e| !e.is_empty()) {
        if path.extension().is_none_or(|e| e.is_empty()) {
            let mut s = path.into_os_string();
            s.push(ext);
            path = PathBuf::from(s);
        }
    }
    let b64 = get_string(required(root, "b64")?)?.unwrap_or_default();
    let bytes = STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("The input is not a valid Base-64 string: {}", e))?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(Reply::Saved {
        purpose,
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        open: Option<PathBuf>,
        save: Option<PathBuf>,
        seen: RefCell<Vec<String>>,
    }
    impl Dialogs for Fake {
        fn pick_open(&self, title: &str) -> Option<PathBuf> {
            self.seen.borrow_mut().push(title.to_string());
            self.open.clone()
        }
        fn pick_save(&self, title: &str, _dir: Option<&Path>, suggested: &str) -> Option<PathBuf> {
            self.seen
                .borrow_mut()
                .push(format!("{}|{}", title, suggested));
            self.save.clone()
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("elk-planner-window-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn open_reads_the_file_and_replies_with_base64() {
        let p = tmp("plan.json");
        std::fs::write(&p, b"{\"a\":1}").unwrap();
        let fake = Fake {
            open: Some(p),
            ..Default::default()
        };
        let r = handle_message(r#"{"cmd":"openFile","purpose":"plan"}"#, &fake);
        assert_eq!(fake.seen.borrow()[0], "Open a data bus plan (.json)");
        assert_eq!(
            r,
            Reply::Opened {
                purpose: Some("plan".into()),
                name: "plan.json".into(),
                b64: "eyJhIjoxfQ==".into()
            }
        );
        assert_eq!(
            r.to_json().unwrap(),
            r#"{"cmd":"opened","purpose":"plan","name":"plan.json","b64":"eyJhIjoxfQ=="}"#
        );
    }

    #[test]
    fn open_titles_follow_the_purpose() {
        let fake = Fake::default();
        handle_message(r#"{"cmd":"openFile","purpose":"zones"}"#, &fake);
        handle_message(r#"{"cmd":"openFile"}"#, &fake);
        assert_eq!(
            *fake.seen.borrow(),
            vec![
                "Open a zone list (Excel .xlsx or CSV)",
                "Open an ElkRP Export Account file"
            ]
        );
    }

    #[test]
    fn open_without_purpose_serialises_null_purpose() {
        let p = tmp("acct.txt");
        std::fs::write(&p, b"x").unwrap();
        let fake = Fake {
            open: Some(p),
            ..Default::default()
        };
        let j = handle_message(r#"{"cmd":"openFile"}"#, &fake)
            .to_json()
            .unwrap();
        assert_eq!(
            j,
            r#"{"cmd":"opened","purpose":null,"name":"acct.txt","b64":"eA=="}"#
        );
    }

    #[test]
    fn cancelled_open_and_save() {
        let fake = Fake::default();
        assert_eq!(
            handle_message(r#"{"cmd":"openFile"}"#, &fake),
            Reply::Cancelled
        );
        assert_eq!(
            handle_message(r#"{"cmd":"saveFile","name":"x","b64":""}"#, &fake),
            Reply::Cancelled
        );
        assert_eq!(
            Reply::Cancelled.to_json().unwrap(),
            r#"{"cmd":"cancelled"}"#
        );
    }

    #[test]
    fn save_writes_bytes_and_appends_a_missing_extension() {
        let p = tmp("noext");
        let _ = std::fs::remove_file(tmp("noext.json"));
        let fake = Fake {
            save: Some(p.clone()),
            ..Default::default()
        };
        let r = handle_message(
            r#"{"cmd":"saveFile","purpose":"plan","ext":".json","name":"plan","b64":"eyJhIjoxfQ=="}"#,
            &fake,
        );
        let want = tmp("noext.json");
        assert_eq!(
            r,
            Reply::Saved {
                purpose: Some("plan".into()),
                path: want.to_string_lossy().into_owned()
            }
        );
        assert_eq!(std::fs::read(&want).unwrap(), b"{\"a\":1}");
        assert_eq!(
            fake.seen.borrow()[0],
            "Save the data bus plan (suggested name: plan)|plan"
        );
    }

    #[test]
    fn save_keeps_an_existing_extension() {
        let p = tmp("has.xlsx");
        let fake = Fake {
            save: Some(p.clone()),
            ..Default::default()
        };
        let r = handle_message(
            r#"{"cmd":"saveFile","purpose":"zones","ext":".csv","name":"z","b64":"AQID"}"#,
            &fake,
        );
        assert_eq!(
            r,
            Reply::Saved {
                purpose: Some("zones".into()),
                path: p.to_string_lossy().into_owned()
            }
        );
        assert_eq!(std::fs::read(&p).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            fake.seen.borrow()[0],
            "Save the zone list (suggested name: z)|z"
        );
    }

    #[test]
    fn save_without_purpose_uses_the_export_title() {
        let fake = Fake::default();
        handle_message(
            r#"{"cmd":"saveFile","name":"Acct (edited)","b64":""}"#,
            &fake,
        );
        assert_eq!(
            fake.seen.borrow()[0],
            "Save edited export (suggested name: Acct (edited))|Acct (edited)"
        );
    }

    #[test]
    fn bad_input_becomes_an_error_reply() {
        let fake = Fake::default();
        assert!(matches!(
            handle_message("not json", &fake),
            Reply::Error { .. }
        ));
        assert!(matches!(handle_message("{}", &fake), Reply::Error { .. }));
        assert!(matches!(handle_message("[1]", &fake), Reply::Error { .. }));
        assert!(matches!(
            handle_message(r#"{"cmd":5}"#, &fake),
            Reply::Error { .. }
        ));
        // saveFile without a name fails before the dialog is shown
        assert!(matches!(
            handle_message(r#"{"cmd":"saveFile"}"#, &fake),
            Reply::Error { .. }
        ));
        assert!(fake.seen.borrow().is_empty());
        let bad_b64 = Fake {
            save: Some(tmp("bad.bin")),
            ..Default::default()
        };
        assert!(matches!(
            handle_message(r#"{"cmd":"saveFile","name":"x","b64":"@@@"}"#, &bad_b64),
            Reply::Error { .. }
        ));
        let missing = Fake {
            open: Some(tmp("does-not-exist.bin")),
            ..Default::default()
        };
        let _ = std::fs::remove_file(tmp("does-not-exist.bin"));
        assert!(matches!(
            handle_message(r#"{"cmd":"openFile"}"#, &missing),
            Reply::Error { .. }
        ));
    }

    #[test]
    fn unknown_command_gets_no_reply() {
        let fake = Fake::default();
        assert_eq!(handle_message(r#"{"cmd":"ping"}"#, &fake), Reply::None);
        assert_eq!(handle_message(r#"{"cmd":null}"#, &fake), Reply::None);
        assert_eq!(Reply::None.to_json(), None);
    }

    #[test]
    fn error_json_escapes_like_the_default_encoder() {
        let j = Reply::Error {
            message: "can't \"x\"".into(),
        }
        .to_json()
        .unwrap();
        assert_eq!(j, r#"{"cmd":"error","message":"can\u0027t \u0022x\u0022"}"#);
    }
}
