//! Rust port of `Elk.Protocol/PanelSync.cs`, `Elk.Editor/LivePanel.cs` and the panel / live routes of `Elk.Editor/Program.cs`.
//! See PORTING-NOTES.md.
//!
//! - [`sync`]: `PanelSync` (diff, pull, apply, estimates), `PanelConnection` and the change / result types.
//! - [`live`]: `LivePanel`, the shared panel session.
//! - [`routes`]: `fn handle_xxx(&Shared, body, &Query) -> HttpResult` for each route, plus [`routes::handle`] to dispatch by method and path.
//! - [`testkit`]: helpers that fill a `SimulatedPanel` (used by the tests and the `sim_panel` example).

pub mod auto_sync;
pub mod error;
pub(crate) mod jsonerr;
pub(crate) mod jsonw;
pub mod live;
pub mod m1xep;
pub mod routes;
pub mod sync;
pub mod testkit;

pub use error::{Kind, PanelError};
pub use live::LivePanel;
pub use routes::{
    default_backup_dir, handle, handle_live_accesscode, handle_live_connect, handle_live_control, handle_live_disconnect, handle_live_enroll, handle_live_log, handle_live_modules,
    handle_live_settime, handle_live_status, handle_live_time, handle_panel_backup, handle_panel_backups, handle_panel_compare, handle_panel_progress, handle_panel_pull,
    handle_panel_restore, handle_panel_send, json_syntax_check, parse_model_body, template_text, HttpResult, Query, Session, Shared,
};
pub use sync::{PanelChange, PanelConnection, PanelSync, Settings, SyncResult};
