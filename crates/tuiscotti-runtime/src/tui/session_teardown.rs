//! [`Session`](super::session::Session) teardown: finish, close, bounded
//! joins, `Drop` (R08).

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::error::{CancelToken, TuiError, WaitError};
use super::exit::ExitStatus;
use super::limits::JOIN_GRACE;
use super::session::Session;
use super::shared::Shared;
use super::worker::Op;

impl Session {
    /// Graceful shutdown: EOF stdin, wait for natural exit until `deadline`,
    /// reap. On timeout the child is killed and a timeout error (with the
    /// final evidence revision noted) is returned; teardown still completes.
    pub fn finish(mut self, deadline: Instant) -> Result<ExitStatus, TuiError> {
        let cancel = CancelToken::new();
        match self.close_input() {
            Ok(()) => {}
            Err(TuiError::ChildExited(_)) => {}
            Err(e) => return Err(e),
        }
        match self.wait_exit(deadline, &cancel) {
            Ok(w) => {
                self.close()?;
                Ok(w.status)
            }
            Err(WaitError::Timeout { evidence, waited }) => {
                // Teardown still completes, but the timeout verdict stays
                // authoritative even when the forced close also fails.
                if self.close().is_err() {
                    // Close failed during timeout teardown; timeout stands.
                }
                Err(TuiError::Timeout(format!(
                    "finish: child still alive after {waited:?}; killed during teardown (evidence at revision {})",
                    evidence.revision
                )))
            }
            Err(WaitError::Cancelled { .. }) => {
                if self.close().is_err() {
                    // Close failed during teardown; the wait error stands.
                }
                Err(TuiError::Timeout(
                    "finish: wait cancelled via internal token (unreachable)".to_string(),
                ))
            }
            Err(WaitError::Unsupported { .. }) => {
                if self.close().is_err() {
                    // Close failed during teardown; the wait error stands.
                }
                Err(TuiError::Timeout(
                    "finish: unsupported wait (unreachable)".to_string(),
                ))
            }
            Err(WaitError::Closed { .. }) => {
                if self.close().is_err() {
                    // Close failed during teardown; the closed error stands.
                }
                Err(TuiError::Closed("finish: session closed".to_string()))
            }
        }
    }

    /// Forceful idempotent teardown: kill a living child (bounded grace),
    /// reap, join threads. Returns the first teardown error, if any.
    pub fn close(&mut self) -> Result<(), TuiError> {
        self.teardown();
        if let Some(msg) = self.shared.teardown_error() {
            return Err(TuiError::Teardown(msg));
        }
        Ok(())
    }

    pub(crate) fn close_input(&self) -> Result<(), TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::CloseInput { reply: tx })?;
        recv_reply(rx, "close stdin")
    }

    /// Run teardown exactly once; never panics (safe from `Drop`).
    pub(crate) fn teardown(&mut self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            // A previous close/drop already shut down; still join in case a
            // concurrent teardown is in flight.
            self.join_threads();
            return;
        }
        if let Some(tx) = self.op_tx.take() {
            // The worker may already be gone; the joins below still reap.
            if tx.send(Op::Shutdown).is_err() {
                // Worker gone; join_threads below still reaps.
            }
        }
        self.join_threads();
    }

    pub(crate) fn join_threads(&mut self) {
        let worker = self.worker.lock().map(|mut g| g.take()).unwrap_or(None);
        let reader = self.reader.lock().map(|mut g| g.take()).unwrap_or(None);
        if let Some(h) = worker {
            join_one(h, &self.shared, "worker", JOIN_GRACE);
        }
        if let Some(h) = reader {
            join_one(h, &self.shared, "reader", JOIN_GRACE);
        }
        self.shared.mark_closed();
    }
}

/// Bounded join of one session thread; never blocks past `grace`, never
/// panics. On timeout the handle is detached (the waiter thread owns it
/// and reaps the thread if it ever exits) and a teardown diagnostic is
/// recorded, so `Drop` can never hang on a reader blocked in `read()`
/// after a failed child kill. On waiter-spawn failure the handle drops
/// here, which also detaches rather than hangs.
pub(crate) fn join_one(
    h: std::thread::JoinHandle<()>,
    shared: &Shared,
    name: &str,
    grace: Duration,
) {
    let (tx, rx) = mpsc::channel::<bool>();
    let waiter = std::thread::Builder::new()
        .name(format!("tuisnap-tui-join-{name}"))
        .spawn(move || {
            let panicked = h.join().is_err();
            // The joiner may have given up waiting (timeout arm); then the
            // verdict is already recorded as detached and this is moot.
            if tx.send(panicked).is_err() {
                // Joiner gone; the thread is reaped either way.
            }
        });
    match waiter {
        Ok(_waiter) => match rx.recv_timeout(grace) {
            Ok(true) => shared.record_teardown(&format!("{name} thread panicked")),
            Ok(false) => {}
            Err(_) => shared.record_teardown(&format!(
                "{name} thread did not exit within {grace:?}; detached"
            )),
        },
        Err(e) => shared.record_teardown(&format!("join waiter spawn failed for {name}: {e}")),
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Never panic from Drop: teardown paths only record errors.
        self.teardown();
    }
}

pub(crate) fn recv_reply(
    rx: mpsc::Receiver<Result<(), TuiError>>,
    what: &str,
) -> Result<(), TuiError> {
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| TuiError::Timeout(format!("{what}: worker unresponsive")))?
}
