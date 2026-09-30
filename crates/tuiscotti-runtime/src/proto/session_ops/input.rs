//! Session transport ops: input + observe.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use super::super::{
    DaemonOp, ObservationView, OpError, OpResult, PtyOwner, SessionBackend, base64_encode,
    classify_owner, read_endpoint, runtime_dir, status, transact, validate_session_name,
};
use super::stop::pty_owner_pid;

/// Send input to a PTY session: exactly one of `text`, `chord`, or
/// `bytes_b64`. Piped sessions have no input transport (`unsupported`);
/// orphaned sessions have no owner left (`not-found`). Never autostarts.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a wrong
/// backend, a silent owner, or a refused/failed delivery.
pub fn session_input(
    name: &str,
    text: Option<String>,
    chord: Option<String>,
    bytes_b64: Option<String>,
) -> Result<(), OpError> {
    validate_session_name(name)?;
    let set = [text.is_some(), chord.is_some(), bytes_b64.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    if set != 1 {
        return Err(OpError::new(
            "invalid-input",
            "stdin needs exactly one of text|chord|bytes_b64",
        ));
    }
    pty_transact(
        name,
        &DaemonOp::Input {
            name: name.to_string(),
            text,
            chord,
            bytes_b64,
        },
    )?;
    Ok(())
}

/// [`session_input`] for raw bytes (the attach loop's stdin forwarding).
///
/// # Errors
///
/// Returns [`OpError`] for empty bytes or any [`session_input`] failure.
pub fn session_input_bytes(name: &str, bytes: &[u8]) -> Result<(), OpError> {
    if bytes.is_empty() {
        return Err(OpError::new("invalid-input", "bytes must not be empty"));
    }
    session_input(name, None, None, Some(base64_encode(bytes)))
}

/// Read a PTY session's current observation projection. Piped sessions
/// have no screen (`unsupported`); orphaned sessions have no owner left
/// (`not-found`). Never autostarts.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a wrong
/// backend, a silent owner, or an unreadable session.
pub fn session_observe(name: &str) -> Result<ObservationView, OpError> {
    validate_session_name(name)?;
    let value = pty_transact(
        name,
        &DaemonOp::Observe {
            name: name.to_string(),
        },
    )?;
    match serde_json::from_value::<OpResult>(value) {
        Ok(OpResult::Observation { observation }) => Ok(observation),
        Ok(_) => Err(OpError::new("internal", "daemon returned the wrong result")),
        Err(e) => Err(OpError::new("internal", format!("bad daemon result: {e}"))),
    }
}

/// Run one op against a PTY session's live owner: validate the endpoint
/// read first (tamper fails here, before any daemon contact), refuse
/// piped backends (no transport exists), refuse orphans (no owner left).
/// A daemon lost mid-op re-classifies once; a newly orphaned session
/// reports `not-found`, a still-live owner reports the transport error.
fn pty_transact(name: &str, op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let dir = runtime_dir()?;
    let ep = read_endpoint(&dir, name)?.ok_or_else(|| OpError::new("not-found", name))?;
    if ep.backend != SessionBackend::Pty {
        return Err(OpError::new(
            "unsupported",
            format!("{name} is a piped session; this op needs --pty"),
        ));
    }
    let recorded = pty_owner_pid(&ep)?;
    match classify_owner(recorded, &status()?)? {
        PtyOwner::Live => match transact(op) {
            Ok(value) => Ok(value),
            Err(e) if e.code == "io" || e.code == "timeout" => {
                match classify_owner(recorded, &status()?)? {
                    PtyOwner::Orphan => Err(orphaned(name)),
                    PtyOwner::Live => Err(e),
                }
            }
            Err(e) => Err(e),
        },
        PtyOwner::Orphan => Err(orphaned(name)),
    }
}

/// The owner-is-down verdict for transport ops (input/observe): the
/// child may linger, but without its owner there is no transport.
fn orphaned(name: &str) -> OpError {
    OpError::new(
        "not-found",
        format!("{name}: owner daemon is down; session orphaned"),
    )
}
