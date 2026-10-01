//! Endpoint record read/write: validated untrusted metadata.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use super::super::current_uid;
use super::super::{
    OpError, SESSION_ENDPOINT_VERSION, SessionBackend, SessionEndpoint, checked_endpoint_path,
    now_unix, validate_pid, validate_session_name,
};
use super::types::MAX_ENDPOINT_BYTES;
use super::validate::checked_join;

/// Read one endpoint record as untrusted metadata: no symlink following, a
/// bounded read, file-ownership check, then full payload validation (version,
/// name match, pid range, non-empty argv, sane start time, required owner).
pub(crate) fn read_endpoint(dir: &Path, name: &str) -> Result<Option<SessionEndpoint>, OpError> {
    let path = checked_endpoint_path(dir, name)?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OpError::new("io", format!("stat {}: {e}", path.display()))),
    };
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to follow", path.display()),
        ));
    }
    if !meta.file_type().is_file() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is not a regular file", path.display()),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = current_uid()?;
        if meta.uid() != me {
            return Err(OpError::new(
                "owner-mismatch",
                format!("{name} endpoint belongs to uid {}, not {me}", meta.uid()),
            ));
        }
    }
    if meta.len() > MAX_ENDPOINT_BYTES {
        return Err(OpError::new(
            "bound-exceeded",
            format!("{} exceeds {MAX_ENDPOINT_BYTES} bytes", path.display()),
        ));
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", path.display())))?;
    let ep: SessionEndpoint = serde_json::from_slice(&bytes).map_err(|e| {
        OpError::new(
            "invalid-input",
            format!("{} is corrupt: {e}", path.display()),
        )
    })?;
    validate_endpoint_payload(&path, name, &ep)?;
    Ok(Some(ep))
}

/// Payload half of [`read_endpoint`]: the record must describe exactly the
/// requested entry and carry complete identity metadata.
fn validate_endpoint_payload(path: &Path, name: &str, ep: &SessionEndpoint) -> Result<(), OpError> {
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
    validate_session_name(&ep.name)?;
    if ep.name != name {
        return Err(OpError::new(
            "invalid-input",
            format!(
                "{} names session {:?}, not requested {name:?}",
                path.display(),
                ep.name
            ),
        ));
    }
    validate_pid(ep.pid)?;
    validate_daemon_pid(path, ep)?;
    if ep.argv.is_empty() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} has empty argv", path.display()),
        ));
    }
    if ep.started_unix == 0 {
        return Err(OpError::new(
            "invalid-input",
            format!("{} has no start time", path.display()),
        ));
    }
    if ep.started_unix > now_unix().saturating_add(60) {
        return Err(OpError::new(
            "invalid-input",
            format!("{} starts in the future", path.display()),
        ));
    }
    #[cfg(unix)]
    {
        let me = current_uid()?;
        if ep.owner != me {
            return Err(OpError::new(
                "owner-mismatch",
                format!("{name} belongs to uid {}, not {me}", ep.owner),
            ));
        }
    }
    Ok(())
}

/// `daemon_pid` is required if and only if the backend is `Pty`, and a
/// present value must itself be a signalable pid (never 0: PID 0 selects
/// process groups in `kill`, so a tampered `daemon_pid: 0` must fail here,
/// before any liveness probe could consult it).
fn validate_daemon_pid(path: &Path, ep: &SessionEndpoint) -> Result<(), OpError> {
    match (&ep.backend, ep.daemon_pid) {
        (SessionBackend::Pty, Some(pid)) => validate_pid(pid).map_err(|_| {
            OpError::new(
                "invalid-input",
                format!("{} has an unusable daemon pid", path.display()),
            )
        }),
        (SessionBackend::Pty, None) => Err(OpError::new(
            "invalid-input",
            format!(
                "{} is a PTY session without an owning daemon",
                path.display()
            ),
        )),
        (SessionBackend::Process, None) => Ok(()),
        (SessionBackend::Process, Some(_)) => Err(OpError::new(
            "invalid-input",
            format!(
                "{} is a piped session carrying a daemon pid",
                path.display()
            ),
        )),
    }
}

