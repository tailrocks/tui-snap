//! Typed op protocol (A01) + named sessions (A02) + trace journal (A04).
//!
//! One JSON-serializable [`Op`]/[`OpResult`]/[`OpError`] vocabulary shared by
//! the Rust library entry ([`execute`]) and the CLI machine mode (`tuisnap
//! --machine`: JSON lines in on stdin, JSON envelopes out on stdout).
//!
//! PTY-backed ops (`spawn`, `stdin`, `observe`, `snapshot`, `screenshot`,
//! `wait`, `exit`) run against an in-process session registry and require the
//! `pty` feature; without it they fail with code `unsupported`. Everything
//! else (version, capabilities, assert, render, diff, named sessions, record,
//! review, report) is feature-independent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    Spawned { session: String, pid: Option<u32> },
    InputAccepted { session: String },
    Observation { observation: ObservationView },
    Snapshot { screen: ScreenView },
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

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Machine-readable op failure. `code` is stable; `message` is human detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl OpError {
    #[must_use]
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            session: None,
        }
    }

    #[must_use]
    pub fn with_session(mut self, session: &str) -> Self {
        self.session = Some(session.to_string());
        self
    }
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.session {
            Some(s) => write!(f, "[{}] {s}: {}", self.code, self.message),
            None => write!(f, "[{}] {}", self.code, self.message),
        }
    }
}

impl std::error::Error for OpError {}

// ---------------------------------------------------------------------------
// Machine envelope (`--machine` JSON lines)
// ---------------------------------------------------------------------------

/// One output line of machine mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OpResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OpError>,
}

/// Parse one input line and execute it. Returns the output line plus whether
/// the op succeeded. Never panics on adversarial input.
pub fn run_machine_line(line: &str) -> (String, bool) {
    let env = match serde_json::from_str::<Op>(line) {
        Ok(op) => match execute(&op) {
            Ok(result) => Envelope {
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Envelope {
                ok: false,
                result: None,
                error: Some(error),
            },
        },
        Err(e) => Envelope {
            ok: false,
            result: None,
            error: Some(OpError::new("invalid-input", format!("not an Op: {e}"))),
        },
    };
    let ok = env.ok;
    let line = serde_json::to_string(&env).unwrap_or_else(|_| {
        r#"{"ok":false,"error":{"code":"internal","message":"envelope serialize failed"}}"#
            .to_string()
    });
    (line, ok)
}

/// JSON Schema (draft 2020-12 subset) for [`Op`], [`OpResult`] and the machine
/// envelope. Hand-written: the protocol makes no promise a codegen schema
/// would keep (e.g. `bytes_b64` must decode).
pub const PROTOCOL_SCHEMA_JSON: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tui-snap op protocol v1",
  "type": "object",
  "definitions": {
    "op": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["spawn", "stdin", "observe", "snapshot", "screenshot", "wait", "exit",
                   "assert", "render", "diff", "session-start", "session-stop",
                   "session-list", "version", "capabilities"]
        }
      },
      "allOf": [
        {"if": {"properties": {"type": {"const": "spawn"}}},
         "then": {"required": ["argv"],
                  "properties": {"argv": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                                 "id": {"type": "string"}, "cols": {"type": "integer"},
                                 "rows": {"type": "integer"}, "cwd": {"type": "string"},
                                 "env": {"type": "object" }}}},
        {"if": {"properties": {"type": {"const": "stdin"}}},
         "then": {"required": ["session"],
                  "properties": {"session": {"type": "string"}, "text": {"type": "string"},
                                 "chord": {"type": "string"}, "bytes_b64": {"type": "string"}}}},
        {"if": {"properties": {"type": {"const": "observe"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "snapshot"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "screenshot"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "wait"}}},
         "then": {"required": ["session", "kind"],
                  "properties": {"session": {"type": "string"}, "timeout_ms": {"type": "integer"},
                                 "kind": {"enum": ["text", "stable", "exit"]},
                                 "needle": {"type": "string"}, "quiet_ms": {"type": "integer"}}}},
        {"if": {"properties": {"type": {"const": "exit"}}},
         "then": {"required": ["session"],
                  "properties": {"session": {"type": "string"}, "timeout_ms": {"type": "integer"}}}},
        {"if": {"properties": {"type": {"const": "assert"}}},
         "then": {"required": ["check"],
                  "properties": {"check": {"enum": ["text-contains", "text-equals"]},
                                 "text": {"type": "string"}, "needle": {"type": "string"},
                                 "actual": {"type": "string"}, "expected": {"type": "string"}}}},
        {"if": {"properties": {"type": {"const": "render"}}},
         "then": {"required": ["frame_json", "format"],
                  "properties": {"frame_json": {"type": "string"},
                                 "format": {"enum": ["png", "ansi", "txt", "svg", "html"]}}}},
        {"if": {"properties": {"type": {"const": "diff"}}},
         "then": {"required": ["expected_png_b64", "actual_png_b64"]}},
        {"if": {"properties": {"type": {"const": "session-start"}}},
         "then": {"required": ["name", "argv"],
                  "properties": {"name": {"type": "string"},
                                 "argv": {"type": "array", "items": {"type": "string"}},
                                 "force": {"type": "boolean"}}}},
        {"if": {"properties": {"type": {"const": "session-stop"}}}, "then": {"required": ["name"]}}
      ]
    },
    "result": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["spawned", "input-accepted", "observation", "snapshot", "screenshot",
                   "waited", "exited", "asserted", "rendered", "diffed", "session",
                   "session-list", "version", "capabilities"]
        }
      }
    },
    "error": {
      "type": "object",
      "required": ["code", "message"],
      "properties": {
        "code": {"type": "string",
                 "enum": ["invalid-input", "unsupported", "not-found", "spawn-failed",
                          "timeout", "cancelled", "op-failed", "io", "render",
                          "session-exists", "owner-mismatch", "version-mismatch",
                          "bound-exceeded", "internal"]},
        "message": {"type": "string"},
        "session": {"type": "string"}
      }
    }
  },
  "properties": {
    "op": {"$ref": "#/definitions/op"},
    "envelope": {
      "type": "object",
      "required": ["ok"],
      "properties": {
        "ok": {"type": "boolean"},
        "result": {"$ref": "#/definitions/result"},
        "error": {"$ref": "#/definitions/error"}
      }
    }
  }
}"##;

