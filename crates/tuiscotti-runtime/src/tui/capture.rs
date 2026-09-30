//! Reader thread, terminal-event drain, and observation publishing.

use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Rgb as VteRgb;
use tuiscotti_core::frame::Rgb;
use tuiscotti_core::screen::CaptureReason;

use super::exit::ExitStatus;
use super::frame::build_observation;
use super::shared::Shared;
use super::worker::{Op, WorkerEventState, cols_of, rows_of};

/// Blocking PTY reads forwarded as ops. The channel is bounded (F12):
/// a flooding child blocks this send — backpressure through the PTY,
/// like a real terminal — instead of queueing unbounded batches.
pub(crate) fn run_reader(mut reader: Box<dyn std::io::Read + Send>, tx: &mpsc::SyncSender<Op>) {
    let mut buf = vec![0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // The worker may be gone already; then EOF is moot.
                if tx.send(Op::Eof(None)).is_err() {
                    // Worker gone; the reader still exits.
                }
                return;
            }
            Ok(n) => {
                if tx.send(Op::Feed(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) => {
                // The worker may be gone already; then EOF is moot.
                if tx.send(Op::Eof(Some(e.to_string()))).is_err() {
                    // Worker gone; the reader still exits.
                }
                return;
            }
        }
    }
}

/// Query replies go back to the PTY; title/bells recorded.
pub(crate) fn drain_term_events<T: EventListener>(
    term: &mut Term<T>,
    event_rx: &mpsc::Receiver<Event>,
    events: &mut WorkerEventState,
    mut writer: Option<&mut (dyn std::io::Write + Send + 'static)>,
) {
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            Event::PtyWrite(text) => {
                if let Some(w) = writer.as_deref_mut() {
                    // A failed reply write means the PTY is gone; the drain
                    // continues so remaining events still update state.
                    if w.write_all(text.as_bytes()).is_err() {
                        // PTY write failed; keep draining.
                    }
                }
            }
            Event::ClipboardLoad(_, respond) => {
                // The harness holds no clipboard: answer honestly empty.
                if let Some(w) = writer.as_deref_mut()
                    && w.write_all(respond("").as_bytes()).is_err()
                {
                    // PTY write failed; keep draining.
                }
            }
            Event::ColorRequest(index, respond) => {
                let rgb = resolve_color(term, index);
                if let Some(w) = writer.as_deref_mut()
                    && w.write_all(respond(rgb).as_bytes()).is_err()
                {
                    // PTY write failed; keep draining.
                }
            }
            Event::TextAreaSizeRequest(respond) => {
                // Headless: grid geometry is exact; cell pixels are nominal.
                let size = WindowSize {
                    num_lines: rows_of(term),
                    num_cols: cols_of(term),
                    cell_width: 8,
                    cell_height: 16,
                };
                if let Some(w) = writer.as_deref_mut()
                    && w.write_all(respond(size).as_bytes()).is_err()
                {
                    // PTY write failed; keep draining.
                }
            }
            Event::ClipboardStore(_, _)
            | Event::MouseCursorDirty
            | Event::Wakeup
            | Event::Exit
            | Event::ChildExit(_)
            | Event::CursorBlinkingChange => {}
        }
    }
}

/// Resolve a palette slot for `OSC 4;n;?` replies: live override, else the
/// xterm default table (documented nominal for fg/bg/cursor).
fn resolve_color<T: EventListener>(term: &Term<T>, index: usize) -> VteRgb {
    if let Some(rgb) = term.colors()[index.min(268)] {
        return rgb;
    }
    let def = |n: u8| {
        let c = Rgb::from_indexed(n);
        VteRgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    };
    match index {
        0..=255 => def(u8::try_from(index).unwrap_or(u8::MAX)),
        256 | 258 => def(7),
        _ => def(0),
    }
}

pub(crate) fn publish_exit<T: EventListener>(
    term: &mut Term<T>,
    events: &mut WorkerEventState,
    event_rx: &mpsc::Receiver<Event>,
    shared: &Shared,
    revision: u64,
    pid: Option<u32>,
    status: ExitStatus,
) {
    // Drain without the writer: replies have nowhere to go, but title and
    // bell state still belong in the final observation.
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            _ => {}
        }
    }
    let cols = cols_of(term);
    let rows = rows_of(term);
    match build_observation(term, events, revision, CaptureReason::Exit, pid, cols, rows) {
        Ok(obs) => shared.publish_exit(status, obs),
        Err(e) => shared.record_teardown(&format!("exit observation build failed: {e}")),
    }
}
