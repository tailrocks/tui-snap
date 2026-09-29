//! Production-style fixture views with deterministic models.
//!
//! Each view module owns one fixture app's model, its **single rendering
//! function**, and its key controller. Pure-view tests call `render` through
//! a `TestBackend`; the `*_fixture` binaries call the *same* `render`
//! inside a real terminal; PTY journeys observe the binaries. Controller
//! step functions stay separate from rendering so action tests never
//! depend on pixels.

pub mod menu;
pub mod protocol;
pub mod streams;

use ratatui::style::Color;

/// Fixture theme: carried by every model so sizes × themes matrices stay
/// deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Dark terminal: light ink on black.
    #[default]
    Dark,
    /// Light terminal: dark ink on white.
    Light,
}

impl Theme {
    /// Parse a `--theme` argument. Unknown values are an explicit error.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "dark" => Ok(Theme::Dark),
            "light" => Ok(Theme::Light),
            _ => Err(format!("unknown theme {s:?} (want dark|light)")),
        }
    }

    /// Background color of the view root. The root paints the background
    /// only: unstyled text keeps the terminal default foreground, so every
    /// view exercises `Default` colors alongside indexed and RGB ones.
    #[must_use]
    pub fn bg(self) -> Color {
        match self {
            Theme::Dark => Color::Black,
            Theme::Light => Color::White,
        }
    }
}