// ---------------------------------------------------------------------------
// execute(): the library entry
// ---------------------------------------------------------------------------

/// Execute one op against the process-local session registry.
pub fn execute(op: &Op) -> Result<OpResult, OpError> {
    match op {
        Op::Version => Ok(OpResult::Version {
            protocol: PROTOCOL_VERSION.to_string(),
            tuisnap: env!("CARGO_PKG_VERSION").to_string(),
        }),
        Op::Capabilities => Ok(OpResult::Capabilities {
            capabilities: capabilities(),
        }),
        Op::Assert {
            check,
            text,
            needle,
            actual,
            expected,
        } => execute_assert(check, text, needle, actual, expected),
        Op::Render { frame_json, format } => execute_render(frame_json, format),
        Op::Diff {
            expected_png_b64,
            actual_png_b64,
        } => execute_diff(expected_png_b64, actual_png_b64),
        Op::SessionStart { name, argv, force } => {
            Ok(OpResult::Session {
                session: session_start(name, argv, *force)?,
            })
        }
        Op::SessionStop { name } => Ok(OpResult::Session {
            session: session_stop(name)?,
        }),
        Op::SessionList => Ok(OpResult::SessionList {
            sessions: session_list()?,
        }),
        #[cfg(feature = "pty")]
        Op::Spawn {
            argv,
            id,
            cols,
            rows,
            cwd,
            env,
        } => pty_registry::spawn(argv, id.clone(), *cols, *rows, cwd.clone(), env),
        #[cfg(feature = "pty")]
        Op::Stdin {
            session,
            text,
            chord,
            bytes_b64,
        } => pty_registry::stdin(session, text.clone(), chord.clone(), bytes_b64.clone()),
        #[cfg(feature = "pty")]
        Op::Observe { session } => pty_registry::observe(session),
        #[cfg(feature = "pty")]
        Op::Snapshot { session } => pty_registry::snapshot(session),
        #[cfg(feature = "pty")]
        Op::Screenshot { session } => pty_registry::screenshot(session),
        #[cfg(feature = "pty")]
        Op::Wait {
            session,
            kind,
            needle,
            quiet_ms,
            timeout_ms,
        } => pty_registry::wait(session, kind, needle.clone(), *quiet_ms, *timeout_ms),
        #[cfg(feature = "pty")]
        Op::Exit {
            session,
            timeout_ms,
        } => pty_registry::exit(session, *timeout_ms),
        #[cfg(not(feature = "pty"))]
        Op::Spawn { .. }
        | Op::Stdin { .. }
        | Op::Observe { .. }
        | Op::Snapshot { .. }
        | Op::Screenshot { .. }
        | Op::Wait { .. }
        | Op::Exit { .. } => Err(OpError::new(
            "unsupported",
            "PTY ops need the `pty` feature",
        )),
    }
}

