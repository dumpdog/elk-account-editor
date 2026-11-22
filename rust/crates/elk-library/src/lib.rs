//! The accounts library of the Elk account editor (Rust editor only, ADR-015): nested folders and stored account export files in one SQLite
//! database inside the data folder, a version history per account, and the settings file of that folder.
//!
//! * The export file is stored as received, byte for byte (a BLOB; ElkRP's Latin-1 text with CRLF), and comes back byte-identical.
//! * An account keeps the last `history_keep` earlier versions; a save that changes nothing makes no version.
//! * Panel connection settings are optional and never hold the RP access code or the encryption passphrase unless "remember code" is set.
//! * Every change is one SQLite transaction (`BEGIN IMMEDIATE`), so a crash or a second editor never leaves a half-written account.

pub mod error;
pub mod location;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub use error::{Error, Result};
pub use location::{DataLocation, DB_FILE, FOLDER_NAME};
pub use settings::{Settings, DEFAULT_HISTORY_KEEP, MAX_HISTORY_KEEP};

const SCHEMA_VERSION: i64 = 1;
const MAX_NAME: usize = 120;
const MAX_NOTES: usize = 4000;

/// Panel connection settings stored with an account (what the Panel dialog asks for).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelSettings {
    pub host: String,
    pub port: i64,
    pub plain: bool,
    pub tls: bool,
    pub serial: Option<String>,
    pub mac: Option<String>,
    /// The user's tick "remember the RP access code": only then are `code` and `passphrase` kept.
    pub remember_code: bool,
    pub code: Option<String>,
    pub passphrase: Option<String>,
}

impl PanelSettings {
    /// What may be written to the database: without the secrets unless `remember_code` is set.
    pub fn for_storage(mut self) -> Self {
        self.host = self.host.trim().to_string();
        if !self.remember_code {
            self.code = None;
            self.passphrase = None;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub id: i64,
    pub parent_id: i64,
    pub name: String,
    pub created: i64,
    pub updated: i64,
}

/// An account without its content.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: i64,
    pub folder_id: i64,
    pub name: String,
    pub notes: String,
    pub tags: String,
    pub created: i64,
    pub updated: i64,
    pub size: i64,
    pub versions: i64,
    pub has_panel: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub folders: Vec<FolderRow>,
    pub accounts: Vec<AccountSummary>,
}

/// A stored account with its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub summary: AccountSummary,
    pub content: Vec<u8>,
    /// 0-based indexes of the rules marked disabled in the editor (ADR-014).
    pub disabled_rules: Vec<usize>,
    pub panel: Option<PanelSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: i64,
    pub saved: i64,
    pub size: i64,
}

/// What a content update did.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateOutcome {
    /// False when the stored file and flags were already identical (nothing was written).
    pub changed: bool,
    pub versions_kept: i64,
}

/// What to store for a new account.
#[derive(Debug, Clone, Default)]
pub struct NewAccount {
    pub name: String,
    pub folder_id: i64,
    pub notes: String,
    pub tags: String,
    pub content: Vec<u8>,
    pub disabled_rules: Vec<usize>,
    pub panel: Option<PanelSettings>,
}

/// Changes to the details of an account (not its file). `None` leaves a field alone; `panel: Some(None)` removes the stored settings.
#[derive(Debug, Clone, Default)]
pub struct AccountEdit {
    pub name: Option<String>,
    pub folder_id: Option<i64>,
    pub notes: Option<String>,
    pub tags: Option<String>,
    pub panel: Option<Option<PanelSettings>>,
}

pub struct Library {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn clean_name(what: &str, name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(Error::Invalid(format!("The {what} needs a name.")));
    }
    if n.chars().count() > MAX_NAME {
        return Err(Error::Invalid(format!(
            "The {what} name is longer than {MAX_NAME} characters."
        )));
    }
    if n.chars().any(|c| c.is_control()) {
        return Err(Error::Invalid(format!(
            "The {what} name must be one line of printable characters."
        )));
    }
    Ok(n.to_string())
}

