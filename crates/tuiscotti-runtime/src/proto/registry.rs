// ---------------------------------------------------------------------------
// PTY session registry (feature `pty`)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
pub(crate) mod pty_registry {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::proto::{
        OpError, OpResult, base64_decode, base64_encode, observation_view, screen_text,
        screen_view, wait_kind,
    };
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    // Process-local map: `crate::tui::Session` has no cross-process reattach,
    // and termpane (unreleased) offers no retained-session API either — its
    // `PtySession` is spawn/write/snapshot/signal/close within one process.
    // Named PTY sessions surviving across CLI invocations (F08-F2) live in
    // this map inside the long-lived daemon (`super::daemon`), which is a
    // thin IPC wrapper around these functions; no path/git override stands
    // in for termpane here.
    //
    // Concurrency (F12): the map holds `Arc<Session>` and every critical
    // section below is short (lookup/insert/remove only) — spawns, waits,
    // and observations all run OUTSIDE the lock, so independent sessions
    // operate concurrently and one blocked wait never stalls another
    // session. An `exit` removes the entry but in-flight holders keep a
    // working `Arc` until close completes.
    static REGISTRY: Mutex<Option<HashMap<String, Arc<crate::tui::Session>>>> = Mutex::new(None);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn with_registry<T>(f: impl FnOnce(&mut HashMap<String, Arc<crate::tui::Session>>) -> T) -> T {
        let mut guard = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let map = guard.get_or_insert_with(HashMap::new);
        f(map)
    }