fn execute_assert(
    check: &str,
    text: &Option<String>,
    needle: &Option<String>,
    actual: &Option<String>,
    expected: &Option<String>,
) -> Result<OpResult, OpError> {
    fn need<'a>(v: &'a Option<String>, check: &str, what: &str) -> Result<&'a str, OpError> {
        v.as_deref().ok_or_else(|| {
            OpError::new("invalid-input", format!("assert {check} needs `{what}`"))
        })
    }
    match check {
        assert_check::TEXT_CONTAINS => {
            let text = need(text, check, "text")?;
            let needle = need(needle, check, "needle")?;
            if needle.is_empty() {
                return Err(OpError::new("invalid-input", "needle must not be empty"));
            }
            let passed = text.contains(needle);
            Ok(OpResult::Asserted {
                passed,
                detail: if passed {
                    format!("text contains {needle:?}")
                } else {
                    format!("text ({} chars) lacks {needle:?}", text.len())
                },
            })
        }
        assert_check::TEXT_EQUALS => {
            let actual = need(actual, check, "actual")?;
            let expected = need(expected, check, "expected")?;
            let passed = actual == expected;
            Ok(OpResult::Asserted {
                passed,
                detail: if passed {
                    "texts equal".to_string()
                } else {
                    format!(
                        "lengths differ: actual {} vs expected {}",
                        actual.len(),
                        expected.len()
                    )
                },
            })
        }
        other => Err(OpError::new(
            "invalid-input",
            format!("unknown assert check {other:?} (want text-contains|text-equals)"),
        )),
    }
}

fn execute_render(frame_json: &str, format: &str) -> Result<OpResult, OpError> {
    use crate::profile::VENDORED_FACES;
    use crate::render::Renderer;
    let frame = crate::frame::Frame::from_json(frame_json)
        .map_err(|e| OpError::new("invalid-input", format!("bad frame JSON: {e}")))?;
    let profile = crate::profile::Profile::default_profile();
    let mut renderer = Renderer::new(&profile, &VENDORED_FACES)
        .map_err(|e| OpError::new("render", e.to_string()))?;
    match format {
        "png" => {
            let rendered = renderer
                .render(&frame)
                .map_err(|e| OpError::new("render", e.to_string()))?;
            Ok(OpResult::Rendered {
                format: format.to_string(),
                data: base64_encode(&rendered.png),
                data_b64: true,
            })
        }
        "ansi" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: crate::render::ansi_dump(&frame),
            data_b64: false,
        }),
        "txt" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: frame.text(),
            data_b64: false,
        }),
        "svg" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: crate::render::render_svg(&frame, &profile),
            data_b64: false,
        }),
        "html" => {
            let html = renderer
                .render_html(&frame, "frame")
                .map_err(|e| OpError::new("render", e.to_string()))?;
            Ok(OpResult::Rendered {
                format: format.to_string(),
                data: html,
                data_b64: false,
            })
        }
        other => Err(OpError::new(
            "invalid-input",
            format!("unknown format {other:?} (want png|ansi|txt|svg|html)"),
        )),
    }
}

fn execute_diff(expected_b64: &str, actual_b64: &str) -> Result<OpResult, OpError> {
    let expected = base64_decode(expected_b64)
        .map_err(|e| OpError::new("invalid-input", format!("bad expected PNG base64: {e}")))?;
    let actual = base64_decode(actual_b64)
        .map_err(|e| OpError::new("invalid-input", format!("bad actual PNG base64: {e}")))?;
    let verdict = crate::diff::compare_png(&expected, &actual)
        .map_err(|e| OpError::new("invalid-input", format!("PNG compare failed: {e}")))?;
    Ok(OpResult::Diffed {
        pixels_equal: verdict.pixels_equal,
        dims_equal: verdict.dims_equal,
        score: verdict.score,
    })
}

// ---------------------------------------------------------------------------
// Screen projections
// ---------------------------------------------------------------------------

/// Plain-text projection of a screen: symbols row by row, continuations
/// skipped, trailing whitespace trimmed per row.
#[must_use]
pub fn screen_text(screen: &crate::screen::Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        if y > 0 {
            out.push('\n');
        }
        let mut row = String::new();
        for x in 0..screen.cols() {
            if let Some(c) = screen.get(x, y) {
                if !c.continuation {
                    row.push_str(&c.symbol);
                }
            }
        }
        while row.ends_with([' ', '\t']) {
            row.pop();
        }
        out.push_str(&row);
    }
    out
}

#[must_use]
pub fn screen_view(screen: &crate::screen::Screen) -> ScreenView {
    let cursor = screen.cursor();
    ScreenView {
        cols: screen.cols(),
        rows: screen.rows(),
        text: screen_text(screen),
        cursor_x: cursor.x,
        cursor_y: cursor.y,
        cursor_visible: cursor.visible,
    }
}

