// ---------------------------------------------------------------------------
// Retained-session daemon wire protocol (F08-F2): newline-delimited JSON
// ---------------------------------------------------------------------------
//
// One request per connection: the client sends a single `\n`-terminated
// JSON [`DaemonRequest`], the daemon replies with a single `\n`-terminated
// [`DaemonResponse`] and closes. Requests are bounded at 1 MiB (mirroring
// the endpoint bound); responses are daemon-generated and bounded by the
// grid being projected (the client caps its read well above any honest
// frame). Every request carries an `id` the response echoes, so a crossed
// or replayed reply cannot be mistaken for the live answer.

use serde::{Deserialize, Serialize};

use super::OpError;

/// Largest request line the daemon reads: mirrors `MAX_ENDPOINT_BYTES`.
#[cfg(any(test, all(unix, feature = "pty")))]
pub(crate) const MAX_REQUEST_BYTES: usize = 1_048_576;

/// Largest response line the client reads. Honest projections stay far
/// below this (a 1000x1000 grid's text is ~1M chars); anything past it is
/// treated as a corrupt stream, not a bigger frame.
pub(crate) const MAX_RESPONSE_BYTES: usize = 16_777_216;

/// Socket read/write timeout, both ends. Must exceed the 10 s worker
/// round-trip inside `tui::Session` (observe/resize round trips); waits
/// bound themselves via `timeout_ms` below this ceiling.
pub(crate) const IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Longest `wait` the daemon serves on one connection: the conn thread
/// lives at most this long past a client disconnect (bounded linger).
#[cfg(any(test, all(unix, feature = "pty")))]
pub(crate) const MAX_WAIT_MS: u64 = 60_000;

/// One client request. Field validation beyond shape (names, wait kinds,
/// signal names, base64) happens at dispatch, not at parse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DaemonRequest {
    /// Client-chosen correlation id, echoed by the response.
    pub(crate) id: u64,
    /// The op to run.
    pub(crate) op: DaemonOp,
}

/// Daemon ops. Results reuse [`OpResult`](super::OpResult) JSON shapes
/// where one fits exactly (`session`, `observation`, `snapshot`,
/// `input-accepted`, `waited`, `exited`); the remainder (`resized`,
/// `signaled`, `sessions`, `pruned`) are small daemon-local shapes —
/// minting new `OpResult` variants would change the versioned machine
/// protocol, which F08-F2 deliberately leaves alone (CLI-only surface).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub(crate) enum DaemonOp {
    /// Reserve `name`, spawn the child in a PTY, publish the endpoint.
    Start {
        /// Session name (64-char `validate_session_name`, enforced here —
        /// not the looser 128-char machine-protocol id rule).
        name: String,
        /// Child argv, each entry base64 (byte-exact spawn; the endpoint
        /// keeps a lossy UTF-8 projection, diagnostic only).
        argv_b64: Vec<String>,
        /// Child working directory (must be absolute when present).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// PTY width; must pair with `rows`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cols: Option<u16>,
        /// PTY height; must pair with `cols`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u16>,
        /// Stop a live same-name session first.
        #[serde(default)]
        force: bool,
    },
    /// Deliver input (exactly one of the three payloads).
    Input {
        /// Session name.
        name: String,
        /// Literal text to type.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// Key chord to press.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chord: Option<String>,
        /// Raw bytes, base64.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_b64: Option<String>,
    },
    /// Current observation projection.
    Observe {
        /// Session name.
        name: String,
    },
    /// Current screen projection.
    Snapshot {
        /// Session name.
        name: String,
    },
    /// Wait for a condition (`text`|`stable`|`exit`).
    Wait {
        /// Session name.
        name: String,
        /// See `wait_kind`.
        kind: String,
        /// Text to wait for (`text` waits).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        needle: Option<String>,
        /// Quiet window for `stable` waits.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quiet_ms: Option<u64>,
        /// Give up after this long (capped at [`MAX_WAIT_MS`]).
        timeout_ms: u64,
    },
    /// Resize the PTY and emulator together.
    Resize {
        /// Session name.
        name: String,
        /// New width in cells.
        cols: u16,
        /// New height in cells.
        rows: u16,
    },
    /// Deliver a named signal (`int|term|kill|quit|hup`).
    Signal {
        /// Session name.
        name: String,
        /// Signal name.
        sig: String,
    },
    /// TERM, grace, owned kill+reap, remove the endpoint.
    Stop {
        /// Session name.
        name: String,
    },
    /// Liveness of every registry entry.
    List,
    /// Drop the named sessions and remove their endpoints.
    Prune {
        /// Session names to prune.
        names: Vec<String>,
    },
}

