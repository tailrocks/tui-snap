//! Input encoding, worker side: bytes derived from the live `TermMode`.
//!
//! Writes never run on the worker (LIFE-6): a dedicated writer thread owns
//! the raw PTY writer, so a child that stops reading wedges only that
//! thread. The worker sends acknowledged write requests with a bound and
//! keeps servicing control ops while waiting.

use std::sync::mpsc;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::term::{Term, TermMode};
use portable_pty::{MasterPty, PtySize};

use super::encode_key::encode_key;
use super::error::TuiError;
use super::input_types::{MouseButton, MouseMods, Wheel};
use super::limits::WRITE_QUEUE_LIMIT;
use super::worker::{Input, MouseAction, WorkerDims};

/// One writer-thread request. `Bytes` is acknowledged (the worker waits
/// with a bound); `Reply` is best-effort (dropped when the queue is full,
/// so query replies never stall the worker); `Close` drops the PTY writer
/// (stdin EOF) after all earlier requests complete.
pub(crate) enum WriteReq {
    Bytes {
        bytes: Vec<u8>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Reply(Vec<u8>),
    Close,
}

/// Worker-side handle to the writer thread. Cloneable; the writer thread
/// exits once every handle is dropped (or is detached with a diagnostic
/// when stuck in a write — see teardown).
#[derive(Clone)]
pub(crate) struct WriteHandle {
    pub(crate) tx: mpsc::SyncSender<WriteReq>,
}

impl WriteHandle {
    /// Best-effort terminal-query reply: never blocks the worker.
    pub(crate) fn reply_best_effort(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // Full queue means a stuck writer; dropping one reply keeps the
        // worker responsive, which is the point of the split.
        if self.tx.try_send(WriteReq::Reply(bytes.to_vec())).is_err() {
            // Writer stuck or gone; the reply is moot.
        }
    }

    /// Enqueue stdin close (stdin EOF once earlier writes complete).
    /// Spins briefly: the worker is serial, so at close time no write of
    /// ours is outstanding and room appears as soon as the writer drains.
    pub(crate) fn close_input(&self) -> Result<(), TuiError> {
        let mut req = WriteReq::Close;
        for _ in 0..1000 {
            match self.tx.try_send(req) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(TuiError::Closed("writer thread is gone".to_string()));
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    req = returned;
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        Err(TuiError::Timeout(
            "close stdin: writer queue stayed full".to_string(),
        ))
    }
}

/// Spawn the writer thread owning `writer`. The thread serves requests in
/// order until every [`WriteHandle`] is dropped.
pub(crate) fn spawn_writer_thread(
    writer: Box<dyn std::io::Write + Send>,
) -> Result<(WriteHandle, std::thread::JoinHandle<()>), String> {
    let (tx, rx) = mpsc::sync_channel::<WriteReq>(WRITE_QUEUE_LIMIT);
    let thread = std::thread::Builder::new()
        .name("tuiscotti-tui-writer".to_string())
        .spawn(move || run_writer(writer, &rx))
        .map_err(|e| format!("writer spawn failed: {e}"))?;
    Ok((WriteHandle { tx }, thread))
}

fn run_writer(mut writer: Box<dyn std::io::Write + Send>, rx: &mpsc::Receiver<WriteReq>) {
    let mut open = true;
    for req in rx {
        match req {
            WriteReq::Bytes { bytes, reply } => {
                let r = if open {
                    writer.write_all(&bytes).map_err(|e| e.to_string())
                } else {
                    Err("stdin is closed".to_string())
                };
                // The worker may have timed out waiting; the write outcome
                // still stands, and the reply is then moot.
                if reply.send(r).is_err() {
                    // Worker moved on; the write outcome stands.
                }
            }
            WriteReq::Reply(bytes) => {
                if open && writer.write_all(&bytes).is_err() {
                    // A failed reply write means the PTY is gone; later
                    // requests report it through their own outcomes.
                }
            }
            WriteReq::Close => {
                open = false;
                // Drop the raw writer now: EOF to the child even though the
                // thread itself lives until the handles drop. Assignment
                // drops the old writer; the sink is never written (`open`
                // gates every path above).
                writer = Box::new(std::io::sink());
            }
        }
    }
}

/// Encode one input against the live `TermMode` (pure: no I/O). `None` or
/// empty means a successful no-op (e.g. a release without kitty).
pub(crate) fn encode_input<T: EventListener>(
    term: &Term<T>,
    input: &Input,
) -> Result<Option<Vec<u8>>, TuiError> {
    let mode = *term.mode();
    match input {
        Input::Bytes(b) => Ok(Some(b.clone())),
        Input::Paste(text) => Ok(Some(encode_paste(text, mode)?)),
        Input::Key { key, mods, kind } => encode_key(key, *mods, *kind, mode),
        Input::Mouse { action, x, y, mods } => {
            let (cols, rows) = (term.columns(), term.screen_lines());
            Ok(Some(encode_mouse(action, *x, *y, *mods, mode, cols, rows)?))
        }
        Input::Focus(focused) => Ok(Some(encode_focus(*focused, mode)?)),
    }
}

pub(crate) fn apply_resize<T: EventListener>(
    master: &dyn MasterPty,
    term: &mut Term<T>,
    cols: u16,
    rows: u16,
) -> Result<(), TuiError> {
    // PTY first: if the kernel refuses, the emulator stays consistent.
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| TuiError::Io(format!("pty resize failed: {e}")))?;
    term.resize(WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    });
    Ok(())
}

