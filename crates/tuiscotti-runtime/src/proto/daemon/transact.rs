//! Daemon client: one request per connection.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;

use super::super::DaemonOp;
use super::super::OpError;
#[cfg(unix)]
use super::super::{
    DaemonRequest, DaemonResponse, IPC_TIMEOUT, MAX_RESPONSE_BYTES, checked_daemon_path,
    read_line_capped, runtime_dir,
};
#[cfg(unix)]
use super::status::check_socket_owner;

// ---------------------------------------------------------------------------
// Client: one request per connection
// ---------------------------------------------------------------------------

/// Connection correlation ids.
#[cfg(unix)]
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Run one op against the serving daemon (no autostart, no retry: the
/// caller owns both policies). Verifies the echoed id and unwraps the
/// envelope into the result value or the daemon's own [`OpError`].
#[cfg(unix)]
pub(crate) fn transact(op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let dir = runtime_dir()?;
    let sock = checked_daemon_path(&dir, "sock")?;
    refuse_unusable_socket(&sock)?;
    let mut stream = UnixStream::connect(&sock)
        .map_err(|e| OpError::new("io", format!("connect {}: {e}", sock.display())))?;
    transact_stream(&mut stream, op)
}

/// Transact over an already-connected stream (the unit-test seam:
/// `UnixStream::pair` stands in for a daemon).
#[cfg(unix)]
fn transact_stream(stream: &mut UnixStream, op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    stream
        .set_read_timeout(Some(IPC_TIMEOUT))
        .map_err(|e| OpError::new("io", format!("socket timeout: {e}")))?;
    stream
        .set_write_timeout(Some(IPC_TIMEOUT))
        .map_err(|e| OpError::new("io", format!("socket timeout: {e}")))?;
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let req = serde_json::to_vec(&DaemonRequest { id, op: op.clone() })
        .map_err(|e| OpError::new("internal", format!("encode IPC request: {e}")))?;
    write_line(stream, &req)?;
    let line = read_line_capped(stream, MAX_RESPONSE_BYTES)?
        .ok_or_else(|| OpError::new("io", "daemon closed the connection"))?;
    let res: DaemonResponse = serde_json::from_slice(&line)
        .map_err(|e| OpError::new("io", format!("bad daemon response: {e}")))?;
    if res.id != id {
        return Err(OpError::new("io", "daemon response id mismatch"));
    }
    if res.ok {
        res.result
            .ok_or_else(|| OpError::new("internal", "daemon ok without a result"))
    } else {
        Err(res
            .error
            .unwrap_or_else(|| OpError::new("internal", "daemon failed without an error")))
    }
}

/// The socket must exist, must not be a symlink, and must be ours.
#[cfg(unix)]
fn refuse_unusable_socket(sock: &Path) -> Result<(), OpError> {
    let meta = std::fs::symlink_metadata(sock).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            OpError::new("io", format!("no daemon socket at {}", sock.display()))
        } else {
            OpError::new("io", format!("stat {}: {e}", sock.display()))
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to connect", sock.display()),
        ));
    }
    check_socket_owner(sock, &meta)
}

/// Write one `\n`-terminated line.
#[cfg(unix)]
fn write_line(stream: &mut UnixStream, bytes: &[u8]) -> Result<(), OpError> {
    use std::io::Write as _;
    stream
        .write_all(bytes)
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|e| OpError::new("io", format!("IPC write: {e}")))
}

/// Non-Unix builds have no socket to transact on.
#[cfg(not(unix))]
pub(crate) fn transact(op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let _ = op;
    Err(OpError::new("unsupported", "the session daemon needs Unix"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn transact_stream_round_trip_and_errors() {
        // Fake daemon over a socket pair: speaks the real envelope.
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = match req.op {
                DaemonOp::List => DaemonResponse::ok(req.id, serde_json::json!({"sessions": []})),
                DaemonOp::Stop { .. } => {
                    DaemonResponse::err(req.id, OpError::new("not-found", "gone"))
                }
                _ => DaemonResponse::err(req.id + 1, OpError::new("internal", "crossed")),
            };
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let value = transact_stream(&mut cli, &DaemonOp::List).expect("list round trip");
        assert_eq!(value, serde_json::json!({"sessions": []}));
        worker.join().expect("fake daemon");
        // Daemon errors surface verbatim with their code.
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = DaemonResponse::err(req.id, OpError::new("not-found", "gone"));
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let e = transact_stream(
            &mut cli,
            &DaemonOp::Stop {
                name: "ghost".to_string(),
            },
        )
        .expect_err("daemon error must surface");
        assert_eq!(e.code, "not-found");
        worker.join().expect("fake daemon");
    }

    #[cfg(unix)]
    #[test]
    fn transact_stream_rejects_crossed_id() {
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = DaemonResponse::ok(req.id + 1, serde_json::json!(null));
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let e = transact_stream(&mut cli, &DaemonOp::List).expect_err("crossed id accepted");
        assert_eq!(e.code, "io");
        worker.join().expect("fake daemon");
    }
}
