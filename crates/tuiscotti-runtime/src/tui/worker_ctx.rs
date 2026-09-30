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

use super::capture::{drain_term_events, publish_exit};
use super::encode::{apply_input, apply_resize};
use super::error::TuiError;
use super::exit::ExitStatus;
use super::frame::build_observation;
use super::limits::DRAIN_GRACE;
use super::shared::Shared;
use super::worker::{
    Input, QueueListener, WorkerEventState, cols_of, poll_child, rows_of, shutdown_child,
};

mod click;

/// Owned worker state: emulator, writer, child, and publication progress.
/// Lives only on the worker thread.
pub(crate) struct WorkerCtx {
    term: Term<QueueListener>,
    processor: Processor,
    writer: Option<Box<dyn std::io::Write + Send>>,
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
        writer: Box<dyn std::io::Write + Send>,
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

    /// Drain terminal events, then publish the current state at `revision`.
    fn publish_current(&mut self, reason: CaptureReason, cols: u16, rows: u16) {
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
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

    /// Feed one reader batch through the emulator and publish.
    pub(crate) fn handle_feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
        );
        self.revision += 1;
        let (c, r) = (cols_of(&self.term), rows_of(&self.term));
        self.publish_current(CaptureReason::Poll, c, r);
    }

    /// Record reader EOF; a read error here is informational only.
    pub(crate) fn handle_eof(&mut self, read_err: Option<&str>) {
        self.eof = true;
        if read_err.is_some() {
            // A read error at EOF (e.g. Linux EIO after child death) is
            // informational; the exit status is authoritative.
        }
    }

    /// Answer one observe request with a fresh manual observation.
    pub(crate) fn handle_observe(&mut self, reply: &mpsc::Sender<Result<Observation, TuiError>>) {
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
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

    /// Apply one input through the emulator; focus also publishes.
    pub(crate) fn handle_input(
        &mut self,
        input: &Input,
        reply: &mpsc::Sender<Result<(), TuiError>>,
    ) {
        let r = apply_input(
            &mut self.term,
            input,
            self.writer.as_deref_mut(),
            self.exited.is_some(),
        );
        drain_term_events(
            &mut self.term,
            &self.event_rx,
            &mut self.events,
            self.writer.as_deref_mut(),
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
        } else if self.writer.take().is_some() {
            Ok(())
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
    pub(crate) fn handle_shutdown(&mut self) {
        shutdown_child(&mut self.child, &self.shared);
        if let Some(status) = poll_child(&mut self.child) {
            self.exited = Some(ExitStatus::from(status));
        }
        if !self.finalized {
            self.revision += 1;
            let status = self.exited.clone().unwrap_or(ExitStatus {
                code: 1,
                signal: Some("unknown".to_string()),
            });
            publish_exit(
                &mut self.term,
                &mut self.events,
                &self.event_rx,
                &self.shared,
                self.revision,
                self.pid,
                status,
            );
        }
        self.shared.mark_closed();
    }

    /// Handle + reader both gone: reap and go away quietly.
    pub(crate) fn handle_disconnect(&mut self) {
        shutdown_child(&mut self.child, &self.shared);
        self.shared.mark_closed();
    }

    /// Exit polling: reap promptly, but give trailing output `DRAIN_GRACE`
    /// after the child dies before publishing the final revision.
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
        let drained = self.eof
            || self
                .exit_seen_at
                .is_some_and(|t| t.elapsed() >= DRAIN_GRACE);
        if self.exited.is_some() && drained {
            self.finalized = true;
            self.revision += 1;
            let status = self.exited.clone().unwrap_or(ExitStatus {
                code: 0,
                signal: None,
            });
            publish_exit(
                &mut self.term,
                &mut self.events,
                &self.event_rx,
                &self.shared,
                self.revision,
                self.pid,
                status,
            );
        }
    }
}