#[must_use]
pub fn observation_view(obs: &crate::screen::Observation) -> ObservationView {
    ObservationView {
        revision: obs.revision,
        reason: format!("{:?}", obs.reason),
        screen: screen_view(&obs.screen),
    }
}

// ---------------------------------------------------------------------------
// Base64 (no new deps: small local implementation over base64 crate)
// ---------------------------------------------------------------------------

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PTY session registry (feature `pty`)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
mod pty_registry {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static REGISTRY: Mutex<Option<HashMap<String, crate::tui::Session>>> = Mutex::new(None);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn with_registry<T>(f: impl FnOnce(&mut HashMap<String, crate::tui::Session>) -> T) -> T {
        let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        f(map)
    }

    fn fresh_id() -> String {
        format!(
            "sess-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn tui_err(e: crate::tui::TuiError) -> OpError {
        let code = match &e {
            crate::tui::TuiError::Spawn(_) => "spawn-failed",
            crate::tui::TuiError::InvalidInput(_) | crate::tui::TuiError::Chord(_) => {
                "invalid-input"
            }
            crate::tui::TuiError::Unsupported(_)
            | crate::tui::TuiError::ModeNotEnabled(_)
            | crate::tui::TuiError::PasteRejected(_) => "unsupported",
            crate::tui::TuiError::ChildExited(_) | crate::tui::TuiError::Closed(_) => "not-found",
            crate::tui::TuiError::Timeout(_) => "timeout",
            crate::tui::TuiError::Io(_)
            | crate::tui::TuiError::Teardown(_)
            | crate::tui::TuiError::Signal(_)
            | crate::tui::TuiError::Assertion(_) => "op-failed",
        };
        OpError::new(code, e.to_string())
    }

    fn wait_err(e: crate::tui::WaitError) -> OpError {
        match &e {
            crate::tui::WaitError::Timeout { .. } => OpError::new("timeout", e.to_string()),
            crate::tui::WaitError::Cancelled { .. } => OpError::new("cancelled", e.to_string()),
            crate::tui::WaitError::Unsupported { .. } => OpError::new("unsupported", e.to_string()),
            crate::tui::WaitError::Closed { .. } => OpError::new("not-found", e.to_string()),
        }
    }

    pub fn spawn(
        argv: &[String],
        id: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        cwd: Option<PathBuf>,
        env: &HashMap<String, String>,
    ) -> Result<OpResult, OpError> {
        if argv.is_empty() {
            return Err(OpError::new("invalid-input", "spawn needs a non-empty argv"));
        }
        let id = id.unwrap_or_else(fresh_id);
        validate_session_id(&id)?;
        let mut builder = crate::tui::Tui::new(argv.to_vec());
        if let (Some(c), Some(r)) = (cols, rows) {
            builder = builder.size(c, r);
        } else if cols.is_some() || rows.is_some() {
            return Err(OpError::new(
                "invalid-input",
                "cols and rows must be given together",
            ));
        }
        for (k, v) in env {
            builder = builder.env(k, v);
        }
        if let Some(cwd) = cwd {
            builder = builder.cwd(cwd);
        }
        let session = builder.spawn().map_err(tui_err)?;
        let pid = session.pid();
        with_registry(|map| {
            if map.contains_key(&id) {
                return Err(OpError::new("session-exists", format!("{id} already spawned"))
                    .with_session(&id));
            }
            map.insert(id.clone(), session);
            Ok(OpResult::Spawned { session: id, pid })
        })
    }

    pub fn stdin(
        session: &str,
        text: Option<String>,
        chord: Option<String>,
        bytes_b64: Option<String>,
    ) -> Result<OpResult, OpError> {
        let set = [text.is_some(), chord.is_some(), bytes_b64.is_some()]
            .into_iter()
            .filter(|b| *b)
            .count();
        if set != 1 {
            return Err(OpError::new(
                "invalid-input",
                "stdin needs exactly one of text|chord|bytes_b64",
            )
            .with_session(session));
        }
        with_registry(|map| {
            let s = map
                .get(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))?;
            let r = if let Some(text) = text {
                if text.is_empty() {
                    return Err(OpError::new("invalid-input", "text must not be empty")
                        .with_session(session));
                }
                s.send_text(&text)
            } else if let Some(chord) = chord {
                s.press(&chord)
            } else if let Some(b64) = bytes_b64 {
                let bytes = base64_decode(&b64).map_err(|e| {
                    OpError::new("invalid-input", format!("bad bytes_b64: {e}"))
                        .with_session(session)
                })?;
                if bytes.is_empty() {
                    return Err(OpError::new("invalid-input", "bytes must not be empty")
                        .with_session(session));
                }
                s.send_bytes(&bytes)
            } else {
                unreachable!("counted above");
            };
            r.map_err(|e| tui_err(e).with_session(session))?;
            Ok(OpResult::InputAccepted {
                session: session.to_string(),
            })
        })
    }

