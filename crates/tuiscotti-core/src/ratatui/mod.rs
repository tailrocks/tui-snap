//! Direct Ratatui buffer adapter: production view → canonical [`Frame`](crate::frame::Frame).
//!
//! No ANSI, no subprocesses, no PTY in unit tests. Render the real view
//! (widget or draw closure, including stateful widgets) into a `TestBackend`,
//! then convert the buffer — styles and cursor preserved.
//!
//! ```rust,no_run
//! use ratatui::{backend::TestBackend, Terminal, widgets::Paragraph};
//! use tuiscotti_core::{Provenance, ratatui as tuiscotti_core_ratatui};
//!
//! let backend = TestBackend::new(80, 24);
//! let mut term = Terminal::new(backend).unwrap();
//! term.draw(|f| f.render_widget(Paragraph::new("hi"), f.area())).unwrap();
//! let frame = tuiscotti_core_ratatui::capture(
//!     &mut term,
//!     Provenance::now("default", "ratatui", vec![]),
//! );
//! assert!(frame.text().contains("hi"));
//! ```
//!
//! The [`Screen`](crate::screen::Screen)-based API below ([`render_screen`], [`screen_from_buffer`],
//! [`screen_from_test_backend`], [`widget_screen`], [`stateful_screen`]) is the
//! M1 path: production draw closures and real `Widget`/`StatefulWidget`
//! renders convert into validated [`Screen`](crate::screen::Screen)s with buffer origins and
//! post-draw cursor state preserved, and an explicit [`EdgePolicy`] for
//! wide glyphs at row ends (backlog M05, M06, M03-partial, M07).

mod capture;
mod convert;
mod edge;
mod screen;

pub use capture::{capture, draw_frame, from_buffer, widget_frame};
pub use edge::{ClippedCell, EdgePolicy, REPLACEMENT, ScreenCapture};
pub use screen::{
    render, render_screen, screen_from_buffer, screen_from_test_backend, stateful_screen,
    widget_screen,
};
