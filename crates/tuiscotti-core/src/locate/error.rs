use super::Span;
use std::time::Duration;

/// Locator failure. [`LocateError::Usage`] and [`LocateError::Unsupported`]
/// are permanent: retry loops return them immediately without waiting for the
/// deadline. All other variants are retryable observations except
/// [`LocateError::StaleTarget`] and [`LocateError::ViewportOnly`], which are
/// action-delivery refusals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateError {
    /// Invalid locator construction or arguments (empty text pattern,
    /// bad regex, newline in pattern, out-of-bounds region, ...). Fix the
    /// caller; retrying cannot help.
    Usage(String),
    /// A capability the matcher cannot provide (currently: case-insensitive
    /// regex over non-ASCII text, where first-char lowering is unreliable).
    Unsupported(String),
    /// No match where at least one was required.
    NotFound { message: String },
    /// 2+ matches where exactly one was required. Lists every match so the
    /// caller can disambiguate (`nth`/`first`/`within`/tighter query).
    Ambiguous { matches: Vec<Span> },
    /// A retryable assertion never reached its condition before its ONE
    /// deadline.
    Timeout { waited: Duration, reason: String },
    /// Action refused: the target lives in scrollback, which has no viewport
    /// coordinates to click.
    ViewportOnly { span: Span },
    /// Action refused: the screen revision changed after readiness was
    /// established. The click/submit was NOT delivered.
    StaleTarget { expected: u64, current: u64 },
    /// [`Locator::remains_absent`](crate::locate::Locator::remains_absent) observed a match during the watch window.
    UnexpectedlyPresent { matches: Vec<Span> },
}

impl LocateError {
    /// True for [`LocateError::Usage`] and [`LocateError::Unsupported`]:
    /// retry loops must return these immediately.
    #[must_use]
    pub fn is_immediate(&self) -> bool {
        matches!(self, Self::Usage(_) | Self::Unsupported(_))
    }

    pub(crate) fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(m) => write!(f, "locator usage error: {m}"),
            Self::Unsupported(m) => write!(f, "locator unsupported: {m}"),
            Self::NotFound { message } => write!(f, "locator found nothing: {message}"),
            Self::Ambiguous { matches } => {
                write!(f, "locator ambiguous: {} matches: ", matches.len())?;
                for (i, s) in matches.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{s}")?;
                }
                Ok(())
            }
            Self::Timeout { waited, reason } => {
                write!(f, "locator timed out after {waited:?}: {reason}")
            }
            Self::ViewportOnly { span } => {
                write!(f, "locator target is scrollback, not viewport: {span}")
            }
            Self::StaleTarget { expected, current } => write!(
                f,
                "locator target stale: prepared at revision {expected}, now {current}"
            ),
            Self::UnexpectedlyPresent { matches } => {
                write!(f, "locator matched {} time(s) while absent", matches.len())
            }
        }
    }
}

impl std::error::Error for LocateError {}
