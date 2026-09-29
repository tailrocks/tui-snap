use super::*;
use tuiscotti::frame::Color;
use tuiscotti::locate::{LocateError, Locator, StyleQuery};

// ---------------------------------------------------------------------------
// Q01: regex subset
// ---------------------------------------------------------------------------
#[test]
fn regex_dot_class_star_and_anchors() {
    // 15 cols: row 0 is not edge-full, so rows stay separate logical lines.
    let screen =
        screen_with(&["order 42 ships", "nothing here"], 15).expect("screen_with succeeds");
    let num = Locator::regex("[0-9]+")
        .expect("Locator::regex(\"[0-9]+\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
    assert_eq!(num.len(), 1);
    assert_eq!(num[0].text, "42");
    assert_eq!((num[0].x, num[0].end_x), (6, 8));

    let anchored = Locator::regex("^order.*ships$")
        .expect("Locator::regex(\"^order.*ships$\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
    assert_eq!(anchored.len(), 1);
    assert_eq!(anchored[0].text, "order 42 ships");

    let dot = Locator::regex("n.thing")
        .expect("Locator::regex(\"n.thing\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
    assert_eq!(dot.len(), 1);
    assert_eq!((dot[0].x, dot[0].y), (0, 1));

    let neg = Locator::regex("[^ ]+")
        .expect("Locator::regex(\"[^ ]+\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
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
    let screen = screen_with(&["AbC"], 4).expect("screen_with succeeds");
    let spans = Locator::regex_case_insensitive("abc")
        .expect("Locator::regex_case_insensitive(\"abc\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
    assert_eq!(spans.len(), 1);

    let mut cells = blank(4, 1);
    put_wide(&mut cells, 4, 0, 0, "好");
    let wide = finish(cells, 4, 1).expect("finish succeeds");
    let err = Locator::regex_case_insensitive("x")
        .expect("Locator::regex_case_insensitive(\"x\") succeeds")
        .resolve(&wide, 0)
        .expect_err("resolve of non-ASCII-insensitive pattern is an error");
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
}

// ---------------------------------------------------------------------------
// Q01: style + region builders
// ---------------------------------------------------------------------------
#[test]
fn style_matches_maximal_runs_and_styled_blanks() {
    let mut cells = blank(8, 1);
    put(&mut cells, 8, 0, 0, "ab  cd");
    for x in [0usize, 1, 2, 3] {
        cells[x].mods.bold = true;
    }
    cells[5].mods.bold = true; // the 'd', with 'c' as a gap
    let screen = finish(cells, 8, 1).expect("finish succeeds");
    let spans = Locator::style(StyleQuery::new().bold(true))
        .resolve(&screen, 0)
        .expect("Locator::style(StyleQuery::new().bold(true)) .resolve(&screen, 0) succeeds");
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
    let screen = finish(cells, 4, 1).expect("finish succeeds");
    let spans = Locator::style(StyleQuery::new().fg(Color::Indexed(3)).underline(true))
        .resolve(&screen, 0)
        .expect("Locator::style(StyleQuery::new().fg(Color::Indexed(3)).underline(true)) .resolve(&scree... succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "b");
}
