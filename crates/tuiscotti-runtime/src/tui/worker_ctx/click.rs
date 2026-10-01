//! Click-target op handler for [`WorkerCtx`](super::WorkerCtx).
//!
//! One impl block moved out of `worker_ctx.rs` so both files stay under the
//! repo line gate; behavior is unchanged.

use std::sync::mpsc;

use tuiscotti_core::locate::{LocateError, Locator, Span};
use tuiscotti_core::screen::CaptureReason;

use super::super::capture::drain_grid_events;
use super::super::error::TuiError;
use super::super::frame::build_observation;
use super::super::input_types::{MouseButton, MouseMods};
use super::super::worker::{CtlOp, Input, MouseAction, Op};
use super::WorkerCtx;
use crate::bound_locator::ActionError;

impl WorkerCtx {
    /// Resolve a locator and deliver ONE click as a single worker step
    /// (F11): one fresh observation at the worker's current revision, one
    /// unique-target resolution against it, then press + release applied with
    /// no interleaving op. There is no re-read window: the revision acted on
    /// is the revision resolved, and the acted-on [`Span`] is the reply, so
    /// the caller can audit exactly what was clicked.
    ///
    /// Failure paths deliver nothing except the documented partial: when the
    /// release write fails after the press was delivered, the error says so
    /// and the caller must NOT retry (a retry would send a second press).
    /// The caller must also not retry a reply timeout: the click may already
    /// have been delivered.
    pub(crate) fn handle_click_target(
        &mut self,
        locator: &Locator,
        button: MouseButton,
        mods: MouseMods,
        reply: &mpsc::Sender<Result<Span, ActionError>>,
        op_rx: &mpsc::Receiver<Op>,
        ctl_rx: &mpsc::Receiver<CtlOp>,
    ) -> bool {
        let Some((span, x, y)) = self.resolve_click_target(locator, reply) else {
            return false;
        };
        let down = Input::Mouse {
            action: MouseAction::Press(button),
            x,
            y,
            mods,
        };
        match self.apply_encoded(&down, op_rx, ctl_rx) {
            Ok(true) => return Self::shutdown_during_click(reply),
            Ok(false) => {}
            Err(e) => {
                if reply.send(Err(ActionError::Session(e))).is_err() {
                    // Requester gone; nothing was delivered.
                }
                return false;
            }
        }
        let up = Input::Mouse {
            action: MouseAction::Release,
            x,
            y,
            mods,
        };
        match self.apply_encoded(&up, op_rx, ctl_rx) {
            Ok(true) => return Self::shutdown_during_click(reply),
            Ok(false) => {}
            Err(e) => {
                // The press WAS delivered: name the partial state so the caller
                // does not retry. (Only a PTY I/O failure can land here — the
                // mode gate cannot change mid-op — but report whatever came.)
                let partial = match e {
                    TuiError::Io(msg) => TuiError::Io(format!(
                        "click press delivered but release failed ({msg}); do not retry"
                    )),
                    other => other,
                };
                if reply.send(Err(ActionError::Session(partial))).is_err() {
                    // Requester gone; the partial delivery stands.
                }
                return false;
            }
        }
        drain_grid_events(&mut self.grid, &mut self.events, self.writer.as_ref());
        if reply.send(Ok(span)).is_err() {
            // Requester gone; the delivered click stands.
        }
        false
    }

    /// Build one fresh observation at the worker's current revision and
    /// resolve the locator's unique viewport target against it. Replies on
    /// every failure path; `None` means the reply was already sent.
    fn resolve_click_target(
        &mut self,
        locator: &Locator,
        reply: &mpsc::Sender<Result<Span, ActionError>>,
    ) -> Option<(Span, u16, u16)> {
        drain_grid_events(&mut self.grid, &mut self.events, self.writer.as_ref());
        let (rows, cols) = self.grid.size();
        let obs = match build_observation(
            &self.grid,
            &self.events,
            self.revision,
            CaptureReason::Manual,
            self.pid,
            cols,
            rows,
        ) {
            Ok(obs) => obs,
            Err(e) => {
                // The requester may have timed out; then the reply is moot.
                if reply.send(Err(ActionError::Session(e))).is_err() {
                    // Requester gone; nothing was delivered.
                }
                return None;
            }
        };
        self.shared.publish(obs.clone(), None);
        let pending = match locator.prepare_action(&obs) {
            Ok(pending) => pending,
            Err(e) => {
                if reply.send(Err(ActionError::Locate(e))).is_err() {
                    // Requester gone; nothing was delivered.
                }
                return None;
            }
        };
        let span = pending.span().clone();
        let Some((x, y)) = span.click_point() else {
            // Unreachable: `prepare_action` rejects scrollback targets, and
            // only scrollback spans lack a click point. Fail closed anyway.
            if reply
                .send(Err(ActionError::Locate(LocateError::Usage(
                    "viewport target lost its click point".to_string(),
                ))))
                .is_err()
            {
                // Requester gone; nothing was delivered.
            }
            return None;
        };
        Some((span, x, y))
    }

    /// A shutdown landed mid-click and was serviced: the press or release
    /// may already be delivered, so the reply names the partial state and
    /// forbids retry. Always requests loop exit.
    fn shutdown_during_click(reply: &mpsc::Sender<Result<Span, ActionError>>) -> bool {
        let partial = TuiError::Closed(
            "shutdown during click; press or release may already have been delivered, \
             do not retry"
                .to_string(),
        );
        if reply.send(Err(ActionError::Session(partial))).is_err() {
            // Requester gone; the partial delivery stands.
        }
        true
    }
}
