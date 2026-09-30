use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::OpError;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Named sessions (A02): versioned endpoints, owner-only runtime dir
// ---------------------------------------------------------------------------
//
// Trust boundary (F08): an endpoint JSON file is untrusted metadata. It never
// authorizes signaling a PID or deleting a path by itself: every read
// validates the payload name against the requested entry, requires complete
// ownership/identity metadata, and rejects PID 0 and out-of-range PIDs. OS
// identity comes from filesystem ownership probes (`std` only, no `PATH`
// lookup, no environment fallback, no zero default). Final wiring waits for a
// released termpane (`termpane::process::{current_uid, pid_alive, signal}`);
// termpane is unreleased (crates.io 404) and path/git overrides are forbidden,
// so no termpane import exists yet.

/// Endpoint file format version. A reader that sees another version refuses
/// the file instead of guessing.
pub const SESSION_ENDPOINT_VERSION: u32 = 1;

/// Largest endpoint file we will parse: a tamper-bounded read.
const MAX_ENDPOINT_BYTES: u64 = 1_048_576;

/// Largest PID we will ever probe or signal. `pid_t` is a 32-bit signed int
/// on our Unix targets; 0 and negatives select process groups, never a child.
const PID_MAX: u32 = 2_147_483_647;

/// Absolute `kill(1)` locations, in preference order. Never a `PATH` lookup:
/// a tampered `PATH` must not redirect session signaling.
const KILL_BINARIES: [&str; 2] = ["/bin/kill", "/usr/bin/kill"];

/// Reservations past this age with a dead owner are crash residue and may be
/// taken over by a new starter.
const RESERVATION_TAKEOVER_SECS: u64 = 30;

/// Unparsable reservation locks only go stale by age (a concurrent starter
/// may not have finished writing yet).
const RESERVATION_CORRUPT_STALE_SECS: u64 = 120;

/// Backend that owns the named session's child.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionBackend {
    /// Plain piped child, owned by no one (liveness via `pid_alive`).
    Process,
    /// PTY-backed session owned by the retained-session daemon (F08-F2):
    /// the daemon holds the live `tui::Session` handle and is
    /// authoritative for liveness; the endpoint names it via `daemon_pid`.
    Pty,
}

/// Liveness of a named session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    /// The recorded pid is alive.
    Running,
    /// The recorded pid is dead.
    Exited,
}

/// What `session list` reports per session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Recorded child pid.
    pub pid: u32,
    /// Spawn argv (lossy UTF-8 projection).
    pub argv: Vec<String>,
    /// Backend that owns the child.
    pub backend: SessionBackend,
    /// Current liveness.
    pub status: SessionStatus,
    /// Start time as unix seconds.
    pub started_unix: u64,
}

/// On-disk endpoint record. Every field is validated on read; a record that
/// disagrees with the requested entry or lacks identity metadata is corrupt,
/// never authoritative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionEndpoint {
    pub(crate) version: u32,
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) argv: Vec<String>,
    pub(crate) backend: SessionBackend,
    pub(crate) started_unix: u64,
    /// Owner uid (required: records without it are corrupt, never adopted).
    pub(crate) owner: u32,
    /// Owning daemon's pid. Required if and only if `backend` is `Pty`
    /// (F08-F2): a `Pty` record without one is corrupt, and a `Process`
    /// record carrying one is corrupt. `Default` keeps pre-F2 records
    /// (which lack the field) parsing as `Process` with `None`, so no
    /// format version bump was needed.
    #[serde(default)]
    pub(crate) daemon_pid: Option<u32>,
}

