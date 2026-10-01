//! Facade error: one typed `Error`/`Result` over every fallible operation.
//!
//! Each variant retains its typed source; nothing is erased to a string.
//! `?` converts automatically via the [`From`] impls.

use std::fmt::{Display, Formatter};

/// Fallible-operation result over the facade [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Every failure a facade operation can report, with sources retained.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Invalid screen, region, or observation construction.
    Screen(tuiscotti_core::screen::ScreenError),
    /// Invalid canonical frame.
    Frame(tuiscotti_core::frame::FrameError),
    /// Query construction, resolution, or action refusal.
    Locate(tuiscotti_core::locate::LocateError),
    /// Rendering or export refused the input.
    Render(tuiscotti_render::render::RenderError),
    /// Sample rendering or evidence writing failed.
    Assert(tuiscotti_insta::assert::AssertError),
    /// Frozen root missing/corrupt/mismatched, or accept rejected.
    Frozen(tuiscotti_insta::assert::FrozenError),
    /// Compound canonical/PNG generation bindings disagree.
    Consistency(tuiscotti_insta::assert::ConsistencyError),
    /// Piped child resolution or spawn failed (no child output exists).
    Spawn(tuiscotti_runtime::command::SpawnError),
    /// Filesystem failure.
    Io(std::io::Error),
    /// Live session failure (feature `pty`).
    #[cfg(feature = "pty")]
    Tui(tuiscotti_runtime::tui::TuiError),
    /// Live wait expired, cancelled, or unsupported (feature `pty`).
    #[cfg(feature = "pty")]
    Wait(tuiscotti_runtime::tui::WaitError),
    /// Session-bound locator action refused (feature `pty`).
    #[cfg(feature = "pty")]
    Action(tuiscotti_runtime::bound_locator::ActionError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Screen(e) => write!(f, "{e}"),
            Self::Frame(e) => write!(f, "{e}"),
            Self::Locate(e) => write!(f, "{e}"),
            Self::Render(e) => write!(f, "{e}"),
            Self::Assert(e) => write!(f, "{e}"),
            Self::Frozen(e) => write!(f, "{e}"),
            Self::Consistency(e) => write!(f, "{e}"),
            Self::Spawn(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            #[cfg(feature = "pty")]
            Self::Tui(e) => write!(f, "{e}"),
            #[cfg(feature = "pty")]
            Self::Wait(e) => write!(f, "{e}"),
            #[cfg(feature = "pty")]
            Self::Action(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Screen(e) => Some(e),
            Self::Frame(e) => Some(e),
            Self::Locate(e) => Some(e),
            Self::Render(e) => Some(e),
            Self::Assert(e) => Some(e),
            Self::Frozen(e) => Some(e),
            Self::Consistency(e) => Some(e),
            Self::Spawn(e) => Some(e),
            Self::Io(e) => Some(e),
            #[cfg(feature = "pty")]
            Self::Tui(e) => Some(e),
            #[cfg(feature = "pty")]
            Self::Wait(e) => Some(e),
            #[cfg(feature = "pty")]
            Self::Action(e) => Some(e),
        }
    }
}

macro_rules! convert {
    ($variant:ident, $type:ty) => {
        impl From<$type> for Error {
            fn from(e: $type) -> Self {
                Self::$variant(e)
            }
        }
    };
}

convert!(Screen, tuiscotti_core::screen::ScreenError);
convert!(Frame, tuiscotti_core::frame::FrameError);
convert!(Locate, tuiscotti_core::locate::LocateError);
convert!(Render, tuiscotti_render::render::RenderError);
convert!(Assert, tuiscotti_insta::assert::AssertError);
convert!(Frozen, tuiscotti_insta::assert::FrozenError);
convert!(Consistency, tuiscotti_insta::assert::ConsistencyError);
convert!(Spawn, tuiscotti_runtime::command::SpawnError);
convert!(Io, std::io::Error);
#[cfg(feature = "pty")]
convert!(Tui, tuiscotti_runtime::tui::TuiError);
#[cfg(feature = "pty")]
convert!(Wait, tuiscotti_runtime::tui::WaitError);
#[cfg(feature = "pty")]
convert!(Action, tuiscotti_runtime::bound_locator::ActionError);
