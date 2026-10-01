//! Session record types + shared limits.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use serde::{Deserialize, Serialize};

/// Endpoint file format version. A reader that sees another version refuses
/// the file instead of guessing.
pub const SESSION_ENDPOINT_VERSION: u32 = 1;

/// Largest endpoint file we will parse: a tamper-bounded read.
pub(super) const MAX_ENDPOINT_BYTES: u64 = 1_048_576;

/// Largest PID we will ever probe or signal. `pid_t` is a 32-bit signed int
/// on our Unix targets; 0 and negatives select process groups, never a child.
pub(super) const PID_MAX: u32 = 2_147_483_647;

/// Absolute `kill(1)` locations, in preference order. Never a `PATH` lookup:
/// a tampered `PATH` must not redirect session signaling.
pub(super) const KILL_BINARIES: [&str; 2] = ["/bin/kill", "/usr/bin/kill"];

/// Reservations past this age with a dead owner are crash residue and may be
/// taken over by a new starter.
pub(super) const RESERVATION_TAKEOVER_SECS: u64 = 30;

/// Unparsable reservation locks only go stale by age (a concurrent starter
/// may not have finished writing yet).
pub(super) const RESERVATION_CORRUPT_STALE_SECS: u64 = 120;

/// Cap on concurrent live sessions (piped + PTY). Each retained PTY
/// session pins a PTY pair, two threads, and a child; each piped session a
/// child plus its endpoint record. 32 bounds the worst case (~100 fds, 64
/// threads) while doubling the test suite's worst case (4 PTY-heavy tests
/// in flight × ~4 sessions each). Enforced exactly at the PTY registry
/// insert and best-effort (pre-spawn endpoint count) at piped start; both
/// reject with [`SESSION_LIMIT_CODE`], never a silent queue.
pub(crate) const MAX_CONCURRENT_SESSIONS: usize = 32;

/// Typed rejection code when a start would exceed
/// [`MAX_CONCURRENT_SESSIONS`]: the caller stops a session first and
/// retries. No child spawns on this path.
pub(crate) const SESSION_LIMIT_CODE: &str = "session-limit";

/// Backend that owns the named session's child.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionBackend {
    /// Plain piped child, owned by no one (liveness via `pid_alive`).
    Process,
    /// PTY-backed session owned by the retained-session daemon (F08-F2):
    /// the daemon holds the live `tui::Session` handle and is
    /// authoritative for liveness; the endpoint names it via `daemon_pid`.
    Pty,
}

/// Liveness of a named session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    /// The recorded pid is alive.
    Running,
    /// The recorded pid is dead.
    Exited,
}

/// What `session list` reports per session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Recorded child pid.
    pub pid: u32,
    /// Spawn argv (lossy UTF-8 projection).
    pub argv: Vec<String>,
    /// Backend that owns the child.
    pub backend: SessionBackend,
    /// Current liveness.
    pub status: SessionStatus,
    /// Start time as unix seconds.
    pub started_unix: u64,
}

/// On-disk endpoint record. Every field is validated on read; a record that
/// disagrees with the requested entry or lacks identity metadata is corrupt,
/// never authoritative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionEndpoint {
    pub(crate) version: u32,
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) argv: Vec<String>,
    pub(crate) backend: SessionBackend,
    pub(crate) started_unix: u64,
    /// Owner uid (required: records without it are corrupt, never adopted).
    pub(crate) owner: u32,
    /// Owning daemon's pid. Required if and only if `backend` is `Pty`
    /// (F08-F2): a `Pty` record without one is corrupt, and a `Process`
    /// record carrying one is corrupt. `Default` keeps pre-F2 records
    /// (which lack the field) parsing as `Process` with `None`, so no
    /// format version bump was needed.
    #[serde(default)]
    pub(crate) daemon_pid: Option<u32>,
}