/// Runtime dir: `$TUISCOTTI_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/tuiscotti`, else a
/// per-uid temp dir. Created owner-only (0o700) on Unix.
#[cfg(any(test, feature = "test-overrides"))]
static RUNTIME_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test-only runtime-dir override (F12: a truly test-only mechanism — this
/// function exists only under `cfg(test)` or the `test-overrides` feature,
/// so production builds cannot call it). No `unsafe`, unlike `set_var`,
/// which is an `unsafe fn` in edition 2024 and cannot be used under the
/// workspace lints. Checked before `$TUISCOTTI_RUNTIME_DIR` by
/// [`runtime_dir`]. Callers sharing a process must serialize (see the CLI
/// tests' `ENV_LOCK`); pass `None` to clear. Tests that can run
/// concurrently should spawn subprocesses with `TUISCOTTI_RUNTIME_DIR` in
/// the child environment instead (explicit per-process context).
#[cfg(any(test, feature = "test-overrides"))]
pub fn set_runtime_dir_override(dir: Option<PathBuf>) {
    *RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = dir;
}

/// Resolve the runtime dir, creating it owner-only (0o700) on Unix.
///
/// # Errors
///
/// Returns [`OpError`] when the dir cannot be created or secured.
pub fn runtime_dir() -> Result<PathBuf, OpError> {
    #[cfg(any(test, feature = "test-overrides"))]
    if let Some(d) = RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return ensure_runtime_dir(&d);
    }
    if let Ok(d) = std::env::var("TUISCOTTI_RUNTIME_DIR") {
        return ensure_runtime_dir(&PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        return ensure_runtime_dir(&PathBuf::from(d).join("tuiscotti"));
    }
    ensure_runtime_dir(&std::env::temp_dir().join(format!("tuiscotti-{}", current_uid()?)))
}

/// Create the runtime dir and validate it before use: real directory (never a
/// symlink), owned by us, owner-only permissions. Never repairs or follows a
/// directory owned by someone else.
fn ensure_runtime_dir(dir: &Path) -> Result<PathBuf, OpError> {
    if dir.as_os_str().is_empty() {
        return Err(OpError::new(
            "invalid-input",
            "runtime dir must not be empty",
        ));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| OpError::new("io", format!("runtime dir {}: {e}", dir.display())))?;
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| OpError::new("io", format!("stat {}: {e}", dir.display())))?;
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("runtime dir {} must not be a symlink", dir.display()),
        ));
    }
    if !meta.file_type().is_dir() {
        return Err(OpError::new(
            "io",
            format!("runtime dir {} is not a directory", dir.display()),
        ));
    }
    #[cfg(unix)]
    secure_runtime_dir(dir, &meta)?;
    Ok(dir.to_path_buf())
}

/// Unix ownership/permission boundary for an existing runtime dir: the dir
/// must already be ours (no chmod of someone else's directory), then it is
/// tightened to 0o700 when needed.
#[cfg(unix)]
fn secure_runtime_dir(dir: &Path, meta: &std::fs::Metadata) -> Result<(), OpError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let me = current_uid()?;
    if meta.uid() != me {
        return Err(OpError::new(
            "owner-mismatch",
            format!(
                "runtime dir {} belongs to uid {}, not {me}",
                dir.display(),
                meta.uid()
            ),
        ));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| OpError::new("io", format!("chmod 700 {}: {e}", dir.display())))?;
    }
    Ok(())
}

/// Counter disambiguating our temp-file probes within this process.
static UID_PROBE_CTR: AtomicU64 = AtomicU64::new(0);

/// Our own uid without `PATH` lookups, environment fallbacks, or a zero
/// default: a freshly created file is necessarily owned by us, so its owner
/// uid (via safe `MetadataExt`, no `unsafe`) is trusted OS identity. Final
/// wiring is `termpane::process::current_uid` once termpane is released.
#[cfg(unix)]
pub(crate) fn current_uid() -> Result<u32, OpError> {
    use std::os::unix::fs::MetadataExt;
    let tmp = std::env::temp_dir();
    for _ in 0..32 {
        let n = UID_PROBE_CTR.fetch_add(1, Ordering::Relaxed);
        let probe = tmp.join(format!(".tuiscotti-uid-{}-{n}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(f) => {
                let uid = f
                    .metadata()
                    .map_err(|e| OpError::new("io", format!("stat uid probe: {e}")))?
                    .uid();
                drop(f);
                if std::fs::remove_file(&probe).is_err() {
                    // Best-effort cleanup of our own 0-byte probe.
                }
                return Ok(uid);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(OpError::new("io", format!("uid probe: {e}")));
            }
        }
    }
    Err(OpError::new("io", "uid probe: no unique temp name"))
}

