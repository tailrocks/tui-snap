//! 05: locators + waits — query the grid, retry to one deadline.
//!
//! Run: `cargo run --example 05-locators-waits`
//!
//! `Locator::text` resolves a unique span with viewport coordinates; the
//! `expect_*` family polls a caller-supplied observer to ONE deadline
//! (usage errors fail immediately, never wait).

use std::time::Duration;
use tuisnap::locate::Locator;
use tuisnap::ratatui::{render_screen, EdgePolicy};
use tuisnap::screen::{CaptureProvenance, CaptureReason, Observation, TermState};

fn main() {
    let shot = render_screen(
        30,
        5,
        |f| {
            f.render_widget(
                ratatui::widgets::Paragraph::new("alpha needle beta"),
                f.area(),
            );
        },
        EdgePolicy::default(),
    )
    .unwrap();
    let screen = shot.into_screen();

    // Unique match: coordinates + text + revision travel on the span.
    let span = Locator::text("needle").resolve_unique(&screen, 7).unwrap();
    assert_eq!(span.text, "needle");
    assert_eq!(span.click_point(), Some((span.x, span.y)));

    // Retryable assertion against a caller-supplied observer (one deadline).
    let mut observe = || {
        Observation::new(
            screen.clone(),
            7,
            CaptureReason::Manual,
            TermState::default(),
            CaptureProvenance::new(0, None, None, 0),
        )
    };
    let spans = Locator::text("needle")
        .expect_visible(&mut observe, Duration::from_secs(2))
        .unwrap();
    assert_eq!(spans.len(), 1);
    let obs = observe();
    assert!(Locator::text("needle").present_now(&obs).unwrap());
    assert!(Locator::text("no-such-text").not_present_now(&obs).unwrap());

    println!("EXAMPLE-05-OK span={span} count={}", spans.len());
}
