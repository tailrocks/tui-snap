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
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    // Process-local only: `crate::tui::Session` has no cross-process reattach,
    // and termpane (unreleased) offers no retained-session API either — its
    // `PtySession` is spawn/write/snapshot/signal/close within one process.
    // Named PTY sessions surviving across CLI invocations (F08-F2) wait for a
    // released termpane plus a retained-session/owner API that does not exist
    // yet; no path/git override stands in for it here.
    static REGISTRY: Mutex<Option<HashMap<String, crate::tui::Session>>> = Mutex::new(None);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn with_registry<T>(f: impl FnOnce(&mut HashMap<String, crate::tui::Session>) -> T) -> T {
        let mut guard = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        // Reserve-then-spawn under one lock: a duplicate id fails before any
        // child exists, so a failed registration never leaves a spawned
        // session to clean up. (The lock is already held across blocking
        // waits elsewhere in this registry.)
        with_registry(|map| {
            if map.contains_key(&id) {
                return Err(
                    OpError::new("session-exists", format!("{id} already spawned"))
                        .with_session(&id),
                );
            }
            let session = builder.spawn().map_err(|e| tui_err(&e).with_session(&id))?;
            let pid = session.pid();
            map.insert(id.clone(), session);
            Ok(OpResult::Spawned { session: id, pid })
        })
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
        with_registry(|map| {
            let s = map.get(session).ok_or_else(|| {
                OpError::new("not-found", "unknown session").with_session(session)
            })?;
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
            r.map_err(|e| tui_err(&e).with_session(session))?;
            Ok(OpResult::InputAccepted {
                session: session.to_string(),
            })
        })
    }

    pub(crate) fn observe(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map.get(session).ok_or_else(|| {
                OpError::new("not-found", "unknown session").with_session(session)
            })?;
            let obs = s
                .observe_now()
                .map_err(|e| tui_err(&e).with_session(session))?;
            Ok(OpResult::Observation {
                observation: observation_view(&obs),
            })
        })
    }

    pub(crate) fn snapshot(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map.get(session).ok_or_else(|| {
                OpError::new("not-found", "unknown session").with_session(session)
            })?;
            let screen = s
                .snapshot()
                .map_err(|e| tui_err(&e).with_session(session))?;
            Ok(OpResult::Snapshot {
                screen: screen_view(&screen),
            })
        })
    }

    pub(crate) fn screenshot(session: &str) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map.get(session).ok_or_else(|| {
                OpError::new("not-found", "unknown session").with_session(session)
            })?;
            let obs = s
                .observe_now()
                .map_err(|e| tui_err(&e).with_session(session))?;
            let canonical = tuiscotti_insta::insta_proto::insta_string(&obs.screen);
            let profile = tuiscotti_render::profile::Profile::default_profile();
            let mut renderer = tuiscotti_render::render::Renderer::new(
                &profile,
                &tuiscotti_render::profile::VENDORED_FACES,
            )
            .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
            let image = renderer
                .render_screen(&obs.screen)
                .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
            Ok(OpResult::Screenshot {
                screen: screen_view(&obs.screen),
                canonical,
                png_b64: base64_encode(&image.png),
            })
        })
    }

    pub(crate) fn wait(
        session: &str,
        kind: &str,
        needle: Option<&str>,
        quiet_ms: Option<u64>,
        timeout_ms: u64,
    ) -> Result<OpResult, OpError> {
        with_registry(|map| {
            let s = map.get(session).ok_or_else(|| {
                OpError::new("not-found", "unknown session").with_session(session)
            })?;
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let cancel = crate::tui::CancelToken::new();
            match kind {
                wait_kind::TEXT => {
                    let needle = needle.ok_or_else(|| {
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
        })
    }

    pub(crate) fn exit(session: &str, timeout_ms: u64) -> Result<OpResult, OpError> {
        let mut s = with_registry(|map| {
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
