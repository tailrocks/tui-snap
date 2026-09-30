//! Worker context: the worker thread's owned emulator state plus op handlers.
//!
//! `run_worker` owns the op channel and the main loop; this context owns
//! everything the loop dispatches on. The op handlers are the exact bodies
//! the loop used to inline, so behavior is unchanged.

use std::sync::{Arc, mpsc};
use std::time::Instant;

use alacritty_terminal::event::Event;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Processor;
use portable_pty::{Child as PtyChild, MasterPty};
use tuiscotti_core::screen::{CaptureReason, Observation};

use super::capture::drain_term_events;
use super::encode::{WriteHandle, apply_resize};
use super::error::TuiError;
use super::exit::ExitStatus;
use super::frame::build_observation;
use super::limits::{DRAIN_GRACE, KILL_GRACE, WORKER_TICK};
use super::shared::{DrainCause, Shared};
use super::worker::{
    CtlOp, Input, Op, QueueListener, WorkerEventState, cols_of, poll_child, rows_of, shutdown_child,
};

mod click;
mod signal;
mod write;

/// Owned worker state: emulator, writer, child, and publication progress.
/// Lives only on the worker thread.
pub(crate) struct WorkerCtx {
    term: Term<QueueListener>,
    processor: Processor,
    writer: Option<WriteHandle>,
    events: WorkerEventState,
    event_rx: mpsc::Receiver<Event>,
    revision: u64,
    eof: bool,
    exited: Option<ExitStatus>,
    exit_seen_at: Option<Instant>,
    finalized: bool,
    pid: Option<u32>,
    shared: Arc<Shared>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn PtyChild + Send + Sync>,
}

impl WorkerCtx {
    pub(crate) fn new(
        term: Term<QueueListener>,
        event_rx: mpsc::Receiver<Event>,
        writer: WriteHandle,
        pid: Option<u32>,
        shared: Arc<Shared>,
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn PtyChild + Send + Sync>,
    ) -> Self {
        Self {
            term,
            processor: Processor::new(),
            writer: Some(writer),
            events: WorkerEventState::new(),
            event_rx,
            revision: 0,
            eof: false,
            exited: None,
            exit_seen_at: None,
            finalized: false,
            pid,
            shared,
            master,
            child,
        }
    }

    /// Revision 0: the Initial observation `spawn()` blocks on.
    pub(crate) fn publish_initial(&mut self, cols: u16, rows: u16) {
        self.publish_current(CaptureReason::Initial, cols, rows);
    }