    pub fn observe(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map
                .get(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))?;
            let obs = s.observe_now().map_err(|e| tui_err(e).with_session(session))?;
            Ok(OpResult::Observation {
                observation: observation_view(&obs),
            })
        })
    }

    pub fn snapshot(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map
                .get(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))?;
            let screen = s.snapshot().map_err(|e| tui_err(e).with_session(session))?;
            Ok(OpResult::Snapshot {
                screen: screen_view(&screen),
            })
        })
    }

    pub fn screenshot(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map
                .get(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))?;
            let obs = s.observe_now().map_err(|e| tui_err(e).with_session(session))?;
            let canonical = crate::insta_proto::insta_string(&obs.screen);
            let profile = crate::profile::Profile::default_profile();
            let mut renderer = crate::render::Renderer::new(&profile, &crate::profile::VENDORED_FACES)
                .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
            let rendered = renderer
                .render_screen(&obs.screen)
                .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
            Ok(OpResult::Screenshot {
                screen: screen_view(&obs.screen),
                canonical,
                png_b64: base64_encode(&rendered.png),
            })
        })
    }

    pub fn wait(
        session: &str,
        kind: &str,
        needle: Option<String>,
        quiet_ms: Option<u64>,
        timeout_ms: u64,
    ) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map
                .get(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))?;
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let cancel = crate::tui::CancelToken::new();
            match kind {
                wait_kind::TEXT => {
                    let needle = needle.as_ref().ok_or_else(|| {
                        OpError::new("invalid-input", "text wait needs `needle`")
                            .with_session(session)
                    })?;
                    if needle.is_empty() {
                        return Err(OpError::new("invalid-input", "needle must not be empty")
                            .with_session(session));
                    }
                    let obs = s
                        .wait_predicate(
                            |o| screen_text(&o.screen).contains(needle),
                            deadline,
                            &cancel,
                        )
                        .map_err(|e| wait_err(e).with_session(session))?;
                    Ok(OpResult::Waited {
                        session: session.to_string(),
                        observation: observation_view(&obs),
                    })
                }
                wait_kind::STABLE => {
                    let quiet = Duration::from_millis(quiet_ms.unwrap_or(200));
                    let obs = s
                        .wait_stable_quiet(deadline, quiet, &cancel)
                        .map_err(|e| wait_err(e).with_session(session))?;
                    Ok(OpResult::Waited {
                        session: session.to_string(),
                        observation: observation_view(&obs),
                    })
                }
                wait_kind::EXIT => {
                    let ew = s
                        .wait_exit(deadline, &cancel)
                        .map_err(|e| wait_err(e).with_session(session))?;
                    Ok(OpResult::Exited {
                        session: session.to_string(),
                        code: ew.status.code(),
                        signal: ew.status.signal().map(str::to_string),
                        observation: observation_view(&ew.observation),
                    })
                }
                other => Err(OpError::new(
                    "invalid-input",
                    format!("unknown wait kind {other:?} (want text|stable|exit)"),
                )
                .with_session(session)),
            }
        })
    }

    pub fn exit(session: &str, timeout_ms: u64) -> Result<OpResult, OpError> {
        let mut s = with_registry(|map| {
            map.remove(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))
        })?;
        // Graceful wait first so `exit` on a running app reaps evidence.
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let cancel = crate::tui::CancelToken::new();
        match s.wait_exit(deadline, &cancel) {
            Ok(ew) => {
                let _ = s.close();
                Ok(OpResult::Exited {
                    session: session.to_string(),
                    code: ew.status.code(),
                    signal: ew.status.signal().map(str::to_string),
                    observation: observation_view(&ew.observation),
                })
            }
            Err(crate::tui::WaitError::Timeout { evidence, .. }) => {
                let _ = s.close();
                Err(OpError::new(
                    "timeout",
                    format!(
                        "child still running after {timeout_ms}ms (evidence at revision {})",
                        evidence.revision
                    ),
                )
                .with_session(session))
            }
            Err(e) => {
                let _ = s.close();
                Err(wait_err(e).with_session(session))
            }
        }
    }

    fn validate_session_id(id: &str) -> Result<(), OpError> {
        if id.is_empty() || id.len() > 128 {
            return Err(OpError::new("invalid-input", "session id must be 1..=128 chars"));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(OpError::new(
                "invalid-input",
                "session id allows only [A-Za-z0-9_.-]",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Named sessions (A02): versioned endpoints, owner-only runtime dir
// ---------------------------------------------------------------------------

/// Endpoint file format version. A reader that sees another version refuses
/// the file instead of guessing.
pub const SESSION_ENDPOINT_VERSION: u32 = 1;

/// Backend that owns the named session's child.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionBackend {
    /// Plain piped child (this version). PTY-backed named sessions arrive
    /// with the daemon transport; the enum reserves the shape.
    Process,
    Pty,
}

/// Liveness of a named session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Running,
    Exited,
}

/// What `session list` reports per session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub name: String,
    pub pid: u32,
    pub argv: Vec<String>,
    pub backend: SessionBackend,
    pub status: SessionStatus,
    pub started_unix: u64,
}

/// On-disk endpoint record.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionEndpoint {
    version: u32,
    name: String,
    pid: u32,
    argv: Vec<String>,
    backend: SessionBackend,
    started_unix: u64,
    /// Owner uid when known (Unix with `pty` feature's libc).
    owner: Option<u32>,
}

