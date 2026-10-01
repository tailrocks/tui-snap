//! Control servicing and encoded-input writes for [`WorkerCtx`](super::WorkerCtx).
//!
//! One impl block moved out of `worker_ctx.rs` so both files stay under the
//! repo line gate. Control ops (`CloseInput`/`Signal`/`Shutdown`) arrive on
//! a dedicated channel and are serviced ahead of the op queue — including
//! while an acknowledged PTY write is outstanding (LIFE-6/LIFE-7).

use std::sync::mpsc;
use std::time::Instant;

use super::super::encode::{WriteReq, encode_input};
use super::super::error::TuiError;
use super::super::limits::{WORKER_TICK, WRITE_TIMEOUT};
use super::super::worker::{CtlOp, Input, Op};
use super::WorkerCtx;

impl WorkerCtx {
    /// Service every pending control op; true requests loop exit (a
    /// shutdown was handled). Never blocks.
    pub(crate) fn service_ctl(
        &mut self,
        ctl_rx: &mpsc::Receiver<CtlOp>,
        op_rx: &mpsc::Receiver<Op>,
    ) -> bool {
        while let Ok(op) = ctl_rx.try_recv() {
            match op {
                CtlOp::CloseInput { reply } => self.handle_close_input(&reply),
                CtlOp::Signal { signal, reply } => self.handle_signal(signal, &reply),
                CtlOp::Shutdown => {
                    self.handle_shutdown(op_rx);
                    return true;
                }
            }
        }
        false
    }

    /// Encode one input and write it through the writer thread. `Ok(false)`
    /// means applied (or a refused write, as `Err`); `Ok(true)` means a
    /// shutdown arrived mid-write, was serviced, and the loop must exit.
    pub(crate) fn apply_encoded(
        &mut self,
        input: &Input,
        op_rx: &mpsc::Receiver<Op>,
        ctl_rx: &mpsc::Receiver<CtlOp>,
    ) -> Result<bool, TuiError> {
        if self.exited.is_some() {
            return Err(TuiError::ChildExited("child already exited".to_string()));
        }
        let Some(handle) = self.writer.clone() else {
            return Err(TuiError::Closed("stdin is closed".to_string()));
        };
        let bytes = match encode_input(&self.grid, input)? {
            Some(b) if !b.is_empty() => b,
            _ => return Ok(false),
        };
        let (ack_tx, ack_rx) = mpsc::channel();
        let mut req = WriteReq::Bytes {
            bytes,
            reply: ack_tx,
        };
        let deadline = Instant::now() + WRITE_TIMEOUT;
        // Enqueue: the writer is serial and usually idle, so room appears
        // immediately; control stays serviceable while waiting regardless.
        loop {
            match handle.tx.try_send(req) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(TuiError::Closed("writer thread is gone".to_string()));
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    req = returned;
                    if self.service_ctl(ctl_rx, op_rx) {
                        return Ok(true);
                    }
                    if Instant::now() >= deadline {
                        return Err(TuiError::Timeout(
                            "pty write: writer queue stayed full past the write bound".to_string(),
                        ));
                    }
                    std::thread::sleep(WORKER_TICK);
                }
            }
        }
        // Acknowledgement: a child that stops reading stalls the writer
        // thread, never the worker — shutdown still lands within a tick.
        loop {
            match ack_rx.recv_timeout(WORKER_TICK) {
                Ok(Ok(())) => return Ok(false),
                Ok(Err(msg)) => {
                    return Err(TuiError::Io(format!("pty write failed: {msg}")));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(TuiError::Closed("writer thread is gone".to_string()));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.service_ctl(ctl_rx, op_rx) {
                        return Ok(true);
                    }
                    if Instant::now() >= deadline {
                        return Err(TuiError::Timeout(
                            "pty write: writer unresponsive past the write bound".to_string(),
                        ));
                    }
                }
            }
        }
    }
}
