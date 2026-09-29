use super::retry::POLL_INTERVAL;
use super::{Action, LocateError, Locator, Span};
use crate::screen::Observation;
use std::time::{Duration, Instant};

/// A readiness-established action target. Built by resolving a locator to a
/// UNIQUE viewport span at one revision; delivered by [`PendingAction::click`]
/// /[`PendingAction::submit`], which re-validate the unique target against the
/// CURRENT observation and refuse stale revisions. The sink runs at most once
/// per `click`/`submit` call, and readiness retries never touch the sink.
#[derive(Debug, Clone)]
pub struct PendingAction {
    locator: Locator,
    span: Span,
    revision: u64,
}

impl Locator {
    /// Single-shot readiness: unique viewport target at this observation's
    /// revision. Scrollback-only matches fail with
    /// [`LocateError::ViewportOnly`].
    ///
    /// # Errors
    ///
    /// Returns [`LocateError`] when no unique viewport target exists at this
    /// revision (not found, ambiguous, or scrollback-only).
    pub fn prepare_action(&self, obs: &Observation) -> Result<PendingAction, LocateError> {
        let span = self.resolve_unique(&obs.screen, obs.revision)?;
        PendingAction::from_span(self.clone(), span, obs.revision)
    }

    /// Retryable readiness: poll until a unique viewport target exists, or
    /// the ONE `timeout` deadline. Never invokes any action sink (Q05).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Timeout`] past the deadline, or an immediate
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] without waiting.
    pub fn prepare_action_retry<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<PendingAction, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.prepare_action(&obs) {
                Ok(pending) => return Ok(pending),
                Err(e) if e.is_immediate() => return Err(e),
                Err(LocateError::ViewportOnly { .. }) => {
                    // A scrollback-only target never becomes clickable by
                    // waiting on viewport revisions; still, the anchor text
                    // may scroll into view, so keep retrying to the deadline.
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: "target stayed outside the viewport".to_string(),
                        });
                    }
                }
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("no unique target: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl PendingAction {
    /// Bind an already-resolved span. Fails with [`LocateError::ViewportOnly`]
    /// for scrollback spans and [`LocateError::Usage`] when the span's
    /// revision differs from `revision` (a cross-revision bind is meaningless).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::ViewportOnly`] for scrollback spans and
    /// [`LocateError::Usage`] on revision mismatch.
    pub fn from_span(locator: Locator, span: Span, revision: u64) -> Result<Self, LocateError> {
        if span.scrollback {
            return Err(LocateError::ViewportOnly { span });
        }
        if span.revision != revision {
            return Err(LocateError::Usage(format!(
                "span revision {} != bind revision {revision}",
                span.revision
            )));
        }
        Ok(Self {
            locator,
            span,
            revision,
        })
    }

    /// The bound target span.
    #[must_use]
    pub fn span(&self) -> &Span {
        &self.span
    }

    /// Revision the target was bound at.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Re-validate the unique target against `current`, then deliver
    /// [`Action::Click`] to `sink` exactly once. Revision mismatch fails with
    /// [`LocateError::StaleTarget`] WITHOUT delivering or resolving further:
    /// a click is never sent stale.
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::StaleTarget`] on revision mismatch (the sink
    /// is not invoked), or re-resolution failures.
    pub fn click(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
    ) -> Result<(), LocateError> {
        self.deliver(current, sink, false)
    }

    /// Like [`PendingAction::click`] but delivers [`Action::Submit`].
    ///
    /// # Errors
    ///
    /// Same failures as [`PendingAction::click`].
    pub fn submit(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
    ) -> Result<(), LocateError> {
        self.deliver(current, sink, true)
    }

    fn deliver(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
        submit: bool,
    ) -> Result<(), LocateError> {
        if current.revision != self.revision {
            return Err(LocateError::StaleTarget {
                expected: self.revision,
                current: current.revision,
            });
        }
        // Same revision: re-resolve to prove the target is still unique
        // pre-delivery (Q03 strictness at the delivery boundary).
        let fresh = self
            .locator
            .resolve_unique(&current.screen, current.revision)?;
        debug_assert_eq!(fresh, self.span, "same revision must resolve identically");
        let (x, y) = self
            .span
            .click_point()
            .ok_or_else(|| LocateError::ViewportOnly {
                span: self.span.clone(),
            })?;
        sink(if submit {
            Action::Submit { x, y }
        } else {
            Action::Click { x, y }
        });
        Ok(())
    }
}
