//! Published [`Observation`](tuiscotti_core::screen::Observation) state (R06, R07).

use std::sync::{Condvar, Mutex};
use std::time::Instant;

use tuiscotti_core::screen::{CaptureReason, Observation};

use super::error::CancelToken;
use super::exit::ExitStatus;
use super::limits::WAIT_SLICE;

/// Cheap metadata of the latest published observation (F12): revision plus
/// grid geometry, readable under one short lock without cloning the screen.
/// [`Session::meta`](super::session::Session::meta) serves this; anything
/// needing cells still takes an [`Observation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionMeta {
    /// Latest published revision.
    pub revision: u64,
    /// Latest grid width in columns.
    pub cols: u16,
    /// Latest grid height in rows.
    pub rows: u16,
    /// Why the latest capture was taken.
    pub reason: CaptureReason,
    /// How the final drain completed, once the exit revision is published
    /// (LIFE-8). `None` until then; later manual observations preserve it.
    pub drain: Option<DrainCause>,
}

/// How the worker's final output drain completed (LIFE-3/LIFE-8):
/// reader EOF, drain-grace expiry, or forced shutdown. Never conflated:
/// a PTY read error is reported separately (see
/// [`Session::read_error`](super::session::Session::read_error)) and the
/// reaped child status stays authoritative for the exit cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainCause {
    /// The reader observed clean EOF before the drain completed.
    Eof,
    /// The drain grace expired without reader EOF (a grandchild may still
    /// hold the PTY open, or the reader died with an error).
    GraceExpiry,
    /// Forced teardown published the final revision.
    Shutdown,
}

struct SharedState {
    latest: Option<Observation>,
    meta: Option<SessionMeta>,
    exit: Option<ExitStatus>,
    exit_observation: Option<Observation>,
    drain: Option<DrainCause>,
    read_error: Option<String>,
    closed: bool,
    teardown_error: Option<String>,
}

pub(crate) struct Shared {
    state: Mutex<SharedState>,
    changed: Condvar,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(SharedState {
                latest: None,
                meta: None,
                exit: None,
                exit_observation: None,
                drain: None,
                read_error: None,
                closed: false,
                teardown_error: None,
            }),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn publish(&self, obs: Observation, exit: Option<ExitStatus>) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if exit.is_some() {
            s.exit = exit;
        }
        // A manual observation after the exit revision must not clobber
        // the recorded drain cause (LIFE-8).
        let drain = s.drain;
        s.meta = Some(SessionMeta {
            revision: obs.revision,
            cols: obs.screen.cols(),
            rows: obs.screen.rows(),
            reason: obs.reason,
            drain,
        });
        s.latest = Some(obs);
        drop(s);
        self.changed.notify_all();
    }

    pub(crate) fn publish_exit(&self, status: ExitStatus, obs: Observation, drain: DrainCause) {
        {
            let mut s = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            s.drain = Some(drain);
            s.exit_observation = Some(obs.clone());
        }
        self.publish(obs, Some(status));
    }

    /// The exit-revision observation, if published. [`Session::wait_exit`]
    /// returns this — never a later manual observation (LIFE-8).
    pub(crate) fn exit_observation(&self) -> Option<Observation> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exit_observation
            .clone()
    }

    /// Record a PTY read error distinctly from EOF (LIFE-3). First error
    /// wins; routine post-exit EIO is evidence, never a teardown failure.
    pub(crate) fn record_read_error(&self, msg: &str) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.read_error.is_none() {
            s.read_error = Some(msg.to_string());
        }
    }

    pub(crate) fn read_error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read_error
            .clone()
    }

    pub(crate) fn latest(&self) -> Option<Observation> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .latest
            .clone()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .meta
            .map_or(0, |m| m.revision)
    }

    /// Latest metadata without cloning the screen. `None` only before the
    /// worker publishes revision 0 (unreachable on a spawned session: spawn
    /// blocks for it and fails otherwise).
    pub(crate) fn meta(&self) -> Option<SessionMeta> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .meta
    }

    pub(crate) fn exit(&self) -> Option<ExitStatus> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exit
            .clone()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed
    }

    pub(crate) fn mark_closed(&self) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.closed = true;
        drop(s);
        self.changed.notify_all();
    }

    pub(crate) fn record_teardown(&self, msg: &str) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.teardown_error.is_none() {
            s.teardown_error = Some(msg.to_string());
        }
    }

    pub(crate) fn teardown_error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .teardown_error
            .clone()
    }

    /// Wait (bounded by `deadline`, `cancel`, and [`WAIT_SLICE`]) for any
    /// publication. Returns immediately on cancel/deadline.
    pub(crate) fn wait_changed(&self, deadline: Instant, cancel: &CancelToken) {
        let s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if cancel.is_cancelled() || now >= deadline {
            return;
        }
        let slice = (deadline - now).min(WAIT_SLICE);
        let (guard, _) = self
            .changed
            .wait_timeout(s, slice)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(guard);
    }

    /// Wait for a revision newer than `seen`; `None` on slice expiry (the
    /// caller re-checks cancel/deadline/quiet).
    pub(crate) fn wait_for_newer_than(
        &self,
        seen: u64,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Option<Observation> {
        // One slice per call: the caller owns quiet/deadline accounting.
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(o) = s.latest.clone()
            && o.revision > seen
        {
            return Some(o);
        }
        if s.closed || cancel.is_cancelled() {
            return None;
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let slice = (deadline - now).min(WAIT_SLICE);
        s = match self.changed.wait_timeout(s, slice) {
            Ok((guard, _)) => guard,
            Err(e) => e.into_inner().0,
        };
        if let Some(o) = s.latest.clone()
            && o.revision > seen
        {
            return Some(o);
        }
        None
    }
}