impl DaemonOp {
    /// The session name this op targets, when it targets one. The daemon
    /// re-validates it with `validate_session_name` before dispatch (the
    /// socket is trusted-local; defense in depth is cheap).
    #[cfg(any(test, all(unix, feature = "pty")))]
    pub(crate) fn target_name(&self) -> Option<&str> {
        match self {
            Self::Start { name, .. }
            | Self::Input { name, .. }
            | Self::Observe { name }
            | Self::Snapshot { name }
            | Self::Wait { name, .. }
            | Self::Resize { name, .. }
            | Self::Signal { name, .. }
            | Self::Stop { name } => Some(name),
            Self::List | Self::Prune { .. } => None,
        }
    }
}

/// One daemon reply: exactly one of `result`/`error` is present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DaemonResponse {
    /// Echo of the request `id` (0 when no request parsed).
    pub(crate) id: u64,
    /// True when the op succeeded.
    pub(crate) ok: bool,
    /// Op result JSON, on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<serde_json::Value>,
    /// Op error, on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<OpError>,
}

impl DaemonResponse {
    /// Successful reply carrying `result`.
    #[cfg(any(test, all(unix, feature = "pty")))]
    pub(crate) fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    /// Failed reply carrying `error`.
    #[cfg(any(test, all(unix, feature = "pty")))]
    pub(crate) fn err(id: u64, error: OpError) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

/// Parse one request line (the trailing newline need not be present).
/// Shape errors are `invalid-input`; semantic checks run at dispatch.
#[cfg(any(test, all(unix, feature = "pty")))]
pub(crate) fn parse_request(line: &[u8]) -> Result<DaemonRequest, OpError> {
    serde_json::from_slice(line)
        .map_err(|e| OpError::new("invalid-input", format!("bad IPC request: {e}")))
}

/// Render one response line (without the trailing newline).
#[cfg(any(test, all(unix, feature = "pty")))]
pub(crate) fn render_response(res: &DaemonResponse) -> Result<Vec<u8>, OpError> {
    serde_json::to_vec(res)
        .map_err(|e| OpError::new("internal", format!("encode IPC response: {e}")))
}

/// Read one `\n`-terminated line (newline excluded), capped at `cap`
/// bytes. `Ok(None)` is a clean EOF before any byte (peer went away);
/// EOF mid-line and over-cap lines are errors, never silent truncations.
pub(crate) fn read_line_capped(
    reader: &mut impl std::io::Read,
    cap: usize,
) -> Result<Option<Vec<u8>>, OpError> {
    let mut line: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) if line.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(OpError::new("invalid-input", "EOF before end of IPC line"));
            }
            Ok(n) => {
                for byte in chunk.iter().take(n) {
                    line.push(*byte);
                    if line.len() > cap {
                        return Err(OpError::new(
                            "bound-exceeded",
                            format!("IPC line exceeds {cap} bytes"),
                        ));
                    }
                    if *byte == b'\n' {
                        line.pop();
                        return Ok(Some(line));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                return Err(OpError::new("timeout", format!("IPC read: {e}")));
            }
            Err(e) => return Err(OpError::new("io", format!("IPC read: {e}"))),
        }
    }
}