fn clean_text(what: &str, s: &str, max: usize) -> Result<String> {
    if s.chars().count() > max {
        return Err(Error::Invalid(format!(
            "The {what} are too long (over {max} characters)."
        )));
    }
    Ok(s.trim().to_string())
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation)
}

fn disabled_json(d: &[usize]) -> String {
    let mut v: Vec<usize> = d.to_vec();
    v.sort_unstable();
    v.dedup();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

fn disabled_from(s: &str) -> Vec<usize> {
    serde_json::from_str(s).unwrap_or_default()
}

fn summary_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<AccountSummary> {
    Ok(AccountSummary {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        name: r.get(2)?,
        notes: r.get(3)?,
        tags: r.get(4)?,
        created: r.get(5)?,
        updated: r.get(6)?,
        size: r.get(7)?,
        versions: r.get(8)?,
        has_panel: r.get::<_, i64>(9)? != 0,
    })
}

const SUMMARY_SQL: &str = "SELECT a.id, a.folder_id, a.name, a.notes, a.tags, a.created, a.updated, length(a.content), \
    (SELECT count(*) FROM history h WHERE h.account_id = a.id), a.panel IS NOT NULL FROM accounts a";

impl Library {
    /// Opens (and creates) the library in `dir`.
    pub fn open(dir: &Path) -> Result<Library> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(DB_FILE))?;
        conn.busy_timeout(Duration::from_secs(15))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let lib = Library {
            dir: dir.to_path_buf(),
            conn: Mutex::new(conn),
        };
        lib.migrate()?;
        Ok(lib)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut g = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    }

    fn write<T>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>) -> Result<T> {
        self.with(|c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = f(&tx)?;
            tx.commit()?;
            Ok(out)
        })
    }

    fn migrate(&self) -> Result<()> {
        self.write(|tx| {
            let v: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if v > SCHEMA_VERSION {
                return Err(Error::Other(format!(
                    "This library was made by a newer editor (schema {v}); update the editor to open it."
                )));
            }
            if v < 1 {
                tx.execute_batch(
                    "CREATE TABLE folders(
                        id INTEGER PRIMARY KEY,
                        parent_id INTEGER NOT NULL DEFAULT 0,
                        name TEXT NOT NULL,
                        created INTEGER NOT NULL,
                        updated INTEGER NOT NULL);
                     CREATE UNIQUE INDEX folders_name ON folders(parent_id, name COLLATE NOCASE);
                     CREATE TABLE accounts(
                        id INTEGER PRIMARY KEY,
                        folder_id INTEGER NOT NULL DEFAULT 0,
                        name TEXT NOT NULL,
                        notes TEXT NOT NULL DEFAULT '',
                        tags TEXT NOT NULL DEFAULT '',
                        created INTEGER NOT NULL,
                        updated INTEGER NOT NULL,
                        content BLOB NOT NULL,
                        disabled_rules TEXT NOT NULL DEFAULT '[]',
                        panel TEXT);
                     CREATE UNIQUE INDEX accounts_name ON accounts(folder_id, name COLLATE NOCASE);
                     CREATE TABLE history(
                        id INTEGER PRIMARY KEY,
                        account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                        saved INTEGER NOT NULL,
                        content BLOB NOT NULL,
                        disabled_rules TEXT NOT NULL DEFAULT '[]');
                     CREATE INDEX history_account ON history(account_id, id);",
                )?;
                tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
            }
            Ok(())
        })
    }

    // ---- folders ------------------------------------------------------------------------------------------------------------

    fn folder_exists(tx: &Connection, id: i64) -> Result<bool> {
        if id == 0 {
            return Ok(true);
        }
        Ok(tx
            .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn require_folder(tx: &Connection, id: i64) -> Result<()> {
        if Self::folder_exists(tx, id)? {
            Ok(())
        } else {
            Err(Error::NotFound("That folder no longer exists.".into()))
        }
    }

    /// New folder under `parent_id` (0 = top level). Returns its id.
    pub fn create_folder(&self, parent_id: i64, name: &str) -> Result<i64> {
        let name = clean_name("folder", name)?;
        self.write(|tx| {
            Self::require_folder(tx, parent_id)?;
            let t = now();
            tx.execute(
                "INSERT INTO folders(parent_id, name, created, updated) VALUES (?1, ?2, ?3, ?3)",
                params![parent_id, name, t],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    Error::Invalid(format!("There is already a folder called \"{name}\" here."))
                } else {
                    e.into()
                }
            })?;
            Ok(tx.last_insert_rowid())
        })
    }

    pub fn rename_folder(&self, id: i64, name: &str) -> Result<()> {
        let name = clean_name("folder", name)?;
        self.write(|tx| {
            let n = tx
                .execute(
                    "UPDATE folders SET name = ?1, updated = ?2 WHERE id = ?3",
                    params![name, now(), id],
                )
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        Error::Invalid(format!("There is already a folder called \"{name}\" here."))
                    } else {
                        e.into()
                    }
                })?;
            if n == 0 {
                return Err(Error::NotFound("That folder no longer exists.".into()));
            }
            Ok(())
        })
    }

    /// Moves a folder (with everything in it) under `new_parent` (0 = top level); not into itself or one of its own sub-folders.
    pub fn move_folder(&self, id: i64, new_parent: i64) -> Result<()> {
        self.write(|tx| {
            let exists: Option<i64> = tx
                .query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if exists.is_none() {
                return Err(Error::NotFound("That folder no longer exists.".into()));
            }
            Self::require_folder(tx, new_parent)?;
            let mut at = new_parent;
            while at != 0 {
                if at == id {
                    return Err(Error::Invalid(
                        "A folder cannot be moved into itself or into one of its own folders."
                            .into(),
                    ));
                }
                at = tx.query_row("SELECT parent_id FROM folders WHERE id = ?1", [at], |r| {
                    r.get(0)
                })?;
            }
            tx.execute(
                "UPDATE folders SET parent_id = ?1, updated = ?2 WHERE id = ?3",
                params![new_parent, now(), id],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    Error::Invalid(
                        "A folder with that name already exists in the target folder.".into(),
                    )
                } else {
                    e.into()
                }
            })?;
            Ok(())
        })
    }

    fn descendants(tx: &Connection, id: i64) -> Result<Vec<i64>> {
        let mut all = vec![id];
        let mut i = 0;
        while i < all.len() {
            let mut st = tx.prepare("SELECT id FROM folders WHERE parent_id = ?1")?;
            let kids: Vec<i64> = st
                .query_map([all[i]], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            all.extend(kids);
            i += 1;
        }
        Ok(all)
    }

    /// Deletes a folder. Without `with_content` a folder that holds anything is refused (`NotEmpty` says what is in it).
    pub fn delete_folder(&self, id: i64, with_content: bool) -> Result<()> {
        self.write(|tx| {
            Self::require_folder(tx, id)?;
            if id == 0 {
                return Err(Error::Invalid("The top level cannot be deleted.".into()));
            }
            let ids = Self::descendants(tx, id)?;
            let marks = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let nacc: i64 = tx.query_row(
                &format!("SELECT count(*) FROM accounts WHERE folder_id IN ({marks})"),
                rusqlite::params_from_iter(ids.iter()),
                |r| r.get(0),
            )?;
            let nfold = ids.len() as i64 - 1;
            if (nacc > 0 || nfold > 0) && !with_content {
                return Err(Error::NotEmpty(format!(
                    "The folder is not empty: {nfold} folder(s) and {nacc} account(s) are inside it."
                )));
            }
            tx.execute(
                &format!("DELETE FROM accounts WHERE folder_id IN ({marks})"),
                rusqlite::params_from_iter(ids.iter()),
            )?;
            tx.execute(
                &format!("DELETE FROM folders WHERE id IN ({marks})"),
                rusqlite::params_from_iter(ids.iter()),
            )?;
            Ok(())
        })
    }

    // ---- accounts -----------------------------------------------------------------------------------------------------------

    fn unique_account_error(name: &str) -> Error {
        Error::Invalid(format!(
            "There is already an account called \"{name}\" in that folder."
        ))
    }

    /// A free name in `folder_id`: `name`, else `name (2)`, `name (3)` ... (used by imports).
    pub fn free_name(&self, folder_id: i64, name: &str) -> Result<String> {
        let base = clean_name("account", name)?;
        self.with(|c| {
            let mut cand = base.clone();
            let mut n = 2;
            loop {
                let taken = c
                    .query_row(
                        "SELECT 1 FROM accounts WHERE folder_id = ?1 AND name = ?2 COLLATE NOCASE",
                        params![folder_id, cand],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if !taken {
                    return Ok(cand);
                }
                cand = format!("{base} ({n})");
                n += 1;
            }
        })
    }

    pub fn add_account(&self, a: NewAccount) -> Result<i64> {
        let name = clean_name("account", &a.name)?;
        let notes = clean_text("notes", &a.notes, MAX_NOTES)?;
        let tags = clean_text("tags", &a.tags, 400)?;
        let panel = a
            .panel
            .map(|p| p.for_storage())
            .map(|p| serde_json::to_string(&p).map_err(|e| Error::Other(e.to_string())))
            .transpose()?;
        self.write(|tx| {
            Self::require_folder(tx, a.folder_id)?;
            let t = now();
            tx.execute(
                "INSERT INTO accounts(folder_id, name, notes, tags, created, updated, content, disabled_rules, panel) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8)",
                params![a.folder_id, name, notes, tags, t, a.content, disabled_json(&a.disabled_rules), panel],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    Self::unique_account_error(&name)
                } else {
                    e.into()
                }
            })?;
            Ok(tx.last_insert_rowid())
        })
    }

    pub fn get_account(&self, id: i64) -> Result<Account> {
        self.with(|c| {
            let summary = c
                .query_row(
                    &format!("{SUMMARY_SQL} WHERE a.id = ?1"),
                    [id],
                    summary_from,
                )
                .optional()?
                .ok_or_else(|| Error::NotFound("That account no longer exists.".into()))?;
            let (content, dis, panel): (Vec<u8>, String, Option<String>) = c.query_row(
                "SELECT content, disabled_rules, panel FROM accounts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            Ok(Account {
                summary,
                content,
                disabled_rules: disabled_from(&dis),
                panel: panel.and_then(|p| serde_json::from_str(&p).ok()),
            })
        })
    }

    /// Everything, or the accounts whose name, notes or tags contain `q` (case-insensitive for ASCII).
    pub fn list(&self, q: Option<&str>) -> Result<Listing> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, parent_id, name, created, updated FROM folders ORDER BY name COLLATE NOCASE")?;
            let folders = st
                .query_map([], |r| {
                    Ok(FolderRow {
                        id: r.get(0)?,
                        parent_id: r.get(1)?,
                        name: r.get(2)?,
                        created: r.get(3)?,
                        updated: r.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let q = q.map(str::trim).filter(|s| !s.is_empty());
            let accounts = match q {
                None => {
                    let mut st = c.prepare(&format!("{SUMMARY_SQL} ORDER BY a.name COLLATE NOCASE"))?;
                    let v = st.query_map([], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
                    v
                }
                Some(q) => {
                    let like = format!(
                        "%{}%",
                        q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
                    );
                    let mut st = c.prepare(&format!(
                        "{SUMMARY_SQL} WHERE a.name LIKE ?1 ESCAPE '\\' OR a.notes LIKE ?1 ESCAPE '\\' OR a.tags LIKE ?1 ESCAPE '\\' \
                         ORDER BY a.name COLLATE NOCASE"
                    ))?;
                    let v = st.query_map([like], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
                    v
                }
            };
            Ok(Listing { folders, accounts })
        })
    }

    /// Renames, moves or re-describes an account; the file is not touched.
    pub fn edit_account(&self, id: i64, e: AccountEdit) -> Result<()> {
        let name = e
            .name
            .as_deref()
            .map(|n| clean_name("account", n))
            .transpose()?;
        let notes = e
            .notes
            .as_deref()
            .map(|n| clean_text("notes", n, MAX_NOTES))
            .transpose()?;
        let tags = e
            .tags
            .as_deref()
            .map(|n| clean_text("tags", n, 400))
            .transpose()?;
        let panel: Option<Option<String>> = match e.panel {
            None => None,
            Some(None) => Some(None),
            Some(Some(p)) => Some(Some(
                serde_json::to_string(&p.for_storage()).map_err(|x| Error::Other(x.to_string()))?,
            )),
        };
        self.write(|tx| {
            let cur = tx
                .query_row("SELECT name FROM accounts WHERE id = ?1", [id], |r| r.get::<_, String>(0))
                .optional()?
                .ok_or_else(|| Error::NotFound("That account no longer exists.".into()))?;
            if let Some(f) = e.folder_id {
                Self::require_folder(tx, f)?;
            }
            let shown = name.clone().unwrap_or(cur);
            tx.execute(
                "UPDATE accounts SET name = COALESCE(?1, name), folder_id = COALESCE(?2, folder_id), notes = COALESCE(?3, notes), \
                 tags = COALESCE(?4, tags), updated = ?5 WHERE id = ?6",
                params![name, e.folder_id, notes, tags, now(), id],
            )
            .map_err(|x| {
                if is_unique_violation(&x) {
                    Self::unique_account_error(&shown)
                } else {
                    x.into()
                }
            })?;
            if let Some(p) = panel {
                tx.execute("UPDATE accounts SET panel = ?1 WHERE id = ?2", params![p, id])?;
            }
            Ok(())
        })
    }

    pub fn delete_account(&self, id: i64) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(Error::NotFound("That account no longer exists.".into()));
            }
            Ok(())
        })
    }

    fn push_history(tx: &Connection, id: i64, keep: u32) -> Result<()> {
        if keep > 0 {
            tx.execute(
                "INSERT INTO history(account_id, saved, content, disabled_rules) \
                 SELECT id, updated, content, disabled_rules FROM accounts WHERE id = ?1",
                [id],
            )?;
        }
        tx.execute(
            "DELETE FROM history WHERE account_id = ?1 AND id NOT IN \
             (SELECT id FROM history WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2)",
            params![id, keep as i64],
        )?;
        Ok(())
    }

    /// Replaces the stored file (and the editor's disabled-rule flags). The previous state becomes the newest history version; only the newest
    /// `history_keep` versions are kept. Identical content and flags change nothing.
    pub fn update_content(
        &self,
        id: i64,
        content: &[u8],
        disabled_rules: &[usize],
        history_keep: u32,
    ) -> Result<UpdateOutcome> {
        let dis = disabled_json(disabled_rules);
        self.write(|tx| {
            let (old, old_dis): (Vec<u8>, String) = tx
                .query_row(
                    "SELECT content, disabled_rules FROM accounts WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::NotFound("That account no longer exists.".into()))?;
            let kept = |tx: &Connection| -> Result<i64> {
                Ok(tx.query_row(
                    "SELECT count(*) FROM history WHERE account_id = ?1",
                    [id],
                    |r| r.get(0),
                )?)
            };
            if old == content && old_dis == dis {
                return Ok(UpdateOutcome {
                    changed: false,
                    versions_kept: kept(tx)?,
                });
            }
            Self::push_history(tx, id, history_keep)?;
            tx.execute(
                "UPDATE accounts SET content = ?1, disabled_rules = ?2, updated = ?3 WHERE id = ?4",
                params![content, dis, now(), id],
            )?;
            Ok(UpdateOutcome {
                changed: true,
                versions_kept: kept(tx)?,
            })
        })
    }

    pub fn history(&self, id: i64) -> Result<Vec<VersionInfo>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, saved, length(content) FROM history WHERE account_id = ?1 ORDER BY id DESC",
            )?;
            let v = st
                .query_map([id], |r| {
                    Ok(VersionInfo { id: r.get(0)?, saved: r.get(1)?, size: r.get(2)? })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(v)
        })
    }

    /// The file of an earlier version.
    pub fn version_content(&self, id: i64, version: i64) -> Result<Vec<u8>> {
        self.with(|c| {
            c.query_row(
                "SELECT content FROM history WHERE id = ?1 AND account_id = ?2",
                params![version, id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound("That version no longer exists.".into()))
        })
    }

    /// Makes an earlier version the current one. The state being replaced goes into the history first, so a restore can itself be undone.
    pub fn restore_version(&self, id: i64, version: i64, history_keep: u32) -> Result<()> {
        self.write(|tx| {
            let (content, dis): (Vec<u8>, String) = tx
                .query_row(
                    "SELECT content, disabled_rules FROM history WHERE id = ?1 AND account_id = ?2",
                    params![version, id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::NotFound("That version no longer exists.".into()))?;
            Self::push_history(tx, id, history_keep.max(1))?;
            tx.execute(
                "UPDATE accounts SET content = ?1, disabled_rules = ?2, updated = ?3 WHERE id = ?4",
                params![content, dis, now(), id],
            )?;
            Ok(())
        })
    }

    // ---- backup -------------------------------------------------------------------------------------------------------------

    /// Writes `library-backup-<time>.zip` into `dest_dir` (created when missing): a consistent copy of the database (`VACUUM INTO`) and the
    /// settings file. The zip is written under a temporary name and renamed, so a half-written backup never has the final name.
    pub fn backup(&self, dest_dir: &Path) -> Result<PathBuf> {
        use std::io::Write;
        std::fs::create_dir_all(dest_dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let snap = dest_dir.join(format!(".snapshot-{}-{stamp}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&snap);
        self.with(|c| {
            c.execute("VACUUM INTO ?1", [snap.to_string_lossy().as_ref()])?;
            Ok(())
        })?;
        let result = (|| -> Result<PathBuf> {
            let mut final_path = dest_dir.join(format!("library-backup-{stamp}.zip"));
            let mut n = 2;
            while final_path.exists() {
                final_path = dest_dir.join(format!("library-backup-{stamp}-{n}.zip"));
                n += 1;
            }
            let tmp = dest_dir.join(format!(
                ".{}.tmp",
                final_path.file_name().unwrap().to_string_lossy()
            ));
            {
                let f = std::fs::File::create(&tmp)?;
                let mut z = zip::ZipWriter::new(f);
                let o = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                z.start_file(DB_FILE, o)?;
                z.write_all(&std::fs::read(&snap)?)?;
                if let Ok(s) = std::fs::read(self.dir.join(settings::SETTINGS_FILE)) {
                    z.start_file(settings::SETTINGS_FILE, o)?;
                    z.write_all(&s)?;
                }
                z.start_file("README.txt", o)?;
                z.write_all(
                    b"Backup of the Elk account editor library. To restore: close the editor, replace library.sqlite in the data folder with the one in this zip, start the editor.\r\n",
                )?;
                z.finish()?;
            }
            std::fs::rename(&tmp, &final_path).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;
            Ok(final_path)
        })();
        let _ = std::fs::remove_file(&snap);
        result
    }
}