/// Runtime dir: `$TUISNAP_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/tuisnap`, else a
/// per-uid temp dir. Created owner-only (0o700) on Unix.
pub fn runtime_dir() -> Result<PathBuf, OpError> {
    let dir = if let Ok(d) = std::env::var("TUISNAP_RUNTIME_DIR") {
        PathBuf::from(d)
    } else if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(d).join("tuisnap")
    } else {
        std::env::temp_dir().join(format!("tuisnap-{}", current_uid()))
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| OpError::new("io", format!("runtime dir {}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dir)
            .map_err(|e| OpError::new("io", format!("stat {}: {e}", dir.display())))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
                OpError::new("io", format!("chmod 700 {}: {e}", dir.display()))
            })?;
        }
    }
    Ok(dir)
}

fn current_uid() -> u32 {
    #[cfg(all(unix, feature = "pty"))]
    {
        // SAFETY: getuid takes no arguments and has no memory effects.
        unsafe { libc::getuid() }
    }
    #[cfg(not(all(unix, feature = "pty")))]
    {
        0
    }
}

fn validate_session_name(name: &str) -> Result<(), OpError> {
    if name.is_empty() || name.len() > 64 {
        return Err(OpError::new("invalid-input", "session name must be 1..=64 chars"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        || name == "."
        || name == ".."
    {
        return Err(OpError::new(
            "invalid-input",
            "session name allows only [A-Za-z0-9_.-] and must not be . or ..",
        ));
    }
    Ok(())
}

fn endpoint_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

fn read_endpoint(dir: &Path, name: &str) -> Result<Option<SessionEndpoint>, OpError> {
    let path = endpoint_path(dir, name);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(OpError::new(
                "io",
                format!("read {}: {e}", path.display()),
            ))
        }
    };
    let ep: SessionEndpoint = serde_json::from_slice(&bytes)
        .map_err(|e| OpError::new("invalid-input", format!("{} is corrupt: {e}", path.display())))?;
    if ep.version != SESSION_ENDPOINT_VERSION {
        return Err(OpError::new(
            "version-mismatch",
            format!(
                "{}: endpoint v{} vs reader v{SESSION_ENDPOINT_VERSION}",
                path.display(),
                ep.version
            ),
        ));
    }
    if let Some(owner) = ep.owner {
        let me = current_uid();
        if owner != me {
            return Err(OpError::new(
                "owner-mismatch",
                format!("{name} belongs to uid {owner}, not {me}"),
            ));
        }
    }
    Ok(Some(ep))
}

/// Atomic endpoint write (tmp file + rename).
fn write_endpoint(dir: &Path, ep: &SessionEndpoint) -> Result<(), OpError> {
    let path = endpoint_path(dir, &ep.name);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(ep)
        .map_err(|e| OpError::new("io", format!("encode {}: {e}", path.display())))?;
    std::fs::write(&tmp, &bytes)
        .map_err(|e| OpError::new("io", format!("write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| OpError::new("io", format!("publish {}: {e}", path.display())))?;
    Ok(())
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Exit code of `kill -0`: portable, no extra deps.
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn kill_pid(pid: u32) -> Result<(), OpError> {
    #[cfg(all(unix, feature = "pty"))]
    {
        // SAFETY: kill(2) with a PID and signal number has no memory effects.
        let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(OpError::new("io", format!("SIGTERM {pid}: {err}")));
            }
        }
        Ok(())
    }
    #[cfg(not(all(unix, feature = "pty")))]
    {
        #[cfg(unix)]
        {
            let st = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .map_err(|e| OpError::new("io", format!("kill {pid}: {e}")))?;
            if st.success() {
                Ok(())
            } else {
                Err(OpError::new("io", format!("kill -TERM {pid} failed")))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Err(OpError::new("unsupported", "session stop needs Unix"))
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Start a named session: spawn `argv` detached (output to the session log),
/// publish the endpoint. A live same-name session is a `session-exists` error
/// unless `force` stops it first.
pub fn session_start(name: &str, argv: &[String], force: bool) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    if argv.is_empty() {
        return Err(OpError::new("invalid-input", "session start needs argv"));
    }
    let dir = runtime_dir()?;
    if let Some(ep) = read_endpoint(&dir, name)? {
        if pid_alive(ep.pid) {
            if !force {
                return Err(OpError::new(
                    "session-exists",
                    format!("{name} already running (pid {})", ep.pid),
                ));
            }
            session_stop(name)?;
        } else {
            std::fs::remove_file(endpoint_path(&dir, name)).map_err(|e| {
                OpError::new("io", format!("remove stale {name}: {e}"))
            })?;
        }
    }
    let log_path = dir.join(format!("{name}.log"));
    let log = std::fs::File::create(&log_path)
        .map_err(|e| OpError::new("io", format!("log {}: {e}", log_path.display())))?;
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| OpError::new("io", e.to_string()))?)
        .stderr(log);
    #[cfg(all(unix, feature = "pty"))]
    {
        // Detach: the session outlives the `session start` process.
        // SAFETY: setsid between fork and exec has no memory effects.
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| OpError::new("spawn-failed", format!("{}: {e}", argv[0])))?;
    let ep = SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid: child.id(),
        argv: argv.to_vec(),
        backend: SessionBackend::Process,
        started_unix: now_unix(),
        owner: Some(current_uid()),
    };
    write_endpoint(&dir, &ep)?;
    // Reaper thread: the child runs detached (own session), but until this
    // process exits it is still ours — without a wait it would linger as a
    // zombie and `pid_alive` would misreport it. The thread only reaps.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(SessionInfo {
        name: ep.name,
        pid: ep.pid,
        argv: ep.argv,
        backend: ep.backend,
        status: SessionStatus::Running,
        started_unix: ep.started_unix,
    })
}

/// Stop a named session: SIGTERM the recorded pid (best effort when already
/// dead), remove the endpoint. Returns the last known info.
pub fn session_stop(name: &str) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    let dir = runtime_dir()?;
    let ep = read_endpoint(&dir, name)?.ok_or_else(|| OpError::new("not-found", name))?;
    let alive = pid_alive(ep.pid);
    if alive {
        kill_pid(ep.pid)?;
        // Brief grace, then SIGKILL via the same helper path.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while pid_alive(ep.pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        #[cfg(all(unix, feature = "pty"))]
        if pid_alive(ep.pid) {
            // SAFETY: kill(2) with a PID and signal number has no memory effects.
            unsafe {
                libc::kill(ep.pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
    std::fs::remove_file(endpoint_path(&dir, name))
        .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))?;
    Ok(SessionInfo {
        name: ep.name,
        pid: ep.pid,
        argv: ep.argv,
        backend: ep.backend,
        status: if alive {
            SessionStatus::Running
        } else {
            SessionStatus::Exited
        },
        started_unix: ep.started_unix,
    })
}

/// List all valid endpoints with liveness. Corrupt files are skipped only via
/// [`session_prune`]'s report; here a corrupt file is an error.
pub fn session_list() -> Result<Vec<SessionInfo>, OpError> {
    let dir = runtime_dir()?;
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if stem.contains('.') || entry.file_type().map(|t| !t.is_file()).unwrap_or(true) {
            continue;
        }
        if let Some(ep) = read_endpoint(&dir, stem)? {
            out.push(SessionInfo {
                name: ep.name,
                pid: ep.pid,
                argv: ep.argv,
                backend: ep.backend,
                status: if pid_alive(ep.pid) {
                    SessionStatus::Running
                } else {
                    SessionStatus::Exited
                },
                started_unix: ep.started_unix,
            });
        }
    }
    Ok(out)
}

/// Remove endpoints whose pid is dead. Returns the pruned names.
pub fn session_prune() -> Result<Vec<String>, OpError> {
    let dir = runtime_dir()?;
    let mut pruned = Vec::new();
    for info in session_list()? {
        if info.status == SessionStatus::Exited {
            std::fs::remove_file(endpoint_path(&dir, &info.name)).map_err(|e| {
                OpError::new("io", format!("prune {}: {e}", info.name))
            })?;
            pruned.push(info.name);
        }
    }
    Ok(pruned)
}

// ---------------------------------------------------------------------------
// Bounded recording (A04/A06 partial)
// ---------------------------------------------------------------------------

/// One journal event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEvent {
    pub seq: u64,
    pub kind: String,
    pub detail: String,
}

