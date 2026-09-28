//! Live observation + replay-vs-rerun (backlog A03, A05).
//!
//! - **A03 [`Watcher`]**: subscribes to a live [`Session`](crate::tui::Session)'s
//!   [`Observation`](crate::screen::Observation) stream over a bounded channel
//!   (latest-wins + dropped counter, never blocks the session) and injects
//!   input through the same owned session — no tmux/multiplexer. The watcher
//!   sees exactly what assertions see: the same `Observation` type.
//! - **A05 [`Replay`] vs [`Rerun`]**: [`Replay`] feeds recorded output bytes to
//!   a fresh emulator deterministically (no process spawn, by construction);
//!   [`Rerun`] re-spawns the recorded command (documented nondeterministic).
//!   [`compare_replay_vs_rerun`] reports same/different with revision maps.
//!
//! The CLI `session attach` (see `src/main.rs`) is a best-effort human view:
//! it tails the named session's log as text frames. Assertions remain on
//! `Observation`s, never on attach output.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::screen::Screen;

// ---------------------------------------------------------------------------
// Shared helpers (feature-independent)
// ---------------------------------------------------------------------------

/// Plain-text rows of a screen, one line per row, trailing blanks trimmed.
///
/// This is a lossy human projection (colors/mods/cursor dropped): fine for a
/// best-effort live view, never an assertion input.
#[must_use]
pub fn screen_text(screen: &Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        let mut row = String::new();
        for x in 0..screen.cols() {
            if let Some(c) = screen.get(x, y) {
                if !c.continuation {
                    row.push_str(&c.symbol);
                }
            }
        }
        if y > 0 {
            out.push('\n');
        }
        out.push_str(row.trim_end());
    }
    out
}

fn screen_hash(screen: &Screen) -> u64 {
    let mut h = DefaultHasher::new();
    screen.hash(&mut h);
    h.finish()
}

/// Same/different verdict for [`compare_replay_vs_rerun`], with revision maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRerunComparison {
    /// True when the final replayed screen equals the re-run screen.
    pub same: bool,
    /// Screen hash per replayed prefix, in prefix order (replay revision map).
    pub replay_hashes: Vec<u64>,
    /// Screen hash of the re-run's final observation.
    pub rerun_hash: u64,
    /// Human-readable verdict detail.
    pub detail: String,
}

/// Compare replayed screens against a re-run screen.
///
/// `replay_screens` is the per-output-prefix screens from
/// [`Recording::replay_observations`](crate::tui_shell::Recording) (or any
/// replay path); only the final screen is compared, the full prefix list is
/// kept as the revision map. Equality is exact [`Screen`] equality.
#[must_use]
pub fn compare_replay_vs_rerun(
    replay_screens: &[Screen],
    rerun_screen: &Screen,
) -> ReplayRerunComparison {
    let replay_hashes: Vec<u64> = replay_screens.iter().map(screen_hash).collect();
    let rerun_hash = screen_hash(rerun_screen);
    let same = replay_screens.last().is_some_and(|s| s == rerun_screen);
    let detail = match replay_screens.last() {
        None => format!("no replayed screens; rerun hash {rerun_hash:016x}: DIFFERENT"),
        Some(_) if same => format!(
            "final replay screen == rerun screen ({} prefixes, hash {rerun_hash:016x}): SAME",
            replay_screens.len()
        ),
        Some(_) => format!(
            "final replay screen != rerun screen ({} prefixes, replay {:016x} vs rerun {rerun_hash:016x}): DIFFERENT",
            replay_screens.len(),
            replay_hashes.last().copied().unwrap_or(0),
        ),
    };
    ReplayRerunComparison {
        same,
        replay_hashes,
        rerun_hash,
        detail,
    }
}

// ---------------------------------------------------------------------------
// A03 + A05 execution paths (need the PTY runtime)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
mod pty_paths {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use crate::screen::{Observation, Screen};
    use crate::tui::{CancelToken, ExitStatus, Session, Tui, TuiError};
    use crate::tui_shell::{replay_bytes, Recording, ReplayError, Replayed};

    // -- A03: Watcher --------------------------------------------------------

    /// One delivered watch item: the observation plus the cumulative eviction
    /// count at delivery time.
    #[derive(Debug, Clone)]
    pub struct Watched {
        /// Exactly what assertions see: the live [`Observation`].
        pub observation: Observation,
        /// Total observations evicted before this one was delivered.
        pub dropped_before: u64,
    }

    struct WatcherInner {
        queue: Mutex<VecDeque<Observation>>,
        changed: Condvar,
        dropped: AtomicU64,
        stop: AtomicBool,
    }

