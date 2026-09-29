use std::collections::HashMap;
use std::path::PathBuf;

use super::SessionInfo;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Version, exit statuses, capabilities
// ---------------------------------------------------------------------------

/// Machine-protocol version. Bumped on any incompatible Op/OpResult change.
pub const PROTOCOL_VERSION: &str = "1.0.0";

/// CLI usage error (matches clap's exit code for parse failures).
pub const EXIT_USAGE: i32 = 2;

/// An op failed (invalid input, I/O, spawn, timeout, session error, ...).
pub const EXIT_OP_ERROR: i32 = 3;

/// Offline verification disagreed (diff mismatch, failing verdicts).
pub const EXIT_VERIFY_FAIL: i32 = 4;

/// What this build can do. Never advertises what it cannot implement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    /// Machine-protocol version string.
    pub protocol: String,
    /// Tuisnap build version string.
    pub tuisnap: String,
    /// True when this build implements PTY-backed ops.
    pub pty: bool,
    /// True when this build implements render/diff ops.
    pub render: bool,
    /// True when this build implements record/review/report ops.
    pub record: bool,
    /// OS identifier (`std::env::consts::OS`).
    pub platform: String,
}

/// Describe what this build can do.
#[must_use]
pub fn capabilities() -> Capabilities {
    Capabilities {
        protocol: PROTOCOL_VERSION.to_string(),
        tuisnap: env!("CARGO_PKG_VERSION").to_string(),
        pty: cfg!(feature = "pty"),
        render: true,
        record: true,
        platform: std::env::consts::OS.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Config responsibilities (referenced by `init --help`)
// ---------------------------------------------------------------------------

/// Which file owns which decision. Printed by `tui-snap init` and embedded in
/// its `--help`.
pub const CONFIG_DOCS: &str = "\
tui-snap.toml        Capture + assertion policy: default viewport, terminal and\n\
                     render profiles, snapshot/screenshot gates, evidence dir.\n\
                     Owned by tui-snap; read by tests via the Rust API.\n\
.config/nextest.toml Scheduling only: profiles, retries, threads, test groups.\n\
                     Owned by cargo-nextest; tui-snap never writes it except\n\
                     via `init` scaffolding and never parses it.\n\
insta config         Snapshot review behaviour (`INSTA_UPDATE`, snapshot\n\
                     paths). Owned by Insta; tui-snap honours it and pins\n\
                     `INSTA_UPDATE=no` only inside its own frozen checks.\n";

// ---------------------------------------------------------------------------
// Ops
// ---------------------------------------------------------------------------

/// `kind` values for [`Op::Wait`]: `text` (screen contains `needle`),
/// `stable` (no new revision for `quiet_ms`, default 200), `exit`.
pub mod wait_kind {
    /// Wait for screen text to contain `needle`.
    pub const TEXT: &str = "text";
    /// Wait for no new revision within `quiet_ms`.
    pub const STABLE: &str = "stable";
    /// Wait for natural child exit.
    pub const EXIT: &str = "exit";
}

/// `check` values for [`Op::Assert`]: `text-contains`, `text-equals`.
pub mod assert_check {
    /// Pass when `text` contains `needle`.
    pub const TEXT_CONTAINS: &str = "text-contains";
    /// Pass when `actual` equals `expected`.
    pub const TEXT_EQUALS: &str = "text-equals";
}

/// One typed operation. `#[serde(tag = "type")]`: each line on the machine
/// protocol is one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Op {
    /// Spawn a child in a new PTY session.
    Spawn {
        /// Child argv; must be non-empty.
        argv: Vec<String>,
        /// Session id; auto-assigned when absent.
        #[serde(default)]
        id: Option<String>,
        /// PTY width; must pair with `rows`.
        #[serde(default)]
        cols: Option<u16>,
        /// PTY height; must pair with `cols`.
        #[serde(default)]
        rows: Option<u16>,
        /// Child working directory.
        #[serde(default)]
        cwd: Option<PathBuf>,
        /// Extra child-only environment entries.
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// Send input to a PTY session.
    Stdin {
        /// Target session id.
        session: String,
        /// Literal text to type.
        #[serde(default)]
        text: Option<String>,
        /// Key chord to press.
        #[serde(default)]
        chord: Option<String>,
        /// Raw bytes, base64.
        #[serde(default)]
        bytes_b64: Option<String>,
    },
    /// Current observation of a PTY session.
    Observe {
        /// Target session id.
        session: String,
    },
    /// Current screen projection of a PTY session.
    Snapshot {
        /// Target session id.
        session: String,
    },
    /// Screen plus canonical string plus PNG of a PTY session.
    Screenshot {
        /// Target session id.
        session: String,
    },
    /// Wait for a condition on a PTY session.
    Wait {
        /// Target session id.
        session: String,
        /// See [`wait_kind`]: `text` | `stable` | `exit`.
        kind: String,
        /// Text to wait for (`text` waits).
        #[serde(default)]
        needle: Option<String>,
        /// Quiet window for `stable` waits (default 200).
        #[serde(default)]
        quiet_ms: Option<u64>,
        /// Give up after this long.
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Wait for natural exit until `timeout_ms`, reap, drop the session.
    Exit {
        /// Target session id.
        session: String,
        /// Give up after this long.
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Run a stateless check.
    Assert {
        /// See [`assert_check`]: `text-contains` | `text-equals`.
        check: String,
        /// Haystack for `text-contains`.
        #[serde(default)]
        text: Option<String>,
        /// Needle for `text-contains`.
        #[serde(default)]
        needle: Option<String>,
        /// Actual text for `text-equals`.
        #[serde(default)]
        actual: Option<String>,
        /// Expected text for `text-equals`.
        #[serde(default)]
        expected: Option<String>,
    },
    /// Render a frame JSON to text or image.
    Render {
        /// Frame JSON to render.
        frame_json: String,
        /// Output format (`png`|`ansi`|`txt`|`svg`|`html`).
        format: String,
    },
    /// Compare two base64 PNGs.
    Diff {
        /// Expected PNG, base64.
        expected_png_b64: String,
        /// Actual PNG, base64.
        actual_png_b64: String,
    },
    /// Start a named (piped) session.
    SessionStart {
        /// Session name.
        name: String,
        /// Child argv; must be non-empty.
        argv: Vec<String>,
        /// Stop a live same-name session first.
        #[serde(default)]
        force: bool,
    },
    /// Stop a named session.
    SessionStop {
        /// Session name.
        name: String,
    },
    /// List named sessions with liveness.
    SessionList,
    /// Report protocol and tuisnap versions.
    Version,
    /// Report what this build can do.
    Capabilities,
}

fn default_timeout_ms() -> u64 {
    5000
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// Serializable screen projection: geometry + text + cursor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenView {
    /// Screen width in cells.
    pub cols: u16,
    /// Screen height in cells.
    pub rows: u16,
    /// Plain-text projection, one line per row.
    pub text: String,
    /// Cursor column.
    pub cursor_x: u16,
    /// Cursor row.
    pub cursor_y: u16,
    /// True when the cursor is visible.
    pub cursor_visible: bool,
}

/// Serializable observation projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationView {
    /// Monotonic observation revision.
    pub revision: u64,
    /// Why this observation was produced.
    pub reason: String,
    /// Screen projection.
    pub screen: ScreenView,
}

/// Typed op result. Results carry evidence, never bare success flags alone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum OpResult {
    /// A PTY session was spawned.
    Spawned {
        /// Session id.
        session: String,
        /// Direct-child pid, when known.
        pid: Option<u32>,
    },
    /// Stdin was delivered.
    InputAccepted {
        /// Session id.
        session: String,
    },
    /// A live observation.
    Observation {
        /// The observation.
        observation: ObservationView,
    },
    /// A screen projection.
    Snapshot {
        /// The screen.
        screen: ScreenView,
    },
    /// Screen, canonical string, and PNG.
    Screenshot {
        /// The screen.
        screen: ScreenView,
        /// Canonical snapshot string.
        canonical: String,
        /// Rendered PNG, base64.
        png_b64: String,
    },
    /// A wait condition fired.
    Waited {
        /// Session id.
        session: String,
        /// Observation at fire time.
        observation: ObservationView,
    },
    /// A session exited and was reaped.
    Exited {
        /// Session id.
        session: String,
        /// Exit code.
        code: u32,
        /// Killing signal, when signaled.
        signal: Option<String>,
        /// Final observation.
        observation: ObservationView,
    },
    /// A check verdict.
    Asserted {
        /// True when the check passed.
        passed: bool,
        /// Human-readable verdict detail.
        detail: String,
    },
    /// A rendered frame.
    Rendered {
        /// Output format.
        format: String,
        /// PNG/SVG/HTML/ANSI/TXT payload; PNG is base64.
        data: String,
        /// True when `data` is base64.
        data_b64: bool,
    },
    /// A PNG comparison verdict.
    Diffed {
        /// True when pixels match.
        pixels_equal: bool,
        /// True when dimensions match.
        dims_equal: bool,
        /// Difference score (0 for identical).
        score: f64,
    },
    /// A named-session record.
    Session {
        /// The session info.
        session: SessionInfo,
    },
    /// Named-session listing.
    SessionList {
        /// All known sessions.
        sessions: Vec<SessionInfo>,
    },
    /// Version report.
    Version {
        /// Machine-protocol version.
        protocol: String,
        /// Tuisnap version.
        tuisnap: String,
    },
    /// Capability report.
    Capabilities {
        /// What this build can do.
        capabilities: Capabilities,
    },
}
