//! `protocol_fixture`: live modes/echo TUI over `views::protocol`.
//!
//! Pure-view tests and this binary call the same `render`; PTY journeys
//! spawn this binary. Printable keys echo, Backspace deletes, pastes append
//! (bracketed when negotiated), `F2` cycles the cursor, `F3` toggles the
//! paste flag, resizes and focus events are logged, `q` quits.

use crossterm::event::{Event, KeyCode};
use std::time::Duration;
use tuiscotti_fixtures::driver::{self, DriveOpts, Scenario};
use tuiscotti_fixtures::views::protocol::{self, Model, ProtocolKey};

/// Map one terminal event to a controller key.
fn map(event: Event) -> Option<ProtocolKey> {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('q') => Some(ProtocolKey::Quit),
            KeyCode::Char(c) => Some(ProtocolKey::Char(c)),
            KeyCode::Backspace => Some(ProtocolKey::Backspace),
            KeyCode::F(2) => Some(ProtocolKey::CycleCursor),
            KeyCode::F(3) => Some(ProtocolKey::TogglePaste),
            _ => None,
        },
        Event::Paste(text) => Some(ProtocolKey::Paste(text)),
        Event::Resize(cols, rows) => Some(ProtocolKey::Resize(cols, rows)),
        Event::FocusGained => Some(ProtocolKey::FocusIn),
        Event::FocusLost => Some(ProtocolKey::FocusOut),
        Event::Mouse(_) => None,
    }
}

/// Deterministic stdout summary for `--print` (piped-projection tests).
fn print_summary(model: &Model) {
    println!("protocol_fixture summary");
    println!("echo={:?}", model.echo);
    println!("size={}x{}", model.size.0, model.size.1);
    for entry in &model.log {
        println!("log: {entry}");
    }
}

fn main() -> anyhow::Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match driver::parse_common(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "protocol_fixture: {e}\n{}",
                driver::usage("protocol_fixture")
            );
            std::process::exit(2);
        }
    };
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let model = match args.scenario {
        Scenario::Empty => Model::empty(args.theme, (cols, rows)),
        Scenario::Demo | Scenario::Error => Model::demo(args.theme, (cols, rows)),
    };
    if args.print_only {
        print_summary(&model);
        return Ok(());
    }
    let opts = DriveOpts {
        tick: Duration::from_millis(50),
        frames: args.frames,
        protocol_modes: true,
    };
    driver::drive(model, &opts, protocol::render, protocol::step, map)
}
