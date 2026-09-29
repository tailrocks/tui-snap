//! menu_fixture: live settings-menu TUI over `views::menu`.
//!
//! Pure-view tests and this binary call the same `render`; PTY journeys
//! spawn this binary. Keys: Up/Down move, Space/Enter toggles, `/` focuses
//! the filter, Esc dismisses errors (or refocuses the list), `q` quits.

use crossterm::event::{Event, KeyCode};
use std::time::Duration;
use tuiscotti_fixtures::driver::{self, DriveOpts, Scenario};
use tuiscotti_fixtures::views::menu::{self, MenuKey, Model};

/// Map one terminal event to a controller key.
fn map(event: Event) -> Option<MenuKey> {
    let Event::Key(key) = event else {
        return None;
    };
    match key.code {
        KeyCode::Up => Some(MenuKey::Up),
        KeyCode::Down => Some(MenuKey::Down),
        KeyCode::Enter => Some(MenuKey::Enter),
        KeyCode::Char(' ') => Some(MenuKey::Toggle),
        KeyCode::Char('/') => Some(MenuKey::FocusFilter),
        KeyCode::Esc => Some(MenuKey::Escape),
        KeyCode::Char('q') => Some(MenuKey::Quit),
        KeyCode::Backspace => Some(MenuKey::Backspace),
        KeyCode::Char(c) => Some(MenuKey::Char(c)),
        _ => None,
    }
}

/// Deterministic stdout summary for `--print` (piped-projection tests).
fn print_summary(model: &Model) {
    println!("menu_fixture summary");
    for item in &model.items {
        let marker = if item.toggled { "[x]" } else { "[ ]" };
        let disabled = if item.disabled { " disabled" } else { "" };
        println!("{marker} {}{disabled}", item.name);
    }
}

fn main() -> anyhow::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match driver::parse_common(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("menu_fixture: {e}\n{}", driver::usage("menu_fixture"));
            std::process::exit(2);
        }
    };
    let model = match args.scenario {
        Scenario::Demo => Model::demo(args.theme),
        Scenario::Empty => Model::empty(args.theme),
        Scenario::Error => Model::with_error(args.theme, "boom: deterministic error"),
    };
    if args.print_only {
        print_summary(&model);
        return Ok(());
    }
    let opts = DriveOpts {
        tick: Duration::from_millis(50),
        frames: args.frames,
        protocol_modes: false,
    };
    driver::drive(model, &opts, menu::render, |m, k| menu::step(m, k), map)
}
