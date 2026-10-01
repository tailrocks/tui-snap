use super::*;
use tuiscotti::locate::{LocateError, Locator, Span, StyleQuery};

#[test]
fn region_resolves_one_span_per_row() {
    let screen = screen_with(&["abcdef", "ghijkl"], 6).expect("screen_with succeeds");
    let spans = Locator::region(1, 0, 3, 2)
        .resolve(&screen, 0)
        .expect("Locator::region(1, 0, 3, 2).resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].text, "bcd");
    assert_eq!((spans[0].x, spans[0].y), (1, 0));
    assert_eq!(spans[1].text, "hij");
    assert_eq!((spans[1].x, spans[1].y), (1, 1));
}

#[test]
fn region_out_of_bounds_and_zero_size_are_usage_errors() {
    let screen = screen_with(&["abcd"], 4).expect("screen_with succeeds");
    assert!(matches!(
        Locator::region(2, 0, 3, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    assert!(matches!(
        Locator::region(0, 0, 0, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
}

#[test]
fn region_never_splits_wide_grapheme() {
    let mut cells = blank(6, 1);
    put_wide(&mut cells, 6, 2, 0, "好"); // lead x=2, continuation x=3
    let screen = finish(cells, 6, 1).expect("finish succeeds");
    assert!(matches!(
        Locator::region(3, 0, 2, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    assert!(matches!(
        Locator::region(1, 0, 2, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    // Whole grapheme is fine.
    let ok = Locator::region(1, 0, 3, 1)
        .resolve(&screen, 0)
        .expect("Locator::region(1, 0, 3, 1).resolve(&screen, 0) succeeds");
    assert_eq!(ok[0].text, " 好");
}

// ---------------------------------------------------------------------------
// Q02: combinators
// ---------------------------------------------------------------------------
#[test]
fn within_before_after_scope_correctly() {
    let screen =
        screen_with(&["alpha beta gamma", "delta beta"], 16).expect("screen_with succeeds");
    let scope = Locator::region(0, 0, 16, 1);
    let inner = Locator::within(scope, Locator::text("beta"));
    let spans = inner
        .resolve(&screen, 0)
        .expect("inner.resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (6, 0));

    let before = Locator::before(Locator::text("beta"), Locator::text("gamma"));
    let spans = before
        .resolve(&screen, 0)
        .expect("before.resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].y, 0);

    let after = Locator::after(Locator::text("beta"), Locator::text("alpha"));
    let spans = after
        .resolve(&screen, 0)
        .expect("after.resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 2);

    let after_none = Locator::after(Locator::text("alpha"), Locator::text("gamma"));
    assert!(
        after_none
            .resolve(&screen, 0)
            .expect("after_none.resolve(&screen, 0) succeeds")
            .is_empty()
    );
}

#[test]
fn nth_first_last_select_in_order() {
    let screen = screen_with(&["x one x two x"], 13).expect("screen_with succeeds");
    let q = || Locator::text("x");
    assert_eq!(
        Locator::first(q())
            .resolve(&screen, 0)
            .expect("Locator::first(q()).resolve(&screen, 0) succeeds")[0]
            .x,
        0
    );
    assert_eq!(
        Locator::last(q())
            .resolve(&screen, 0)
            .expect("Locator::last(q()).resolve(&screen, 0) succeeds")[0]
            .x,
        12
    );
    assert_eq!(
        Locator::nth(q(), 1)
            .resolve(&screen, 0)
            .expect("Locator::nth(q(), 1).resolve(&screen, 0) succeeds")[0]
            .x,
        6
    );
    assert!(
        Locator::nth(q(), 3)
            .resolve(&screen, 0)
            .expect("Locator::nth(q(), 3).resolve(&screen, 0) succeeds")
            .is_empty()
    );
}

#[test]
fn and_or_filter_combine_sets() {
    let mut cells = blank(9, 1);
    put(&mut cells, 9, 0, 0, "foo bar  ");
    for cell in cells.iter_mut().take(3) {
        cell.mods.bold = true;
    }
    let screen = finish(cells, 9, 1).expect("finish succeeds");

    let both = Locator::and(
        Locator::text("o"),
        Locator::style(StyleQuery::new().bold(true)),
    );
    let spans = both
        .resolve(&screen, 0)
        .expect("both.resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 2); // both "o"s of bold "foo"

    let either = Locator::or(Locator::text("foo"), Locator::text("bar"));
    let spans = either
        .resolve(&screen, 0)
        .expect("either.resolve(&screen, 0) succeeds");
    assert_eq!(spans.len(), 2);

    // or deduplicates identical spans.
    let dup = Locator::or(Locator::text("foo"), Locator::text("foo"));
    assert_eq!(
        dup.resolve(&screen, 0)
            .expect("dup.resolve(&screen, 0) succeeds")
            .len(),
        1
    );

    let filtered = Locator::filter(either, long_span);
    assert_eq!(
        filtered
            .resolve(&screen, 0)
            .expect("filtered.resolve(&screen, 0) succeeds")
            .len(),
        0
    );
    let filtered = Locator::filter(Locator::text("foo"), long_span);
    assert_eq!(
        filtered
            .resolve(&screen, 0)
            .expect("filtered.resolve(&screen, 0) succeeds")
            .len(),
        0
    );
    let kept = Locator::filter(Locator::text("bar"), |_s: &Span| true);
    assert_eq!(
        kept.resolve(&screen, 0)
            .expect("kept.resolve(&screen, 0) succeeds")
            .len(),
        1
    );
}

// ---------------------------------------------------------------------------
// Q03: strict uniqueness + scrollback non-clickability
// ---------------------------------------------------------------------------
#[test]
fn resolve_unique_ok_not_found_and_ambiguous() {
    let screen = screen_with(&["solo", "pair pair"], 9).expect("screen_with succeeds");
    let one = Locator::text("solo")
        .resolve_unique(&screen, 3)
        .expect("Locator::text(\"solo\").resolve_unique(&screen, 3) succeeds");
    assert_eq!((one.x, one.y), (0, 0));

    assert!(matches!(
        Locator::text("missing").resolve_unique(&screen, 3),
        Err(LocateError::NotFound { .. })
    ));

    let err = Locator::text("pair")
        .resolve_unique(&screen, 3)
        .expect_err("Locator::text(\"pair\") .resolve_unique(&screen, 3) is an error");
    match err {
        LocateError::Ambiguous { matches } => {
            assert_eq!(matches.len(), 2);
            assert_eq!((matches[0].x, matches[0].y), (0, 1));
            assert_eq!((matches[1].x, matches[1].y), (5, 1));
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}
