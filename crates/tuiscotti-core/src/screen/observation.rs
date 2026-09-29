use super::Screen;
use crate::frame::Rgb;
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// Maybe: Unknown/Unsupported are never false/empty/default.
// ---------------------------------------------------------------------------

/// Tri-state terminal observation: a required-but-unavailable property is
/// [`Maybe::Unknown`] (not yet observed) or [`Maybe::Unsupported`] (backend
/// cannot provide it) — never `false`, empty, or default-conflated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Maybe<T> {
    Known(T),
    Unknown,
    Unsupported,
}

impl<T> Maybe<T> {
    #[must_use]
    pub fn known(&self) -> Option<&T> {
        match self {
            Maybe::Known(v) => Some(v),
            Maybe::Unknown | Maybe::Unsupported => None,
        }
    }

    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Maybe::Known(_))
    }
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// Why a capture was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureReason {
    Initial,
    Poll,
    Input,
    Resize,
    Exit,
    Manual,
}

/// Observed terminal state beyond the grid. Every field is [`Maybe`]-wrapped:
/// unknown and unsupported backends stay visible instead of collapsing to
/// empty/default values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TermState {
    /// Set DEC/private mode numbers.
    pub modes: Maybe<Vec<u16>>,
    /// Palette overrides as (index, color) pairs.
    pub palette: Maybe<Vec<(u8, Rgb)>>,
    /// Window/icon title.
    pub title: Maybe<String>,
    /// Bell count since session start.
    pub bells: Maybe<u64>,
}

impl Default for TermState {
    fn default() -> Self {
        Self {
            modes: Maybe::Unknown,
            palette: Maybe::Unknown,
            title: Maybe::Unknown,
            bells: Maybe::Unknown,
        }
    }
}

/// Informational capture provenance: timestamps, PIDs, paths, attempt
/// counters. Recorded for auditability; EXCLUDED from [`Observation`]
/// equality and hashing (M08) so runtime metadata never becomes an
/// approval key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProvenance {
    pub captured_unix_ms: u64,
    pub pid: Option<u32>,
    pub source_path: Option<String>,
    pub attempt: u64,
}

impl CaptureProvenance {
    #[must_use]
    pub fn new(
        captured_unix_ms: u64,
        pid: Option<u32>,
        source_path: Option<String>,
        attempt: u64,
    ) -> Self {
        Self {
            captured_unix_ms,
            pid,
            source_path,
            attempt,
        }
    }
}

/// One atomic capture: an owned [`Screen`] plus revision, capture reason,
/// terminal state, and informational provenance.
///
/// `PartialEq`/`Hash` are manual over the approval-relevant subset
/// (`screen`, `revision`, `reason`, `state`); [`CaptureProvenance`] is
/// excluded so identical captures compare equal despite different
/// timestamps, PIDs, paths, or attempt counters (M08).
#[derive(Debug, Clone)]
pub struct Observation {
    pub screen: Screen,
    pub revision: u64,
    pub reason: CaptureReason,
    pub state: TermState,
    pub provenance: CaptureProvenance,
}

impl Observation {
    #[must_use]
    pub fn new(
        screen: Screen,
        revision: u64,
        reason: CaptureReason,
        state: TermState,
        provenance: CaptureProvenance,
    ) -> Self {
        Self {
            screen,
            revision,
            reason,
            state,
            provenance,
        }
    }
}

impl PartialEq for Observation {
    fn eq(&self, other: &Self) -> bool {
        self.screen == other.screen
            && self.revision == other.revision
            && self.reason == other.reason
            && self.state == other.state
        // provenance deliberately excluded (M08)
    }
}

impl Eq for Observation {}

impl Hash for Observation {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.screen.hash(state);
        self.revision.hash(state);
        self.reason.hash(state);
        self.state.hash(state);
        // provenance deliberately excluded (M08)
    }
}
