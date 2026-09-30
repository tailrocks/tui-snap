//! Process-local PTY session map (feature `pty`).
//!
//! `crate::tui::Session` has no cross-process reattach, and termpane
//! (unreleased) offers no retained-session API either — its `PtySession` is
//! spawn/write/snapshot/signal/close within one process. Named PTY sessions
//! surviving across CLI invocations (F08-F2) live in this map inside the
//! long-lived daemon (`super::daemon`), which is a thin IPC wrapper around
//! these functions; no path/git override stands in for termpane here.
//!
//! Concurrency (F12): the map holds `Arc<Session>` and every critical
//! section below is short (lookup/insert/remove only) — spawns, waits,
//! and observations all run OUTSIDE the lock, so independent sessions
//! operate concurrently and one blocked wait never stalls another
//! session. An `exit` removes the entry but in-flight holders keep a
//! working `Arc` until close completes.
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::proto::OpError;

mod io;
mod lifecycle;
mod spawn;
mod wait;

pub(crate) use io::{observe, screenshot, snapshot, stdin};
pub(crate) use lifecycle::{
    close_all, drop_exited, drop_session, is_empty, resize, sessions_status, signal, stop,
};
pub(crate) use spawn::{spawn, spawn_os};
pub(crate) use wait::{exit, wait};

static REGISTRY: Mutex<Option<HashMap<String, Arc<crate::tui::Session>>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Liveness of one registry entry for the retained-session daemon's
/// `list` merge: the daemon is authoritative for PTY sessions (owned
/// handle, no pid-reuse window while it lives).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SessionLiveness {
    /// Registry key (= validated session name on the daemon path).
    pub(crate) name: String,
    /// Direct-child pid, when the platform reports one.
    pub(crate) pid: Option<u32>,
    /// True while the child has not been reaped.
    pub(crate) running: bool,
}

fn with_registry<T>(f: impl FnOnce(&mut HashMap<String, Arc<crate::tui::Session>>) -> T) -> T {
    let mut guard = REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Short-lock lookup: clone the `Arc` out, then operate lock-free.
fn lookup(session: &str) -> Result<Arc<crate::tui::Session>, OpError> {
    with_registry(|map| map.get(session).cloned())
        .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))
}

fn fresh_id() -> String {
    format!(
        "sess-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn tui_err(e: &crate::tui::TuiError) -> OpError {
    let code = match e {
        crate::tui::TuiError::Spawn(_) => "spawn-failed",
        crate::tui::TuiError::InvalidInput(_) | crate::tui::TuiError::Chord(_) => "invalid-input",
        crate::tui::TuiError::Unsupported(_)
        | crate::tui::TuiError::ModeNotEnabled(_)
        | crate::tui::TuiError::PasteRejected(_) => "unsupported",
        crate::tui::TuiError::ChildExited(_) | crate::tui::TuiError::Closed(_) => "not-found",
        crate::tui::TuiError::Timeout(_) => "timeout",
        crate::tui::TuiError::Io(_)
        | crate::tui::TuiError::Teardown(_)
        | crate::tui::TuiError::Signal(_)
        | crate::tui::TuiError::Assertion(_) => "op-failed",
    };
    OpError::new(code, e.to_string())
}

fn wait_err(e: &crate::tui::WaitError) -> OpError {
    match e {
        crate::tui::WaitError::Timeout { .. } => OpError::new("timeout", e.to_string()),
        crate::tui::WaitError::Cancelled { .. } => OpError::new("cancelled", e.to_string()),
        crate::tui::WaitError::Unsupported { .. } => OpError::new("unsupported", e.to_string()),
        crate::tui::WaitError::Closed { .. } => OpError::new("not-found", e.to_string()),
    }
}

fn validate_session_id(id: &str) -> Result<(), OpError> {
    if id.is_empty() || id.len() > 128 {
        return Err(OpError::new(
            "invalid-input",
            "session id must be 1..=128 chars",
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(OpError::new(
            "invalid-input",
            "session id allows only [A-Za-z0-9_.-]",
        ));
    }
    Ok(())
}
