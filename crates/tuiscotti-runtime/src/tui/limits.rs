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
/// Op-channel capacity (F12): the reader-to-worker queue is bounded, so a
/// flooding child applies backpressure through the PTY (like a real
/// terminal) instead of piling unbounded `Feed` batches in memory. At the
/// 8 KiB reader batch size this caps queued flood bytes near 512 KiB.
pub(crate) const OP_QUEUE_LIMIT: usize = 64;
/// Bound for one session-side op send (F12): past this the worker is
/// genuinely stuck (not merely draining a flood — draining is fast), so
/// the send fails instead of blocking forever.
pub(crate) const OP_SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum bytes merged into one worker emulator advance (F12): pending
/// `Feed` batches coalesce up to this cap, so a flood costs one grid build
/// per cap instead of one per 8 KiB batch. Larger floods keep the remainder
/// queued (bounded by [`OP_QUEUE_LIMIT`]) for the next advance.
pub(crate) const COALESCE_BYTES: usize = 256 * 1024;
/// Capacity of the worker→writer request queue (LIFE-6): the worker
/// processes ops serially, so at most one acknowledged write plus a few
/// best-effort query replies are ever outstanding; the bound keeps a
/// stuck writer from piling unbounded input behind it.
pub(crate) const WRITE_QUEUE_LIMIT: usize = 16;
/// Bound for one worker-side PTY write, enqueue plus acknowledgement
/// (LIFE-6). Healthy PTY writes complete in microseconds; past this bound
/// the writer is genuinely stuck and the input fails instead of wedging
/// the worker. Control ops stay serviceable while waiting (LIFE-7): the
/// worker pumps the control channel in [`WORKER_TICK`] slices.
pub(crate) const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Capacity of the session→worker control channel (LIFE-7):
/// `CloseInput`/`Signal`/`Shutdown` bypass the mixed op queue so teardown
/// and signals stay serviceable under flood or a stuck write.
pub(crate) const CTL_QUEUE_LIMIT: usize = 8;
