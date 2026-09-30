//! Click-target op handler for [`WorkerCtx`](super::WorkerCtx).
//!
//! One impl block moved out of `worker_ctx.rs` so both files stay under the
//! repo line gate; behavior is unchanged.

use std::sync::mpsc;

use tuiscotti_core::locate::{LocateError, Locator, Span};
use tuiscotti_core::screen::CaptureReason;

use super::super::capture::drain_term_events;
use super::super::encode::apply_input;
use super::super::error::TuiError;
use super::super::frame::build_observation;
use super::super::input_types::{MouseButton, MouseMods};
use super::super::worker::{Input, MouseAction, cols_of, rows_of};
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
    ) {
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
        );
        let (cols, rows) = (cols_of(&self.term), rows_of(&self.term));
        let obs = match build_observation(
            &self.term,
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
                return;
            }
        };
        self.shared.publish(obs.clone(), None);
        let pending = match locator.prepare_action(&obs) {
            Ok(pending) => pending,
            Err(e) => {
                if reply.send(Err(ActionError::Locate(e))).is_err() {
                    // Requester gone; nothing was delivered.
                }
                return;
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
            return;
        };
        let exited = self.exited.is_some();
        let down = Input::Mouse {
            action: MouseAction::Press(button),
            x,
            y,
            mods,
        };
        if let Err(e) = apply_input(&mut self.term, &down, self.writer.as_deref_mut(), exited) {
            if reply.send(Err(ActionError::Session(e))).is_err() {
                // Requester gone; nothing was delivered.
            }
            return;
        }
        let up = Input::Mouse {
            action: MouseAction::Release,
            x,
            y,
            mods,
        };
        if let Err(e) = apply_input(&mut self.term, &up, self.writer.as_deref_mut(), exited) {
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
            return;
        }
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
        );
        if reply.send(Ok(span)).is_err() {
            // Requester gone; the delivered click stands.
        }
    }
}