    /// Publish the final exit revision at the next revision.
    fn publish_exit(&mut self, status: ExitStatus, drain: DrainCause) {
        // Drain without the writer: replies have nowhere to go, but title
        // and bell state still belong in the final observation.
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::Title(t) => self.events.title = Some(t),
                Event::ResetTitle => self.events.title = None,
                Event::Bell => self.events.bells += 1,
                _ => {}
            }
        }
        self.revision += 1;
        let cols = cols_of(&self.term);
        let rows = rows_of(&self.term);
        match build_observation(
            &self.term,
            &self.events,
            self.revision,
            CaptureReason::Exit,
            self.pid,
            cols,
            rows,
        ) {
            Ok(obs) => self.shared.publish_exit(status, obs, drain),
            Err(e) => self
                .shared
                .record_teardown(&format!("exit observation build failed: {e}")),
        }
    }

    /// Drain terminal events, then publish the current state at `revision`.
    fn publish_current(&mut self, reason: CaptureReason, cols: u16, rows: u16) {
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_ref(),
        );
        match build_observation(
            &self.term,
            &self.events,
            self.revision,
            reason,
            self.pid,
            cols,
            rows,
        ) {
            Ok(obs) => self.shared.publish(obs, None),
            Err(e) => self
                .shared
                .record_teardown(&format!("observation build failed: {e}")),
        }
    }

    /// Feed one reader batch through the emulator and publish. After the
    /// exit revision is finalized, trailing bytes are dropped: the final
    /// observation is already published, and a post-exit `Poll` revision
    /// would lie about what `wait_exit` returned (LIFE-8).
    pub(crate) fn handle_feed(&mut self, bytes: &[u8]) {
        if self.finalized {
            return;
        }
        self.processor.advance(&mut self.term, bytes);
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_ref(),
        );
        self.revision += 1;
        let (c, r) = (cols_of(&self.term), rows_of(&self.term));
        self.publish_current(CaptureReason::Poll, c, r);
    }

    /// Record clean reader EOF (LIFE-3).
    pub(crate) fn handle_eof(&mut self) {
        if self.finalized {
            return;
        }
        self.eof = true;
    }

    /// Record a reader error distinctly from EOF (LIFE-3): evidence only,
    /// never a teardown failure. The drain still completes — via EOF if it
    /// arrives, else via the drain grace — and the reaped exit status stays
    /// authoritative for the exit cause.
    pub(crate) fn handle_read_error(&mut self, msg: &str) {
        self.shared.record_read_error(msg);
    }

    /// Answer one observe request with a fresh manual observation.
    pub(crate) fn handle_observe(&mut self, reply: &mpsc::Sender<Result<Observation, TuiError>>) {
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_ref(),
        );
        let (c, r) = (cols_of(&self.term), rows_of(&self.term));
        let obs = build_observation(
            &self.term,
            &self.events,
            self.revision,
            CaptureReason::Manual,
            self.pid,
            c,
            r,
        );
        match obs {
            Ok(obs) => {
                self.shared.publish(obs.clone(), None);
                // The requester may have timed out; then the reply is moot
                // but the freshly published observation still serves later
                // waits.
                if reply.send(Ok(obs)).is_err() {
                    // Requester gone; the published observation stands.
                }
            }
            Err(e) => {
                if reply.send(Err(e)).is_err() {
                    // Requester gone; nothing further to report.
                }
            }
        }
    }

    /// Apply one input through the emulator; focus also publishes. True
    /// requests loop exit (a shutdown arrived while the write was in
    /// flight and was serviced instead of the reply).
    pub(crate) fn handle_input(
        &mut self,
        input: &Input,
        reply: &mpsc::Sender<Result<(), TuiError>>,
        op_rx: &mpsc::Receiver<Op>,
        ctl_rx: &mpsc::Receiver<CtlOp>,
    ) -> bool {
        let r = match self.apply_encoded(input, op_rx, ctl_rx) {
            Ok(true) => return true,
            Ok(false) => Ok(()),
            Err(e) => Err(e),
        };
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_ref(),
        );
        if let Input::Focus(focused) = input {
            // Focus is emulator state too: record it and publish so
            // waits can observe the round-trip.
            self.term.is_focused = *focused;
            self.revision += 1;
            let (c, r) = (cols_of(&self.term), rows_of(&self.term));
            self.publish_current(CaptureReason::Input, c, r);
        }
        // The requester may have timed out; input was still applied.
        if reply.send(r).is_err() {
            // Requester gone; the applied input stands.
        }
        false
    }

    /// Resize PTY + emulator, publishing on success.
    pub(crate) fn handle_resize(
        &mut self,
        cols: u16,
        rows: u16,
        reply: &mpsc::Sender<Result<(), TuiError>>,
    ) {
        let r = apply_resize(self.master.as_ref(), &mut self.term, cols, rows);
        if r.is_ok() {
            self.revision += 1;
            self.publish_current(CaptureReason::Resize, cols, rows);
        }
        // The requester may have timed out; the resize (if applied) stands.
        if reply.send(r).is_err() {
            // Requester gone; the resize outcome stands.
        }
    }

    /// Drop the PTY writer (stdin EOF) unless already gone.
    pub(crate) fn handle_close_input(&mut self, reply: &mpsc::Sender<Result<(), TuiError>>) {
        let outcome = if self.exited.is_some() {
            Err(TuiError::ChildExited("child already exited".to_string()))
        } else if let Some(w) = self.writer.take() {
            w.close_input()
        } else {
            Err(TuiError::Closed("stdin already closed".to_string()))
        };
        // The requester may have timed out; stdin state already changed.
        if reply.send(outcome).is_err() {
            // Requester gone; the stdin outcome stands.
        }
    }

    /// Shut down: kill + reap the child, publish the final revision once,
    /// and mark the session closed. The caller exits the loop after this.
    pub(crate) fn handle_shutdown(&mut self, op_rx: &mpsc::Receiver<Op>) {
        shutdown_child(&mut self.child, &self.shared);
        if let Some(status) = poll_child(&mut self.child) {
            self.exited = Some(ExitStatus::from(status));
        }
        if !self.finalized {
            self.finalized = true;
            let status = self.exited.clone().unwrap_or(ExitStatus {
                code: 1,
                signal: Some("unknown".to_string()),
            });
            self.publish_exit(status, DrainCause::Shutdown);
        }
        self.shared.mark_closed();
        // Drain the op queue (LIFE-7): shutdown jumps the queue, so queued
        // `Feed` batches and the reader's final send would otherwise block
        // the reader thread forever on a full queue once the worker exits.
        // Teardown drops its sender before requesting shutdown, so only
        // the reader still sends; its exit disconnects and ends the drain.
        let deadline = Instant::now() + KILL_GRACE;
        loop {
            match op_rx.recv_timeout(WORKER_TICK) {
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if Instant::now() >= deadline {
                        return;
                    }
                }
            }
        }
    }

    /// Handle + reader both gone: reap and go away quietly.
    pub(crate) fn handle_disconnect(&mut self) {
        shutdown_child(&mut self.child, &self.shared);
        self.shared.mark_closed();
    }

    /// Exit polling: reap promptly, but give trailing output `DRAIN_GRACE`
    /// after the child dies before publishing the final revision. The
    /// recorded drain cause keeps EOF distinct from grace expiry (LIFE-8):
    /// only clean reader EOF reports [`DrainCause::Eof`].
    pub(crate) fn poll_exit_progress(&mut self) {
        if self.finalized {
            return;
        }
        if self.exited.is_none()
            && let Some(status) = poll_child(&mut self.child)
        {
            self.exited = Some(ExitStatus::from(status));
            self.exit_seen_at = Some(Instant::now());
        }
        let grace_expired = self
            .exit_seen_at
            .is_some_and(|t| t.elapsed() >= DRAIN_GRACE);
        if self.exited.is_some() && (self.eof || grace_expired) {
            self.finalized = true;
            let status = self.exited.clone().unwrap_or(ExitStatus {
                code: 0,
                signal: None,
            });
            let drain = if self.eof {
                DrainCause::Eof
            } else {
                DrainCause::GraceExpiry
            };
            self.publish_exit(status, drain);
        }
    }
}
