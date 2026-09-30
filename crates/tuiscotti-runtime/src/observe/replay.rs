// ---------------------------------------------------------------------------
// A05 execution paths (need the PTY runtime)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
mod replay_paths {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crate::tui::{CancelToken, ExitStatus, Tui};
    use crate::tui_shell::{Recording, ReplayError, Replayed, replay_bytes};
    use tuiscotti_core::screen::{Observation, Screen};

    // -- A05: Replay vs Rerun -----------------------------------------------

    impl Recording {
        /// Feed recorded output bytes through a fresh emulator, capturing one
        /// [`Screen`] after each recorded output event (cumulative prefixes).
        ///
        /// Deterministic: the same bytes always yield the same screens; no
        /// process is spawned — the path only runs the in-process emulator.
        /// Recorded input is never fed (see
        /// [`replay_recording`](crate::tui_shell::replay_recording)).
        ///
        /// # Errors
        ///
        /// Returns [`ReplayError`] when the output bytes cannot be replayed.
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
        ///
        /// # Errors
        ///
        /// Returns [`ReplayError`] when the output bytes cannot be replayed.
        pub fn execute(&self) -> Result<Replayed, ReplayError> {
            replay_bytes(&self.output, self.cols, self.rows)
        }
    }

    /// Re-run failure.
    #[derive(Debug, Clone)]
    pub enum RerunError {
        /// The re-run child could not be spawned.
        Spawn(String),
        /// The re-run child did not exit in time.
        Timeout(String),
        /// The re-run session failed mid-run.
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
        /// Final observation at exit.
        pub observation: Observation,
        /// How the re-run child ended.
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
        ///
        /// # Errors
        ///
        /// Returns [`RerunError`] when the spawn, wait, or teardown fails.
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
            let session = builder
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
pub use replay_paths::{Replay, Rerun, RerunError, RerunOutput};
