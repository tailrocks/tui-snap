//! Worker-side signal delivery for [`WorkerCtx`](super::WorkerCtx).
//!
//! One impl block moved out of `worker_ctx.rs` so both files stay under the
//! repo line gate. Signals run on the worker — the sole reaper — so the
//! pid can never be recycled underneath the delivery (LIFE-2).

use std::sync::mpsc;

use super::super::error::TuiError;
use super::super::exit::ExitStatus;
#[cfg(unix)]
use super::super::exit::process_exists;
use super::super::input_types::Signal;
use super::super::worker::poll_child;
use super::WorkerCtx;

impl WorkerCtx {
    /// Deliver a signal to the direct child (Unix). The worker is the sole
    /// reaper and this runs on the worker thread: a fresh poll that finds
    /// the child unreaped proves the pid is still ours (alive or zombie),
    /// so no recycled pid can be signalled — the reap-before-publish window
    /// that a handle-side `kill` cannot close does not exist here (LIFE-2).
    pub(crate) fn handle_signal(
        &mut self,
        signal: Signal,
        reply: &mpsc::Sender<Result<(), TuiError>>,
    ) {
        // Fresh poll first: publish lags the reap, but this check cannot.
        if let Some(status) = poll_child(&mut self.child) {
            self.exited = Some(ExitStatus::from(status));
        }
        let outcome = self.deliver_signal(signal);
        // The requester may have timed out; the signal outcome stands.
        if reply.send(outcome).is_err() {
            // Requester gone; the signal outcome stands.
        }
    }

    #[cfg(unix)]
    fn deliver_signal(&self, signal: Signal) -> Result<(), TuiError> {
        if self.exited.is_some() {
            return Err(TuiError::ChildExited("child already exited".to_string()));
        }
        let pid = self
            .pid
            .ok_or_else(|| TuiError::Signal("child PID unknown on this platform".to_string()))?;
        // No libc: `kill(1)` (the workspace forbids `unsafe`). Unreaped
        // here means the pid is still the child's, so a failed delivery
        // classifies honestly below instead of risking a recycled pid.
        let delivered = std::process::Command::new("kill")
            .arg(format!("-{}", signal.number()))
            .arg(pid.to_string())
            .status()
            .is_ok_and(|s| s.success());
        if delivered {
            return Ok(());
        }
        if !process_exists(pid) {
            return Err(TuiError::ChildExited(format!(
                "child {pid} no longer exists"
            )));
        }
        Err(TuiError::Signal(format!("kill({pid}) failed")))
    }

    #[cfg(not(unix))]
    fn deliver_signal(&self, _signal: Signal) -> Result<(), TuiError> {
        Err(TuiError::Unsupported("signals require a Unix platform"))
    }
}