/// Non-Unix builds have no trusted uid probe; failing closed beats a zero.
#[cfg(not(unix))]
pub(crate) fn current_uid() -> Result<u32, OpError> {
    Err(OpError::new("unsupported", "uid identity needs Unix"))
}

pub(crate) fn validate_session_name(name: &str) -> Result<(), OpError> {
    if name.is_empty() || name.len() > 64 {
        return Err(OpError::new(
            "invalid-input",
            "session name must be 1..=64 chars",
        ));
    }
    // `daemon` is reserved for the retained-session daemon's own files
    // (`daemon.lock` single-flight, `daemon.pid`, `daemon.sock`): a session
    // by that name would collide with them (`daemon.lock` doubles as its
    // start reservation).
    if name == "daemon" {
        return Err(OpError::new(
            "invalid-input",
            "session name \"daemon\" is reserved",
        ));
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

/// Reject PID 0 (scheduler/idle, and group-selection in `kill`) and values
/// outside the `pid_t` range before any liveness or signal operation.
pub(crate) fn validate_pid(pid: u32) -> Result<(), OpError> {
    if pid == 0 {
        return Err(OpError::new(
            "invalid-input",
            "pid 0 is never a session child",
        ));
    }
    if pid > PID_MAX {
        return Err(OpError::new(
            "invalid-input",
            format!("pid {pid} is outside the pid_t range"),
        ));
    }
    Ok(())
}

/// Join a validated session name to the runtime dir, proving containment:
/// the result's parent is exactly `dir`, so prune/delete paths can never be
/// steered outside the registry by an untrusted payload name.
fn checked_join(dir: &Path, file: &str) -> Result<PathBuf, OpError> {
    let path = dir.join(file);
    if path.parent() != Some(dir) {
        return Err(OpError::new(
            "invalid-input",
            format!("{} escapes the runtime dir", path.display()),
        ));
    }
    Ok(path)
}

pub(crate) fn checked_endpoint_path(dir: &Path, name: &str) -> Result<PathBuf, OpError> {
    validate_session_name(name)?;
    checked_join(dir, &format!("{name}.json"))
}

pub(crate) fn checked_aux_path(dir: &Path, name: &str, suffix: &str) -> Result<PathBuf, OpError> {
    validate_session_name(name)?;
    checked_join(dir, &format!("{name}.{suffix}"))
}

/// Containment-checked daemon file path (`daemon.{suffix}` for the fixed
/// `sock`/`pid`/`err`/`lock` literals). Bypasses [`validate_session_name`],
/// which reserves `daemon` for exactly these files; containment is still
/// proven by [`checked_join`].
#[cfg(unix)]
pub(crate) fn checked_daemon_path(dir: &Path, suffix: &str) -> Result<PathBuf, OpError> {
    checked_join(dir, &format!("daemon.{suffix}"))
}

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
/// [`NameReservation`], so no live entry exists under our name at publish.
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

/// Exclusive same-name start reservation (`{name}.lock`, `create_new`). The
/// guard removes our lock on drop, so every early return releases the name;
/// call [`NameReservation::release`] on the success path. A lock whose owner
/// is dead (or is corrupt and old) is crash residue and is taken over.
#[derive(Debug)]
pub(crate) struct NameReservation {
    lock_path: PathBuf,
    released: bool,
}

impl NameReservation {
    pub(crate) fn acquire(dir: &Path, name: &str) -> Result<Self, OpError> {
        let lock_path = checked_aux_path(dir, name, "lock")?;
        Self::acquire_at(dir, name, lock_path)
    }

    /// Single-flight daemon autostart lock (`daemon.lock`). Bypasses
    /// [`validate_session_name`] (which reserves `daemon` for exactly the
    /// daemon's files); exclusivity, stale takeover, and the pid-tagged
    /// drop guard are identical to session reservations.
    #[cfg(all(unix, feature = "pty"))]
    pub(crate) fn acquire_daemon(dir: &Path) -> Result<Self, OpError> {
        let lock_path = checked_daemon_path(dir, "lock")?;
        Self::acquire_at(dir, "daemon", lock_path)
    }

    fn acquire_at(dir: &Path, name: &str, lock_path: PathBuf) -> Result<Self, OpError> {
        for _ in 0..4 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut f) => {
                    if let Err(e) = std::io::Write::write_all(
                        &mut f,
                        format!("{} {}\n", std::process::id(), now_unix()).as_bytes(),
                    ) {
                        drop(f);
                        if std::fs::remove_file(&lock_path).is_err() {
                            // Best-effort cleanup of our own unwritten lock.
                        }
                        return Err(OpError::new(
                            "io",
                            format!("write {}: {e}", lock_path.display()),
                        ));
                    }
                    return Ok(Self {
                        lock_path,
                        released: false,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock_path)? {
                        if std::fs::remove_file(&lock_path).is_err() {
                            // Someone else took over first; re-probe below.
                        }
                        continue;
                    }
                    return Err(lock_busy_error(dir, name)?);
                }
                Err(e) => {
                    return Err(OpError::new("io", format!("reserve {name}: {e}")));
                }
            }
        }
        Err(OpError::new(
            "session-exists",
            format!("{name} is starting elsewhere"),
        ))
    }

    /// Publish succeeded: remove our lock now (drop is the backstop).
    pub(crate) fn release(mut self) {
        self.released = true;
        if std::fs::remove_file(&self.lock_path).is_err() {
            // Best-effort release of our own lock.
        }
    }
}

