use tuiscotti_core::frame::Rgb;
use tuiscotti_core::screen::{Maybe, Observation};

// ---------------------------------------------------------------------------
// R13: terminal-state snapshot + explicit assertions
// ---------------------------------------------------------------------------

/// Which clipboard an OSC 52 store targeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardTarget {
    /// Explicit clipboard buffer (`CLIPBOARD`).
    Clipboard,
    /// Primary selection buffer (`PRIMARY`).
    Selection,
}

/// One captured OSC 52 store: decoded text plus its target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipboardItem {
    /// Which buffer the store targeted.
    pub target: ClipboardTarget,
    /// Decoded clipboard text.
    pub text: String,
}

/// Sandboxed clipboard capture: process memory only.
///
/// Bytes come from the replayed stream's OSC 52 sequences (base64-decoded,
/// UTF-8 only, like the backend). This type has no host-clipboard API to
/// call: capture can never read or modify the real clipboard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SandboxClipboard {
    items: Vec<ClipboardItem>,
}

impl SandboxClipboard {
    /// Empty in-memory clipboard capture.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn store(&mut self, item: ClipboardItem) {
        self.items.push(item);
    }

    /// True when no stores were captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of captured stores.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Most recent captured store, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&ClipboardItem> {
        self.items.last()
    }

    /// All captured stores, oldest first.
    #[must_use]
    pub fn items(&self) -> &[ClipboardItem] {
        &self.items
    }
}

/// Default (non-indexed) foreground/background colors. `None` = terminal
/// default, i.e. no OSC 10/11 override observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefaultColors {
    /// OSC 10 foreground override, if observed.
    pub fg: Option<Rgb>,
    /// OSC 11 background override, if observed.
    pub bg: Option<Rgb>,
}

/// One OSC 8 hyperlink URI observed on the grid (order of first appearance).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    /// Link target URI.
    pub uri: String,
}

/// Full terminal state: everything assertions may target.
///
/// [`TermSnapshot::from_observation`] fills only what a live
/// [`Observation`] carries (title/bells/modes/palette); the rest is
/// [`Maybe::Unsupported`] because the live path drops it. Replay
/// ([`Replayed::state`]) fills everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermSnapshot {
    /// Window/icon title.
    pub title: Maybe<String>,
    /// Bell count since session start.
    pub bells: Maybe<u64>,
    /// Set DEC/private mode numbers.
    pub modes: Maybe<Vec<u16>>,
    /// Palette overrides as (index, color) pairs.
    pub palette: Maybe<Vec<(u8, Rgb)>>,
    /// Default fg/bg overrides.
    pub defaults: Maybe<DefaultColors>,
    /// Sandboxed clipboard capture.
    pub clipboard: Maybe<SandboxClipboard>,
    /// Observed hyperlink URIs.
    pub hyperlinks: Maybe<Vec<Hyperlink>>,
    /// Scrollback lines, oldest first, viewport excluded.
    pub scrollback: Maybe<Vec<String>>,
}

impl TermSnapshot {
    /// Project a live observation onto the assertion surface. Fields the
    /// live path cannot provide are `Unsupported`, never fabricated.
    #[must_use]
    pub fn from_observation(obs: &Observation) -> Self {
        Self {
            title: obs.state.title.clone(),
            bells: obs.state.bells.clone(),
            modes: obs.state.modes.clone(),
            palette: obs.state.palette.clone(),
            defaults: Maybe::Unsupported,
            clipboard: Maybe::Unsupported,
            hyperlinks: Maybe::Unsupported,
            scrollback: Maybe::Unsupported,
        }
    }
}

/// Terminal-state assertion failure (mismatch, unknown, or unsupported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError(pub String);

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "terminal-state assertion failed: {}", self.0)
    }
}

impl std::error::Error for StateError {}

