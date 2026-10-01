//! Worker thread: sole owner of the emulator grid, writer, and child.

#[cfg(not(unix))]
use std::sync::mpsc;
#[cfg(unix)]
use std::sync::{Arc, mpsc};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use termpane::process::ExitStatus as TermExitStatus;
#[cfg(unix)]
use termpane::pty::{Master, PtyChild};
use tuiscotti_core::locate::{Locator, Span};
use tuiscotti_core::screen::Observation;

use crate::bound_locator::ActionError;

use super::error::TuiError;
use super::input_types::{Key, KeyEventKind, KeyMods, MouseButton, MouseMods, Signal, Wheel};
#[cfg(unix)]
use super::limits::{COALESCE_BYTES, KILL_GRACE, PTY_LIFECYCLE, WORKER_TICK};
#[cfg(unix)]
use super::shared::Shared;
#[cfg(unix)]
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

#[cfg(unix)]
pub(crate) struct WorkerEventState {
    pub(crate) title: Option<String>,
    pub(crate) bells: u64,
}

#[cfg(unix)]
impl WorkerEventState {
    pub(crate) fn new() -> Self {
        Self {
            title: None,
            bells: 0,
        }
    }
}

/// Owned worker inputs: PTY handles and channels.
#[cfg(unix)]
pub(crate) struct WorkerParams {
    pub(crate) master: Master,
    pub(crate) child: PtyChild,
    pub(crate) writer: super::encode::WriteHandle,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) pid: Option<u32>,
    pub(crate) op_rx: mpsc::Receiver<Op>,
    pub(crate) ctl_rx: mpsc::Receiver<CtlOp>,
    pub(crate) shared: Arc<Shared>,
}

#[cfg(unix)]
pub(crate) fn run_worker(p: WorkerParams) {
    let WorkerParams {
        master,
        child,
        writer,
        cols,
        rows,
        pid,
        op_rx,
        ctl_rx,
        shared,
    } = p;
    let grid = termpane::DamageGrid::new(rows, cols, LIVE_SCROLLBACK);
    let mut ctx = WorkerCtx::new(grid, writer, pid, shared, master, child);
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

/// Scrollback rows retained by the live emulator. The live path never
/// reads scrollback (snapshots report it as `Unsupported`); 10000
/// preserves the old live backend's default.
#[cfg(unix)]
const LIVE_SCROLLBACK: usize = 10_000;

/// Dispatch one op; returns true when the loop must exit.
#[cfg(unix)]
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
#[cfg(unix)]
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

/// Best-effort child poll under the lifecycle guard.
#[cfg(unix)]
pub(crate) fn poll_child(child: &mut PtyChild) -> Option<TermExitStatus> {
    let _guard = PTY_LIFECYCLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    child.try_wait().unwrap_or(None)
}

/// Bounded kill + reap. Each child operation runs under the lifecycle
/// guard; the guard is released while sleeping so teardown never blocks an
/// unrelated session's spawn. Records teardown errors instead of failing.
#[cfg(unix)]
pub(crate) fn shutdown_child(child: &mut PtyChild, shared: &Shared) {
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