impl Drop for NameReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Only remove a lock we still own (same-process pid tag): never
        // delete a lock that a successor already recreated.
        let tag = format!("{} ", std::process::id());
        let owned = std::fs::read_to_string(&self.lock_path).is_ok_and(|c| c.starts_with(&tag));
        if owned && std::fs::remove_file(&self.lock_path).is_err() {
            // Best-effort release of our own lock.
        }
    }
}

/// A live same-name starter reports the running pid; otherwise the name is
/// briefly busy. Both are `session-exists`: the caller never spawns. The
/// daemon lock (`daemon`) has no endpoint by construction (`daemon` is a
/// reserved name), so it skips the probe — probing would fail name
/// validation and mask the `session-exists` the starter retries on.
fn lock_busy_error(dir: &Path, name: &str) -> Result<OpError, OpError> {
    if name != "daemon"
        && let Some(ep) = read_endpoint(dir, name)?
        && pid_alive(ep.pid)
    {
        return Ok(OpError::new(
            "session-exists",
            format!("{name} already running (pid {})", ep.pid),
        ));
    }
    Ok(OpError::new(
        "session-exists",
        format!("{name} is starting elsewhere"),
    ))
}

/// Crash-residue probe: a parsed lock goes stale with a dead owner; an
/// unparsable one only by age (its writer may still be mid-write).
fn lock_is_stale(lock_path: &Path) -> Result<bool, OpError> {
    let content = match std::fs::read_to_string(lock_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => {
            return Err(OpError::new(
                "io",
                format!("read {}: {e}", lock_path.display()),
            ));
        }
    };
    let mut parts = content.split_whitespace();
    if let (Some(pid), Some(created)) = (parts.next(), parts.next())
        && let (Ok(pid), Ok(created)) = (pid.parse::<u32>(), created.parse::<u64>())
    {
        let old = created.saturating_add(RESERVATION_TAKEOVER_SECS) < now_unix();
        return Ok(old && !pid_alive(pid));
    }
    let age = std::fs::symlink_metadata(lock_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok());
    Ok(age.is_some_and(|a| a.as_secs() > RESERVATION_CORRUPT_STALE_SECS))
}

