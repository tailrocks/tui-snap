//! Registry spawn ops: [`spawn`] + [`spawn_os`].
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::proto::{MAX_CONCURRENT_SESSIONS, OpError, OpResult, SESSION_LIMIT_CODE};

use super::{fresh_id, tui_err, validate_session_id, with_registry};

/// Outcome of the under-lock registry insert: the cap check inside the
/// same critical section is authoritative (a pre-spawn count can only
/// fast-fail the common full case — concurrent starters race past it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertOutcome {
    Won,
    Duplicate,
    Full,
}

fn limit_error(id: &str) -> OpError {
    OpError::new(
        SESSION_LIMIT_CODE,
        format!("at the session limit ({MAX_CONCURRENT_SESSIONS}); stop one first"),
    )
    .with_session(id)
}

pub(crate) fn spawn(
    argv: &[String],
    id: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    cwd: Option<PathBuf>,
    env: &HashMap<String, String>,
) -> Result<OpResult, OpError> {
    let owned: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
    spawn_os(&owned, id, cols, rows, cwd, env)
}

/// [`spawn`] with native [`OsString`](std::ffi::OsString) argv: the child
/// spawns byte-exact. The retained-session daemon (F08-F2) uses this so
/// PTY starts match piped starts (`session_start_os`); the endpoint
/// record keeps a lossy UTF-8 projection (diagnostic, never re-spawned).
pub(crate) fn spawn_os(
    argv: &[std::ffi::OsString],
    id: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    cwd: Option<PathBuf>,
    env: &HashMap<String, String>,
) -> Result<OpResult, OpError> {
    if argv.is_empty() {
        return Err(OpError::new(
            "invalid-input",
            "spawn needs a non-empty argv",
        ));
    }
    let id = id.unwrap_or_else(fresh_id);
    validate_session_id(&id)?;
    let mut builder = crate::tui::Tui::new(argv.to_vec());
    if let (Some(c), Some(r)) = (cols, rows) {
        builder = builder.size(c, r);
    } else if cols.is_some() || rows.is_some() {
        return Err(OpError::new(
            "invalid-input",
            "cols and rows must be given together",
        ));
    }
    for (k, v) in env {
        builder = builder.env(k, v);
    }
    if let Some(cwd) = cwd {
        builder = builder.cwd(cwd);
    }
    // Check-then-spawn-then-insert, all outside one lock: the pre-checks
    // fail the common duplicate/full cases before any child exists; the
    // spawn itself runs lock-free (it blocks for the worker's revision
    // 0); the insert re-checks under the lock so a race loser closes
    // what it just spawned instead of clobbering the winner or
    // overshooting the cap.
    if with_registry(|map| map.contains_key(&id)) {
        return Err(
            OpError::new("session-exists", format!("{id} already spawned")).with_session(&id),
        );
    }
    if with_registry(|map| map.len()) >= MAX_CONCURRENT_SESSIONS {
        return Err(limit_error(&id));
    }
    let session = builder.spawn().map_err(|e| tui_err(&e).with_session(&id))?;
    let pid = session.pid();
    let session = Arc::new(session);
    let outcome = with_registry(|map| {
        if map.contains_key(&id) {
            return InsertOutcome::Duplicate;
        }
        if map.len() >= MAX_CONCURRENT_SESSIONS {
            return InsertOutcome::Full;
        }
        map.insert(id.clone(), Arc::clone(&session));
        InsertOutcome::Won
    });
    match outcome {
        InsertOutcome::Won => Ok(OpResult::Spawned { session: id, pid }),
        InsertOutcome::Duplicate => {
            // Lost a same-id race: close what we spawned (best-effort;
            // the duplicate verdict stays authoritative either way).
            if session.close().is_err() {
                // Close failed after a lost race; the verdict stands.
            }
            Err(OpError::new("session-exists", format!("{id} already spawned")).with_session(&id))
        }
        InsertOutcome::Full => {
            // Lost a cap race: the child we spawned is surplus — close
            // it (owned kill+reap) so no stray survives the rejection.
            if session.close().is_err() {
                // Close failed after a lost race; the verdict stands.
            }
            Err(limit_error(&id))
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::super::{is_empty, sessions_status, stop};
    use super::*;
    #[cfg(unix)]
    use crate::proto::pid_alive;

    #[cfg(unix)]
    fn sleep_argv() -> Vec<std::ffi::OsString> {
        vec![
            std::ffi::OsString::from("/bin/sleep"),
            std::ffi::OsString::from("20"),
        ]
    }

    #[cfg(unix)]
    fn spawned_pid(result: &OpResult) -> Option<u32> {
        match result {
            OpResult::Spawned { pid, .. } => *pid,
            _ => None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn registry_admits_up_to_cap_then_rejects_without_strays() {
        // Sole registry user in this test binary: the global starts empty.
        assert!(is_empty(), "registry must start empty");
        let me = std::process::id();
        let mut ids = Vec::with_capacity(MAX_CONCURRENT_SESSIONS);
        let mut pids = Vec::with_capacity(MAX_CONCURRENT_SESSIONS + 1);
        for i in 0..MAX_CONCURRENT_SESSIONS {
            let id = format!("admit-{me}-{i}");
            let result = spawn_os(
                &sleep_argv(),
                Some(id.clone()),
                None,
                None,
                None,
                &HashMap::new(),
            )
            .expect("spawn succeeds under the cap");
            pids.push(spawned_pid(&result).expect("spawned result carries a pid"));
            ids.push(id);
        }
        // Over the cap: typed rejection, no entry, no child.
        let over = format!("admit-{me}-over");
        let err = spawn_os(
            &sleep_argv(),
            Some(over.clone()),
            None,
            None,
            None,
            &HashMap::new(),
        )
        .expect_err("over-cap spawn must fail");
        assert_eq!(err.code, SESSION_LIMIT_CODE);
        assert_eq!(err.session.as_deref(), Some(over.as_str()));
        assert!(
            !sessions_status().iter().any(|s| s.name == over),
            "rejected spawn must leave no entry"
        );
        // One freed slot reopens admission.
        stop(&ids[0]).expect("stop succeeds");
        let reopened = format!("admit-{me}-reopened");
        let result = spawn_os(
            &sleep_argv(),
            Some(reopened.clone()),
            None,
            None,
            None,
            &HashMap::new(),
        )
        .expect("spawn succeeds after a slot frees");
        pids.push(spawned_pid(&result).expect("spawned result carries a pid"));
        // Cleanup: every child reaped, registry empty, no strays.
        stop(&reopened).expect("stop succeeds");
        for id in &ids[1..] {
            stop(id).expect("stop succeeds");
        }
        assert!(is_empty(), "registry must drain after stops");
        for pid in pids {
            assert!(!pid_alive(pid), "stray child {pid} survived");
        }
    }
}
