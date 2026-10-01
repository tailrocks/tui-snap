//! PTY open + child spawn with rollback on partial failure (LIFE-4).
//!
//! Every step after the child exists can fail (reader/writer handles, the
//! initial poll, any thread spawn); each failure kills and reaps the child
//! before the error propagates, so a failed spawn never orphans a live
//! child.

use std::sync::{Arc, mpsc};
use std::time::Instant;

use termpane::process::SpawnParams;
use termpane::pty::{Master, PtyChild, PtyReader, PtyWriter, spawn_pty};

use super::capture::run_reader;
use super::encode::spawn_writer_thread;
use super::error::TuiError;
use super::limits::{CTL_QUEUE_LIMIT, KILL_GRACE, OP_QUEUE_LIMIT, PTY_LIFECYCLE};
use super::shared::Shared;
use super::worker::{CtlOp, Op, WorkerParams, run_worker};

/// An opened PTY pair with the child spawned and I/O handles taken.
pub(crate) struct SpawnedPty {
    pub(crate) master: Master,
    pub(crate) child: PtyChild,
    pub(crate) reader: PtyReader,
    pub(crate) writer: PtyWriter,
    pub(crate) pid: Option<u32>,
}

/// Open the PTY, spawn the child, and take I/O handles — all under the
/// process-global lifecycle guard, released before the threads start.
/// Any failure after the spawn rolls the child back (kill + bounded reap)
/// and reports the original error.
pub(crate) fn spawn_pty_child(
    params: &SpawnParams,
    cols: u16,
    rows: u16,
) -> Result<SpawnedPty, TuiError> {
    let guard = PTY_LIFECYCLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // One call: open + spawn + parent-slave-drop under the backend's own
    // lifecycle lock (nested inside ours; the order never inverts, so no
    // deadlock). Sizes are (cols, rows) here.
    let (master, mut child) =
        spawn_pty(params, cols, rows).map_err(|e| TuiError::Spawn(format!("spawn failed: {e}")))?;
    // Drain discipline: take I/O handles before any wait can run. Each
    // fallible step rolls back: kill + reap the child, then report.
    let reader = match master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            let rb = rollback_child_inner(&mut child);
            return Err(spawn_failed(format!("pty reader failed: {e}"), rb));
        }
    };
    let writer = match master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            let rb = rollback_child_inner(&mut child);
            return Err(spawn_failed(format!("pty writer failed: {e}"), rb));
        }
    };
    if let Err(e) = child.try_wait() {
        let rb = rollback_child_inner(&mut child);
        return Err(spawn_failed(format!("child poll failed: {e}"), rb));
    }
    let pid = child.pid();
    drop(guard);
    Ok(SpawnedPty {
        master,
        child,
        reader,
        writer,
        pid,
    })
}

/// Roll back a spawned-but-unowned child: kill plus a bounded reap under
/// the lifecycle guard (released while sleeping, as in teardown).
/// Returns a diagnostic suffix when the child may still be alive; `None`
/// when the rollback verifiably reaped or the child was already gone.
pub(crate) fn rollback_child(mut child: PtyChild) -> Option<String> {
    rollback_child_inner(&mut child)
}

