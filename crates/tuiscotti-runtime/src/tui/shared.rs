//! Published [`Observation`](tuiscotti_core::screen::Observation) state (R06, R07).

use std::sync::{Condvar, Mutex};
use std::time::Instant;

use tuiscotti_core::screen::Observation;

use super::error::CancelToken;
use super::exit::ExitStatus;
use super::limits::WAIT_SLICE;

struct SharedState {
    latest: Option<Observation>,
    exit: Option<ExitStatus>,
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
                exit: None,
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
        s.latest = Some(obs);
        drop(s);
        self.changed.notify_all();
    }

    pub(crate) fn publish_exit(&self, status: ExitStatus, obs: Observation) {
        self.publish(obs, Some(status));
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
            .latest
            .as_ref()
            .map_or(0, |o| o.revision)
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
