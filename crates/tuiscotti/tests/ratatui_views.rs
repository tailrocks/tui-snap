//! Production Ratatui → Screen adapters (M05, M06, M03-partial, M07 edge policy).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color as RColor, Modifier, Style};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use tuiscotti::frame::CursorStyle;
use tuiscotti::ratatui::{
    EdgePolicy, REPLACEMENT, render_screen, screen_from_buffer, screen_from_test_backend,
    stateful_screen, widget_screen,
};

fn row_text(screen: &tuiscotti::Screen, y: u16) -> String {
    let mut row = String::new();
    for x in 0..screen.cols() {
        let c = screen.get(x, y).unwrap();
        if !c.continuation {
            row.push_str(&c.symbol);
        }
    }
    row.trim_end().to_string()
}

#[test]
fn draw_closure_renders_content_and_places_cursor() {
    let cap = render_screen(
        20,
        5,
        |f| {
            f.render_widget(Paragraph::new("hello"), f.area());
            f.set_cursor_position((5, 2));
        },
        EdgePolicy::default(),
    )
    .unwrap();
    assert_eq!(row_text(&cap.screen, 0), "hello");
    assert_eq!(cap.screen.origin(), (0, 0));
    let cur = cap.screen.cursor();
    assert!(cur.visible);
    assert_eq!((cur.x, cur.y), (5, 2));
    assert!(!cap.has_clips());
    assert_eq!(cap.policy, EdgePolicy::ClipWithReplacement);
}

#[test]
fn draw_closure_without_cursor_leaves_it_hidden() {
    let cap = render_screen(
        10,
        3,
        |f| {
            f.render_widget(Paragraph::new("x"), f.area());
        },
        EdgePolicy::default(),
    )
    .unwrap();
    assert!(!cap.screen.cursor().visible);
}

#[test]
fn draw_closure_supports_stateful_widget() {
    let mut state = ListState::default();
    state.select(Some(1));
    let cap = render_screen(
        12,
        4,
        |f| {
            let list = List::new(["aa", "bb", "cc"])
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            f.render_stateful_widget(list, f.area(), &mut state);
        },
        EdgePolicy::default(),
    )
    .unwrap();
    assert_eq!(row_text(&cap.screen, 1), "bb");
    assert!(cap.screen.get(0, 1).unwrap().mods.reverse);
    assert!(!cap.screen.get(0, 0).unwrap().mods.reverse);
}

#[test]
fn stateful_screen_uses_production_render_fn() {
    let list = List::new([ListItem::new("one"), ListItem::new("two")])
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(0));
    let cap = stateful_screen(list, &mut state, 10, 3, EdgePolicy::default()).unwrap();
    assert_eq!(row_text(&cap.screen, 0), "one");
    assert!(cap.screen.get(0, 0).unwrap().mods.bold);
    assert!(!cap.screen.get(0, 1).unwrap().mods.bold);
}

#[test]
fn widget_screen_renders_fullscreen() {
    let cap = widget_screen(Paragraph::new("wide"), 10, 3, EdgePolicy::default()).unwrap();
    assert_eq!(row_text(&cap.screen, 0), "wide");
    assert_eq!((cap.screen.cols(), cap.screen.rows()), (10, 3));
    assert!(!cap.screen.cursor().visible);
}

#[test]
fn buffer_with_nonzero_origin_preserves_origin() {
    let mut buf = Buffer::empty(Rect::new(5, 3, 10, 4));
    buf.set_string(5, 3, "hi", Style::default());
    buf.set_string(6, 5, "yo", Style::default());
    let cap = screen_from_buffer(&buf, None, EdgePolicy::default()).unwrap();
    assert_eq!(cap.screen.origin(), (5, 3));
    assert_eq!((cap.screen.cols(), cap.screen.rows()), (10, 4));
    assert_eq!(row_text(&cap.screen, 0), "hi");
    assert_eq!(row_text(&cap.screen, 2), " yo");
    assert!(cap.screen.get(0, 0).unwrap().symbol == "h");
}

#[test]
fn buffer_cursor_translates_to_grid_local() {
    let buf = Buffer::empty(Rect::new(5, 3, 10, 4));
    let cap = screen_from_buffer(
        &buf,
        Some((Position::new(7, 4), true)),
        EdgePolicy::default(),
    )
    .unwrap();
    let cur = cap.screen.cursor();
    assert!(cur.visible);
    assert_eq!((cur.x, cur.y), (2, 1));
}