    /// Live subscription to an owned [`Session`]'s observation stream.
    ///
    /// A poll thread tracks [`Session::revision`] and forwards each new
    /// [`Session::observe_now`] result into a bounded queue: when full, the
    /// oldest item is evicted (latest-wins) and the dropped counter grows.
    /// The session is only ever polled — the watcher can never block it.
    /// [`Watcher::inject`] sends input through the same owned session.
    pub struct Watcher {
        session: Arc<Session>,
        inner: Arc<WatcherInner>,
        capacity: usize,
        thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl Watcher {
        /// Subscribe to `session`. `capacity` bounds the queue (0 becomes 1);
        /// `poll` is the revision-poll cadence.
        pub fn subscribe(session: Arc<Session>, capacity: usize, poll: Duration) -> Self {
            let inner = Arc::new(WatcherInner {
                queue: Mutex::new(VecDeque::new()),
                changed: Condvar::new(),
                dropped: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            });
            let capacity = capacity.max(1);
            let worker_session = Arc::clone(&session);
            let worker_inner = Arc::clone(&inner);
            let thread = std::thread::Builder::new()
                .name("tuisnap-observe-watcher".to_string())
                .spawn(move || {
                    watch_loop(&worker_session, &worker_inner, capacity, poll);
                })
                .expect("watcher thread spawn failed");
            Self {
                session,
                inner,
                capacity,
                thread: Mutex::new(Some(thread)),
            }
        }

        /// Queue bound.
        #[must_use]
        pub fn capacity(&self) -> usize {
            self.capacity
        }

        /// Total observations evicted so far (latest-wins lag signal).
        #[must_use]
        pub fn dropped(&self) -> u64 {
            self.inner.dropped.load(Ordering::SeqCst)
        }

        /// Take the oldest queued observation, waiting up to `timeout`.
        /// Returns `None` on timeout (or after [`Watcher::stop`]).
        pub fn next_timeout(&self, timeout: Duration) -> Option<Watched> {
            let deadline = Instant::now() + timeout;
            let mut q = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            while q.is_empty() {
                if self.inner.stop.load(Ordering::SeqCst) {
                    return None;
                }
                let now = Instant::now();
                if now >= deadline {
                    return None;
                }
                let (guard, _) = self
                    .inner
                    .changed
                    .wait_timeout(q, deadline - now)
                    .unwrap_or_else(|e| e.into_inner());
                q = guard;
            }
            q.pop_front().map(|observation| Watched {
                observation,
                dropped_before: self.inner.dropped.load(Ordering::SeqCst),
            })
        }

        /// Take the oldest queued observation without waiting.
        pub fn try_next(&self) -> Option<Watched> {
            let mut q = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.pop_front().map(|observation| Watched {
                observation,
                dropped_before: self.inner.dropped.load(Ordering::SeqCst),
            })
        }

        /// Inject raw input bytes through the watched session.
        pub fn inject(&self, bytes: &[u8]) -> Result<(), TuiError> {
            self.session.send_bytes(bytes)
        }

        /// Inject literal text through the watched session.
        pub fn inject_text(&self, text: &str) -> Result<(), TuiError> {
            self.session.send_text(text)
        }

        /// Stop the poll thread and drop queued items. Idempotent.
        pub fn stop(&self) {
            self.inner.stop.store(true, Ordering::SeqCst);
            self.inner.changed.notify_all();
            if let Ok(mut guard) = self.thread.lock() {
                if let Some(h) = guard.take() {
                    let _ = h.join();
                }
            }
        }
    }

    impl Drop for Watcher {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn watch_loop(session: &Session, inner: &WatcherInner, capacity: usize, poll: Duration) {
        let mut pushed = session.revision();
        // Publish the current frame immediately so a quiet session still
        // yields one observation.
        if let Ok(obs) = session.observe_now() {
            pushed = pushed.max(obs.revision);
            push_watched(inner, obs, capacity);
        }
        loop {
            if inner.stop.load(Ordering::SeqCst) {
                return;
            }
            if session.revision() > pushed {
                match session.observe_now() {
                    Ok(obs) => {
                        if obs.revision > pushed {
                            pushed = obs.revision;
                            push_watched(inner, obs, capacity);
                        } else {
                            pushed = pushed.max(session.revision());
                        }
                    }
                    Err(TuiError::Closed(_)) => return,
                    Err(_) => std::thread::sleep(poll),
                }
            } else {
                std::thread::sleep(poll);
            }
        }
    }

    fn push_watched(inner: &WatcherInner, obs: Observation, capacity: usize) {
        let mut q = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        while q.len() >= capacity {
            q.pop_front();
            inner.dropped.fetch_add(1, Ordering::SeqCst);
        }
        q.push_back(obs);
        drop(q);
        inner.changed.notify_one();
    }

    // -- A05: Replay vs Rerun -----------------------------------------------

    impl Recording {
        /// Feed recorded output bytes through a fresh emulator, capturing one
        /// [`Screen`] after each recorded output event (cumulative prefixes).
        ///
        /// Deterministic: the same bytes always yield the same screens; no
        /// process is spawned — the path only runs the in-process emulator.
        /// Recorded input is never fed (see
        /// [`replay_recording`](crate::tui_shell::replay_recording)).
        pub fn replay_observations(&self) -> Result<Vec<Screen>, ReplayError> {
            let output = self.output_bytes();
            if output.is_empty() {
                return Ok(Vec::new());
            }
            // `Recording` retains no per-event boundaries (only the
            // concatenated output), so the exact deterministic replay is the
            // single full-stream screen.
            Ok(vec![
                replay_bytes(&output, self.cols(), self.rows())?.screen,
            ])
        }
    }

