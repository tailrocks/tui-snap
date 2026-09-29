use super::*;
use tuiscotti::locate::{LocateError, Locator, TextMode};

#[test]
fn text_substring_reports_viewport_coords_origin_and_revision() {
    let screen = screen_with(&["hello world", "second line"], 12).expect("screen_with succeeds");
    let spans = Locator::text("world")
        .resolve(&screen, 7)
        .expect("Locator::text(\"world\").resolve(&screen, 7) succeeds");
    assert_eq!(spans.len(), 1);
    let s = &spans[0];
    assert_eq!((s.x, s.y, s.end_x, s.end_y), (6, 0, 11, 0));
    assert_eq!(s.width_cols, 5);
    assert_eq!(s.origin, (0, 0));
    assert_eq!(s.revision, 7);
    assert_eq!(s.text, "world");
    assert!(!s.scrollback);
    assert_eq!(s.click_point(), Some((6, 0)));
}

#[test]
fn text_exact_requires_whole_row() {
    let screen = screen_with(&["hello", "hello world"], 12).expect("screen_with succeeds");
    let spans = Locator::text("hello")
        .mode(TextMode::Exact)
        .resolve(&screen, 0)
        .expect("Locator::text(\"hello\") .mode(TextMode::Exact) .resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (0, 0));
    assert_eq!(spans[0].text, "hello");
}

#[test]
fn text_case_insensitive_folds_ascii() {
    let screen = screen_with(&["Hello World"], 12).expect("screen_with succeeds");
    let spans = Locator::text("hello")
        .mode(TextMode::CaseInsensitive)
        .resolve(&screen, 0)
        .expect("Locator::text(\"hello\") .mode(TextMode::CaseInsensitive) .resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "Hello");
    // Sensitive default does not match.
    assert!(
        Locator::text("hello")
            .resolve(&screen, 0)
            .expect("Locator::text(\"hello\") .resolve(&screen, 0) succeeds")
            .is_empty()
    );
}

#[test]
fn text_case_insensitive_non_ascii_is_unsupported() {
    let mut cells = blank(6, 1);
    put_wide(&mut cells, 6, 0, 0, "好");
    let screen = finish(cells, 6, 1).expect("finish succeeds");
    let err = Locator::text("x")
        .mode(TextMode::CaseInsensitive)
        .resolve(&screen, 0)
        .expect_err("Locator::text(\"x\") .mode(TextMode::CaseInsensitive) .resolve(&screen, 0) is an error");
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
}

#[test]
fn text_normalized_collapses_whitespace_runs() {
    let screen = screen_with(&["a   b\t\tc"], 10).expect("screen_with succeeds");
    let spans = Locator::text("a b c")
        .mode(TextMode::Normalized)
        .resolve(&screen, 0)
        .expect(
            "Locator::text(\"a b c\") .mode(TextMode::Normalized) .resolve(&screen, 0) succeeds",
        );
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].end_x), (0, 8));
    assert_eq!(spans[0].text, "a   b\t\tc");
}

#[test]
fn text_empty_or_multiline_pattern_is_usage_error() {
    let screen = screen_with(&["hi"], 4).expect("screen_with succeeds");
    assert!(matches!(
        Locator::text("").resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    assert!(matches!(
        Locator::text("a\nb").resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
}
