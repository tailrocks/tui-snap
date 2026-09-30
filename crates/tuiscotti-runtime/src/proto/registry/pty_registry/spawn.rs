//! Registry spawn ops: [`spawn`] + [`spawn_os`].
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::proto::{OpError, OpResult};

use super::{fresh_id, tui_err, validate_session_id, with_registry};

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
    // Check-then-spawn-then-insert, all outside one lock: the pre-check
    // fails the common duplicate before any child exists; the spawn
    // itself runs lock-free (it blocks for the worker's revision 0);
    // the insert re-checks so a same-id race loser closes what it just
    // spawned instead of clobbering the winner.
    if with_registry(|map| map.contains_key(&id)) {
        return Err(
            OpError::new("session-exists", format!("{id} already spawned")).with_session(&id),
        );
    }
    let session = builder.spawn().map_err(|e| tui_err(&e).with_session(&id))?;
    let pid = session.pid();
    let session = Arc::new(session);
    let won = with_registry(|map| {
        if map.contains_key(&id) {
            return false;
        }
        map.insert(id.clone(), Arc::clone(&session));
        true
    });
    if !won {
        // Lost a same-id race: close what we spawned (best-effort; the
        // duplicate verdict stays authoritative either way).
        if session.close().is_err() {
            // Close failed after a lost race; the verdict stands.
        }
        return Err(
            OpError::new("session-exists", format!("{id} already spawned")).with_session(&id),
        );
    }
    Ok(OpResult::Spawned { session: id, pid })
}
