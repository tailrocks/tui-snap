//! Worker thread: sole owner of `Term`, writer, and child.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::term::{Config as TermConfig, Term};
use portable_pty::{Child as PtyChild, MasterPty};
use tuiscotti_core::locate::{Locator, Span};
use tuiscotti_core::screen::Observation;

use crate::bound_locator::ActionError;

use super::error::TuiError;
use super::input_types::{Key, KeyEventKind, KeyMods, MouseButton, MouseMods, Signal, Wheel};
use super::limits::{COALESCE_BYTES, KILL_GRACE, PTY_LIFECYCLE, WORKER_TICK};
use super::shared::Shared;
use super::worker_ctx::WorkerCtx;

#[derive(Debug)]
pub(crate) enum MouseAction {
    Press(MouseButton),
    // Release carries no button: every encoding reports release as 3.
    Release,
    Move { held: Option<MouseButton> },
    Wheel(Wheel),
}

#[derive(Debug)]
pub(crate) enum Input {
    Bytes(Vec<u8>),
    Paste(String),
    Key {
        key: Key,
        mods: KeyMods,
        kind: KeyEventKind,
    },
    Mouse {
        action: MouseAction,
        x: u16,
        y: u16,
        mods: MouseMods,
    },
    Focus(bool),
}

pub(crate) enum Op {
    Feed(Vec<u8>),
    /// Clean reader EOF (`read` returned 0). Read errors travel as
    /// [`Op::ReadError`] instead — never conflated (LIFE-3).
    Eof,
    /// The reader died with an error (e.g. Linux EIO after child death).
    /// Recorded as evidence; the reaped exit status stays authoritative.
    ReadError(String),
    Observe {
        reply: mpsc::Sender<Result<Observation, TuiError>>,
    },
    Input {
        input: Input,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    /// Resolve a locator + deliver one click as a single worker step (F11):
    /// the worker builds ONE fresh observation, resolves the UNIQUE viewport
    /// target at its own current revision, and applies press + release with
    /// no interleaving op — never two reads plus a separate unchecked click.
    ClickTarget {
        locator: Locator,
        button: MouseButton,
        mods: MouseMods,
        reply: mpsc::Sender<Result<Span, ActionError>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
}

/// Priority control ops (LIFE-7): stdin close, signal delivery, and
/// shutdown bypass the mixed op queue on a dedicated channel, so they stay
/// serviceable under flood or while a write is stuck.
pub(crate) enum CtlOp {
    CloseInput {
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    Signal {
        signal: Signal,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    Shutdown,
}

pub(crate) struct WorkerDims {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
}

impl GridDims for WorkerDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone)]
pub(crate) struct QueueListener {
    tx: mpsc::Sender<Event>,
}

impl EventListener for QueueListener {
    fn send_event(&self, event: Event) {
        // The worker owns the receiver and outlives the emulator, so this
        // only fails during teardown races; the event is then moot.
        if self.tx.send(event).is_err() {
            // Worker gone; the event has no consumer.
        }
    }
}

pub(crate) struct WorkerEventState {
    pub(crate) title: Option<String>,
    pub(crate) bells: u64,
}

impl WorkerEventState {
    pub(crate) fn new() -> Self {
        Self {
            title: None,
            bells: 0,
        }
    }
}

/// Owned worker inputs: PTY handles, emulator config, and channels.
pub(crate) struct WorkerParams {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) child: Box<dyn PtyChild + Send + Sync>,
    pub(crate) writer: super::encode::WriteHandle,
    pub(crate) term_config: TermConfig,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) pid: Option<u32>,
    pub(crate) op_rx: mpsc::Receiver<Op>,
    pub(crate) ctl_rx: mpsc::Receiver<CtlOp>,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) fn run_worker(p: WorkerParams) {
    let WorkerParams {
        master,
        child,
        writer,
        term_config,
        cols,
        rows,
        pid,
        op_rx,
        ctl_rx,
        shared,
    } = p;
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let dims = WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    };
    let term = Term::new(term_config, &dims, QueueListener { tx: event_tx });
    let mut ctx = WorkerCtx::new(term, event_rx, writer, pid, shared, master, child);
    ctx.publish_initial(cols, rows);

    loop {
        // Control first (LIFE-7): shutdown/close/signal never wait behind
        // a flooded op queue.
        if ctx.service_ctl(&ctl_rx, &op_rx) {
            return;
        }
        match op_rx.recv_timeout(WORKER_TICK) {
            Ok(op) => {
                if dispatch(&mut ctx, op, &op_rx, &ctl_rx) {
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                ctx.handle_disconnect();
                return;
            }
        }
        if ctx.service_ctl(&ctl_rx, &op_rx) {
            return;
        }
        ctx.poll_exit_progress();
    }
}

