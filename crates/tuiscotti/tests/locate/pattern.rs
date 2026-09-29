use super::*;
use tuiscotti::frame::Color;
use tuiscotti::locate::{LocateError, Locator, StyleQuery};

// ---------------------------------------------------------------------------
// Q01: regex subset
// ---------------------------------------------------------------------------
#[test]
fn regex_dot_class_star_and_anchors() {
    // 15 cols: row 0 is not edge-full, so rows stay separate logical lines.
    let screen = screen_with(&["order 42 ships", "nothing here"], 15);
    let num = Locator::regex("[0-9]+")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(num.len(), 1);
    assert_eq!(num[0].text, "42");
    assert_eq!((num[0].x, num[0].end_x), (6, 8));

    let anchored = Locator::regex("^order.*ships$")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(anchored.len(), 1);
    assert_eq!(anchored[0].text, "order 42 ships");

    let dot = Locator::regex("n.thing")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(dot.len(), 1);
    assert_eq!((dot[0].x, dot[0].y), (0, 1));

    let neg = Locator::regex("[^ ]+")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert!(neg.iter().any(|s| s.text == "order"));
}

#[test]
fn regex_unsupported_constructs_fail_at_build() {
    for bad in ["(a)", "a|b", "a{2}", "a[", "a\\", "*", "[z-a]"] {
        assert!(
            matches!(Locator::regex(bad), Err(LocateError::Usage(_))),
            "{bad}"
        );
    }
    assert!(Locator::regex("^a*?$").is_ok());
    assert!(Locator::regex("").is_err());
}

#[test]
fn regex_case_insensitive_and_non_ascii_unsupported() {
    let screen = screen_with(&["AbC"], 4);
    let spans = Locator::regex_case_insensitive("abc")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);

    let mut cells = blank(4, 1);
    put_wide(&mut cells, 4, 0, 0, "好");
    let wide = finish(cells, 4, 1);
    let err = Locator::regex_case_insensitive("x")
        .unwrap()
        .resolve(&wide, 0)
        .unwrap_err();
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
}

// ---------------------------------------------------------------------------
// Q01: style + region builders
// ---------------------------------------------------------------------------
#[test]
fn style_matches_maximal_runs_and_styled_blanks() {
    let mut cells = blank(8, 1);
    put(&mut cells, 8, 0, 0, "ab  cd");
    for x in [0, 1, 2, 3] {
        cells[x as usize].mods.bold = true;
    }
    cells[5].mods.bold = true; // the 'd', with 'c' as a gap
    let screen = finish(cells, 8, 1);
    let spans = Locator::style(StyleQuery::new().bold(true))
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].text, "ab  ");
    assert_eq!((spans[0].x, spans[0].end_x), (0, 4));
    assert_eq!(spans[1].text, "d");
    assert_eq!((spans[1].x, spans[1].end_x), (5, 6));
}

#[test]
fn style_matches_colors_and_combines_constraints() {
    let mut cells = blank(4, 1);
    put(&mut cells, 4, 0, 0, "abcd");
    cells[1].fg = Color::Indexed(3);
    cells[1].mods.underline = true;
    cells[2].fg = Color::Indexed(3);
    let screen = finish(cells, 4, 1);
    let spans = Locator::style(StyleQuery::new().fg(Color::Indexed(3)).underline(true))
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "b");
}
