//! Port of LivePanel.cs: one panel session that stays open between requests.
//!
//! The M1XEP accepts a single programming connection at a time, so while this is open every other panel action (compare, send, read, backup, restore)
//! borrows it instead of logging in again, and the Status tab polls it. Requests are serialised with a lock; when the connection breaks it is closed and
//! [`LivePanel::error`] says why.
//!
//! Decision: `SemaphoreSlim` becomes a `std::sync::Mutex` around the session; the public status fields (`connected`, `host`, `error`, `firmware`, `serial`)
//! live behind a second small mutex so a status request can read them while a long operation holds the session (the C# reads them without the gate too).

use crate::error::{PanelError, Result};
use crate::sync::{PanelConnection, Settings, DEFAULT_ATTEMPTS, DEFAULT_RETRY_DELAY};
use elk_core::{BusModule, ElkError, ElkSession, LiveSnapshot, PanelAccess, PanelLive, PanelModules};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[derive(Default)]
struct Meta {
    connected: bool,
    host: Option<String>,
    error: Option<String>,
    firmware: Option<String>,
    serial: Option<String>,
}

struct Inner {
    s: Option<ElkSession>,
    settings: Option<Settings>,
}

/// The shared live panel session.
pub struct LivePanel {
    gate: Mutex<Inner>,
    meta: Mutex<Meta>,
    /// Connection attempts per login (C#: 6).
    pub attempts: u32,
    /// Pause between attempts (C#: 5 s). Tests lower it.
    pub retry_delay: Duration,
    /// How long `enroll` waits for the panel to finish scanning (C#: 12 s). Tests lower it.
    pub enroll_wait: Duration,
}

impl Default for LivePanel {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn same(a: &Settings, b: &Settings) -> bool {
    a.host == b.host && a.port == b.port
}

impl LivePanel {
    /// A panel with no session.
    pub fn new() -> Self {
        LivePanel {
            gate: Mutex::new(Inner { s: None, settings: None }),
            meta: Mutex::new(Meta::default()),
            attempts: DEFAULT_ATTEMPTS,
            retry_delay: DEFAULT_RETRY_DELAY,
            enroll_wait: Duration::from_secs(12),
        }
    }

    /// A session is open.
    pub fn connected(&self) -> bool {
        lock(&self.meta).connected
    }
    /// Host of the last successful connect (stays after a disconnect, like the C# `_settings`).
    pub fn host(&self) -> Option<String> {
        lock(&self.meta).host.clone()
    }
    /// Why the last session broke.
    pub fn error(&self) -> Option<String> {
        lock(&self.meta).error.clone()
    }
    /// Firmware of the connected panel (None when not connected).
    pub fn firmware(&self) -> Option<String> {
        lock(&self.meta).firmware.clone()
    }
    /// Serial of the connected panel (None when not connected).
    pub fn serial(&self) -> Option<String> {
        lock(&self.meta).serial.clone()
    }

    fn set_error(&self, e: Option<String>) {
        lock(&self.meta).error = e;
    }

    fn sync_meta(&self, g: &Inner) {
        let mut m = lock(&self.meta);
        m.connected = g.s.is_some();
        m.firmware = g.s.as_ref().and_then(|s| s.panel.as_ref()).map(|p| p.firmware.clone());
        m.serial = g.s.as_ref().and_then(|s| s.panel.as_ref()).map(|p| p.serial_hex.clone());
        m.host = g.settings.as_ref().map(|s| s.host.clone());
    }

    fn close_locked(&self, g: &mut Inner) {
        if let Some(mut s) = g.s.take() {
            let _ = s.disconnect();
        }
        self.sync_meta(g);
    }

    fn link_failed(&self, g: &mut Inner, e: &PanelError) {
        self.set_error(Some(e.message()));
        self.close_locked(g);
    }

    fn open(&self, st: &Settings, log: &mut dyn FnMut(&str)) -> std::result::Result<ElkSession, ElkError> {
        PanelConnection::open(st, log, self.attempts, self.retry_delay)
    }

    /// `ConnectAsync`: closes any session, clears the error and opens a new one.
    pub fn connect(&self, st: &Settings, log: &mut dyn FnMut(&str)) -> std::result::Result<(), ElkError> {
        let mut g = lock(&self.gate);
        self.close_locked(&mut g);
        self.set_error(None);
        g.s = Some(self.open(st, log)?);
        g.settings = Some(st.clone());
        self.sync_meta(&g);
        Ok(())
    }