/// Counter disambiguating our publish tempfiles within this process.
static PUBLISH_CTR: AtomicU64 = AtomicU64::new(0);

/// Atomic endpoint publish: write a collision-resistant tempfile
/// (`{name}.{pid}.{ctr}.tmp`, `create_new` so concurrent starters never share
/// one), sync it, then rename over the entry. The caller holds a
/// [`NameReservation`](super::super::NameReservation), so no live entry exists under our name at publish.
pub(crate) fn write_endpoint(dir: &Path, ep: &SessionEndpoint) -> Result<(), OpError> {
    let path = checked_endpoint_path(dir, &ep.name)?;
    let bytes = serde_json::to_vec_pretty(ep)
        .map_err(|e| OpError::new("io", format!("encode {}: {e}", path.display())))?;
    let (tmp, mut file) = create_publish_tmp(dir, &ep.name)?;
    let failed = |e: std::io::Error| {
        if std::fs::remove_file(&tmp).is_err() {
            // Best-effort cleanup of our own failed tempfile.
        }
        OpError::new("io", format!("write {}: {e}", tmp.display()))
    };
    std::io::Write::write_all(&mut file, &bytes).map_err(&failed)?;
    file.sync_all().map_err(&failed)?;
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, &path) {
        if std::fs::remove_file(&tmp).is_err() {
            // Best-effort cleanup of our own orphaned tempfile.
        }
        return Err(OpError::new(
            "io",
            format!("publish {}: {e}", path.display()),
        ));
    }
    Ok(())
}