    /// Deterministic offline replay: recorded output bytes + geometry, run
    /// through a fresh emulator. Never spawns a process.
    #[derive(Debug, Clone)]
    pub struct Replay {
        output: Vec<u8>,
        cols: u16,
        rows: u16,
    }

    impl Replay {
        /// Build from a recording (output bytes only; input never replayed).
        #[must_use]
        pub fn from_recording(recording: &Recording) -> Self {
            Self {
                output: recording.output_bytes(),
                cols: recording.cols(),
                rows: recording.rows(),
            }
        }

        /// Build from raw output bytes + geometry.
        #[must_use]
        pub fn from_bytes(output: Vec<u8>, cols: u16, rows: u16) -> Self {
            Self { output, cols, rows }
        }

        /// Run the replay. Pure emulation: no child process is spawned, by
        /// construction (this path contains no spawn call).
        pub fn execute(&self) -> Result<Replayed, ReplayError> {
            replay_bytes(&self.output, self.cols, self.rows)
        }
    }

    /// Re-run failure.
    #[derive(Debug, Clone)]
    pub enum RerunError {
        Spawn(String),
        Timeout(String),
        Session(String),
    }

    impl std::fmt::Display for RerunError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Spawn(m) => write!(f, "rerun spawn failed: {m}"),
                Self::Timeout(m) => write!(f, "rerun timed out: {m}"),
                Self::Session(m) => write!(f, "rerun session failed: {m}"),
            }
        }
    }

    impl std::error::Error for RerunError {}

    /// What a re-run produced: final observation + exit proof.
    #[derive(Debug, Clone)]
    pub struct RerunOutput {
        pub observation: Observation,
        pub status: ExitStatus,
        /// Direct-child PID: `Some` proves a process was actually spawned.
        pub pid: Option<u32>,
    }

    /// Nondeterministic re-execution: re-spawns the recorded command in a
    /// fresh PTY and waits for natural exit. Output may differ from the
    /// recording (time, randomness, environment) — that divergence is the
    /// point of [`compare_replay_vs_rerun`](crate::observe::compare_replay_vs_rerun).
    #[derive(Debug, Clone)]
    pub struct Rerun {
        argv: Vec<String>,
        cols: u16,
        rows: u16,
        env: Vec<(String, String)>,
        cwd: Option<PathBuf>,
        timeout: Duration,
    }

    impl Rerun {
        /// Re-run `argv` at `cols` x `rows` (PTY backend limits apply).
        #[must_use]
        pub fn new(argv: Vec<String>, cols: u16, rows: u16) -> Self {
            Self {
                argv,
                cols,
                rows,
                env: Vec::new(),
                cwd: None,
                timeout: Duration::from_secs(30),
            }
        }

        /// Child-only environment entry.
        #[must_use]
        pub fn env(mut self, key: &str, value: &str) -> Self {
            self.env.push((key.to_string(), value.to_string()));
            self
        }

        /// Child working directory.
        #[must_use]
        pub fn cwd(mut self, dir: PathBuf) -> Self {
            self.cwd = Some(dir);
            self
        }

        /// How long to wait for natural exit.
        #[must_use]
        pub fn timeout(mut self, timeout: Duration) -> Self {
            self.timeout = timeout;
            self
        }

        /// Spawn the command, wait for exit, reap, and return the final
        /// observation. The child PID in [`RerunOutput::pid`] proves a
        /// process was spawned (vs [`Replay`], which never spawns).
        pub fn execute(&self) -> Result<RerunOutput, RerunError> {
            if self.argv.is_empty() {
                return Err(RerunError::Spawn("empty argv".to_string()));
            }
            let mut builder = Tui::new(self.argv.clone()).size(self.cols, self.rows);
            for (k, v) in &self.env {
                builder = builder.env(k, v);
            }
            if let Some(cwd) = &self.cwd {
                builder = builder.cwd(cwd.clone());
            }
            let mut session = builder
                .spawn()
                .map_err(|e| RerunError::Spawn(e.to_string()))?;
            let pid = session.pid();
            let cancel = CancelToken::new();
            let deadline = Instant::now() + self.timeout;
            let waited = session.wait_exit(deadline, &cancel).map_err(|e| match e {
                crate::tui::WaitError::Timeout { waited, .. } => {
                    RerunError::Timeout(format!("no exit after {waited:?}"))
                }
                other => RerunError::Session(other.to_string()),
            })?;
            session
                .close()
                .map_err(|e| RerunError::Session(e.to_string()))?;
            Ok(RerunOutput {
                observation: waited.observation,
                status: waited.status,
                pid,
            })
        }
    }
}

#[cfg(feature = "pty")]
pub use pty_paths::{Replay, Rerun, RerunError, RerunOutput, Watched, Watcher};
