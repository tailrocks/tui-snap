use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{ClipboardType, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as VteColor, CursorShape, NamedColor, Processor, Rgb as VteRgb,
};

use super::*;
use crate::tui::{CancelToken, ExitWait, Session, Tui, TuiError, WaitError};
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{Maybe, Observation, Screen};

// ---------------------------------------------------------------------------
// R09: scoped guardian (process-group containment with PID-reuse guards)
// ---------------------------------------------------------------------------

/// Cap on pids signalled during one sweep (bounded containment).
pub(crate) const MAX_SWEEP_TARGETS: usize = 256;

/// Cap on survivors listed in a report.
pub(crate) const MAX_SURVIVORS: usize = 64;

/// Cap on `ps` snapshot lines parsed.
pub(crate) const MAX_PS_LINES: usize = 131_072;

/// Post-kill settle polling budget per sweep.
pub(crate) const SWEEP_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// How completely the guardian contained the child's process group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Containment {
    /// Group verified empty after the sweep (or was already empty).
    Full,
    /// Some group members survived (listed in
    /// [`GuardianReport::survivors`]).
    Partial,
    /// The sweep refused to signal: killing would have risked unrelated
    /// pids (foreign/reused group id, unresolvable identity, ...).
    Refused { reason: String },
    /// Identity or enumeration failed; nothing was signalled.
    Unknown { reason: String },
    /// Platform cannot enumerate process groups.
    Unsupported,
}

/// What a guardian teardown did, pid by pid. Escaping descendants (setsid/
/// setpgid) are invisible to the group scan by nature; see
/// [`GuardianReport::escape_boundary_note`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianReport {
    pub child_pid: Option<u32>,
    pub pgid: Option<i32>,
    /// Every signalled pid was re-verified in the expected session.
    pub sid_verified: bool,
    /// The direct child's start time still matched at sweep time
    /// (`None` = start time unavailable on this platform/run).
    pub start_verified: Option<bool>,
    /// Pids sent SIGKILL (bounded to [`MAX_SWEEP_TARGETS`]).
    pub signalled: Vec<u32>,
    /// Group members still alive after the sweep (bounded).
    pub survivors: Vec<u32>,
    pub containment: Containment,
    pub teardown_error: Option<String>,
}

impl GuardianReport {
    /// The documented escape boundary: descendants that called
    /// `setsid(2)`/`setpgid(2)` leave the child's process group (and
    /// possibly its session), so the group sweep cannot see or contain
    /// them. Containment is process-group-scoped, not a sandbox.
    #[must_use]
    pub fn escape_boundary_note() -> &'static str {
        "escape boundary: descendants that called setsid(2)/setpgid(2) leave the \
         child's process group and are NOT contained by the group sweep; use an \
         OS sandbox for untrusted code"
    }
}

/// Owns a [`Session`] and contains its whole process group on teardown.
///
/// `finish`/`drop` close the session (reaping the direct child) and then
/// sweep the child's process group with PID-reuse guards. Nothing is ever
/// signalled blindly: see the guards in the sweep implementation.
pub struct Guardian {
    session: Option<Session>,
    child: Option<ChildIds>,
    swept: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildIds {
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) sid: i32,
    pub(crate) start: Option<String>,
}

impl std::fmt::Debug for Guardian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guardian")
            .field("child", &self.child)
            .field("swept", &self.swept)
            .finish()
    }
}

impl Guardian {
    /// Adopt a session, recording the child's pid/group/session/start-time
    /// for the teardown guards. Never fails; unresolvable identity degrades
    /// to a sweep that refuses to signal (reported, never blind).
    #[must_use]
    pub fn wrap(session: Session) -> Self {
        let child = session.pid().and_then(ChildIds::capture);
        Self {
            session: Some(session),
            child,
            swept: false,
        }
    }

    #[must_use]
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Close the session and sweep the child's process group (bounded by
    /// `deadline`), returning the per-pid report.
    pub fn finish(mut self, deadline: Instant) -> Result<GuardianReport, TuiError> {
        let mut teardown_error = None;
        if let Some(mut s) = self.session.take() {
            if let Err(e) = s.close() {
                teardown_error = Some(e.to_string());
            }
        }
        self.swept = true;
        Ok(sweep_group(&self.child, Some(deadline), teardown_error))
    }
}

impl Drop for Guardian {
    fn drop(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.close();
        }
        if !self.swept {
            self.swept = true;
            let _ = sweep_group(&self.child, None, None);
        }
    }
}

#[cfg(unix)]
impl ChildIds {
    fn capture(pid: u32) -> Option<Self> {
        guardian_unix::capture_ids(pid)
    }
}

#[cfg(unix)]
fn sweep_group(
    child: &Option<ChildIds>,
    deadline: Option<Instant>,
    teardown_error: Option<String>,
) -> GuardianReport {
    guardian_unix::sweep(child, deadline, teardown_error)
}

#[cfg(not(unix))]
impl ChildIds {
    fn capture(_pid: u32) -> Option<Self> {
        None
    }
}

#[cfg(not(unix))]
fn sweep_group(
    child: &Option<ChildIds>,
    _deadline: Option<Instant>,
    teardown_error: Option<String>,
) -> GuardianReport {
    GuardianReport {
        child_pid: child.as_ref().map(|c| c.pid),
        pgid: None,
        sid_verified: false,
        start_verified: None,
        signalled: Vec::new(),
        survivors: Vec::new(),
        containment: Containment::Unsupported,
        teardown_error,
    }
}

// The new handles stay shareable without any unsafe impl.
const _: fn() = || {
    fn share<T: Send + Sync>() {}
    share::<Shell>();
    share::<Guardian>();
};
