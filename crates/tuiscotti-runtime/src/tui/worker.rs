//! Worker thread: sole owner of `Term`, writer, and child.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::term::{Config as TermConfig, Term};
use portable_pty::{Child as PtyChild, MasterPty};
use tuiscotti_core::screen::Observation;

use super::error::TuiError;
use super::input_types::{Key, KeyEventKind, KeyMods, MouseButton, MouseMods, Wheel};
use super::limits::{KILL_GRACE, PTY_LIFECYCLE, WORKER_TICK};
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
    Eof(Option<String>),
    Observe {
        reply: mpsc::Sender<Result<Observation, TuiError>>,
    },
    Input {
        input: Input,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    CloseInput {
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

pub(crate) fn run_worker(
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn PtyChild + Send + Sync>,
    writer: Box<dyn std::io::Write + Send>,
    term_config: TermConfig,
    cols: u16,
    rows: u16,
    pid: Option<u32>,
    op_rx: mpsc::Receiver<Op>,
    shared: Arc<Shared>,
) {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let dims = WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    };
    let term = Term::new(term_config, &dims, QueueListener { tx: event_tx });
    let mut ctx = WorkerCtx::new(term, event_rx, writer, pid, shared, master, child);
    ctx.publish_initial(cols, rows);

    loop {
        match op_rx.recv_timeout(WORKER_TICK) {
            Ok(Op::Feed(bytes)) => ctx.handle_feed(bytes),
            Ok(Op::Eof(read_err)) => ctx.handle_eof(read_err),
            Ok(Op::Observe { reply }) => ctx.handle_observe(reply),
            Ok(Op::Input { input, reply }) => ctx.handle_input(input, reply),
            Ok(Op::Resize { cols, rows, reply }) => ctx.handle_resize(cols, rows, reply),
            Ok(Op::CloseInput { reply }) => ctx.handle_close_input(reply),
            Ok(Op::Shutdown) => {
                ctx.handle_shutdown();
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                ctx.handle_disconnect();
                return;
            }
        }
        ctx.poll_exit_progress();
    }
}

pub(crate) fn cols_of<T: EventListener>(term: &Term<T>) -> u16 {
    term.columns().min(u16::MAX as usize) as u16
}

pub(crate) fn rows_of<T: EventListener>(term: &Term<T>) -> u16 {
    term.screen_lines().min(u16::MAX as usize) as u16
}

/// Best-effort child poll under the lifecycle guard.
pub(crate) fn poll_child(
    child: &mut Box<dyn PtyChild + Send + Sync>,
) -> Option<portable_pty::ExitStatus> {
    let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    child.try_wait().unwrap_or(None)
}

/// Bounded kill + reap. Each child operation runs under the lifecycle
/// guard; the guard is released while sleeping so teardown never blocks an
/// unrelated session's spawn. Records teardown errors instead of failing.
pub(crate) fn shutdown_child(child: &mut Box<dyn PtyChild + Send + Sync>, shared: &Shared) {
    {
        let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
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
            let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
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
