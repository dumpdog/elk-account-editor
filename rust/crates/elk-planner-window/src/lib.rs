//! Two ports for the Elk account editor: the data bus planner (`DataBusPlan.cs`) and the native window (`NativeWindow.cs`).
//! See `PORTING-NOTES.md`.

pub mod bridge;
pub mod data_bus_plan;
#[cfg(any(windows, target_os = "macos"))]
pub mod dialogs;
mod jsonw;
pub mod native_window;

pub use bridge::{handle_message, Dialogs, Reply};
pub use data_bus_plan::{check, check_json, BusPlan, PlanResult};
#[cfg(any(windows, target_os = "macos"))]
pub use dialogs::RfdDialogs;
pub use native_window::run;
#[cfg(any(windows, target_os = "macos"))]
pub use native_window::run_with_dialogs;
