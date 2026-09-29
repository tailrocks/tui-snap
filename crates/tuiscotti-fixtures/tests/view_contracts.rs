//! View contracts: coverage matrix over pure views.
//!
//! Sizes × themes × views, RGB/indexed/default colors, independent
//! modifiers, styled spaces, wide continuations, combining characters,
//! CJK/icons, clipped views, 1-row/column cases, cursor states, and
//! error/empty/focus/selection states — all through the public API.

#[path = "common/mod.rs"]
mod common;

#[path = "common/pure.rs"]
mod pure;

use common::{menu_frame, streams_frame};
use pure::protocol_frame;
use tuiscotti::frame::Color;
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;

#[test]
fn matrix_sizes_themes_validate() {
    let sizes = [(80u16, 24u16), (120, 40), (40, 10), (160, 50), (48, 12)];
    for theme in [Theme::Dark, Theme::Light] {
        for (cols, rows) in sizes {
            for (name, frame) in [
                ("menu", menu_frame(cols, rows, theme, Scenario::Demo)),
                ("streams", streams_frame(cols, rows, theme, false)),
                ("protocol", protocol_frame(cols, rows, theme, false)),
            ] {
                frame.validate().expect("valid frame");
                assert_eq!((frame.cols, frame.rows), (cols, rows), "{name}");
                assert!(!frame.text().is_empty(), "{name} renders content");
                if frame.cursor.visible {
                    assert!(
                        frame.cursor.x < cols && frame.cursor.y < rows,
                        "{name} cursor"
                    );
                }
            }
        }
    }
}

#[test]
fn tiny_viewports_validate() {
    for (cols, rows) in [(1u16, 1u16), (1, 24), (80, 1), (2, 1), (10, 4)] {
        menu_frame(cols, rows, Theme::Dark, Scenario::Demo)
            .validate()
            .expect("menu tiny valid");
        streams_frame(cols, rows, Theme::Light, false)
            .validate()
            .expect("streams tiny valid");
        protocol_frame(cols, rows, Theme::Dark, false)
            .validate()
            .expect("protocol tiny valid");
    }
}

#[test]
fn color_sources_covered() {
    let frame = streams_frame(80, 24, Theme::Dark, false);
    let (mut rgb, mut indexed, mut default) = (false, false, false);
    for cell in &frame.cells {
        match cell.fg {
            Color::Rgb(_) => rgb = true,
            Color::Indexed(_) => indexed = true,
            Color::Default => default = true,
        }
    }
    assert!(rgb, "direct RGB line present");
    assert!(indexed, "indexed palette line present");
    assert!(default, "default-color line present");
    let menu = menu_frame(80, 24, Theme::Dark, Scenario::Demo);
    assert!(
        menu.cells.iter().any(|c| c.bg == Color::Indexed(4)),
        "selected menu row carries indexed blue background"
    );
}

#[test]
fn modifiers_independent() {
    let menu = menu_frame(80, 24, Theme::Dark, Scenario::Demo);
    let mods: Vec<_> = menu.cells.iter().map(|c| c.mods).collect();
    assert!(
        mods.iter().any(|m| m.bold && !m.italic),
        "bold alone (title/selection)"
    );
    assert!(
        mods.iter().any(|m| m.dim && !m.bold),
        "dim alone (disabled row)"
    );
    assert!(
        mods.iter().any(|m| m.reverse && !m.bold),
        "reverse alone (status bar)"
    );
    let streams = streams_frame(80, 24, Theme::Dark, false);
    assert!(
        streams.cells.iter().any(|c| c.mods.underline),
        "selected log line underlined"
    );
    assert!(
        streams.cells.iter().any(|c| c.mods.bold && c.mods.reverse),
        "error line bold + reversed"
    );
}

#[test]
fn styled_spaces_carried_in_state_trimmed_in_txt() {
    let frame = streams_frame(80, 24, Theme::Dark, false);
    let styled_blank = frame
        .cells
        .iter()
        .any(|c| c.symbol == " " && !c.continuation && c.bg == Color::Indexed(236));
    assert!(styled_blank, "warn line trailing spaces carry a background");
    for line in frame.text().lines() {
        assert!(!line.ends_with(' '), "TXT trims styled tails too");
    }
}

#[test]
fn wide_continuations_and_combining() {
    let frame = streams_frame(80, 24, Theme::Dark, false);
    let mut wide_ok = false;
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let Some(cell) = frame.get(x, y) else {
                continue;
            };
            if cell.width == 2 {
                let next = frame.get(x + 1, y).expect("follower in grid");
                assert!(next.continuation && next.width == 0, "well-formed follower");
                assert!(next.symbol.is_empty());
                wide_ok = true;
            }
        }
    }
    assert!(wide_ok, "wide CJK leads with continuations present");
    let combining = frame
        .cells
        .iter()
        .any(|c| !c.continuation && c.symbol.chars().count() > 1 && c.width == 1);
    assert!(combining, "combining sequences share one width-1 cell");
}

