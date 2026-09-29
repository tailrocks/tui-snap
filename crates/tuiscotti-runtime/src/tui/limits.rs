//! Backend grid limits, timing constants, and the process-global PTY guard.

use std::sync::Mutex;
use std::time::Duration;

/// Serializes PTY open/spawn against kill/reap process-wide. This guards a
/// kernel race, not an emulator bug; every `portable-pty` consumer needs it.
pub(crate) static PTY_LIFECYCLE: Mutex<()> = Mutex::new(());

/// Backend grid limits (alacritty minimum columns = 2; generous maximum).
pub const MIN_COLS: u16 = 2;
/// Minimum PTY rows.
pub const MIN_ROWS: u16 = 1;
/// Maximum PTY columns.
pub const MAX_COLS: u16 = 1000;
/// Maximum PTY rows.
pub const MAX_ROWS: u16 = 1000;

/// How long after child exit the worker still accepts trailing reader bytes.
pub(crate) const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// Worker tick: child-exit polling cadence.
pub(crate) const WORKER_TICK: Duration = Duration::from_millis(25);
/// Wait polling slice: cancel/deadline responsiveness (R07).
pub(crate) const WAIT_SLICE: Duration = Duration::from_millis(25);
/// Default quiet period for [`Session::wait_stable`](crate::tui::Session::wait_stable).
pub const DEFAULT_STABLE_QUIET: Duration = Duration::from_millis(200);
/// Grace for SIGKILL-triggered reap during teardown.
pub(crate) const KILL_GRACE: Duration = Duration::from_secs(2);
/// Bound for joining one session thread during teardown. Must exceed the
/// worker's worst case (`KILL_GRACE` + tick) so a healthy-but-slow kill is
/// never misreported as stuck. The reader has no unblock handle (it owns
/// the only PTY reader), so a child that survives kill would block `read()`
/// forever — past this grace the thread is detached, never joined forever.
pub(crate) const JOIN_GRACE: Duration = Duration::from_secs(5);
