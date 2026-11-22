//! Port of PanelProgress.cs: progress of a long panel operation.

use std::sync::Mutex;
use std::time::Instant;

/// A point-in-time copy of the progress state.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressSnapshot {
    /// An operation is running.
    pub active: bool,
    /// Current phase.
    pub phase: String,
    /// Free-text note.
    pub note: String,
    /// Transfers done in this phase.
    pub done: i32,
    /// Transfers expected (0 = not known).
    pub total: i32,
    /// Seconds since the operation began (0 when idle).
    pub seconds: f64,
}

struct Inner {
    phase: String,
    note: String,
    done: i32,
    total: i32,
    active: bool,
    started: Instant,
}

/// Progress of a long panel operation (read, compare, send, backup, restore). The session calls [`PanelProgress::tick`] after every record or
/// block it reads or writes (`ElkSession::on_transfer`); the operation sets the phase and the number of transfers it expects with
/// [`PanelProgress::next`]. A total of 0 means "not known". If more transfers happen than expected the total grows with them.
pub struct PanelProgress {
    l: Mutex<Inner>,
}

impl Default for PanelProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl PanelProgress {
    /// An idle progress object.
    pub fn new() -> Self {
        PanelProgress { l: Mutex::new(Inner { phase: String::new(), note: String::new(), done: 0, total: 0, active: false, started: Instant::now() }) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.l.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts an operation (the clock for "seconds so far" starts here).
    pub fn begin(&self, phase: &str, total: i32) {
        let mut g = self.lock();
        g.active = true;
        g.started = Instant::now();
        g.phase = phase.into();
        g.note.clear();
        g.done = 0;
        g.total = total;
    }

    /// Moves to the next phase of the same operation.
    pub fn next(&self, phase: &str, total: i32) {
        let mut g = self.lock();
        if !g.active {
            g.active = true;
            g.started = Instant::now();
        }
        g.phase = phase.into();
        g.note.clear();
        g.done = 0;
        g.total = total;
    }

    /// Adds to the expected total.
    pub fn add_total(&self, n: i32) {
        self.lock().total += n;
    }

    /// Sets the note.
    pub fn note(&self, text: &str) {
        self.lock().note = text.into();
    }

    /// Counts one transfer (ignored while idle).
    pub fn tick(&self) {
        let mut g = self.lock();
        if !g.active {
            return;
        }
        g.done += 1;
        if g.total > 0 && g.done > g.total {
            g.total = g.done;
        }
    }

    /// Ends the operation.
    pub fn end(&self) {
        self.lock().active = false;
    }

    /// The current state.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let g = self.lock();
        ProgressSnapshot {
            active: g.active,
            phase: g.phase.clone(),
            note: g.note.clone(),
            done: g.done,
            total: g.total,
            seconds: if g.active { g.started.elapsed().as_secs_f64() } else { 0.0 },
        }
    }
}
