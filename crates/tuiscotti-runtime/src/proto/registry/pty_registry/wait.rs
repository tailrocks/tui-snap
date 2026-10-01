//! Registry wait/exit ops.
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::time::{Duration, Instant};

use crate::proto::{OpError, OpResult, observation_view, screen_text, wait_kind};

use super::{lookup, wait_err, with_registry};

pub(crate) fn wait(
    session: &str,
    kind: &str,
    needle: Option<&str>,
    quiet_ms: Option<u64>,
    timeout_ms: u64,
) -> Result<OpResult, OpError> {
    let s = lookup(session)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let cancel = crate::tui::CancelToken::new();
    match kind {
        wait_kind::TEXT => {
            let needle = needle.ok_or_else(|| {
                OpError::new("invalid-input", "text wait needs `needle`").with_session(session)
            })?;
            if needle.is_empty() {
                return Err(
                    OpError::new("invalid-input", "needle must not be empty").with_session(session)
                );
            }
            let obs = s
                .wait_predicate(
                    |o| screen_text(&o.screen).contains(needle),
                    deadline,
                    &cancel,
                )
                .map_err(|e| wait_err(&e).with_session(session))?;
            Ok(OpResult::Waited {
                session: session.to_string(),
                observation: observation_view(&obs),
            })
        }
        wait_kind::STABLE => {
            let quiet = Duration::from_millis(quiet_ms.unwrap_or(200));
            let obs = s
                .wait_stable_quiet(deadline, quiet, &cancel)
                .map_err(|e| wait_err(&e).with_session(session))?;
            Ok(OpResult::Waited {
                session: session.to_string(),
                observation: observation_view(&obs),
            })
        }
        wait_kind::EXIT => {
            let ew = s
                .wait_exit(deadline, &cancel)
                .map_err(|e| wait_err(&e).with_session(session))?;
            Ok(OpResult::Exited {
                session: session.to_string(),
                code: ew.status.code(),
                signal: ew.status.signal().map(str::to_string),
                observation: observation_view(&ew.observation),
            })
        }
        other => Err(OpError::new(
            "invalid-input",
            format!("unknown wait kind {other:?} (want text|stable|exit)"),
        )
        .with_session(session)),
    }
}

pub(crate) fn exit(session: &str, timeout_ms: u64) -> Result<OpResult, OpError> {
    let s = with_registry(|map| {
        map.remove(session)
            .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))
    })?;
    // Graceful wait first so `exit` on a running app reaps evidence.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let cancel = crate::tui::CancelToken::new();
    match s.wait_exit(deadline, &cancel) {
        Ok(ew) => {
            // Teardown is best-effort once the session left the registry:
            // the observed exit verdict stays authoritative.
            if s.close().is_err() {
                // Close failed after exit; the exit evidence stands.
            }
            Ok(OpResult::Exited {
                session: session.to_string(),
                code: ew.status.code(),
                signal: ew.status.signal().map(str::to_string),
                observation: observation_view(&ew.observation),
            })
        }
        Err(crate::tui::WaitError::Timeout { evidence, .. }) => {
            // Teardown is best-effort: the timeout verdict below stays
            // authoritative even when the forced close also fails.
            if s.close().is_err() {
                // Close failed during timeout teardown; timeout stands.
            }
            Err(OpError::new(
                "timeout",
                format!(
                    "child still running after {timeout_ms}ms (evidence at revision {})",
                    evidence.revision
                ),
            )
            .with_session(session))
        }
        Err(e) => {
            // Teardown is best-effort: the wait error below stays
            // authoritative even when the close also fails.
            if s.close().is_err() {
                // Close failed during error teardown; wait error stands.
            }
            Err(wait_err(&e).with_session(session))
        }
    }
}
