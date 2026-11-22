//! What the server holds beyond the panel state: the accounts library (ADR-015) and the data folder it lives in.

use std::path::{Path, PathBuf};

use elk_library::{DataLocation, Library, Settings};
use elk_panel::Shared;

/// The accounts library, or the reason it is not available (the editor itself keeps working without it).
pub struct LibState {
    pub lib: Result<Library, String>,
    pub location: Option<DataLocation>,
}

impl LibState {
    pub fn open_at(location: DataLocation) -> LibState {
        let lib = Library::open(&location.dir).map_err(|e| e.to_string());
        LibState {
            lib,
            location: Some(location),
        }
    }

    pub fn unavailable(why: String) -> LibState {
        LibState {
            lib: Err(why),
            location: None,
        }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.location.as_ref().map(|l| l.dir.as_path())
    }

    /// The settings file of the data folder (read each time, so a hand edit applies at once) and a warning when it is unreadable.
    pub fn settings(&self) -> (Settings, Option<String>) {
        match self.dir() {
            Some(d) => Settings::load(d),
            None => (Settings::default(), None),
        }
    }
}

/// Everything a request handler needs.
pub struct App {
    pub shared: Shared,
    pub library: LibState,
    /// The running executable (what an update replaces).
    pub exe: PathBuf,
    /// False in tests: an update then does not start the new program and end this one.
    pub restart: bool,
}

impl App {
    /// Resolves the data folder (`--data-dir`, `ELKEDITOR_DATA`, next to the program, per-user fallback) and opens the library in it.
    pub fn new(shared: Shared, data_dir_arg: Option<&str>) -> App {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        let env = std::env::var("ELKEDITOR_DATA").ok();
        let library = match elk_library::location::resolve(data_dir_arg, env.as_deref(), &exe) {
            Ok(loc) => LibState::open_at(loc),
            Err(e) => LibState::unavailable(e.to_string()),
        };
        App {
            shared,
            library,
            exe,
            restart: true,
        }
    }

    /// For tests: a library in `dir`.
    #[cfg(test)]
    pub fn with_dir(shared: Shared, dir: &Path) -> App {
        let location = DataLocation {
            dir: dir.to_path_buf(),
            fell_back: false,
            note: format!("Data folder: {}", dir.display()),
        };
        App {
            shared,
            library: LibState::open_at(location),
            exe: PathBuf::from("elkaccounteditor"),
            restart: false,
        }
    }
}
