//! `session --pty`: retained PTY sessions behind the daemon (F08-F2).
//!
//! Cross-process CLI tests: every op runs in its own `tuiscotti`
//! invocation against an isolated `TUISCOTTI_RUNTIME_DIR`, with
//! `TUISCOTTI_DAEMON_IDLE_SECS=1` so daemons exit a second after their
//! last session. Every test ends by asserting its daemon and children
//! are gone (no strays outlive the suite).
//!
//! Split into one module per area so each file stays under the repo line
//! gate; behavior is unchanged.

#[path = "ops_session_pty/helpers.rs"]
mod helpers;
#[path = "ops_session_pty/lifecycle.rs"]
mod lifecycle;
#[path = "ops_session_pty/orphans.rs"]
mod orphans;
#[path = "ops_session_pty/races.rs"]
mod races;
#[path = "ops_session_pty/tamper.rs"]
mod tamper;
