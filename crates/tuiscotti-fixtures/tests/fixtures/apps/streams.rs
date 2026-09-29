//! streams_fixture: live scrollable-log TUI over `views::streams`.
//!
//! Pure-view tests and this binary call the same `render`; PTY journeys
//! spawn this binary. Keys: Up/Down/PgUp/PgDn/Home/End scroll, `f` toggles
//! follow, `q` quits. `--emit-raw` writes raw bytes incl. invalid UTF-8 to
//! stdout for piped-projection tests (no TUI).

use crossterm::event::{Event, KeyCode};
use std::io::Write;
use std::time::Duration;
use tuiscotti_fixtures::driver::{self, DriveOpts, Scenario};
use tuiscotti_fixtures::views::streams::{self, Model, StreamsKey};

/// Raw pipe payload: valid lines, invalid sequences, and valid CJK.
/// Deterministic; `pipe_projection` accounts every invalid sequence.
const RAW_PAYLOAD: &[u8] =
    b"streams-raw v1\nline-ok\n\xff\xfe bad-bytes\n\xe6\x97\xa5 valid-cjk\n\x80lone-continuation\n";

/// Map one terminal event to a controller key.
fn map(event: Event) -> Option<StreamsKey> {
    let Event::Key(key) = event else {
        return None;
    };
    match key.code {
        KeyCode::Up => Some(StreamsKey::Up),
        KeyCode::Down => Some(StreamsKey::Down),
        KeyCode::PageUp => Some(StreamsKey::PageUp),
        KeyCode::PageDown => Some(StreamsKey::PageDown),
        KeyCode::Home => Some(StreamsKey::Home),
        KeyCode::End => Some(StreamsKey::End),
        KeyCode::Char('f') => Some(StreamsKey::ToggleFollow),
        KeyCode::Char('q') => Some(StreamsKey::Quit),
        _ => None,
    }
}

/// Deterministic stdout summary for `--print` (piped-projection tests).
fn print_summary(model: &Model) {
    println!("streams_fixture summary");
    for line in &model.lines {
        println!("{:?}: {}", line.level, line.text);
    }
}

fn main() -> anyhow::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--emit-raw") {
        std::io::stdout().write_all(RAW_PAYLOAD)?;
        return Ok(());
    }
    let args = match driver::parse_common(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("streams_fixture: {e}\n{}", driver::usage("streams_fixture"));
            std::process::exit(2);
        }
    };
    let model = match args.scenario {
        Scenario::Empty => Model::empty(args.theme),
        Scenario::Demo | Scenario::Error => Model::demo(args.theme),
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
    // Viewport rows for scroll math: read the live terminal size once.
    let (_, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let view_rows = usize::from(rows.saturating_sub(4)).max(1);
    driver::drive(
        model,
        &opts,
        streams::render,
        |m, k| streams::step(m, k, view_rows),
        map,
    )
}
