//! `settings.json` in the data folder: a small file a person can edit by hand. Written atomically (temp file, then rename).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const SETTINGS_FILE: &str = "settings.json";
pub const DEFAULT_HISTORY_KEEP: u32 = 20;
pub const MAX_HISTORY_KEEP: u32 = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// How many earlier saves of each account are kept (0 keeps none).
    pub history_keep: u32,
    /// Releases API URL for "Check for updates" (the `ELKEDITOR_UPDATE_URL` variable wins over this).
    pub update_url: Option<String>,
    /// After a panel connect / read / compare / send the open account is saved into the library (ADR-018). On by default.
    pub auto_save_panel: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            history_keep: DEFAULT_HISTORY_KEEP,
            update_url: None,
            auto_save_panel: true,
        }
    }
}

impl Settings {
    /// The settings of `dir` and a warning when the file exists but cannot be read (the defaults are used then; the file is not touched).
    pub fn load(dir: &Path) -> (Settings, Option<String>) {
        let p = dir.join(SETTINGS_FILE);
        match std::fs::read(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(format!("{SETTINGS_FILE}: {e}"))),
            Ok(b) => match serde_json::from_slice::<Settings>(&b) {
                Ok(mut s) => {
                    s.history_keep = s.history_keep.min(MAX_HISTORY_KEEP);
                    (s, None)
                }
                Err(e) => (
                    Settings::default(),
                    Some(format!(
                        "{SETTINGS_FILE} is not valid ({e}); the defaults are used"
                    )),
                ),
            },
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut s = self.clone();
        s.history_keep = s.history_keep.min(MAX_HISTORY_KEEP);
        let text = serde_json::to_string_pretty(&s).map_err(|e| Error::Other(e.to_string()))?;
        let tmp = dir.join(format!("{SETTINGS_FILE}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text.as_bytes())?;
        if let Err(e) = std::fs::rename(&tmp, dir.join(SETTINGS_FILE)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}
