// ---------------------------------------------------------------------------
// terminal-control .termctrl recordings
// ---------------------------------------------------------------------------

/// Explicit loss report for competitor-trace import (backlog A10): nothing is
/// silently dropped or normalized away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LossReport {
    /// Unknown fields seen on otherwise-valid entries, each with line + byte
    /// offset (e.g. `line 4 (byte 120): output.extra`). Non-fatal.
    pub unsupported_fields: Vec<String>,
    /// Entries skipped deliberately with reasons (unknown entry types,
    /// version-gated entries such as v2 `mouse` in a v1 file), each with
    /// line + byte offset. Non-fatal.
    pub dropped_events: Vec<String>,
}

impl LossReport {
    /// True when nothing was lost: no unknown fields, no dropped entries.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unsupported_fields.is_empty() && self.dropped_events.is_empty()
    }
}

/// One `.termctrl` entry. `Input`/`Mouse` carry delivered CLIENT input bytes:
/// non-executable, excluded from [`TermctrlTrace::output_deltas`] and
/// [`TermctrlTrace::screens_via`], never fed as output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermctrlEvent {
    /// Terminal output at `at_ms` milliseconds since recording start.
    Output {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Raw output bytes.
        bytes: Vec<u8>,
    },
    /// Typed input written to the app (NON-EXECUTABLE, never replayed).
    Input {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// `client` or `host` origin from the source.
        origin: String,
        /// Raw input bytes.
        bytes: Vec<u8>,
    },
    /// Delivered mouse input (NON-EXECUTABLE, never replayed). v2 only.
    Mouse {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Raw encoded mouse bytes.
        bytes: Vec<u8>,
    },
    /// Terminal resize.
    Resize {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },
    /// Named marker (data only — never executed).
    Marker {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Marker name.
        name: String,
    },
}

impl TermctrlEvent {
    /// Milliseconds since recording start.
    #[must_use]
    pub fn at_ms(&self) -> u64 {
        match self {
            TermctrlEvent::Output { at_ms, .. }
            | TermctrlEvent::Input { at_ms, .. }
            | TermctrlEvent::Mouse { at_ms, .. }
            | TermctrlEvent::Resize { at_ms, .. }
            | TermctrlEvent::Marker { at_ms, .. } => *at_ms,
        }
    }
}

/// An imported `.termctrl` trace: validated header dims + entries in file
/// order + the explicit [`LossReport`].
#[derive(Debug, Clone)]
pub struct TermctrlTrace {
    /// Recording schema version (1 or 2).
    pub version: u8,
    /// Header terminal width in columns.
    pub cols: u16,
    /// Header terminal height in rows.
    pub rows: u16,
    /// Entries in file order (dropped entries excluded; see `loss`).
    pub events: Vec<TermctrlEvent>,
    /// Explicit loss report: unknown fields + dropped entries.
    pub loss: LossReport,
}

impl TermctrlTrace {
    /// Output entries as `(dt, bytes)` with `dt` = seconds since the previous
    /// OUTPUT entry (first output relative to start). Input/mouse/resize/marker
    /// entries never appear here.
    #[must_use]
    pub fn output_deltas(&self) -> Vec<(f64, Vec<u8>)> {
        let mut out = Vec::new();
        let mut prev = 0u64;
        for e in &self.events {
            if let TermctrlEvent::Output { at_ms, bytes } = e {
                out.push((ms_to_secs(at_ms.saturating_sub(prev)), bytes.clone()));
                prev = *at_ms;
            }
        }
        out
    }

    /// Feed OUTPUT bytes through caller-supplied `replay`, collecting the
    /// state after each output entry. Input/mouse bytes are never passed
    /// (output-only direction); resize/marker entries are skipped. No emulator
    /// coupling: `replay` is the caller's.
    pub fn screens_via<S: Clone>(
        &self,
        initial: S,
        mut replay: impl FnMut(S, &[u8]) -> S,
    ) -> Vec<S> {
        let mut state = initial;
        let mut out = Vec::new();
        for e in &self.events {
            if let TermctrlEvent::Output { bytes, .. } = e {
                state = replay(state, bytes);
                out.push(state.clone());
            }
        }
        out
    }
}

/// Milliseconds as fractional seconds via exact 32-bit halves (no lossy cast).
fn ms_to_secs(ms: u64) -> f64 {
    let hi = u32::try_from(ms >> 32).unwrap_or(u32::MAX);
    let lo = u32::try_from(ms & 0xffff_ffff).unwrap_or(u32::MAX);
    (f64::from(hi) * 4_294_967_296.0 + f64::from(lo)) / 1000.0
}
