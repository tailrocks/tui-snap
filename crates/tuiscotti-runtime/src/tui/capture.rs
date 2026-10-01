//! Reader thread, grid-event drain, and observation publishing.

use std::sync::mpsc;

#[cfg(unix)]
use termpane::{DamageGrid, PassthroughEvent};

#[cfg(unix)]
use super::encode::WriteHandle;
use super::worker::Op;
#[cfg(unix)]
use super::worker::WorkerEventState;

/// Blocking PTY reads forwarded as ops. The channel is bounded (F12):
/// a flooding child blocks this send — backpressure through the PTY,
/// like a real terminal — instead of queueing unbounded batches.
///
/// Termination causes stay separated (LIFE-3): clean EOF (`Ok(0)`)
/// reports [`Op::Eof`], a read error reports [`Op::ReadError`], and
/// `Interrupted` retries — a signal never masquerades as EOF.
pub(crate) fn run_reader(mut reader: Box<dyn std::io::Read + Send>, tx: &mpsc::SyncSender<Op>) {
    let mut buf = vec![0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // The worker may be gone already; then EOF is moot.
                if tx.send(Op::Eof).is_err() {
                    // Worker gone; the reader still exits.
                }
                return;
            }
            Ok(n) => {
                if tx.send(Op::Feed(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                // The worker may be gone already; then the error is moot.
                if tx.send(Op::ReadError(e.to_string())).is_err() {
                    // Worker gone; the reader still exits.
                }
                return;
            }
        }
    }
}

/// Query replies go back to the PTY; title/bells recorded. Replies are
/// best-effort through the writer thread: a stuck writer drops them
/// rather than stalling the worker (LIFE-6).
#[cfg(unix)]
pub(crate) fn drain_grid_events(
    grid: &mut DamageGrid,
    events: &mut WorkerEventState,
    writer: Option<&WriteHandle>,
) {
    for event in grid.drain_passthrough() {
        match event {
            PassthroughEvent::TitleChanged(t) => {
                // An empty title resets, matching the old backend's
                // reset-on-empty semantics.
                events.title = if t.is_empty() { None } else { Some(t) };
            }
            PassthroughEvent::IconNameChanged(name) => {
                // The old backend folded OSC 1 into the title; keep that.
                events.title = if name.is_empty() { None } else { Some(name) };
            }
            PassthroughEvent::Bell => events.bells += 1,
            PassthroughEvent::Reply(bytes) => {
                if let Some(w) = writer {
                    w.reply_best_effort(&bytes);
                }
            }
            // Clipboard stores are replay-side state only; the live path
            // reports clipboard as `Unsupported`. Clipboard `?` reads stay
            // silent: the old backend denied them by policy (its reply arm
            // never fired), so silence preserves behavior exactly.
            PassthroughEvent::ClipboardWrite(_)
            | PassthroughEvent::CwdChanged(_)
            | PassthroughEvent::Notification(_)
            | PassthroughEvent::ApplicationCursorKeys(_)
            | PassthroughEvent::FocusEvents(_)
            | PassthroughEvent::BracketedPaste(_)
            | PassthroughEvent::Hyperlink { .. }
            | PassthroughEvent::UnhandledCsi(_)
            | PassthroughEvent::DroppedCsi(_)
            // CSI 3J already cleared the grid's own scrollback; the event
            // only informs outer layers with retained history, which the
            // live path has none of.
            | PassthroughEvent::ScrollbackClear => {}
        }
    }
}
