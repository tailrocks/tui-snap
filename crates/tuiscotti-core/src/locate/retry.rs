use super::{LocateError, Locator, Span};
use crate::screen::Observation;
use std::time::{Duration, Instant};

/// Fixed poll interval for all retry loops.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(5);

impl Locator {
    /// Retry until at least one match, or the ONE `timeout` deadline.
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] fail immediately.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Timeout`] past the deadline, or an immediate
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] without waiting.
    pub fn expect_visible<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<Vec<Span>, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if !spans.is_empty() => return Ok(spans),
                Ok(_) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: "no match became visible".to_string(),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Retry until exactly one match whose text equals `expected`, or the ONE
    /// `timeout` deadline. Wrong text / zero / 2+ matches all keep retrying
    /// (the screen may still be settling); usage/unsupported fail immediately.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Timeout`] past the deadline, or an immediate
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] without waiting.
    pub fn expect_text<F>(
        &self,
        observe: &mut F,
        expected: &str,
        timeout: Duration,
    ) -> Result<Span, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            let state: Result<Option<Span>, LocateError> = match self.resolve_obs(&obs) {
                Ok(spans)
                    if spans.len() == 1 && spans.first().is_some_and(|s| s.text == expected) =>
                {
                    let mut spans = spans;
                    return Ok(spans.swap_remove(0));
                }
                Ok(_) => Ok(None),
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => Err(e),
            };
            if Instant::now() >= deadline {
                let reason = match state {
                    Ok(_) => format!("no unique match with text {expected:?}"),
                    Err(e) => format!("still failing: {e}"),
                };
                return Err(LocateError::Timeout {
                    waited: timeout,
                    reason,
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Retry until the match count equals `expected`, or the ONE `timeout`
    /// deadline. Usage/unsupported fail immediately.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Timeout`] past the deadline, or an immediate
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] without waiting.
    pub fn expect_count<F>(
        &self,
        observe: &mut F,
        expected: usize,
        timeout: Duration,
    ) -> Result<Vec<Span>, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.len() == expected => return Ok(spans),
                Ok(spans) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("count {} != expected {expected}", spans.len()),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Single-shot presence check against one observation (no retry).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Usage`] for invalid patterns/regions and
    /// [`LocateError::Unsupported`] for non-ASCII case-insensitive haystacks.
    pub fn present_now(&self, obs: &Observation) -> Result<bool, LocateError> {
        Ok(!self.resolve_obs(obs)?.is_empty())
    }

    /// Single-shot absence check against one observation (no retry).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Usage`] for invalid patterns/regions and
    /// [`LocateError::Unsupported`] for non-ASCII case-insensitive haystacks.
    pub fn not_present_now(&self, obs: &Observation) -> Result<bool, LocateError> {
        Ok(self.resolve_obs(obs)?.is_empty())
    }

    /// Retry until zero matches, or the ONE `timeout` deadline.
    /// Usage/unsupported fail immediately.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Timeout`] past the deadline, or an immediate
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] without waiting.
    pub fn eventually_absent<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<(), LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.is_empty() => return Ok(()),
                Ok(spans) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still {} match(es) present", spans.len()),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Watch for the FULL `duration`: any match at any poll fails with
    /// [`LocateError::UnexpectedlyPresent`]. Usage/unsupported fail
    /// immediately. Returns `Ok(())` only after the whole window stays empty.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::UnexpectedlyPresent`] on any match during the
    /// window, or an immediate [`LocateError::Usage`]/[`LocateError::Unsupported`].
    pub fn remains_absent<F>(&self, observe: &mut F, duration: Duration) -> Result<(), LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + duration;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.is_empty() => {}
                Ok(spans) => {
                    return Err(LocateError::UnexpectedlyPresent { matches: spans });
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(_) => {}
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}