    /// Short-lock lookup: clone the `Arc` out, then operate lock-free.
    fn lookup(session: &str) -> Result<Arc<crate::tui::Session>, OpError> {
        with_registry(|map| map.get(session).cloned())
            .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))
    }

    fn fresh_id() -> String {
        format!(
            "sess-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn tui_err(e: &crate::tui::TuiError) -> OpError {
        let code = match e {
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

    fn wait_err(e: &crate::tui::WaitError) -> OpError {
        match e {
            crate::tui::WaitError::Timeout { .. } => OpError::new("timeout", e.to_string()),
            crate::tui::WaitError::Cancelled { .. } => OpError::new("cancelled", e.to_string()),
            crate::tui::WaitError::Unsupported { .. } => OpError::new("unsupported", e.to_string()),
            crate::tui::WaitError::Closed { .. } => OpError::new("not-found", e.to_string()),
        }
    }

    pub(crate) fn spawn(
        argv: &[String],
        id: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        cwd: Option<PathBuf>,
        env: &HashMap<String, String>,
    ) -> Result<OpResult, OpError> {
        let owned: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
        spawn_os(&owned, id, cols, rows, cwd, env)
    }

    /// [`spawn`] with native [`OsString`](std::ffi::OsString) argv: the child
    /// spawns byte-exact. The retained-session daemon (F08-F2) uses this so
    /// PTY starts match piped starts (`session_start_os`); the endpoint
    /// record keeps a lossy UTF-8 projection (diagnostic, never re-spawned).
    pub(crate) fn spawn_os(
        argv: &[std::ffi::OsString],
        id: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        cwd: Option<PathBuf>,
        env: &HashMap<String, String>,
    ) -> Result<OpResult, OpError> {
        if argv.is_empty() {
            return Err(OpError::new(
                "invalid-input",
                "spawn needs a non-empty argv",
            ));
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
        // Check-then-spawn-then-insert, all outside one lock: the pre-check
        // fails the common duplicate before any child exists; the spawn
        // itself runs lock-free (it blocks for the worker's revision 0);
        // the insert re-checks so a same-id race loser closes what it just
        // spawned instead of clobbering the winner.
        if with_registry(|map| map.contains_key(&id)) {
            return Err(
                OpError::new("session-exists", format!("{id} already spawned")).with_session(&id),
            );
        }
        let session = builder.spawn().map_err(|e| tui_err(&e).with_session(&id))?;
        let pid = session.pid();
        let session = Arc::new(session);
        let won = with_registry(|map| {
            if map.contains_key(&id) {
                return false;
            }
            map.insert(id.clone(), Arc::clone(&session));
            true
        });
        if !won {
            // Lost a same-id race: close what we spawned (best-effort; the
            // duplicate verdict stays authoritative either way).
            if session.close().is_err() {
                // Close failed after a lost race; the verdict stands.
            }
            return Err(
                OpError::new("session-exists", format!("{id} already spawned")).with_session(&id),
            );
        }
        Ok(OpResult::Spawned { session: id, pid })
    }

    pub(crate) fn stdin(
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
        let s = lookup(session)?;
        let r = if let Some(text) = text {
            if text.is_empty() {
                return Err(
                    OpError::new("invalid-input", "text must not be empty").with_session(session)
                );
            }
            s.send_text(&text)
        } else if let Some(chord) = chord {
            s.press(&chord)
        } else if let Some(b64) = bytes_b64 {
            let bytes = base64_decode(&b64).map_err(|e| {
                OpError::new("invalid-input", format!("bad bytes_b64: {e}")).with_session(session)
            })?;
            if bytes.is_empty() {
                return Err(
                    OpError::new("invalid-input", "bytes must not be empty").with_session(session)
                );
            }
            s.send_bytes(&bytes)
        } else {
            unreachable!("counted above");
        };
        r.map_err(|e| tui_err(&e).with_session(session))?;
        Ok(OpResult::InputAccepted {
            session: session.to_string(),
        })
    }

    pub(crate) fn observe(session: &str) -> Result<OpResult, OpError> {
        let s = lookup(session)?;
        let obs = s
            .observe_now()
            .map_err(|e| tui_err(&e).with_session(session))?;
        Ok(OpResult::Observation {
            observation: observation_view(&obs),
        })
    }

    pub(crate) fn snapshot(session: &str) -> Result<OpResult, OpError> {
        let s = lookup(session)?;
        let screen = s
            .snapshot()
            .map_err(|e| tui_err(&e).with_session(session))?;
        Ok(OpResult::Snapshot {
            screen: screen_view(&screen),
        })
    }

    pub(crate) fn screenshot(session: &str) -> Result<OpResult, OpError> {
        let s = lookup(session)?;
        let obs = s
            .observe_now()
            .map_err(|e| tui_err(&e).with_session(session))?;
        let canonical = tuiscotti_core::screen::canonical_string(&obs.screen);
        let profile = tuiscotti_render::profile::Profile::default_profile();
        // Shared default renderer (F12): faces parsed once per thread,
        // glyph cache shared across screenshots.
        let image = tuiscotti_render::render::Renderer::with_profile(
            &profile,
            &tuiscotti_render::profile::VENDORED_FACES,
            |r| r.render_screen(&obs.screen),
        )
        .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
        Ok(OpResult::Screenshot {
            screen: screen_view(&obs.screen),
            canonical,
            png_b64: base64_encode(&image.png),
        })
    }

    pub(crate) fn wait(
        session: &str,
        kind: &str,
        needle: Option<&str>,
        quiet_ms: Option<u64>,
        timeout_ms: u64,
    ) -> Result<OpResult, OpError> {
        let s = lookup(session)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let cancel = crate::tui::CancelToken::new();
        match kind {
            wait_kind::TEXT => {
                let needle = needle.ok_or_else(|| {
                    OpError::new("invalid-input", "text wait needs `needle`").with_session(session)
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
                    .map_err(|e| wait_err(&e).with_session(session))?;
                Ok(OpResult::Waited {
                    session: session.to_string(),
                    observation: observation_view(&obs),
                })
            }
            wait_kind::STABLE => {
                let quiet = Duration::from_millis(quiet_ms.unwrap_or(200));
                let obs = s
                    .wait_stable_quiet(deadline, quiet, &cancel)
                    .map_err(|e| wait_err(&e).with_session(session))?;
                Ok(OpResult::Waited {
                    session: session.to_string(),
                    observation: observation_view(&obs),
                })
            }
            wait_kind::EXIT => {
                let ew = s
                    .wait_exit(deadline, &cancel)
                    .map_err(|e| wait_err(&e).with_session(session))?;
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
    }

    pub(crate) fn exit(session: &str, timeout_ms: u64) -> Result<OpResult, OpError> {
        let s = with_registry(|map| {
            map.remove(session)
                .ok_or_else(|| OpError::new("not-found", "unknown session").with_session(session))
        })?;
        // Graceful wait first so `exit` on a running app reaps evidence.
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let cancel = crate::tui::CancelToken::new();
        match s.wait_exit(deadline, &cancel) {
            Ok(ew) => {
                // Teardown is best-effort once the session left the registry:
                // the observed exit verdict stays authoritative.
                if s.close().is_err() {
                    // Close failed after exit; the exit evidence stands.
                }
                Ok(OpResult::Exited {
                    session: session.to_string(),
                    code: ew.status.code(),
                    signal: ew.status.signal().map(str::to_string),
                    observation: observation_view(&ew.observation),
                })
            }
            Err(crate::tui::WaitError::Timeout { evidence, .. }) => {
                // Teardown is best-effort: the timeout verdict below stays
                // authoritative even when the forced close also fails.
                if s.close().is_err() {
                    // Close failed during timeout teardown; timeout stands.
                }
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
                // Teardown is best-effort: the wait error below stays
                // authoritative even when the close also fails.
                if s.close().is_err() {
                    // Close failed during error teardown; wait error stands.
                }
                Err(wait_err(&e).with_session(session))
            }
        }
    }

    /// Liveness of one registry entry for the retained-session daemon's
    /// `list` merge: the daemon is authoritative for PTY sessions (owned
    /// handle, no pid-reuse window while it lives).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub(crate) struct SessionLiveness {
        /// Registry key (= validated session name on the daemon path).
        pub(crate) name: String,
        /// Direct-child pid, when the platform reports one.
        pub(crate) pid: Option<u32>,
        /// True while the child has not been reaped.
        pub(crate) running: bool,
    }

    /// Snapshot every entry's liveness. Arcs come out under one short lock;
    /// the polls run lock-free, like every other op in this module.
    pub(crate) fn sessions_status() -> Vec<SessionLiveness> {
        let entries: Vec<(String, Arc<crate::tui::Session>)> = with_registry(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), Arc::clone(v)))
                .collect()
        });
        let mut out: Vec<SessionLiveness> = entries
            .into_iter()
            .map(|(name, s)| SessionLiveness {
                name,
                pid: s.pid(),
                running: s.poll_exit().is_none(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// True when the registry holds no sessions (daemon idle-exit gate).
    pub(crate) fn is_empty() -> bool {
        with_registry(|map| map.is_empty())
    }

    /// Drop one entry, closing its session best-effort. Used by daemon
    /// `prune`/error paths for sessions already known-exited: the close is
    /// hygiene (join threads), so its verdict never fails the prune.
    pub(crate) fn drop_session(session: &str) {
        let removed = with_registry(|map| map.remove(session));
        if let Some(s) = removed
            && s.close().is_err()
        {
            // Best-effort hygiene close; the drop stands.
        }
    }

    /// Drop one entry only when it is already exited (or absent): returns
    /// whether the caller may remove the endpoint record. A running entry
    /// is never dropped — a start racing the prune must not lose a live
    /// session to a stale listing.
    pub(crate) fn drop_exited(session: &str) -> bool {
        let entry = with_registry(|map| map.get(session).cloned());
        match entry {
            None => true,
            Some(s) if s.poll_exit().is_some() => {
                drop_session(session);
                true
            }
            Some(_) => false,
        }
    }

    /// Drop every entry, closing sessions best-effort (live children are
    /// killed through their owned handles). Only the daemon's
    /// last-resort exit uses this: the runtime dir vanished, so the
    /// endpoint records are already gone and the sessions are garbage.
    pub(crate) fn close_all() {
        let sessions: Vec<Arc<crate::tui::Session>> =
            with_registry(|map| map.values().cloned().collect());
        with_registry(HashMap::clear);
        for s in sessions {
            if s.close().is_err() {
                // Best-effort last-resort close; the drop stands.
            }
        }
    }

    /// Deliver a named signal to a session's direct child. The name set is
    /// exactly `int|term|kill|quit|hup`. An already-exited session is an
    /// error and is never signaled (post-reap no-signal rule, checked here
    /// and again inside `Session::signal`, which refuses `ChildExited`).
    pub(crate) fn signal(session: &str, sig: &str) -> Result<(), OpError> {
        let signal = match sig {
            "int" => crate::tui::Signal::Int,
            "term" => crate::tui::Signal::Term,
            "kill" => crate::tui::Signal::Kill,
            "quit" => crate::tui::Signal::Quit,
            "hup" => crate::tui::Signal::Hup,
            _ => {
                return Err(OpError::new(
                    "invalid-input",
                    format!("unknown signal {sig:?} (want int|term|kill|quit|hup)"),
                )
                .with_session(session));
            }
        };
        let s = lookup(session)?;
        if s.poll_exit().is_some() {
            return Err(OpError::new(
                "not-found",
                format!("{session} already exited; not signaling"),
            )
            .with_session(session));
        }
        s.signal(signal)
            .map_err(|e| tui_err(&e).with_session(session))
    }

    /// Resize a session's PTY and emulator together. Range errors come from
    /// the session itself (`InvalidInput` → `invalid-input`).
    pub(crate) fn resize(session: &str, cols: u16, rows: u16) -> Result<(), OpError> {
        let s = lookup(session)?;
        s.resize(cols, rows)
            .map_err(|e| tui_err(&e).with_session(session))
    }

    /// Stop a session: TERM, a 500 ms grace (mirroring the piped stop's
    /// TERM→grace→KILL cadence), then the owned kill+reap+join via
    /// `close()`. An already-exited session closes without signaling.
    /// Any failure preserves the registry entry (fail-closed, like the
    /// piped stop preserving the endpoint); only success removes it.
    pub(crate) fn stop(session: &str) -> Result<(), OpError> {
        let s = lookup(session)?;
        if s.poll_exit().is_some() {
            return close_stopped(session, &s);
        }
        match s.signal(crate::tui::Signal::Term) {
            Ok(()) | Err(crate::tui::TuiError::ChildExited(_)) => {}
            Err(e) => return Err(tui_err(&e).with_session(session)),
        }
        let cancel = crate::tui::CancelToken::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        // Grace elapsed or not, `close()` kills and reaps what remains;
        // its verdict below stays authoritative either way.
        if s.wait_exit(deadline, &cancel).is_err() {
            // Still running past the grace; the owned kill follows.
        }
        close_stopped(session, &s)
    }

    /// `close()` + registry removal for [`stop`]: the close verdict is
    /// authoritative (a failed close keeps the entry).
    fn close_stopped(session: &str, s: &Arc<crate::tui::Session>) -> Result<(), OpError> {
        if let Err(e) = s.close() {
            return Err(tui_err(&e).with_session(session));
        }
        with_registry(|map| map.remove(session));
        Ok(())
    }

    fn validate_session_id(id: &str) -> Result<(), OpError> {
        if id.is_empty() || id.len() > 128 {
            return Err(OpError::new(
                "invalid-input",
                "session id must be 1..=128 chars",
            ));
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
