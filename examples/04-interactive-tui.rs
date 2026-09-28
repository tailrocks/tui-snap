//! 04: interactive TUI journey — spawn, wait, snapshot, close.
//!
//! Run: `cargo run --example 04-interactive-tui`
//!
//! A real `/bin/sh` child in a real PTY prints a two-line menu, then idles.
//! The journey waits for the second line (readiness, not sleep), snapshots the
//! screen, and tears the session down. No fixtures: the script is inline.

#[cfg(feature = "pty")]
fn main() {
    use std::time::{Duration, Instant};
    use tuisnap::screen::Screen;
    use tuisnap::tui::{CancelToken, Tui};

    fn rows(screen: &Screen) -> Vec<String> {
        (0..screen.rows())
            .map(|y| {
                let mut s = String::new();
                for x in 0..screen.cols() {
                    let c = screen.get(x, y).unwrap();
                    if !c.continuation {
                        s.push_str(&c.symbol);
                    }
                }
                s.trim_end().to_string()
            })
            .collect()
    }

    let mut s = Tui::new([
        "/bin/sh",
        "-c",
        "printf 'menu: alpha\\nmenu: beta\\n'; sleep 30",
    ])
    .size(40, 8)
    .spawn()
    .unwrap();

    // Readiness wait: fail loudly on timeout, never report false success.
    let cancel = CancelToken::new();
    let obs = s
        .wait_predicate(
            |o| rows(&o.screen).iter().any(|r| r.contains("menu: beta")),
            Instant::now() + Duration::from_secs(5),
            &cancel,
        )
        .unwrap();
    let screen = s.snapshot().unwrap();
    let text = rows(&screen).join("\n");
    assert!(text.contains("menu: alpha") && text.contains("menu: beta"));
    s.close().unwrap();
    println!("EXAMPLE-04-OK revision={} rows={}", obs.revision, screen.rows());
}

/// Without the `pty` feature there is no PTY backend; stay green, say so.
#[cfg(not(feature = "pty"))]
fn main() {
    println!("EXAMPLE-04-SKIP no pty feature");
}