/// Allocate our publish tempfile with `create_new`, retrying on collision.
fn create_publish_tmp(dir: &Path, name: &str) -> Result<(PathBuf, std::fs::File), OpError> {
    for _ in 0..32 {
        let n = PUBLISH_CTR.fetch_add(1, Ordering::Relaxed);
        let cand = checked_join(dir, &format!("{name}.tmp.{}-{n}", std::process::id()))?;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cand)
        {
            Ok(f) => return Ok((cand, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(OpError::new(
                    "io",
                    format!("publish tmp {}: {e}", cand.display()),
                ));
            }
        }
    }
    Err(OpError::new("io", "publish: no unique temp name"))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::super::current_uid;
    use super::super::types::{MAX_ENDPOINT_BYTES, PID_MAX};
    use super::super::{
        SESSION_ENDPOINT_VERSION, cleanup, now_unix, sample_endpoint, sample_owner, test_dir,
    };
    use super::*;

    #[test]
    fn endpoint_round_trip_and_missing() {
        let dir = test_dir("roundtrip");
        assert!(read_endpoint(&dir, "ghost").expect("read").is_none());
        let ep = sample_endpoint("rt1", std::process::id(), sample_owner());
        write_endpoint(&dir, &ep).expect("write");
        let back = read_endpoint(&dir, "rt1").expect("read").expect("some");
        assert_eq!(back.name, "rt1");
        assert_eq!(back.owner, ep.owner);
        cleanup(&dir);
    }

    #[test]
    fn endpoint_rejects_name_mismatch() {
        let dir = test_dir("namemismatch");
        let ep = sample_endpoint("other", std::process::id(), sample_owner());
        std::fs::write(
            dir.join("mine.json"),
            serde_json::to_vec(&ep).expect("json"),
        )
        .expect("seed");
        let e = read_endpoint(&dir, "mine").expect_err("mismatch accepted");
        assert_eq!(e.code, "invalid-input");
        cleanup(&dir);
    }

    #[test]
    fn endpoint_rejects_missing_owner() {
        let dir = test_dir("noowner");
        std::fs::write(
            dir.join("x.json"),
            format!(
                r#"{{"version":{SESSION_ENDPOINT_VERSION},"name":"x","pid":{},"argv":["sleep"],"backend":"process","started_unix":{}}}"#,
                std::process::id(),
                now_unix()
            ),
        )
        .expect("seed");
        let e = read_endpoint(&dir, "x").expect_err("ownerless accepted");
        assert_eq!(e.code, "invalid-input");
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn endpoint_rejects_foreign_owner() {
        let dir = test_dir("owner");
        let me = current_uid().expect("uid");
        let ep = sample_endpoint("x", std::process::id(), me ^ 1);
        write_endpoint(&dir, &ep).expect("write");
        let e = read_endpoint(&dir, "x").expect_err("foreign owner accepted");
        assert_eq!(e.code, "owner-mismatch");
        cleanup(&dir);
    }

    #[test]
    fn endpoint_rejects_bad_pids() {
        let dir = test_dir("badpid");
        for pid in [0, PID_MAX + 1, u32::MAX] {
            let ep = sample_endpoint("x", pid, sample_owner());
            write_endpoint(&dir, &ep).expect("write");
            let e = read_endpoint(&dir, "x").expect_err("bad pid accepted");
            assert_eq!(e.code, "invalid-input", "pid {pid}");
        }
        cleanup(&dir);
    }

    #[test]
    fn endpoint_rejects_bad_metadata() {
        let dir = test_dir("badmeta");
        let mut ep = sample_endpoint("x", std::process::id(), sample_owner());
        ep.argv.clear();
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "x")
                .expect_err("empty argv accepted")
                .code,
            "invalid-input"
        );
        ep.argv = vec!["sleep".to_string()];
        ep.started_unix = 0;
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "x")
                .expect_err("zero start accepted")
                .code,
            "invalid-input"
        );
        ep.started_unix = now_unix().saturating_add(3600);
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "x")
                .expect_err("future start accepted")
                .code,
            "invalid-input"
        );
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn endpoint_rejects_symlink_and_dir() {
        let dir = test_dir("linkdir");
        let target = dir.join("target.json");
        std::fs::write(&target, b"{}").expect("seed");
        std::os::unix::fs::symlink(&target, dir.join("s.json")).expect("symlink");
        let e = read_endpoint(&dir, "s").expect_err("symlink followed");
        assert_eq!(e.code, "invalid-input");
        std::fs::create_dir(dir.join("d.json")).expect("mkdir");
        let e = read_endpoint(&dir, "d").expect_err("dir read");
        assert_eq!(e.code, "invalid-input");
        cleanup(&dir);
    }

    #[test]
    fn endpoint_rejects_oversize() {
        let dir = test_dir("oversize");
        let big_len = usize::try_from(MAX_ENDPOINT_BYTES).expect("fits") + 1;
        let big = vec![b'x'; big_len];
        std::fs::write(dir.join("big.json"), big).expect("seed");
        let e = read_endpoint(&dir, "big").expect_err("oversize accepted");
        assert_eq!(e.code, "bound-exceeded");
        cleanup(&dir);
    }

    #[test]
    fn publish_tmpfiles_are_collision_resistant() {
        let dir = test_dir("publish");
        let dir = std::sync::Arc::new(dir);
        let mut handles = Vec::new();
        for t in 0..8 {
            let dir = std::sync::Arc::clone(&dir);
            handles.push(std::thread::spawn(move || {
                for i in 0..8 {
                    let name = format!("t{t}n{i}");
                    let ep = sample_endpoint(&name, std::process::id(), sample_owner());
                    write_endpoint(&dir, &ep).expect("concurrent write");
                }
            }));
        }
        for h in handles {
            h.join().expect("thread");
        }
        for t in 0..8 {
            for i in 0..8 {
                let name = format!("t{t}n{i}");
                assert!(read_endpoint(&dir, &name).expect("read").is_some());
            }
        }
        cleanup(dir.as_path());
    }

    #[test]
    fn write_endpoint_reports_failure() {
        let dir = test_dir("writefail");
        std::fs::create_dir(dir.join("x.json")).expect("mkdir");
        let ep = sample_endpoint("x", std::process::id(), sample_owner());
        let e = write_endpoint(&dir, &ep).expect_err("dir publish accepted");
        assert_eq!(e.code, "io");
        assert!(dir.join("x.json").is_dir(), "target untouched");
        cleanup(&dir);
    }
}
