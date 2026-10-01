//! Duration-based wait conveniences over [`Session`] (G6).
//!
//! The simple path: each wait takes a [`Duration`] bound and manages its own
//! cancellation internally. The explicit deadline + [`CancelToken`] forms on
//! [`Session`] (`wait_predicate`, `wait_stable`,
//! `wait_frame`, `wait_exit`, `expect_exit`) stay available for advanced
//! control without any required token boilerplate here.

use std::time::{Duration, Instant};

use tuiscotti_core::screen::Observation;

use crate::tui::{CancelToken, ExitWait, Session, WaitError};

impl Session {
    /// Default bound for the `*_timeout` waits (10s).
    pub const DEFAULT_WAIT: Duration = Duration::from_secs(10);

    /// [`Session::wait_predicate`] with a relative bound and internal
    /// cancellation. Timeout yields evidence; it never reports success.
    /// # Errors
    ///
    /// Returns [`WaitError`] when the predicate never matches in time.
    pub fn wait_predicate_timeout<F>(
        &self,
        predicate: F,
        timeout: Duration,
    ) -> Result<Observation, WaitError>
    where
        F: Fn(&Observation) -> bool,
    {
        let cancel = CancelToken::new();
        self.wait_predicate(predicate, Instant::now() + timeout, &cancel)
    }

    /// [`Session::wait_stable`] with a relative bound and internal
    /// cancellation: output settled, not business completion.
    /// # Errors
    ///
    /// Returns [`WaitError`] when output never settles in time.
    pub fn wait_stable_timeout(&self, timeout: Duration) -> Result<Observation, WaitError> {
        let cancel = CancelToken::new();
        self.wait_stable(Instant::now() + timeout, &cancel)
    }

    /// [`Session::wait_frame`] with a relative bound and internal
    /// cancellation. Still fails closed with [`WaitError::Unsupported`]: the
    /// backend cannot track synchronized frames.
    /// # Errors
    ///
    /// Returns [`WaitError`] when no frame arrives in time.
    pub fn wait_frame_timeout(&self, timeout: Duration) -> Result<Observation, WaitError> {
        let cancel = CancelToken::new();
        self.wait_frame(Instant::now() + timeout, &cancel)
    }

    /// [`Session::wait_exit`] with a relative bound and internal
    /// cancellation: the direct child exits and is reaped.
    /// # Errors
    ///
    /// Returns [`WaitError`] when the child never exits in time.
    pub fn wait_exit_timeout(&self, timeout: Duration) -> Result<ExitWait, WaitError> {
        let cancel = CancelToken::new();
        self.wait_exit(Instant::now() + timeout, &cancel)
    }

    /// [`Session::expect_exit`] with a relative bound and internal
    /// cancellation. The returned [`ExitWait`] offers `.success()` / `.code(n)`.
    /// # Errors
    ///
    /// Returns [`WaitError`] when the child never exits in time.
    pub fn expect_exit_timeout(&self, timeout: Duration) -> Result<ExitWait, WaitError> {
        self.wait_exit_timeout(timeout)
    }
}