/// Trusted `kill(1)` without `PATH`: the first absolute candidate that is a
/// regular file. Final wiring is `termpane::process::{pid_alive, signal}`.
#[cfg(unix)]
fn kill_binary() -> Result<&'static str, OpError> {
    for cand in KILL_BINARIES {
        if std::fs::symlink_metadata(cand).is_ok_and(|m| m.file_type().is_file()) {
            return Ok(cand);
        }
    }
    Err(OpError::new(
        "io",
        "no trusted kill binary (/bin/kill, /usr/bin/kill)",
    ))
}

/// Best-effort liveness of a validated pid. Invalid pids (0, out of range)
/// and missing kill binaries report dead without executing anything.
#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    if validate_pid(pid).is_err() {
        return false;
    }
    let Ok(bin) = kill_binary() else {
        return false;
    };
    std::process::Command::new(bin)
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Non-Unix builds cannot probe liveness; every pid reports dead.
#[cfg(not(unix))]
pub(crate) fn pid_alive(pid: u32) -> bool {
    let _ = pid;
    false
}

/// Deliver `SIGTERM` to a validated pid. An already-dead pid still succeeds;
/// a failure against a live pid is an error (the caller keeps the endpoint).
pub(crate) fn kill_pid(pid: u32) -> Result<(), OpError> {
    signal_pid(pid, "-TERM")
}

/// Deliver `SIGKILL` to a validated pid, with the same dead-pid semantics as
/// [`kill_pid`].
pub(crate) fn kill9_pid(pid: u32) -> Result<(), OpError> {
    signal_pid(pid, "-KILL")
}