/// Append-only JSONL recorder with hard bounds. Exceeding a bound is an
/// error, never silent truncation.
pub struct Recorder {
    file: std::fs::File,
    seq: u64,
    bytes: u64,
    max_events: u64,
    max_bytes: u64,
}

impl Recorder {
    pub fn create(path: &Path, max_events: u64, max_bytes: u64) -> Result<Self, OpError> {
        if max_events == 0 || max_bytes == 0 {
            return Err(OpError::new("invalid-input", "record bounds must be nonzero"));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| OpError::new("io", format!("mkdir {}: {e}", parent.display())))?;
            }
        }
        let file = std::fs::File::create(path)
            .map_err(|e| OpError::new("io", format!("create {}: {e}", path.display())))?;
        Ok(Self {
            file,
            seq: 0,
            bytes: 0,
            max_events,
            max_bytes,
        })
    }

    pub fn record(&mut self, kind: &str, detail: &str) -> Result<(), OpError> {
        if self.seq >= self.max_events {
            return Err(OpError::new(
                "bound-exceeded",
                format!("event cap {} reached", self.max_events),
            ));
        }
        let ev = JournalEvent {
            seq: self.seq,
            kind: kind.to_string(),
            detail: detail.to_string(),
        };
        let mut line = serde_json::to_vec(&ev)
            .map_err(|e| OpError::new("io", format!("encode event: {e}")))?;
        line.push(b'\n');
        if self.bytes + line.len() as u64 > self.max_bytes {
            return Err(OpError::new(
                "bound-exceeded",
                format!("byte cap {} reached", self.max_bytes),
            ));
        }
        use std::io::Write;
        self.file
            .write_all(&line)
            .map_err(|e| OpError::new("io", format!("append journal: {e}")))?;
        self.seq += 1;
        self.bytes += line.len() as u64;
        Ok(())
    }

    #[must_use]
    pub fn events(&self) -> u64 {
        self.seq
    }
}

