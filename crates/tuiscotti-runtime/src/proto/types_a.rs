use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
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
    pub protocol: String,
    pub tuisnap: String,
    pub pty: bool,
    pub render: bool,
    pub record: bool,
    pub platform: String,
}

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
insta config         Snapshot review behaviour ([`INSTA_UPDATE`], snapshot\n\
                     paths). Owned by Insta; tui-snap honours it and pins\n\
                     `INSTA_UPDATE=no` only inside its own frozen checks.\n";

// ---------------------------------------------------------------------------
// Ops
// ---------------------------------------------------------------------------

/// `kind` values for [`Op::Wait`]: `text` (screen contains `needle`),
/// `stable` (no new revision for `quiet_ms`, default 200), `exit`.
pub mod wait_kind {
    pub const TEXT: &str = "text";
    pub const STABLE: &str = "stable";
    pub const EXIT: &str = "exit";
}

/// `check` values for [`Op::Assert`]: `text-contains`, `text-equals`.
pub mod assert_check {
    pub const TEXT_CONTAINS: &str = "text-contains";
    pub const TEXT_EQUALS: &str = "text-equals";
}

/// One typed operation. `#[serde(tag = "type")]`: each line on the machine
/// protocol is one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Op {
    Spawn {
        argv: Vec<String>,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        cols: Option<u16>,
        #[serde(default)]
        rows: Option<u16>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Stdin {
        session: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        chord: Option<String>,
        /// Raw bytes, base64.
        #[serde(default)]
        bytes_b64: Option<String>,
    },
    Observe {
        session: String,
    },
    Snapshot {
        session: String,
    },
    Screenshot {
        session: String,
    },
    Wait {
        session: String,
        /// See [`wait_kind`]: `text` | `stable` | `exit`.
        kind: String,
        #[serde(default)]
        needle: Option<String>,
        #[serde(default)]
        quiet_ms: Option<u64>,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Wait for natural exit until `timeout_ms`, reap, drop the session.
    Exit {
        session: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    Assert {
        /// See [`assert_check`]: `text-contains` | `text-equals`.
        check: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        needle: Option<String>,
        #[serde(default)]
        actual: Option<String>,
        #[serde(default)]
        expected: Option<String>,
    },
    Render {
        frame_json: String,
        format: String,
    },
    Diff {
        expected_png_b64: String,
        actual_png_b64: String,
    },
    SessionStart {
        name: String,
        argv: Vec<String>,
        #[serde(default)]
        force: bool,
    },
    SessionStop {
        name: String,
    },
    SessionList,
    Version,
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
    pub cols: u16,
    pub rows: u16,
    pub text: String,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_visible: bool,
}

/// Serializable observation projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationView {
    pub revision: u64,
    pub reason: String,
    pub screen: ScreenView,
}

/// Typed op result. Results carry evidence, never bare success flags alone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum OpResult {
    Spawned {
        session: String,
        pid: Option<u32>,
    },
    InputAccepted {
        session: String,
    },
    Observation {
        observation: ObservationView,
    },
    Snapshot {
        screen: ScreenView,
    },
    Screenshot {
        screen: ScreenView,
        canonical: String,
        png_b64: String,
    },
    Waited {
        session: String,
        observation: ObservationView,
    },
    Exited {
        session: String,
        code: u32,
        signal: Option<String>,
        observation: ObservationView,
    },
    Asserted {
        passed: bool,
        detail: String,
    },
    Rendered {
        format: String,
        /// PNG/SVG/HTML/ANSI/TXT payload; PNG is base64.
        data: String,
        data_b64: bool,
    },
    Diffed {
        pixels_equal: bool,
        dims_equal: bool,
        score: f64,
    },
    Session {
        session: SessionInfo,
    },
    SessionList {
        sessions: Vec<SessionInfo>,
    },
    Version {
        protocol: String,
        tuisnap: String,
    },
    Capabilities {
        capabilities: Capabilities,
    },
}
