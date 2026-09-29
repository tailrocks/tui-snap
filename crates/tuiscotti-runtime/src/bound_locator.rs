//! Session-bound locators: fresh observation, unique targets, stale checks (G6).
//!
//! [`BoundLocator`] ties a [`Locator`] to a live [`Session`]. Every method
//! takes a FRESH observation — no manually fabricated revisions — resolves to
//! a UNIQUE viewport target, and actions re-validate against the current
//! revision before delivery: scrollback is never clicked and a target that
//! moved is refused with [`LocateError::StaleTarget`] instead of being
//! delivered stale. Readiness retries never touch the action sink, and
//! destructive input is never retried because an assertion has not passed.
//!
//! Detached query evaluation ([`Locator::resolve`], [`Locator::resolve_obs`])
//! stays available in `tuiscotti_core::locate` for advanced offline use.

use tuiscotti_core::locate::{Action, LocateError, Locator, Span};

use crate::tui::{MouseButton, MouseMods, Session, TuiError};

/// A locator action refused or failed: either side retains its typed source.
#[derive(Debug)]
pub enum ActionError {
    /// Fresh observation or input delivery failed.
    Session(TuiError),
    /// Resolution, uniqueness, or staleness refused the action.
    Locate(LocateError),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(e) => write!(f, "session failed: {e}"),
            Self::Locate(e) => write!(f, "locator refused: {e}"),
        }
    }
}

impl std::error::Error for ActionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(e) => Some(e),
            Self::Locate(e) => Some(e),
        }
    }
}

impl From<TuiError> for ActionError {
    fn from(e: TuiError) -> Self {
        Self::Session(e)
    }
}

impl From<LocateError> for ActionError {
    fn from(e: LocateError) -> Self {
        Self::Locate(e)
    }
}

/// A [`Locator`] bound to a live [`Session`].
///
/// Created by [`Session::get_by`] / [`Session::get_by_text`]. `Send + Sync`
/// when the session is shared; each call observes afresh.
#[derive(Debug)]
pub struct BoundLocator<'s> {
    session: &'s Session,
    locator: Locator,
}

impl Session {
    /// Bind an arbitrary locator (scoped composition via
    /// [`Locator::within`], [`Locator::before`], [`Locator::after`], ...).
    #[must_use]
    pub fn get_by(&self, locator: Locator) -> BoundLocator<'_> {
        BoundLocator {
            session: self,
            locator,
        }
    }

    /// Bind a text locator ([`Locator::text`]) to this session.
    #[must_use]
    pub fn get_by_text(&self, text: &str) -> BoundLocator<'_> {
        self.get_by(Locator::text(text.to_string()))
    }
}

impl BoundLocator<'_> {
    /// The underlying detached locator (for offline composition).
    #[must_use]
    pub fn locator(&self) -> &Locator {
        &self.locator
    }

    /// Fresh observation, resolved to the UNIQUE viewport match. Ambiguous,
    /// missing, or scrollback-only targets fail without side effects.
    /// # Errors
    ///
    /// Returns [`ActionError`] when observation fails or no unique target resolves.
    pub fn expect_visible(&self) -> Result<Span, ActionError> {
        let obs = self.session.observe_now()?;
        Ok(self.locator.resolve_unique(&obs.screen, obs.revision)?)
    }

    /// Left-click the unique viewport target: resolve on a fresh observation,
    /// then stale-check against the current revision before delivery. The
    /// click is delivered at most once; a moved target fails with
    /// [`LocateError::StaleTarget`] and is never delivered.
    /// # Errors
    ///
    /// Returns [`ActionError`] when observation, resolution, or delivery fails.
    pub fn click(&self) -> Result<(), ActionError> {
        let obs = self.session.observe_now()?;
        let pending = self.locator.prepare_action(&obs)?;
        let current = self.session.observe_now()?;
        let mut delivered: Option<Action> = None;
        pending.click(&current, &mut |action| {
            delivered = Some(action);
        })?;
        match delivered {
            Some(Action::Click { x, y }) => {
                self.session
                    .click(MouseButton::Left, x, y, MouseMods::NONE)?;
                Ok(())
            }
            Some(Action::Submit { .. }) => {
                // Unreachable: `click` delivers `Action::Click` only.
                Ok(())
            }
            None => Err(ActionError::Locate(LocateError::Usage(
                "action sink never ran".to_string(),
            ))),
        }
    }
}
