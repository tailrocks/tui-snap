//! Named sessions (A02): versioned endpoints, owner-only runtime dir.
//!
//! Trust boundary (F08): an endpoint JSON file is untrusted metadata. It never
//! authorizes signaling a PID or deleting a path by itself: every read
//! validates the payload name against the requested entry, requires complete
//! ownership/identity metadata, and rejects PID 0 and out-of-range PIDs. OS
//! identity comes from filesystem ownership probes (`std` only, no `PATH`
//! lookup, no environment fallback, no zero default). termpane =0.1.0 from
//! crates.io is now the backend; these `std`-only identity helpers stay until
//! final wiring lands (`termpane::process::{current_uid, pid_alive, signal}`).
//!
//! Split into one module per area so each file stays under the repo line
//! gate; behavior is unchanged.

#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

mod endpoint;
mod process;
mod reservation;
mod runtime_dir;
mod types;
mod validate;

pub(crate) use endpoint::{read_endpoint, write_endpoint};
pub(crate) use process::{now_unix, pid_alive, stop_pid};
pub(crate) use reservation::NameReservation;
pub(crate) use runtime_dir::current_uid;
pub use runtime_dir::runtime_dir;
#[cfg(any(test, feature = "test-overrides"))]
pub use runtime_dir::set_runtime_dir_override;
pub(crate) use types::{MAX_CONCURRENT_SESSIONS, SESSION_LIMIT_CODE, SessionEndpoint};
pub use types::{SESSION_ENDPOINT_VERSION, SessionBackend, SessionInfo, SessionStatus};
#[cfg(unix)]
pub(crate) use validate::checked_daemon_path;
pub(crate) use validate::{
    checked_aux_path, checked_endpoint_path, validate_pid, validate_session_name,
};

#[cfg(test)]
static TEST_CTR: AtomicU64 = AtomicU64::new(0);

/// Best-effort scratch cleanup (test teardown must not fail the test).
#[cfg(test)]
fn cleanup(dir: &Path) {
    if std::fs::remove_dir_all(dir).is_err() {
        // Leftover scratch in the temp dir is harmless.
    }
}

/// Unique scratch dir per test (parallel-safe, no process-global state).
#[cfg(test)]
fn test_dir(tag: &str) -> PathBuf {
    let n = TEST_CTR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("tuiscotti-sec-{}-{tag}-{n}", std::process::id()));
    cleanup(&dir);
    std::fs::create_dir_all(&dir).expect("test dir");
    dir
}

#[cfg(test)]
fn sample_endpoint(name: &str, pid: u32, owner: u32) -> SessionEndpoint {
    SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid,
        argv: vec!["sleep".to_string(), "30".to_string()],
        backend: SessionBackend::Process,
        started_unix: now_unix(),
        owner,
        daemon_pid: None,
    }
}

#[cfg(test)]
fn sample_pty_endpoint(
    name: &str,
    pid: u32,
    owner: u32,
    daemon_pid: Option<u32>,
) -> SessionEndpoint {
    SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid,
        argv: vec!["sh".to_string()],
        backend: SessionBackend::Pty,
        started_unix: now_unix(),
        owner,
        daemon_pid,
    }
}

#[cfg(test)]
fn sample_owner() -> u32 {
    current_uid().unwrap_or(0)
}