#[test]
fn buffer_cursor_outside_area_captured_hidden_with_note() {
    let buf = Buffer::empty(Rect::new(5, 3, 10, 4));
    let cap = screen_from_buffer(
        &buf,
        Some((Position::new(0, 0), true)),
        EdgePolicy::default(),
    )
    .unwrap();
    assert!(!cap.screen.cursor().visible);
    assert!(cap.notes.iter().any(|n| n.contains("outside buffer area")));
}

#[test]
fn test_backend_capture_includes_cursor() {
    let backend = TestBackend::new(16, 4);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| {
        f.render_widget(Paragraph::new("tb"), f.area());
        f.set_cursor_position((3, 1));
    })
    .unwrap();
    let cap = screen_from_test_backend(&mut term, EdgePolicy::default()).unwrap();
    assert_eq!(row_text(&cap.screen, 0), "tb");
    let cur = cap.screen.cursor();
    assert!(cur.visible);
    assert_eq!((cur.x, cur.y), (3, 1));
}

#[test]
fn wide_glyph_mid_row_gets_continuation() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
    buf[(3, 1)].set_symbol("漢");
    let cap = screen_from_buffer(&buf, None, EdgePolicy::default()).unwrap();
    let lead = cap.screen.get(3, 1).unwrap();
    assert_eq!(lead.symbol, "漢");
    assert_eq!(lead.width, 2);
    assert!(!lead.continuation);
    let cont = cap.screen.get(4, 1).unwrap();
    assert!(cont.continuation);
    assert_eq!(cont.width, 0);
    assert!(cont.symbol.is_empty());
    assert!(!cap.has_clips());
}

#[test]
fn wide_glyph_at_row_end_clips_with_replacement_by_default() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
    buf[(9, 0)].set_symbol("漢");
    let cap = screen_from_buffer(&buf, None, EdgePolicy::default()).unwrap();
    let cell = cap.screen.get(9, 0).unwrap();
    assert_eq!(cell.symbol, REPLACEMENT);
    assert_eq!(cell.width, 1);
    assert!(!cell.continuation);
    assert_eq!(cap.policy, EdgePolicy::ClipWithReplacement);
    assert!(cap.has_clips());
    assert_eq!(cap.clipped.len(), 1);
    assert_eq!(cap.clipped[0].x, 9);
    assert_eq!(cap.clipped[0].y, 0);
    assert_eq!(cap.clipped[0].symbol, "漢");
    assert!(cap.notes.iter().any(|n| n.contains("ClipWithReplacement")));
}

#[test]
fn wide_glyph_at_row_end_fails_under_error_policy() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
    buf[(9, 2)].set_symbol("漢");
    let err = screen_from_buffer(&buf, None, EdgePolicy::Error).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("漢"), "missing glyph: {msg}");
    assert!(msg.contains("(9,2)"), "missing position: {msg}");
}

#[test]
fn styled_blank_and_mods_survive_round_trip() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
    buf[(2, 0)].set_style(
        Style::default()
            .fg(RColor::Red)
            .bg(RColor::Blue)
            .add_modifier(Modifier::UNDERLINED | Modifier::HIDDEN | Modifier::SLOW_BLINK),
    );
    let cap = screen_from_buffer(
        &buf,
        Some((Position::new(2, 0), true)),
        EdgePolicy::default(),
    )
    .unwrap();
    let cell = cap.screen.get(2, 0).unwrap();
    assert_eq!(cell.symbol, " ");
    assert_eq!(cell.fg, tuiscotti::frame::Color::Indexed(1));
    assert_eq!(cell.bg, tuiscotti::frame::Color::Indexed(4));
    assert!(cell.mods.underline);
    assert!(cell.mods.hidden);
    assert!(cell.mods.blink);
    let cur = cap.screen.cursor();
    assert!(cur.visible);
    assert_eq!((cur.x, cur.y), (2, 0));
    assert_eq!(cur.style, CursorStyle::Block);
}

#[test]
fn rgb_and_bold_italic_styles_preserved() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
    buf.set_string(
        0,
        0,
        "ab",
        Style::default()
            .fg(RColor::Rgb(1, 2, 3))
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    );
    let cap = screen_from_buffer(&buf, None, EdgePolicy::default()).unwrap();
    let cell = cap.screen.get(0, 0).unwrap();
    assert_eq!(
        cell.fg,
        tuiscotti::frame::Color::Rgb(tuiscotti::frame::Rgb::new(1, 2, 3))
    );
    assert!(cell.mods.bold);
    assert!(cell.mods.italic);
}

#[test]
fn render_screen_origin_is_zero_zero() {
    let cap = render_screen(4, 4, |_| {}, EdgePolicy::default()).unwrap();
    assert_eq!(cap.screen.origin(), (0, 0));
    assert_eq!((cap.screen.cols(), cap.screen.rows()), (4, 4));
}