/// Assert the window/icon title equals `expected`.
///
/// # Errors
///
/// Returns [`StateError`] on mismatch, unknown, or unsupported title.
pub fn assert_title_eq(state: &TermSnapshot, expected: &str) -> Result<(), StateError> {
    match &state.title {
        Maybe::Known(t) if t == expected => Ok(()),
        Maybe::Known(t) => Err(StateError(format!(
            "title mismatch: expected {expected:?}, observed {t:?}"
        ))),
        Maybe::Unknown => Err(StateError("title is unknown (never set)".to_string())),
        Maybe::Unsupported => Err(StateError(
            "title is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert the bell count since session start equals `expected`.
///
/// # Errors
///
/// Returns [`StateError`] on mismatch, unknown, or unsupported bells.
pub fn assert_bells_eq(state: &TermSnapshot, expected: u64) -> Result<(), StateError> {
    match &state.bells {
        Maybe::Known(n) if *n == expected => Ok(()),
        Maybe::Known(n) => Err(StateError(format!(
            "bell count mismatch: expected {expected}, observed {n}"
        ))),
        Maybe::Unknown => Err(StateError("bell count is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "bell count is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert DEC/private mode `mode` is currently set.
///
/// # Errors
///
/// Returns [`StateError`] when unset, unknown, or unsupported.
pub fn assert_mode_set(state: &TermSnapshot, mode: u16) -> Result<(), StateError> {
    match &state.modes {
        Maybe::Known(m) if m.contains(&mode) => Ok(()),
        Maybe::Known(m) => Err(StateError(format!(
            "mode {mode} is not set (observed: {m:?})"
        ))),
        Maybe::Unknown => Err(StateError("modes are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "modes are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert DEC/private mode `mode` is currently unset.
///
/// # Errors
///
/// Returns [`StateError`] when set, unknown, or unsupported.
pub fn assert_mode_unset(state: &TermSnapshot, mode: u16) -> Result<(), StateError> {
    match &state.modes {
        Maybe::Known(m) if !m.contains(&mode) => Ok(()),
        Maybe::Known(_) => Err(StateError(format!("mode {mode} is set"))),
        Maybe::Unknown => Err(StateError("modes are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "modes are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert palette entry `index` resolves to `expected`: a live OSC 4
/// override when present, else the documented nominal xterm default.
///
/// # Errors
///
/// Returns [`StateError`] on mismatch, unknown, or unsupported palette.
pub fn assert_palette_entry(
    state: &TermSnapshot,
    index: u8,
    expected: Rgb,
) -> Result<(), StateError> {
    match &state.palette {
        Maybe::Known(list) => {
            let observed = list
                .iter()
                .find(|(i, _)| *i == index)
                .map_or_else(|| Rgb::from_indexed(index), |(_, c)| *c);
            if observed == expected {
                Ok(())
            } else {
                Err(StateError(format!(
                    "palette {index} mismatch: expected {expected:?}, observed {observed:?}"
                )))
            }
        }
        Maybe::Unknown => Err(StateError("palette is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "palette is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert the default fg/bg (OSC 10/11 overrides; `None` = terminal default).
///
/// # Errors
///
/// Returns [`StateError`] on mismatch, unknown, or unsupported defaults.
pub fn assert_default_colors(
    state: &TermSnapshot,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
) -> Result<(), StateError> {
    match &state.defaults {
        Maybe::Known(d) if d.fg == fg && d.bg == bg => Ok(()),
        Maybe::Known(d) => Err(StateError(format!(
            "default colors mismatch: expected fg={fg:?} bg={bg:?}, observed fg={:?} bg={:?}",
            d.fg, d.bg
        ))),
        Maybe::Unknown => Err(StateError("default colors are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "default colors are unsupported by this capture path (live sessions drop OSC 10/11)"
                .to_string(),
        )),
    }
}

/// Assert the latest sandboxed clipboard store equals `expected`.
///
/// # Errors
///
/// Returns [`StateError`] on mismatch, empty, unknown, or unsupported.
pub fn assert_clipboard_latest_eq(state: &TermSnapshot, expected: &str) -> Result<(), StateError> {
    match &state.clipboard {
        Maybe::Known(sb) => match sb.latest() {
            Some(item) if item.text == expected => Ok(()),
            Some(item) => Err(StateError(format!(
                "clipboard mismatch: expected {expected:?}, observed {:?}",
                item.text
            ))),
            None => Err(StateError("no clipboard stores captured".to_string())),
        },
        Maybe::Unknown => Err(StateError("clipboard is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "clipboard is unsupported by this capture path (live sessions drop OSC 52)".to_string(),
        )),
    }
}

/// Assert no clipboard stores were captured.
///
/// # Errors
///
/// Returns [`StateError`] when stores exist, unknown, or unsupported.
pub fn assert_clipboard_empty(state: &TermSnapshot) -> Result<(), StateError> {
    match &state.clipboard {
        Maybe::Known(sb) if sb.is_empty() => Ok(()),
        Maybe::Known(sb) => Err(StateError(format!(
            "expected no clipboard stores, observed {}",
            sb.len()
        ))),
        Maybe::Unknown => Err(StateError("clipboard is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "clipboard is unsupported by this capture path (live sessions drop OSC 52)".to_string(),
        )),
    }
}

/// Assert a hyperlink with exactly `uri` is present on the grid.
///
/// # Errors
///
/// Returns [`StateError`] when absent, unknown, or unsupported.
pub fn assert_hyperlink_present(state: &TermSnapshot, uri: &str) -> Result<(), StateError> {
    match &state.hyperlinks {
        Maybe::Known(links) if links.iter().any(|l| l.uri == uri) => Ok(()),
        Maybe::Known(links) => Err(StateError(format!(
            "hyperlink {uri:?} not present ({} links observed)",
            links.len()
        ))),
        Maybe::Unknown => Err(StateError("hyperlinks are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "hyperlinks are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert scrollback (oldest-first, viewport excluded) contains `needle`.
///
/// # Errors
///
/// Returns [`StateError`] when absent, unknown, or unsupported.
pub fn assert_scrollback_contains(state: &TermSnapshot, needle: &str) -> Result<(), StateError> {
    match &state.scrollback {
        Maybe::Known(lines) if lines.iter().any(|l| l.contains(needle)) => Ok(()),
        Maybe::Known(lines) => Err(StateError(format!(
            "scrollback ({} lines) does not contain {needle:?}",
            lines.len()
        ))),
        Maybe::Unknown => Err(StateError("scrollback is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "scrollback is unsupported by this capture path (live sessions capture the viewport only)"
                .to_string(),
        )),
    }
}