// -- paste ---------------------------------------------------------------

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

fn encode_paste(text: &str, mode: TermMode) -> Result<Vec<u8>, TuiError> {
    if text.contains(PASTE_START) || text.contains(PASTE_END) {
        return Err(TuiError::PasteRejected(
            "content contains bracketed-paste delimiters".to_string(),
        ));
    }
    if mode.contains(TermMode::BRACKETED_PASTE) {
        Ok(format!("{PASTE_START}{text}{PASTE_END}").into_bytes())
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

// -- focus ---------------------------------------------------------------

fn encode_focus(focused: bool, mode: TermMode) -> Result<Vec<u8>, TuiError> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return Err(TuiError::ModeNotEnabled(
            "focus tracking (DEC 1004) not enabled by the application",
        ));
    }
    Ok(if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    })
}

// -- mouse ---------------------------------------------------------------

fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Required live mode bits for one mouse action, plus the error text when
/// the application has not enabled them.
fn mouse_mode_gate(action: &MouseAction) -> (TermMode, &'static str) {
    let any = TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION;
    let required: TermMode = match action {
        MouseAction::Press(_) | MouseAction::Release | MouseAction::Wheel(_) => any,
        MouseAction::Move { held: None } => TermMode::MOUSE_MOTION,
        MouseAction::Move { held: Some(_) } => TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
    };
    let what = match action {
        MouseAction::Press(_) | MouseAction::Release | MouseAction::Wheel(_) => {
            "mouse reporting (DEC 1000/1002/1003) not enabled by the application"
        }
        MouseAction::Move { held: None } => {
            "mouse motion reporting (DEC 1003) not enabled by the application"
        }
        MouseAction::Move { held: Some(_) } => {
            "mouse drag reporting (DEC 1002/1003) not enabled by the application"
        }
    };
    (required, what)
}

fn encode_mouse(
    action: &MouseAction,
    x: u16,
    y: u16,
    mods: MouseMods,
    term_mode: TermMode,
    cols: usize,
    rows: usize,
) -> Result<Vec<u8>, TuiError> {
    if x as usize >= cols || y as usize >= rows {
        return Err(TuiError::InvalidInput(format!(
            "mouse ({x},{y}) outside {cols}x{rows} grid"
        )));
    }
    let (required, what) = mouse_mode_gate(action);
    if !term_mode.intersects(required) {
        return Err(TuiError::ModeNotEnabled(what));
    }

    let mut cb: u32 = match action {
        MouseAction::Press(b) => u32::from(button_code(*b)),
        MouseAction::Release => 3,
        MouseAction::Move { held: None } => 3 + 32,
        MouseAction::Move { held: Some(b) } => u32::from(button_code(*b)) + 32,
        MouseAction::Wheel(Wheel::Up) => 64,
        MouseAction::Wheel(Wheel::Down) => 65,
        MouseAction::Wheel(Wheel::Left) => 66,
        MouseAction::Wheel(Wheel::Right) => 67,
    };
    if mods.shift {
        cb += 4;
    }
    if mods.alt {
        cb += 8;
    }
    if mods.ctrl {
        cb += 16;
    }
    let cx = u32::from(x) + 1;
    let cy = u32::from(y) + 1;

    if term_mode.contains(TermMode::SGR_MOUSE) {
        let marker = if matches!(action, MouseAction::Release) {
            'm'
        } else {
            'M'
        };
        return Ok(format!("\x1b[<{cb};{cx};{cy}{marker}").into_bytes());
    }
    if term_mode.contains(TermMode::UTF8_MOUSE) {
        let mut out = b"\x1b[M".to_vec();
        for v in [cb + 32, cx + 32, cy + 32] {
            let ch = char::from_u32(v).ok_or_else(|| {
                TuiError::InvalidInput(format!("mouse coordinate {v} unencodable"))
            })?;
            let mut tmp = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
        }
        return Ok(out);
    }
    // Legacy X10: single bytes, coordinates must fit.
    for (v, name) in [(cb + 32, "button"), (cx + 32, "x"), (cy + 32, "y")] {
        if v > 255 {
            return Err(TuiError::InvalidInput(format!(
                "mouse {name} {v} exceeds legacy X10 encoding"
            )));
        }
    }
    Ok(vec![
        0x1b,
        b'[',
        b'M',
        u8::try_from(cb + 32).unwrap_or(u8::MAX),
        u8::try_from(cx + 32).unwrap_or(u8::MAX),
        u8::try_from(cy + 32).unwrap_or(u8::MAX),
    ])
}