// Non-Unix builds carry no daemon: reference every item so the module
// stays warning-free there too (the `Session: Send + Sync` const-assert
// in `tui::session` uses the same trick).
#[cfg(not(unix))]
const _: () = {
    let _ = MAX_RESPONSE_BYTES;
    let _ = IPC_TIMEOUT;
    let _ = std::mem::size_of::<DaemonRequest>();
    let _ = std::mem::size_of::<DaemonOp>();
    let _ = std::mem::size_of::<DaemonResponse>();
    let _ = read_line_capped::<&[u8]>;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shapes_round_trip() {
        let req = DaemonRequest {
            id: 7,
            op: DaemonOp::Start {
                name: "a.b".to_string(),
                argv_b64: vec!["c2g=".to_string()],
                cwd: None,
                cols: Some(80),
                rows: Some(24),
                force: true,
            },
        };
        let bytes = serde_json::to_vec(&req).expect("encode");
        let text = String::from_utf8(bytes.clone()).expect("utf8");
        assert!(text.contains(r#""op":"start""#), "{text}");
        let back = parse_request(&bytes).expect("parse");
        assert_eq!(back.id, 7);
        assert_eq!(back.op.target_name(), Some("a.b"));
        assert_eq!(DaemonOp::List.target_name(), None);
        assert_eq!(DaemonOp::Prune { names: vec![] }.target_name(), None);
    }

    #[test]
    fn request_parse_rejects_garbage() {
        for bad in ["", "{", "[]", r#"{"id":1}"#, "[summary truncated]"] {
            let e = parse_request(bad.as_bytes()).expect_err("garbage accepted");
            assert_eq!(e.code, "invalid-input", "{bad:?}");
        }
    }

    #[test]
    fn response_ok_err_envelopes() {
        let ok = DaemonResponse::ok(3, serde_json::json!({"resized": "x"}));
        let bytes = render_response(&ok).expect("encode");
        let back: DaemonResponse = serde_json::from_slice(&bytes).expect("decode");
        assert!(back.ok && back.error.is_none());
        assert_eq!(back.id, 3);
        assert_eq!(back.result, Some(serde_json::json!({"resized": "x"})));
        let err = DaemonResponse::err(4, OpError::new("not-found", "gone"));
        let bytes = render_response(&err).expect("encode");
        let back: DaemonResponse = serde_json::from_slice(&bytes).expect("decode");
        assert!(!back.ok && back.result.is_none());
        assert_eq!(back.error.expect("error").code, "not-found");
    }

    #[test]
    fn bounds_mirror_endpoints_and_exceed_worker_round_trips() {
        // Requests mirror the endpoint bound; the socket timeout must
        // exceed the 10 s worker round trip inside `tui::Session`.
        assert_eq!(MAX_REQUEST_BYTES, 1_048_576);
        assert_eq!(MAX_RESPONSE_BYTES, 16_777_216);
        assert!(IPC_TIMEOUT.as_secs() > 10);
        assert_eq!(MAX_WAIT_MS, 60_000);
    }

    #[test]
    fn framed_read_clean_eof_midline_and_caps() {
        let mut eof = std::io::Cursor::new(b"");
        assert!(read_line_capped(&mut eof, 16).expect("eof").is_none());
        let mut line = std::io::Cursor::new(b"{\"a\":1}\nrest");
        let got = read_line_capped(&mut line, 16)
            .expect("line")
            .expect("some");
        assert_eq!(got, b"{\"a\":1}");
        let mut mid = std::io::Cursor::new(b"abc");
        let e = read_line_capped(&mut mid, 16).expect_err("mid-line EOF accepted");
        assert_eq!(e.code, "invalid-input");
        let mut big = std::io::Cursor::new(vec![b'x'; 32]);
        let e = read_line_capped(&mut big, 16).expect_err("over-cap accepted");
        assert_eq!(e.code, "bound-exceeded");
    }
}
