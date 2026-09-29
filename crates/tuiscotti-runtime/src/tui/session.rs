//! [`Session`] handle: observation and waits (R06, R07, R10).
//!
//! Input methods live in [`super::session_input`], teardown in
//! [`super::session_teardown`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tuiscotti_core::screen::{Observation, Screen};

use super::error::{CancelToken, TuiError, WaitError};
use super::exit::{ExitStatus, ExitWait};
use super::limits::DEFAULT_STABLE_QUIET;
use super::session_teardown::recv_reply;
use super::shared::Shared;
use super::worker::{Input, Op};

/// Owned PTY session: the child, its emulator, and both I/O threads.
/// `Send + Sync`; concurrent sessions are fully independent (R07).
pub struct Session {
    pub(crate) op_tx: Option<mpsc::Sender<Op>>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub(crate) reader: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub(crate) closed: AtomicBool,
    pub(crate) pid: Option<u32>,
}

// No unsafe impls: every field is Send + Sync by construction, while the
// `Term` itself never leaves the worker thread.

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pid", &self.pid)
            .field("revision", &self.revision())
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .field("exited", &self.poll_exit().is_some())
            .finish_non_exhaustive()
    }
}

// The handle must stay shareable without any unsafe impl (R07).
const _: fn() = || {
    fn share<T: Send + Sync>() {}
    share::<Session>();
    share::<CancelToken>();
};

impl Session {
    /// Direct child PID, when the platform reports one.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Latest published revision (a short critical section; never blocks on
    /// the worker).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.shared.revision()
    }

    /// Non-blocking exit poll. `Some` once the worker reaped the child.
    #[must_use]
    pub fn poll_exit(&self) -> Option<ExitStatus> {
        self.shared.exit()
    }

    // -- observation ------------------------------------------------------

    /// Fresh atomic capture at the worker's current revision (R06).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if the session is closed or the worker stalls.
    pub fn observe_now(&self) -> Result<Observation, TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::Observe { reply: tx })?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| TuiError::Timeout("observe_now: worker unresponsive".to_string()))?
    }

    /// Fresh grid snapshot.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if the session is closed or the worker stalls.
    pub fn snapshot(&self) -> Result<Screen, TuiError> {
        Ok(self.observe_now()?.screen)
    }

    /// Wait until `predicate` holds, the deadline passes, or `cancel` fires.
    /// Timeout/cancel yield evidence; they never report success.
    ///
    /// # Errors
    ///
    /// Returns `WaitError` on timeout, cancel, or a closed session.
    pub fn wait_predicate<F>(
        &self,
        predicate: F,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError>
    where
        F: Fn(&Observation) -> bool,
    {
        self.wait_loop(deadline, cancel, |latest| {
            latest.filter(|o| predicate(o)).cloned()
        })
    }

    /// Wait until no new revision arrives for `quiet` (default
    /// [`DEFAULT_STABLE_QUIET`]): output settled, not business completion.
    ///
    /// # Errors
    ///
    /// Returns `WaitError` on timeout, cancel, or a closed session.
    pub fn wait_stable(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        self.wait_stable_quiet(deadline, DEFAULT_STABLE_QUIET, cancel)
    }

    /// [`Session::wait_stable`] with an explicit quiet period.
    ///
    /// # Errors
    ///
    /// Returns `WaitError` on timeout, cancel, or a closed session.
    pub fn wait_stable_quiet(
        &self,
        deadline: Instant,
        quiet: Duration,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        let start = Instant::now();
        // Seed from the latest revision; a settled session returns quickly.
        let mut seen = self.latest_or_closed()?.revision;
        let mut quiet_since = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            if let Some(obs) = self.shared.wait_for_newer_than(seen, deadline, cancel) {
                seen = obs.revision;
                quiet_since = Instant::now();
            } else {
                if cancel.is_cancelled() {
                    return Err(WaitError::Cancelled {
                        evidence: Box::new(self.latest_or_closed()?),
                    });
                }
                if Instant::now() >= deadline {
                    return Err(WaitError::Timeout {
                        waited: start.elapsed(),
                        evidence: Box::new(self.latest_or_closed()?),
                    });
                }
                // No newer revision within the slice: check quiet.
                if quiet_since.elapsed() >= quiet {
                    return self.latest_or_closed();
                }
            }
        }
    }

    /// Wait for the next synchronized frame (DEC 2026). The backend does
    /// not track synchronized output, so this always fails closed with
    /// [`WaitError::Unsupported`] plus an evidence snapshot (R10).
    ///
    /// # Errors
    ///
    /// Always fails: `Unsupported`, or `Cancelled`/`Closed` if raced.
    pub fn wait_frame(
        &self,
        _deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        let evidence = Box::new(self.latest_or_closed()?);
        if cancel.is_cancelled() {
            return Err(WaitError::Cancelled { evidence });
        }
        Err(WaitError::Unsupported {
            capability: "synchronized-output (DEC 2026)",
            evidence,
        })
    }

    /// Wait until the direct child exits and is reaped.
    ///
    /// # Errors
    ///
    /// Returns `WaitError` on timeout, cancel, or a closed session.
    pub fn wait_exit(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<ExitWait, WaitError> {
        let start = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            if let Some(status) = self.shared.exit() {
                return Ok(ExitWait {
                    status,
                    observation: self.latest_or_closed()?,
                });
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            self.shared.wait_changed(deadline, cancel);
        }
    }

    /// [`Session::wait_exit`] as an assertion entry point; the returned
    /// [`ExitWait`] offers `.success()` / `.code(n)`.
    ///
    /// # Errors
    ///
    /// Returns `WaitError` on timeout, cancel, or a closed session.
    pub fn expect_exit(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<ExitWait, WaitError> {
        self.wait_exit(deadline, cancel)
    }

    // -- internals ---------------------------------------------------------

    pub(crate) fn send(&self, op: Op) -> Result<(), TuiError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TuiError::Closed("session is closed".to_string()));
        }
        match &self.op_tx {
            Some(tx) => tx
                .send(op)
                .map_err(|_| TuiError::Closed("worker is gone".to_string())),
            None => Err(TuiError::Closed("session is closed".to_string())),
        }
    }

    pub(crate) fn send_input(&self, input: Input) -> Result<(), TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::Input { input, reply: tx })?;
        recv_reply(&rx, "input")
    }

    pub(crate) fn await_initial(&self) -> Result<(), TuiError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let cancel = CancelToken::new();
        loop {
            if self.shared.latest().is_some() {
                return Ok(());
            }
            if self.shared.is_closed() {
                return Err(TuiError::Spawn(
                    "worker exited before publishing revision 0".to_string(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(TuiError::Spawn(
                    "worker did not publish revision 0 in time".to_string(),
                ));
            }
            self.shared.wait_changed(deadline, &cancel);
        }
    }

    pub(crate) fn latest_or_closed(&self) -> Result<Observation, WaitError> {
        match self.shared.latest() {
            Some(o) => Ok(o),
            None if self.shared.is_closed() => Err(WaitError::Closed { evidence: None }),
            None => Err(WaitError::Closed { evidence: None }),
        }
    }

    pub(crate) fn wait_loop<F>(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
        mut done: F,
    ) -> Result<Observation, WaitError>
    where
        F: FnMut(Option<&Observation>) -> Option<Observation>,
    {
        let start = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            let latest = self.shared.latest();
            if let Some(obs) = done(latest.as_ref()) {
                return Ok(obs);
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            self.shared.wait_changed(deadline, cancel);
        }
    }
}