#[cfg(unix)]
fn signal_pid(pid: u32, sig: &str) -> Result<(), OpError> {
    validate_pid(pid)?;
    let bin = kill_binary()?;
    let status = std::process::Command::new(bin)
        .arg(sig)
        .arg(pid.to_string())
        .status()
        .map_err(|e| OpError::new("io", format!("{bin} {sig} {pid}: {e}")))?;
    if status.success() {
        return Ok(());
    }
    if pid_alive(pid) {
        return Err(OpError::new(
            "io",
            format!("{bin} {sig} {pid} failed against a live pid"),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn signal_pid(pid: u32, sig: &str) -> Result<(), OpError> {
    let _ = (pid, sig);
    Err(OpError::new("unsupported", "session stop needs Unix"))
}

/// SIGTERM a validated pid, grace, SIGKILL, then verify dead. Any failure
/// returns before the caller removes state, so a failed stop preserves the
/// endpoint (piped sessions) or the endpoint plus the daemon entry (PTY
/// orphans). The single pid-stop implementation for both backends.
#[cfg(unix)]
pub(crate) fn stop_pid(pid: u32) -> Result<(), OpError> {
    kill_pid(pid)?;
    wait_until_dead(pid, std::time::Duration::from_millis(500));
    if pid_alive(pid) {
        kill9_pid(pid)?;
        wait_until_dead(pid, std::time::Duration::from_millis(500));
    }
    if pid_alive(pid) {
        return Err(OpError::new(
            "op-failed",
            format!("pid {pid} survived SIGKILL; endpoint preserved"),
        ));
    }
    Ok(())
}

/// Non-Unix builds cannot stop pids; every pid stop fails closed.
#[cfg(not(unix))]
pub(crate) fn stop_pid(pid: u32) -> Result<(), OpError> {
    let _ = pid;
    Err(OpError::new("unsupported", "session stop needs Unix"))
}

#[cfg(unix)]
fn wait_until_dead(pid: u32, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_CTR: AtomicU64 = AtomicU64::new(0);

    /// Best-effort scratch cleanup (test teardown must not fail the test).
    fn cleanup(dir: &Path) {
        if std::fs::remove_dir_all(dir).is_err() {
            // Leftover scratch in the temp dir is harmless.
        }
    }

    /// Unique scratch dir per test (parallel-safe, no process-global state).
    fn test_dir(tag: &str) -> PathBuf {
        let n = TEST_CTR.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tuiscotti-sec-{}-{tag}-{n}", std::process::id()));
        cleanup(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn sample_endpoint(name: &str, pid: u32, owner: u32) -> SessionEndpoint {
        SessionEndpoint {
            version: SESSION_ENDPOINT_VERSION,
            name: name.to_string(),
            pid,
            argv: vec!["sleep".to_string(), "30".to_string()],
            backend: SessionBackend::Process,
            started_unix: now_unix(),
            owner,
            daemon_pid: None,
        }
    }

    fn sample_pty_endpoint(
        name: &str,
        pid: u32,
        owner: u32,
        daemon_pid: Option<u32>,
    ) -> SessionEndpoint {
        SessionEndpoint {
            version: SESSION_ENDPOINT_VERSION,
            name: name.to_string(),
            pid,
            argv: vec!["sh".to_string()],
            backend: SessionBackend::Pty,
            started_unix: now_unix(),
            owner,
            daemon_pid,
        }
    }

    fn sample_owner() -> u32 {
        current_uid().unwrap_or(0)
    }

    #[test]
    fn names_valid_including_dotted() {
        for good in ["a", "rt1", "a.b", "x.y.z", "v1.2-rc_3", "UPPER.lower-1_2"] {
            validate_session_name(good).expect(good);
        }
        assert!(validate_session_name(&"n".repeat(64)).is_ok());
    }

    #[test]
    fn names_invalid_rejected() {
        for bad in [
            "",
            ".",
            "..",
            "daemon",
            "a/b",
            "../evil",
            "a\\b",
            "sp ace",
            "semi;colon",
            "dollar$",
            &"x".repeat(65),
        ] {
            let e = validate_session_name(bad).expect_err("bad name accepted");
            assert_eq!(e.code, "invalid-input", "{bad:?}");
        }
    }

    #[test]
    fn checked_paths_stay_contained() {
        let dir = test_dir("paths");
        let ep = checked_endpoint_path(&dir, "a.b").expect("endpoint path");
        assert_eq!(ep.parent(), Some(dir.as_path()));
        let aux = checked_aux_path(&dir, "a.b", "log").expect("aux path");
        assert_eq!(aux.parent(), Some(dir.as_path()));
        assert!(checked_endpoint_path(&dir, "../evil").is_err());
        cleanup(&dir);
    }

    #[test]
    fn runtime_dir_fresh_is_usable() {
        let dir = test_dir("fresh");
        let back = ensure_runtime_dir(&dir.join("rt")).expect("ensure");
        assert!(back.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&back).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        cleanup(&dir);
    }

    #[test]
    fn runtime_dir_rejects_file_in_place() {
        let dir = test_dir("file");
        let file = dir.join("rt");
        std::fs::write(&file, b"x").expect("seed");
        assert!(ensure_runtime_dir(&file).is_err());
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_rejects_symlink() {
        let dir = test_dir("symlink");
        let real = dir.join("real");
        std::fs::create_dir(&real).expect("real");
        let link = dir.join("rt");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let e = ensure_runtime_dir(&link).expect_err("symlink accepted");
        assert_eq!(e.code, "invalid-input");
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_repairs_loose_perms() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("perms");
        let rt = dir.join("rt");
        std::fs::create_dir(&rt).expect("mkdir");
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        ensure_runtime_dir(&rt).expect("ensure");
        let mode = std::fs::metadata(&rt).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn current_uid_stable_no_zero_fallback() {
        let a = current_uid().expect("uid");
        let b = current_uid().expect("uid");
        assert_eq!(a, b);
    }

    #[test]
    fn pid_validation_rejects_group_selection() {
        assert!(validate_pid(0).is_err());
        assert!(validate_pid(PID_MAX + 1).is_err());
        assert!(validate_pid(u32::MAX).is_err());
        validate_pid(1).expect("pid 1");
        validate_pid(PID_MAX).expect("pid max");
        assert!(!pid_alive(0));
        assert!(!pid_alive(u32::MAX));
    }

    #[cfg(unix)]
    #[test]
    fn kill_probe_uses_absolute_binary() {
        let bin = super::kill_binary().expect("kill binary");
        assert!(bin.starts_with('/'), "{bin}");
        assert!(std::fs::symlink_metadata(bin).expect("stat").is_file());
    }

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

    #[test]
    fn reservation_is_exclusive() {
        let dir = test_dir("reserve");
        let first = NameReservation::acquire(&dir, "n").expect("first");
        let e = NameReservation::acquire(&dir, "n").expect_err("double acquire");
        assert_eq!(e.code, "session-exists");
        drop(first);
        assert!(!dir.join("n.lock").exists(), "drop releases");
        let again = NameReservation::acquire(&dir, "n").expect("reacquire");
        again.release();
        assert!(!dir.join("n.lock").exists(), "release removes");
        cleanup(&dir);
    }

    #[test]
    fn endpoint_daemon_pid_required_iff_pty() {
        let dir = test_dir("daemonpid");
        let me = std::process::id();
        // Pty with a live daemon pid round-trips.
        let ep = sample_pty_endpoint("p", me, sample_owner(), Some(me));
        write_endpoint(&dir, &ep).expect("write");
        let back = read_endpoint(&dir, "p").expect("read").expect("some");
        assert_eq!(back.daemon_pid, Some(me));
        // Pty without one is corrupt.
        let ep = sample_pty_endpoint("p", me, sample_owner(), None);
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "p")
                .expect_err("pidless Pty accepted")
                .code,
            "invalid-input"
        );
        // Pty with pid 0 is corrupt (never a signal target).
        let ep = sample_pty_endpoint("p", me, sample_owner(), Some(0));
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "p")
                .expect_err("pid-0 daemon accepted")
                .code,
            "invalid-input"
        );
        // Process carrying one is corrupt.
        let mut ep = sample_endpoint("q", me, sample_owner());
        ep.daemon_pid = Some(me);
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "q")
                .expect_err("daemon pid on Process accepted")
                .code,
            "invalid-input"
        );
        // Pre-F2 records (no daemon_pid field) still parse as Process.
        std::fs::write(
            dir.join("old.json"),
            format!(
                r#"{{"version":{SESSION_ENDPOINT_VERSION},"name":"old","pid":{me},"argv":["sleep"],"backend":"process","started_unix":{},"owner":{}}}"#,
                now_unix(),
                sample_owner()
            ),
        )
        .expect("seed");
        let back = read_endpoint(&dir, "old").expect("read").expect("some");
        assert_eq!(back.daemon_pid, None);
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn reservation_takes_over_stale_locks() {
        let dir = test_dir("stale");
        // Dead owner (invalid pid never probes alive) + ancient stamp.
        std::fs::write(dir.join("s.lock"), format!("{} 1\n", u32::MAX)).expect("seed");
        let taken = NameReservation::acquire(&dir, "s").expect("takeover");
        taken.release();
        // Live owner (ourselves) + fresh stamp stays busy.
        std::fs::write(
            dir.join("b.lock"),
            format!("{} {}\n", std::process::id(), now_unix()),
        )
        .expect("seed");
        let e = NameReservation::acquire(&dir, "b").expect_err("busy lock taken");
        assert_eq!(e.code, "session-exists");
        cleanup(&dir);
    }
}