    /// `DisconnectAsync`.
    pub fn disconnect(&self) {
        let mut g = lock(&self.gate);
        self.close_locked(&mut g);
        self.set_error(None);
    }

    /// `UseAsync`: runs `work` on the open session when it is for the same panel (host and port), otherwise on a session opened (and closed) just for this call.
    pub fn use_session<T>(&self, st: &Settings, log: &mut dyn FnMut(&str), work: impl FnOnce(&mut ElkSession) -> Result<T>) -> Result<T> {
        let mut g = lock(&self.gate);
        if g.s.is_some() && g.settings.as_ref().is_some_and(|c| same(c, st)) {
            let r = work(g.s.as_mut().expect("checked"));
            if let Err(e) = &r {
                if e.is_link_error() {
                    self.link_failed(&mut g, e);
                }
            }
            return r;
        }
        let mut s = self.open(st, log)?;
        let r = work(&mut s);
        let _ = s.disconnect();
        r
    }

    /// `UseOpenAsync`: runs `work` on the open session; `InvalidOperation` when there is none.
    pub fn use_open<T>(&self, work: impl FnOnce(&mut ElkSession) -> Result<T>) -> Result<T> {
        let mut g = lock(&self.gate);
        if g.s.is_none() {
            return Err(PanelError::Elk(ElkError::InvalidOperation("Not connected. Connect on the Status tab first.".into())));
        }
        let r = work(g.s.as_mut().expect("checked"));
        if let Err(e) = &r {
            if e.is_link_error() {
                self.link_failed(&mut g, e);
            }
        }
        r
    }

    /// `ChangeAccessCodeAsync`: changes the RP access code on the panel and, once the panel acknowledges, remembers the new code so a reconnect after a
    /// dropped link uses it.
    pub fn change_access_code(&self, new_code: &str) -> Result<bool> {
        let mut g = lock(&self.gate);
        if g.s.is_none() || g.settings.is_none() {
            return Err(PanelError::Elk(ElkError::InvalidOperation("Not connected. Connect on the Live tab first.".into())));
        }
        let ok = match PanelAccess::change(g.s.as_mut().expect("checked"), new_code) {
            Ok(ok) => ok,
            Err(e) => {
                let e = PanelError::from(e);
                if e.is_link_error() {
                    self.link_failed(&mut g, &e);
                }
                return Err(e);
            }
        };
        if ok {
            if let Some(st) = g.settings.as_mut() {
                st.code = new_code.to_string();
            }
        }
        Ok(ok)
    }

    /// `EnrollAsync`: starts bus module enrollment on the open session, waits for the panel to finish scanning, makes sure the session still works (logs in
    /// again if the panel dropped it) and lists the devices.
    pub fn enroll(&self, log: &mut dyn FnMut(&str)) -> Result<(String, Vec<BusModule>)> {
        let mut g = lock(&self.gate);
        if g.s.is_none() || g.settings.is_none() {
            return Err(PanelError::Elk(ElkError::InvalidOperation("Not connected. Connect on the Status tab first.".into())));
        }
        let note = PanelModules::enroll(g.s.as_mut().expect("checked"))?;
        std::thread::sleep(self.enroll_wait);
        if let Err(e) = g.s.as_mut().expect("checked").read_block(7, 1, 0) {
            let e = PanelError::from(e);
            if !e.is_link_error() {
                return Err(e);
            }
            self.close_locked(&mut g);
            let st = g.settings.clone().expect("checked");
            g.s = Some(self.open(&st, log)?);
            self.sync_meta(&g);
        }
        let list = PanelModules::scan(g.s.as_mut().expect("session"))?;
        Ok((note, list))
    }

    /// `PollAsync`: reads the live status; `None` when not connected (or the connection just broke). Errors that are not link errors are returned.
    pub fn poll(&self) -> Result<Option<LiveSnapshot>> {
        let mut g = lock(&self.gate);
        let Some(s) = g.s.as_mut() else {
            return Ok(None);
        };
        match PanelLive::read(s) {
            Ok(snap) => Ok(Some(snap)),
            Err(e) => {
                let e = PanelError::from(e);
                if e.is_link_error() {
                    self.link_failed(&mut g, &e);
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }
}