fn rollback_child_inner(child: &mut PtyChild) -> Option<String> {
    let pid = child.pid();
    {
        let _guard = PTY_LIFECYCLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A kill failure against an already-exited child is routine; the
        // reap below decides whether anything is actually orphaned.
        if child.kill().is_err() {
            // Kill failed; the reap below is authoritative.
        }
    }
    let deadline = Instant::now() + KILL_GRACE;
    loop {
        {
            let _guard = PTY_LIFECYCLE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return None,
                Ok(None) => {}
            }
        }
        if Instant::now() >= deadline {
            return Some(match pid {
                Some(p) => format!("rollback: child {p} still alive after kill grace"),
                None => "rollback: child still alive after kill grace".to_string(),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Roll back a worker that never started (LIFE-4): kill + reap the child,
/// drop the writer handle so the writer thread exits, and reap it. The
/// reader was never spawned; the channel ends drop with `params`.
fn rollback_prespawn(
    params: WorkerParams,
    reader: PtyReader,
    writer_thread: std::thread::JoinHandle<()>,
) {
    let WorkerParams {
        master,
        child,
        writer,
        ..
    } = params;
    rollback_child(child);
    drop(writer);
    join_quiet(writer_thread);
    drop(master);
    drop(reader);
}

/// A running session's channels and threads, before the handle exists.
pub(crate) struct StartedThreads {
    pub(crate) op_tx: mpsc::SyncSender<Op>,
    pub(crate) ctl_tx: mpsc::SyncSender<CtlOp>,
    pub(crate) worker: std::thread::JoinHandle<()>,
    pub(crate) reader_thread: std::thread::JoinHandle<()>,
    pub(crate) writer_thread: std::thread::JoinHandle<()>,
}

/// Start the writer, worker, and reader threads for a spawned child.
/// Every failure rolls back (kill + reap + reap threads), so the caller
/// either gets a complete running session or a clean error (LIFE-4).
pub(crate) fn start_session_threads(
    spawned: SpawnedPty,
    cols: u16,
    rows: u16,
    shared: Arc<Shared>,
) -> Result<StartedThreads, TuiError> {
    let SpawnedPty {
        master,
        child,
        reader,
        writer,
        pid,
    } = spawned;
    // Bounded (F12): a flooding child blocks the reader on a full
    // queue — backpressure through the PTY, like a real terminal —
    // instead of piling unbounded `Feed` batches in memory.
    let (op_tx, op_rx) = mpsc::sync_channel::<Op>(OP_QUEUE_LIMIT);
    let (ctl_tx, ctl_rx) = mpsc::sync_channel::<CtlOp>(CTL_QUEUE_LIMIT);

    // The writer thread owns the raw PTY writer (LIFE-6); the worker
    // only holds a request handle, so it never blocks in `write_all`.
    let (write_handle, writer_thread) = match spawn_writer_thread(Box::new(writer)) {
        Ok(pair) => pair,
        Err(msg) => {
            rollback_child(child);
            drop(reader);
            drop(master);
            return Err(TuiError::Spawn(msg));
        }
    };

    let params = WorkerParams {
        master,
        child,
        writer: write_handle,
        cols,
        rows,
        pid,
        op_rx,
        ctl_rx,
        shared,
    };
    // Handover channel: the child moves into the worker only after the
    // spawn succeeds. A failed `spawn` drops its closure — moving
    // `params` into the closure directly would drop the child handle
    // without kill/reap, orphaning a live child (LIFE-4).
    let (params_tx, params_rx) = mpsc::channel::<WorkerParams>();
    let worker = std::thread::Builder::new()
        .name("tuiscotti-tui-worker".to_string())
        .spawn(move || {
            if let Ok(p) = params_rx.recv() {
                run_worker(p);
            }
        });
    let worker = match worker {
        Ok(w) => w,
        Err(e) => {
            rollback_prespawn(params, reader, writer_thread);
            return Err(TuiError::Spawn(format!("worker spawn failed: {e}")));
        }
    };
    // The only send failure is an instantly-dead worker thread; the
    // `SendError` hands `params` back for the same rollback.
    if let Err(not_sent) = params_tx.send(params) {
        rollback_prespawn(not_sent.0, reader, writer_thread);
        join_quiet(worker);
        return Err(TuiError::Spawn("worker died before startup".to_string()));
    }

    let feed_tx = op_tx.clone();
    let reader_thread = std::thread::Builder::new()
        .name("tuiscotti-tui-reader".to_string())
        .spawn(move || run_reader(Box::new(reader), &feed_tx));
    let reader_thread = match reader_thread {
        Ok(t) => t,
        Err(e) => {
            // The worker is already running and owns the child: drop
            // every sender so it observes disconnect, reaps the child,
            // and exits; then reap both threads by hand. (`feed_tx`
            // died with the failed spawn closure, which owned it.)
            drop(op_tx);
            drop(ctl_tx);
            join_quiet(worker);
            join_quiet(writer_thread);
            return Err(TuiError::Spawn(format!("reader spawn failed: {e}")));
        }
    };
    Ok(StartedThreads {
        op_tx,
        ctl_tx,
        worker,
        reader_thread,
        writer_thread,
    })
}

/// Join a thread on a rollback path: a panic there is already moot (the
/// `Spawn` error under construction is authoritative), so only the
/// reaping matters.
fn join_quiet(h: std::thread::JoinHandle<()>) {
    if h.join().is_err() {
        // Panicked on a rollback path; the Spawn error is authoritative.
    }
}

/// The original spawn error, with the rollback diagnostic appended when
/// the child may have survived (honest, still a `Spawn` error).
fn spawn_failed(mut msg: String, rollback: Option<String>) -> TuiError {
    if let Some(rb) = rollback {
        msg.push_str("; ");
        msg.push_str(&rb);
    }
    TuiError::Spawn(msg)
}
