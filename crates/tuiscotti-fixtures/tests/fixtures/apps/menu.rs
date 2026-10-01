//! `menu_fixture`: live settings-menu TUI over `views::menu`.
//!
//! Pure-view tests and this binary call the same `render`; PTY journeys
//! spawn this binary. Keys: Up/Down move, Space/Enter toggles, `/` focuses
//! the filter, Esc dismisses errors (or refocuses the list), `q` quits.
//!
//! `--journey` runs the byte-exact 3-row settings menu the
//! `settings_navigation` journey drives (Up/Down move, Space toggles, `q`
//! quits with exit = toggled count). Draw bytes match the retired shell
//! fixture exactly, so committed journey snapshots pin the output.

use crossterm::event::{Event, KeyCode};
use std::time::Duration;
use tuiscotti_fixtures::driver::{self, DriveOpts, Scenario};
use tuiscotti_fixtures::views::menu::{self, MenuKey, Model};

/// Map one terminal event to a controller key.
fn map(event: &Event) -> Option<MenuKey> {
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

/// Journey rows, in order.
const JOURNEY_ROWS: [&str; 3] = ["autosave", "line_numbers", "word_wrap"];

/// Draw one `--journey` frame: clear + home + hide cursor, title, then the
/// 3 rows with the selected row in reverse video. PTY-consumer bytes match
/// the retired shell fixture exactly (same escapes and labels); newlines
/// are explicit `\r\n` because raw mode disables the PTY's `onlcr`
/// translation the script relied on to turn its `\n` into `\r\n`.
fn draw_journey(
    sel: usize,
    toggled: [bool; 3],
    out: &mut impl std::io::Write,
) -> std::io::Result<()> {
    out.write_all(b"\x1b[2J\x1b[H\x1b[?25l")?;
    out.write_all(b"Settings (space toggles, q quits)\r\n\r\n")?;
    for (i, name) in JOURNEY_ROWS.iter().enumerate() {
        let mark = if toggled[i] { "[x]" } else { "[ ]" };
        if i == sel {
            write!(out, "\x1b[7m> {mark} {name}\x1b[0m\r\n")?;
        } else {
            write!(out, "  {mark} {name}\r\n")?;
        }
    }
    out.flush()
}

/// `--journey` input loop: Up/Down move, Space toggles, `q` quits. Reads
/// raw bytes (`q` = 113, Space = 32, arrows arrive as `ESC [ A/B`);
/// anything else is ignored. Returns the toggled count as the exit code.
fn journey_loop() -> anyhow::Result<i32> {
    use std::io::Read as _;
    let mut sel = 0_usize;
    let mut toggled = [false; 3];
    let mut out = std::io::stdout();
    draw_journey(sel, toggled, &mut out)?;
    let mut stdin = std::io::stdin().lock();
    let mut byte = [0_u8; 1];
    loop {
        stdin.read_exact(&mut byte)?;
        match byte[0] {
            113 => break, // `q`
            32 => {
                toggled[sel] = !toggled[sel];
                draw_journey(sel, toggled, &mut out)?;
            }
            27 => {
                let mut seq = [0_u8; 2];
                stdin.read_exact(&mut seq)?;
                match seq {
                    [91, 65] => sel = (sel + 2) % 3, // Up
                    [91, 66] => sel = (sel + 1) % 3, // Down
                    _ => continue,
                }
                draw_journey(sel, toggled, &mut out)?;
            }
            _ => {}
        }
    }
    Ok(toggled.iter().fold(0, |n, t| n + i32::from(*t)))
}

/// Run `--journey`: raw mode + loop, then always restore the terminal
/// (like the retired script's `stty sane` + show cursor) before exiting
/// with the toggled count.
fn run_journey() -> anyhow::Result<()> {
    use anyhow::Context as _;
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::io::Write as _;
    enable_raw_mode().context("enable raw mode")?;
    let count = journey_loop();
    disable_raw_mode().context("disable raw mode")?;
    print!("\x1b[?25h");
    std::io::stdout().flush().context("flush stdout")?;
    std::process::exit(count?);
}

fn main() -> anyhow::Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let journey = raw.iter().any(|a| a == "--journey");
    let rest: Vec<String> = raw.into_iter().filter(|a| a != "--journey").collect();
    let args = match driver::parse_common(&rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("menu_fixture: {e}\n{}", driver::usage("menu_fixture"));
            std::process::exit(2);
        }
    };
    if journey {
        return run_journey();
    }
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
    driver::drive(model, &opts, menu::render, menu::step, |event| map(&event))
}