#[test]
fn cjk_icons_present() {
    let txt = streams_frame(80, 24, Theme::Dark, false).text();
    for glyph in ["日本語", "한국어", "中文", "→", "✓", "✗", "★", "⠋"] {
        assert!(txt.contains(glyph), "txt carries {glyph:?}");
    }
    assert!(
        menu_frame(80, 24, Theme::Dark, Scenario::Demo)
            .text()
            .contains("日本語モード")
    );
}

#[test]
fn clipped_views_keep_valid_frames() {
    // 40x10 shows one list row though five exist: clipped, not broken.
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    frame.validate().expect("clipped frame valid");
    let txt = frame.text();
    assert!(txt.contains("autosave"));
    assert!(!txt.contains("word_wrap"), "rows below the fold clipped");
}

#[test]
fn cursor_states() {
    use tuiscotti_fixtures::views::protocol::{CursorState, Model};
    for (state, visible) in [
        (CursorState::Block, true),
        (CursorState::Underline, true),
        (CursorState::Bar, true),
        (CursorState::Hidden, false),
    ] {
        let mut model = Model::demo(Theme::Dark, (50, 12));
        model.cursor = state;
        let frame = tuiscotti::ratatui::draw_frame(50, 12, common::prov("cursor"), |f| {
            tuiscotti_fixtures::views::protocol::render(f, &model);
        });
        frame.validate().expect("cursor frame valid");
        assert_eq!(frame.cursor.visible, visible, "{state:?} visibility");
    }
    // Menu: filter focus shows the hardware cursor, list focus hides it.
    let mut model = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    model.focus = tuiscotti_fixtures::views::menu::Focus::Filter;
    let frame = tuiscotti::ratatui::draw_frame(40, 10, common::prov("menu-cursor"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model);
    });
    assert!(frame.cursor.visible, "filter focus shows cursor");
    let listed = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    assert!(!listed.cursor.visible, "list focus hides cursor");
}

#[test]
fn error_empty_focus_selection() {
    let error = menu_frame(48, 14, Theme::Dark, Scenario::Error);
    let txt = error.text();
    assert!(txt.contains("boom: deterministic error"), "popup text");
    let empty = menu_frame(48, 14, Theme::Dark, Scenario::Empty);
    assert!(empty.text().contains("No rows match"), "menu empty state");
    assert!(
        streams_frame(60, 12, Theme::Dark, true)
            .text()
            .contains("No log lines")
    );
    assert!(
        protocol_frame(50, 12, Theme::Dark, true)
            .text()
            .contains("No events yet")
    );
    assert!(
        menu_frame(40, 10, Theme::Dark, Scenario::Demo)
            .text()
            .contains("focus=list")
    );
    // Controller: Esc dismisses the error without touching selection.
    let mut model = tuiscotti_fixtures::views::menu::Model::with_error(Theme::Dark, "x");
    let selected = model.selected;
    assert!(!tuiscotti_fixtures::views::menu::step(
        &mut model,
        &tuiscotti_fixtures::views::menu::MenuKey::Escape
    ));
    assert!(model.error.is_none());
    assert_eq!(model.selected, selected);
}

#[test]
fn data_files_mirror_models() {
    let raw = String::from_utf8(common::read_data("menu-items.txt").expect("fixture data"))
        .expect("utf8");
    let rows: Vec<Vec<&str>> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(';').collect())
        .collect();
    let model = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    assert_eq!(rows.len(), model.items.len(), "menu data row count");
    for (row, item) in rows.iter().zip(model.items.iter()) {
        assert_eq!(row[0], item.name);
        assert_eq!(row[1] == "1", item.toggled);
        assert_eq!(row[2] == "1", item.disabled);
    }
    let raw = String::from_utf8(common::read_data("streams-log.txt").expect("fixture data"))
        .expect("utf8");
    let rows: Vec<&str> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    let model = tuiscotti_fixtures::views::streams::Model::demo(Theme::Dark);
    assert_eq!(rows.len(), model.lines.len(), "streams data row count");
    for (row, line) in rows.iter().zip(model.lines.iter()) {
        let level = row.split(':').next().expect("level prefix");
        assert_eq!(*row, &format!("{level}:{}", line.text));
    }
}

#[test]
fn pure_views_are_deterministic() {
    let a = menu_frame(40, 10, Theme::Dark, Scenario::Demo).to_json();
    let b = menu_frame(40, 10, Theme::Dark, Scenario::Demo).to_json();
    assert_eq!(a, b);
    let a = streams_frame(60, 12, Theme::Light, false).to_json();
    let b = streams_frame(60, 12, Theme::Light, false).to_json();
    assert_eq!(a, b);
}
