//! Registry lifecycle ops: liveness, drops, signal, resize, stop.
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::proto::OpError;

use super::{SessionLiveness, lookup, tui_err, with_registry};

/// Snapshot every entry's liveness. Arcs come out under one short lock;
/// the polls run lock-free, like every other op in this module.
pub(crate) fn sessions_status() -> Vec<SessionLiveness> {
    let entries: Vec<(String, Arc<crate::tui::Session>)> = with_registry(|map| {
        map.iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    });
    let mut out: Vec<SessionLiveness> = entries
        .into_iter()
        .map(|(name, s)| SessionLiveness {
            name,
            pid: s.pid(),
            running: s.poll_exit().is_none(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// True when the registry holds no sessions (daemon idle-exit gate).
pub(crate) fn is_empty() -> bool {
    with_registry(|map| map.is_empty())
}

/// Drop one entry, closing its session best-effort. Used by daemon
/// `prune`/error paths for sessions already known-exited: the close is
/// hygiene (join threads), so its verdict never fails the prune.
pub(crate) fn drop_session(session: &str) {
    let removed = with_registry(|map| map.remove(session));
    if let Some(s) = removed
        && s.close().is_err()
    {
        // Best-effort hygiene close; the drop stands.
    }
}

/// Drop one entry only when it is already exited (or absent): returns
/// whether the caller may remove the endpoint record. A running entry
/// is never dropped — a start racing the prune must not lose a live
/// session to a stale listing.
pub(crate) fn drop_exited(session: &str) -> bool {
    let entry = with_registry(|map| map.get(session).cloned());
    match entry {
        None => true,
        Some(s) if s.poll_exit().is_some() => {
            drop_session(session);
            true
        }
        Some(_) => false,
    }
}

/// Drop every entry, closing sessions best-effort (live children are
/// killed through their owned handles). Only the daemon's
/// last-resort exit uses this: the runtime dir vanished, so the
/// endpoint records are already gone and the sessions are garbage.
pub(crate) fn close_all() {
    let sessions: Vec<Arc<crate::tui::Session>> =
        with_registry(|map| map.values().cloned().collect());
    with_registry(HashMap::clear);
    for s in sessions {
        if s.close().is_err() {
            // Best-effort last-resort close; the drop stands.
        }
    }
}

/// Deliver a named signal to a session's direct child. The name set is
/// exactly `int|term|kill|quit|hup`. An already-exited session is an
/// error and is never signaled (post-reap no-signal rule, checked here
/// and again inside `Session::signal`, which refuses `ChildExited`).
pub(crate) fn signal(session: &str, sig: &str) -> Result<(), OpError> {
    let signal = match sig {
        "int" => crate::tui::Signal::Int,
        "term" => crate::tui::Signal::Term,
        "kill" => crate::tui::Signal::Kill,
        "quit" => crate::tui::Signal::Quit,
        "hup" => crate::tui::Signal::Hup,
        _ => {
            return Err(OpError::new(
                "invalid-input",
                format!("unknown signal {sig:?} (want int|term|kill|quit|hup)"),
            )
            .with_session(session));
        }
    };
    let s = lookup(session)?;
    if s.poll_exit().is_some() {
        return Err(OpError::new(
            "not-found",
            format!("{session} already exited; not signaling"),
        )
        .with_session(session));
    }
    s.signal(signal)
        .map_err(|e| tui_err(&e).with_session(session))
}

/// Resize a session's PTY and emulator together. Range errors come from
/// the session itself (`InvalidInput` → `invalid-input`).
pub(crate) fn resize(session: &str, cols: u16, rows: u16) -> Result<(), OpError> {
    let s = lookup(session)?;
    s.resize(cols, rows)
        .map_err(|e| tui_err(&e).with_session(session))
}

/// Stop a session: TERM, a 500 ms grace (mirroring the piped stop's
/// TERM→grace→KILL cadence), then the owned kill+reap+join via
/// `close()`. An already-exited session closes without signaling.
/// Any failure preserves the registry entry (fail-closed, like the
/// piped stop preserving the endpoint); only success removes it.
pub(crate) fn stop(session: &str) -> Result<(), OpError> {
    let s = lookup(session)?;
    if s.poll_exit().is_some() {
        return close_stopped(session, &s);
    }
    match s.signal(crate::tui::Signal::Term) {
        Ok(()) | Err(crate::tui::TuiError::ChildExited(_)) => {}
        Err(e) => return Err(tui_err(&e).with_session(session)),
    }
    let cancel = crate::tui::CancelToken::new();
    let deadline = Instant::now() + Duration::from_millis(500);
    // Grace elapsed or not, `close()` kills and reaps what remains;
    // its verdict below stays authoritative either way.
    if s.wait_exit(deadline, &cancel).is_err() {
        // Still running past the grace; the owned kill follows.
    }
    close_stopped(session, &s)
}

/// `close()` + registry removal for [`stop`]: the close verdict is
/// authoritative (a failed close keeps the entry).
fn close_stopped(session: &str, s: &Arc<crate::tui::Session>) -> Result<(), OpError> {
    if let Err(e) = s.close() {
        return Err(tui_err(&e).with_session(session));
    }
    with_registry(|map| map.remove(session));
    Ok(())
}