/// Dispatch one op; returns true when the loop must exit.
fn dispatch(
    ctx: &mut WorkerCtx,
    op: Op,
    op_rx: &mpsc::Receiver<Op>,
    ctl_rx: &mpsc::Receiver<CtlOp>,
) -> bool {
    match op {
        Op::Feed(bytes) => {
            // Coalesce (F12): merge immediately-pending `Feed` batches up
            // to `COALESCE_BYTES` into ONE emulator advance, so a flood
            // costs one grid build per cap instead of one per batch. A
            // non-`Feed` op met while draining runs right after the merged
            // advance, preserving channel order.
            let mut merged = bytes;
            let mut pending: Option<Op> = None;
            while merged.len() < COALESCE_BYTES {
                match op_rx.try_recv() {
                    Ok(Op::Feed(more)) => merged.extend_from_slice(&more),
                    Ok(other) => {
                        pending = Some(other);
                        break;
                    }
                    Err(_) => break,
                }
            }
            ctx.handle_feed(&merged);
            match pending {
                Some(next) => dispatch_one(ctx, next, op_rx, ctl_rx),
                None => false,
            }
        }
        other => dispatch_one(ctx, other, op_rx, ctl_rx),
    }
}

/// Dispatch one op without coalescing; true requests loop exit.
fn dispatch_one(
    ctx: &mut WorkerCtx,
    op: Op,
    op_rx: &mpsc::Receiver<Op>,
    ctl_rx: &mpsc::Receiver<CtlOp>,
) -> bool {
    match op {
        Op::Feed(bytes) => {
            ctx.handle_feed(&bytes);
            false
        }
        Op::Eof => {
            ctx.handle_eof();
            false
        }
        Op::ReadError(msg) => {
            ctx.handle_read_error(&msg);
            false
        }
        Op::Observe { reply } => {
            ctx.handle_observe(&reply);
            false
        }
        Op::Input { input, reply } => ctx.handle_input(&input, &reply, op_rx, ctl_rx),
        Op::ClickTarget {
            locator,
            button,
            mods,
            reply,
        } => ctx.handle_click_target(&locator, button, mods, &reply, op_rx, ctl_rx),
        Op::Resize { cols, rows, reply } => {
            ctx.handle_resize(cols, rows, &reply);
            false
        }
    }
}

pub(crate) fn cols_of<T: EventListener>(term: &Term<T>) -> u16 {
    u16::try_from(term.columns().min(usize::from(u16::MAX))).unwrap_or(u16::MAX)
}

pub(crate) fn rows_of<T: EventListener>(term: &Term<T>) -> u16 {
    u16::try_from(term.screen_lines().min(usize::from(u16::MAX))).unwrap_or(u16::MAX)
}

/// Best-effort child poll under the lifecycle guard.
pub(crate) fn poll_child(
    child: &mut Box<dyn PtyChild + Send + Sync>,
) -> Option<portable_pty::ExitStatus> {
    let _guard = PTY_LIFECYCLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    child.try_wait().unwrap_or(None)
}

/// Bounded kill + reap. Each child operation runs under the lifecycle
/// guard; the guard is released while sleeping so teardown never blocks an
/// unrelated session's spawn. Records teardown errors instead of failing.
pub(crate) fn shutdown_child(child: &mut Box<dyn PtyChild + Send + Sync>, shared: &Shared) {
    {
        let _guard = PTY_LIFECYCLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(e) => {
                shared.record_teardown(&format!("child poll during teardown failed: {e}"));
            }
        }
        if let Err(e) = child.kill() {
            shared.record_teardown(&format!("child kill during teardown failed: {e}"));
        }
    }
    let deadline = Instant::now() + KILL_GRACE;
    loop {
        {
            let _guard = PTY_LIFECYCLE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(e) => {
                    shared.record_teardown(&format!("child reap during teardown failed: {e}"));
                    return;
                }
            }
        }
        if Instant::now() >= deadline {
            shared.record_teardown("child still alive after kill grace");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
