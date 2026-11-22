//! Storage round trips, folders, history, panel settings, backup, concurrency, settings and the data folder choice.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use elk_library::location::{bundle_parent, choose, primary_dir, resolve};
use elk_library::*;

static N: AtomicU32 = AtomicU32::new(0);

fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "elk-library-test-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn lib(name: &str) -> (Library, PathBuf) {
    let d = temp(name);
    (Library::open(&d.join("data")).unwrap(), d)
}

fn acc(name: &str, folder: i64, content: &[u8]) -> NewAccount {
    NewAccount {
        name: name.into(),
        folder_id: folder,
        content: content.to_vec(),
        ..Default::default()
    }
}

#[test]
fn stored_file_comes_back_byte_exact() {
    let (l, d) = lib("bytes");
    // every byte value, CR and LF in all combinations, a Latin-1 text with a trailing CRLF
    let mut b: Vec<u8> = (0..=255u8).collect();
    b.extend_from_slice(b"\r\n\n\r\r\n\xe9\xfc\r\n");
    let id = l.add_account(acc("Everything", 0, &b)).unwrap();
    assert_eq!(l.get_account(id).unwrap().content, b);
    let reopened = Library::open(&d.join("data")).unwrap();
    assert_eq!(reopened.get_account(id).unwrap().content, b);
    assert_eq!(
        reopened.list(None).unwrap().accounts[0].size,
        b.len() as i64
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn folders_nest_rename_move_and_refuse_cycles() {
    let (l, d) = lib("folders");
    let a = l.create_folder(0, "Customers").unwrap();
    let b = l.create_folder(a, "Smith").unwrap();
    let c = l.create_folder(b, "2026").unwrap();
    assert!(
        l.create_folder(a, "smith").is_err(),
        "names are unique per folder, ignoring case"
    );
    assert!(l.create_folder(0, "  ").is_err());
    assert!(l.create_folder(99, "x").is_err());
    l.rename_folder(b, "Smith family").unwrap();
    assert!(l.rename_folder(b, "").is_err());
    assert!(l.move_folder(a, c).is_err(), "into its own sub-folder");
    assert!(l.move_folder(a, a).is_err());
    l.move_folder(c, 0).unwrap();
    l.move_folder(b, 0).unwrap();
    let f = l.list(None).unwrap().folders;
    assert_eq!(f.iter().find(|x| x.id == c).unwrap().parent_id, 0);
    assert_eq!(f.iter().find(|x| x.id == b).unwrap().name, "Smith family");
    // accounts move between folders; a name clash is refused
    let x = l.add_account(acc("Site 1", a, b"1")).unwrap();
    let y = l.add_account(acc("Site 1", b, b"2")).unwrap();
    assert!(l.add_account(acc("site 1", a, b"3")).is_err());
    assert!(l
        .edit_account(
            y,
            AccountEdit {
                folder_id: Some(a),
                ..Default::default()
            }
        )
        .is_err());
    l.edit_account(
        x,
        AccountEdit {
            folder_id: Some(c),
            name: Some("Site one".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let got = l.get_account(x).unwrap().summary;
    assert_eq!((got.folder_id, got.name.as_str()), (c, "Site one"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn deleting_folders_needs_empty_or_confirmation() {
    let (l, d) = lib("delete");
    let a = l.create_folder(0, "A").unwrap();
    let b = l.create_folder(a, "B").unwrap();
    let x = l.add_account(acc("In B", b, b"x")).unwrap();
    match l.delete_folder(a, false) {
        Err(Error::NotEmpty(m)) => assert!(m.contains("1 folder(s) and 1 account(s)"), "{m}"),
        o => panic!("{o:?}"),
    }
    assert!(l.get_account(x).is_ok());
    l.delete_folder(a, true).unwrap();
    assert!(matches!(l.get_account(x), Err(Error::NotFound(_))));
    assert!(l.list(None).unwrap().folders.is_empty());
    let e = l.create_folder(0, "Empty").unwrap();
    l.delete_folder(e, false).unwrap();
    assert!(matches!(l.delete_folder(e, false), Err(Error::NotFound(_))));
    // the history of a deleted account goes with it
    let y = l.add_account(acc("Y", 0, b"1")).unwrap();
    l.update_content(y, b"2", &[], 20).unwrap();
    l.delete_account(y).unwrap();
    assert!(l.history(y).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn history_keeps_the_last_n_and_restores() {
    let (l, d) = lib("history");
    let id = l.add_account(acc("Acc", 0, b"v0")).unwrap();
    for i in 1..=5 {
        let o = l
            .update_content(id, format!("v{i}").as_bytes(), &[], 3)
            .unwrap();
        assert!(o.changed);
    }
    assert_eq!(l.get_account(id).unwrap().content, b"v5");
    let h = l.history(id).unwrap();
    assert_eq!(h.len(), 3, "only the last 3 earlier versions are kept");
    let contents: Vec<Vec<u8>> = h
        .iter()
        .map(|v| l.version_content(id, v.id).unwrap())
        .collect();
    assert_eq!(
        contents,
        vec![b"v4".to_vec(), b"v3".to_vec(), b"v2".to_vec()]
    );
    // a save that changes nothing makes no version
    let o = l.update_content(id, b"v5", &[], 3).unwrap();
    assert!(!o.changed);
    assert_eq!(l.history(id).unwrap().len(), 3);
    // restoring v3 keeps v5 in the history, so the restore can be undone
    l.restore_version(id, h[1].id, 3).unwrap();
    assert_eq!(l.get_account(id).unwrap().content, b"v3");
    let after: Vec<Vec<u8>> = l
        .history(id)
        .unwrap()
        .iter()
        .map(|v| l.version_content(id, v.id).unwrap())
        .collect();
    assert_eq!(after[0], b"v5");
    assert!(l.restore_version(id, 999_999, 3).is_err());
    // keep 0 keeps nothing
    l.update_content(id, b"w", &[], 0).unwrap();
    assert!(l.history(id).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn disabled_rule_flags_are_stored_and_are_part_of_a_change() {
    let (l, d) = lib("flags");
    let mut n = acc("Rules", 0, b"file");
    n.disabled_rules = vec![2, 0, 2];
    let id = l.add_account(n).unwrap();
    assert_eq!(l.get_account(id).unwrap().disabled_rules, vec![0, 2]);
    // same file, other flags: a new version
    assert!(l.update_content(id, b"file", &[1], 20).unwrap().changed);
    assert_eq!(l.get_account(id).unwrap().disabled_rules, vec![1]);
    let h = l.history(id).unwrap();
    assert_eq!(h.len(), 1);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn panel_settings_never_keep_the_code_unless_asked() {
    let (l, d) = lib("panel");
    let p = PanelSettings {
        host: " 192.0.2.10 ".into(),
        port: 2601,
        tls: true,
        serial: Some("00000001".into()),
        code: Some("SECRET-CODE".into()),
        passphrase: Some("SECRET-PASS".into()),
        ..Default::default()
    };
    let mut n = acc("With panel", 0, b"x");
    n.panel = Some(p.clone());
    let id = l.add_account(n).unwrap();
    let got = l.get_account(id).unwrap().panel.unwrap();
    assert_eq!(got.host, "192.0.2.10");
    assert_eq!((got.code, got.passphrase), (None, None));
    // not even in the database file
    let raw = std::fs::read(l.dir().join(DB_FILE)).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("SECRET"));
    // remembered on request
    l.edit_account(
        id,
        AccountEdit {
            panel: Some(Some(PanelSettings {
                remember_code: true,
                ..p
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let got = l.get_account(id).unwrap().panel.unwrap();
    assert_eq!(got.code.as_deref(), Some("SECRET-CODE"));
    assert!(l.get_account(id).unwrap().summary.has_panel);
    l.edit_account(
        id,
        AccountEdit {
            panel: Some(None),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(l.get_account(id).unwrap().panel.is_none());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn search_matches_name_notes_and_tags_and_escapes_wildcards() {
    let (l, d) = lib("search");
    let mut a = acc("Harbour View", 0, b"1");
    a.notes = "100% done".into();
    a.tags = "marine, M1G".into();
    l.add_account(a).unwrap();
    l.add_account(acc("Other", 0, b"2")).unwrap();
    let names = |q: &str| -> Vec<String> {
        l.list(Some(q))
            .unwrap()
            .accounts
            .into_iter()
            .map(|a| a.name)
            .collect()
    };
    assert_eq!(names("harbour"), vec!["Harbour View"]);
    assert_eq!(names("marine"), vec!["Harbour View"]);
    assert_eq!(names("100%"), vec!["Harbour View"]);
    assert!(names("%").len() == 1, "a percent sign is not a wildcard");
    assert!(names("_").is_empty());
    assert_eq!(l.list(Some("  ")).unwrap().accounts.len(), 2);
    assert_eq!(l.free_name(0, "other").unwrap(), "other (2)");
    assert_eq!(l.free_name(0, "New").unwrap(), "New");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn backup_zip_holds_a_working_copy_of_the_library() {
    let (l, d) = lib("backup");
    let f = l.create_folder(0, "F").unwrap();
    let id = l.add_account(acc("Kept", f, b"content\r\n")).unwrap();
    Settings {
        history_keep: 7,
        update_url: None,
        ..Settings::default()
    }
    .save(l.dir())
    .unwrap();
    let p = l.backup(&d.join("backups")).unwrap();
    assert!(p.extension().unwrap() == "zip");
    assert_eq!(
        std::fs::read_dir(d.join("backups")).unwrap().count(),
        1,
        "no temporary file is left"
    );
    let mut z = zip::ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
    let out = d.join("restored");
    std::fs::create_dir_all(&out).unwrap();
    for name in [DB_FILE, "settings.json", "README.txt"] {
        let mut e = z.by_name(name).unwrap();
        let mut o = std::fs::File::create(out.join(name)).unwrap();
        std::io::copy(&mut e, &mut o).unwrap();
    }
    let r = Library::open(&out).unwrap();
    assert_eq!(r.get_account(id).unwrap().content, b"content\r\n");
    assert_eq!(r.list(None).unwrap().folders.len(), 1);
    // a second backup in the same second gets another name
    let p2 = l.backup(&d.join("backups")).unwrap();
    assert_ne!(p, p2);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn concurrent_writers_lose_nothing() {
    let d = temp("concurrent");
    let dir = d.join("data");
    Library::open(&dir).unwrap();
    let handles: Vec<_> = (0..6)
        .map(|t| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                // each thread has its own connection, like a second editor process on the same folder
                let l = Library::open(&dir).unwrap();
                for i in 0..15 {
                    let id = l
                        .add_account(acc(&format!("t{t}-{i}"), 0, format!("{t}/{i}").as_bytes()))
                        .unwrap();
                    l.update_content(id, format!("{t}/{i}/b").as_bytes(), &[], 20)
                        .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let l = Library::open(&dir).unwrap();
    let all = l.list(None).unwrap().accounts;
    assert_eq!(all.len(), 90);
    assert!(all.iter().all(|a| a.versions == 1));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_failed_change_leaves_nothing_behind() {
    let (l, d) = lib("atomic");
    let id = l.add_account(acc("A", 0, b"one")).unwrap();
    let other = l.add_account(acc("B", 0, b"two")).unwrap();
    // the rename fails on the name clash; the file, history and name are as before
    assert!(l
        .edit_account(
            id,
            AccountEdit {
                name: Some("b".into()),
                notes: Some("n".into()),
                ..Default::default()
            }
        )
        .is_err());
    let a = l.get_account(id).unwrap();
    assert_eq!(
        (a.summary.name.as_str(), a.summary.notes.as_str()),
        ("A", "")
    );
    assert!(l.update_content(12345, b"x", &[], 20).is_err());
    assert!(l.delete_folder(0, true).is_err());
    assert_eq!(l.get_account(other).unwrap().content, b"two");
    // an older library file is opened as is (the schema is versioned)
    drop(l);
    assert!(Library::open(&d.join("data")).is_ok());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn settings_round_trip_and_bad_files_fall_back() {
    let d = temp("settings");
    let (s, w) = Settings::load(&d);
    assert_eq!((s.history_keep, w), (20, None));
    assert!(
        s.auto_save_panel,
        "autosave after a panel connection is on by default (ADR-018)"
    );
    Settings {
        history_keep: 5,
        update_url: Some("http://127.0.0.1:1/x".into()),
        auto_save_panel: false,
    }
    .save(&d)
    .unwrap();
    let (s, w) = Settings::load(&d);
    assert_eq!(
        (s.history_keep, s.update_url.as_deref(), w),
        (5, Some("http://127.0.0.1:1/x"), None)
    );
    assert!(!s.auto_save_panel);
    assert_eq!(
        std::fs::read_dir(&d).unwrap().count(),
        1,
        "the temporary file is renamed away"
    );
    std::fs::write(d.join("settings.json"), b"{ not json").unwrap();
    let (s, w) = Settings::load(&d);
    assert_eq!(s.history_keep, 20);
    assert!(w.unwrap().contains("not valid"));
    std::fs::write(d.join("settings.json"), br#"{"historyKeep": 99999}"#).unwrap();
    assert_eq!(Settings::load(&d).0.history_keep, MAX_HISTORY_KEEP);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn data_folder_choice() {
    let d = temp("location");
    // next to the exe on Windows, next to the .app bundle on macOS
    let exe =
        Path::new("/Applications/Tools/Elk Account Editor.app/Contents/MacOS/elkaccounteditor");
    assert_eq!(
        bundle_parent(exe).unwrap(),
        Path::new("/Applications/Tools")
    );
    assert_eq!(
        primary_dir(exe),
        Path::new("/Applications/Tools/ElkEditor data")
    );
    let win = Path::new("C:/Tools/ElkEditor/elkaccounteditor.exe");
    assert_eq!(bundle_parent(win), None);
    assert_eq!(
        primary_dir(win),
        Path::new("C:/Tools/ElkEditor/ElkEditor data")
    );

    // a place that cannot be created (a file is in the way) is skipped for the next one
    let blocker = d.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let bad = blocker.join("ElkEditor data");
    let good = d.join("fallback");
    let c = choose(&[bad.clone(), good.clone()]).unwrap();
    assert_eq!((c.dir.clone(), c.fell_back), (good.clone(), true));
    assert!(c.note.contains("not writable"));
    let c = choose(&[d.join("first"), good.clone()]).unwrap();
    assert!(!c.fell_back);
    assert!(choose(std::slice::from_ref(&bad)).is_err());
    // existing data wins over an empty preferred folder
    Library::open(&good).unwrap();
    let c = choose(&[d.join("empty-first"), good.clone()]).unwrap();
    assert_eq!(c.dir, good);

    // an explicit choice is used as given and never replaced by a fallback
    let given = d.join("given");
    let r = resolve(Some(given.to_str().unwrap()), Some("ignored"), exe).unwrap();
    assert_eq!(r.dir, given);
    assert!(r.note.contains("--data-dir"));
    let env = d.join("fromenv");
    let r = resolve(None, Some(env.to_str().unwrap()), exe).unwrap();
    assert_eq!(r.dir, env);
    assert!(resolve(Some(bad.to_str().unwrap()), None, exe).is_err());
    assert!(resolve(Some("  "), Some(""), &d.join("x").join("prog.exe")).is_ok());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn library_dir_names_are_what_the_docs_say() {
    assert_eq!(FOLDER_NAME, "ElkEditor data");
    assert_eq!(DB_FILE, "library.sqlite");
    assert_eq!(DEFAULT_HISTORY_KEEP, 20);
}
