use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::Processor;

use super::{
    SandboxClipboard, TermSnapshot, build_replay_screen, build_replay_state, drain_replay_events,
};
use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// R14: bounded raw replay (direction-tagged recordings, fresh emulator)
// ---------------------------------------------------------------------------

/// Byte cap for one [`Recording`] and for one replay call.
pub const MAX_REPLAY_BYTES: usize = 1 << 20;

/// Scrollback lines retained by the replay emulator.
pub(crate) const REPLAY_HISTORY: usize = 1000;

/// One recorded event. The direction tag is structural: [`replay_recording`]
/// only feeds [`RecEvent::Output`], so recorded input can never be mistaken
/// for terminal output, by construction rather than by caller discipline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecEvent {
    /// PTY output bytes (terminal input): fed to the emulator on replay.
    Output(Vec<u8>),
    /// Bytes the test sent to the PTY: never fed on replay.
    Input(Vec<u8>),
}

/// A bounded, direction-tagged recording of a PTY conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    cols: u16,
    rows: u16,
    events: Vec<RecEvent>,
    bytes: usize,
}

/// Replay failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// Recording or byte total exceeds [`MAX_REPLAY_BYTES`].
    TooLarge {
        /// Observed byte total.
        bytes: usize,
        /// Enforced cap.
        max: usize,
    },
    /// Replay dimensions outside 1..=1000.
    InvalidSize(String),
    /// Bad chunking (`chunk_len` 0).
    InvalidChunks(String),
    /// Final screen failed validation.
    ScreenBuild(String),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes, max } => {
                write!(f, "recording too large: {bytes} bytes exceed cap {max}")
            }
            Self::InvalidSize(m) => write!(f, "invalid replay size: {m}"),
            Self::InvalidChunks(m) => write!(f, "invalid chunking: {m}"),
            Self::ScreenBuild(m) => write!(f, "replay screen build failed: {m}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl Recording {
    /// Start an empty recording for a `cols`x`rows` viewport.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            events: Vec::new(),
            bytes: 0,
        }
    }

    /// Record PTY output bytes. Refused past [`MAX_REPLAY_BYTES`].
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::TooLarge`] past the byte cap.
    pub fn push_output(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
        self.push(RecEvent::Output(bytes.to_vec()))
    }

    /// Record input bytes (never replayed as output). Counts toward the cap.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::TooLarge`] past the byte cap.
    pub fn push_input(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
        self.push(RecEvent::Input(bytes.to_vec()))
    }

    fn push(&mut self, event: RecEvent) -> Result<(), ReplayError> {
        let n = match &event {
            RecEvent::Output(b) | RecEvent::Input(b) => b.len(),
        };
        if self.bytes + n > MAX_REPLAY_BYTES {
            return Err(ReplayError::TooLarge {
                bytes: self.bytes + n,
                max: MAX_REPLAY_BYTES,
            });
        }
        self.bytes += n;
        self.events.push(event);
        Ok(())
    }

    /// Recorded viewport width.
    #[must_use]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Recorded viewport height.
    #[must_use]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Total recorded bytes (input + output).
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.bytes
    }

    /// Number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when no events were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Concatenated output bytes in record order (input excluded).
    #[must_use]
    pub fn output_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for e in &self.events {
            if let RecEvent::Output(b) = e {
                out.extend_from_slice(b);
            }
        }
        out
    }
}

/// The result of one replay: final screen + full terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// Final viewport screen after replay.
    pub screen: Screen,
    /// Full terminal state after replay.
    pub state: TermSnapshot,
    /// Total output bytes fed to the emulator.
    pub bytes_fed: usize,
    /// Number of chunks the bytes were fed in.
    pub chunks: usize,
}

/// Replay raw output bytes through a fresh emulator, fed as one chunk.
///
/// # Errors
///
/// Returns [`ReplayError`] on bad size, over-cap bytes, or screen build.
pub fn replay_bytes(output: &[u8], cols: u16, rows: u16) -> Result<Replayed, ReplayError> {
    replay_chunks([output], cols, rows)
}

/// Replay raw output bytes through a fresh emulator, fed in the given
/// chunks. Splits may fall anywhere, including mid-UTF-8 and mid-escape:
/// the streaming parser makes chunking unobservable in the result.
///
/// # Errors
///
/// Returns [`ReplayError`] on bad size, over-cap bytes, or screen build.
pub fn replay_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a [u8]>,
    cols: u16,
    rows: u16,
) -> Result<Replayed, ReplayError> {
    if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        return Err(ReplayError::InvalidSize(format!(
            "replay size {cols}x{rows} outside 1..=1000"
        )));
    }
    let chunks: Vec<&[u8]> = chunks.into_iter().collect();
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    if total > MAX_REPLAY_BYTES {
        return Err(ReplayError::TooLarge {
            bytes: total,
            max: MAX_REPLAY_BYTES,
        });
    }

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let dims = ReplayDims {
        cols: cols as usize,
        rows: rows as usize,
    };
    let config = TermConfig {
        scrolling_history: REPLAY_HISTORY,
        kitty_keyboard: true,
        ..TermConfig::default()
    };
    let mut term = Term::new(config, &dims, ReplayListener { tx: event_tx });
    let mut processor: Processor = Processor::new();
    for chunk in &chunks {
        processor.advance(&mut term, chunk);
    }

    let mut replayed = ReplayEvents::default();
    drain_replay_events(&mut term, &event_rx, &mut replayed);
    let screen = build_replay_screen(&term, cols, rows)?;
    let state = build_replay_state(&term, &replayed, cols);
    Ok(Replayed {
        screen,
        state,
        bytes_fed: total,
        chunks: chunks.len(),
    })
}

/// Replay a recording through a fresh emulator: only [`RecEvent::Output`]
/// bytes are fed, in record order. `chunk_len` re-splits the output stream
/// (`None` = one chunk); recorded input is always skipped.
///
/// # Errors
///
/// Returns [`ReplayError`] on bad chunking, bad size, or screen build.
pub fn replay_recording(
    recording: &Recording,
    chunk_len: Option<usize>,
) -> Result<Replayed, ReplayError> {
    let output = recording.output_bytes();
    match chunk_len {
        None => replay_chunks([output.as_slice()], recording.cols, recording.rows),
        Some(0) => Err(ReplayError::InvalidChunks("chunk length 0".to_string())),
        Some(n) => {
            let chunks: Vec<&[u8]> = output.chunks(n).collect();
            replay_chunks(chunks, recording.cols, recording.rows)
        }
    }
}

struct ReplayDims {
    cols: usize,
    rows: usize,
}

impl GridDims for ReplayDims {
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
struct ReplayListener {
    tx: mpsc::Sender<Event>,
}

impl EventListener for ReplayListener {
    fn send_event(&self, event: Event) {
        // The drain runs after the feed, so the receiver is alive; a
        // failure would only mean replay was abandoned mid-parse.
        if self.tx.send(event).is_err() {
            // Receiver gone; replay is abandoned anyway.
        }
    }
}

#[derive(Default)]
pub(crate) struct ReplayEvents {
    pub(crate) title: Option<String>,
    pub(crate) bells: u64,
    pub(crate) clipboard: SandboxClipboard,
}
