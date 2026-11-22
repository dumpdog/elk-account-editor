//! The real file dialogs, through the `rfd` crate.

use crate::bridge::Dialogs;
use std::path::{Path, PathBuf};

/// Native open and save dialogs (Win32 on Windows, AppKit on macOS).
pub struct RfdDialogs;

impl Dialogs for RfdDialogs {
    fn pick_open(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_file()
    }

    fn pick_save(&self, title: &str, dir: Option<&Path>, suggested: &str) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(suggested);
        if let Some(dir) = dir {
            d = d.set_directory(dir);
        }
        d.save_file()
    }
}
