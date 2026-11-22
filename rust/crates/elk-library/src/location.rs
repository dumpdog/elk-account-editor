//! Where the data folder lives. Next to the program by default (Windows: beside the exe; macOS: beside the `.app` bundle), with a per-user
//! fallback when that place is not writable (a read-only disk image, macOS app translocation, `Program Files`). `--data-dir` and the
//! `ELKEDITOR_DATA` environment variable override everything and are never replaced by a fallback.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Name of the folder next to the program.
pub const FOLDER_NAME: &str = "ElkEditor data";
/// File name of the database inside the data folder.
pub const DB_FILE: &str = "library.sqlite";

/// The chosen data folder and what to tell the user about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLocation {
    pub dir: PathBuf,
    /// True when the preferred place could not be used and the per-user folder was taken.
    pub fell_back: bool,
    /// One sentence for the page ("Data folder: ..." plus the reason for a fallback).
    pub note: String,
}

/// The folder that contains the `.app` bundle when `exe` is inside one (`.../Elk Account Editor.app/Contents/MacOS/elkaccounteditor`).
pub fn bundle_parent(exe: &Path) -> Option<PathBuf> {
    for a in exe.ancestors() {
        let is_app = a
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".app"))
            .unwrap_or(false);
        if is_app {
            return a.parent().map(Path::to_path_buf);
        }
    }
    None
}

/// `<folder of the exe, or of the .app bundle>/ElkEditor data`.
pub fn primary_dir(exe: &Path) -> PathBuf {
    let base = bundle_parent(exe)
        .or_else(|| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(FOLDER_NAME)
}

/// The per-user folder: `~/Library/Application Support/ElkEditor` on macOS, `%APPDATA%\ElkEditor` on Windows, `~/.local/share/ElkEditor` elsewhere.
pub fn fallback_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("ElkEditor"))
}

/// True when `dir` exists (or can be created) and a file can be written into it.
pub fn is_usable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn has_library(dir: &Path) -> bool {
    dir.join(DB_FILE).is_file()
}

/// Picks the first usable candidate; when an earlier one is usable but empty and a later usable one already holds a library, the library wins
/// (an app moved to a writable place keeps finding its old data, and nothing is ever silently started empty beside existing data).
pub fn choose(candidates: &[PathBuf]) -> Result<DataLocation> {
    let usable: Vec<&PathBuf> = candidates.iter().filter(|c| is_usable(c)).collect();
    let pick = usable
        .iter()
        .find(|c| has_library(c))
        .or_else(|| usable.first())
        .copied();
    let Some(dir) = pick else {
        return Err(Error::Other(format!(
            "No writable place for the library data folder was found (tried: {}). Use --data-dir or the ELKEDITOR_DATA variable.",
            candidates
                .iter()
                .map(|c| c.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    let fell_back = candidates.first().is_some_and(|f| f != dir);
    let note = if fell_back {
        format!(
            "Data folder: {} (the folder next to the program, {}, is not writable).",
            dir.display(),
            candidates[0].display()
        )
    } else {
        format!("Data folder: {}", dir.display())
    };
    Ok(DataLocation {
        dir: dir.clone(),
        fell_back,
        note,
    })
}

/// The folder to use: `--data-dir`, else `ELKEDITOR_DATA`, else beside the program with the per-user fallback. An explicit choice must be usable.
pub fn resolve(arg: Option<&str>, env: Option<&str>, exe: &Path) -> Result<DataLocation> {
    let explicit = arg
        .filter(|s| !s.trim().is_empty())
        .map(|s| (s, "--data-dir"))
        .or_else(|| {
            env.filter(|s| !s.trim().is_empty())
                .map(|s| (s, "ELKEDITOR_DATA"))
        });
    if let Some((p, from)) = explicit {
        let dir = PathBuf::from(p.trim());
        if !is_usable(&dir) {
            return Err(Error::Other(format!(
                "The data folder given by {from} ({}) cannot be created or written.",
                dir.display()
            )));
        }
        return Ok(DataLocation {
            note: format!("Data folder: {} (set by {from})", dir.display()),
            dir,
            fell_back: false,
        });
    }
    let mut c = vec![primary_dir(exe)];
    c.extend(fallback_dir());
    choose(&c)
}