/// Read a journal back (offline; used by `trace`).
pub fn read_journal(path: &Path) -> Result<Vec<JournalEvent>, OpError> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)
        .map_err(|e| OpError::new("io", format!("open {}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| OpError::new("io", format!("read {}: {e}", path.display())))?;
        if line.trim().is_empty() {
            continue;
        }
        let ev: JournalEvent = serde_json::from_str(&line).map_err(|e| {
            OpError::new(
                "invalid-input",
                format!("{} line {}: bad event: {e}", path.display(), n + 1),
            )
        })?;
        out.push(ev);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Offline review/report
// ---------------------------------------------------------------------------

/// One offline verdict file (`<name>.verdict.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verdict {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

impl Verdict {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.status == "pass"
    }
}

/// Read all `*.verdict.json` files in `dir` (sorted by name). Non-verdict
/// files are ignored; a malformed verdict file is an error.
pub fn read_verdicts(dir: &Path) -> Result<Vec<Verdict>, OpError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?;
    entries.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".verdict.json") {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .map_err(|e| OpError::new("io", format!("read {}: {e}", name)))?;
        let v: Verdict = serde_json::from_slice(&bytes)
            .map_err(|e| OpError::new("invalid-input", format!("{name}: bad verdict: {e}")))?;
        out.push(v);
    }
    Ok(out)
}

/// Write a standalone offline HTML report from `verdicts`. Pure rendering over
/// the given verdicts; reads nothing else.
pub fn write_html_report(verdicts: &[Verdict], title: &str) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let passed = verdicts.iter().filter(|v| v.passed()).count();
    let failed = verdicts.len() - passed;
    let mut rows = String::new();
    for v in verdicts {
        let cls = if v.passed() { "pass" } else { "fail" };
        rows.push_str(&format!(
            "<tr class=\"{cls}\"><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            esc(&v.name),
            esc(&v.status),
            esc(&v.detail)
        ));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{t}</title>\
<style>body{{font-family:sans-serif}}table{{border-collapse:collapse}}\
td,th{{border:1px solid #ccc;padding:4px 8px}}.pass td{{background:#e6f4ea}}\
.fail td{{background:#fce8e6}}</style></head><body><h1>{t}</h1>\
<p>{} passed, {} failed (protocol v{})</p>\
<table><tr><th>name</th><th>status</th><th>detail</th></tr>\n{rows}</table></body></html>\n",
        passed,
        failed,
        PROTOCOL_VERSION,
        t = esc(title),
    )
}
