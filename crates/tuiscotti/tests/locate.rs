//! M4 locator core tests (backlog Q01-Q05, Q08, Q10).

use std::time::{Duration, Instant};
use tuiscotti::frame::{Cell, Color, Cursor};
use tuiscotti::locate::{Action, LocateError, Locator, Span, StyleQuery, TextMode};
use tuiscotti::screen::{CaptureProvenance, CaptureReason, Observation, Screen, TermState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn blank(cols: u16, rows: u16) -> Vec<Cell> {
    (0..rows)
        .flat_map(|y| (0..cols).map(move |x| Cell::blank(x, y)))
        .collect()
}

fn put(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str) {
    for (i, ch) in text.chars().enumerate() {
        let idx = y as usize * cols as usize + x as usize + i;
        cells[idx].symbol = ch.to_string();
    }
}

fn put_wide(cells: &mut [Cell], cols: u16, x: u16, y: u16, sym: &str) {
    let idx = y as usize * cols as usize + x as usize;
    cells[idx].symbol = sym.to_string();
    cells[idx].width = 2;
    cells[idx + 1].symbol = String::new();
    cells[idx + 1].width = 0;
    cells[idx + 1].continuation = true;
}

fn finish(cells: Vec<Cell>, cols: u16, rows: u16) -> Screen {
    Screen::validate(cols, rows, 0, 0, cells, Cursor::default()).unwrap()
}

fn obs(screen: Screen, revision: u64) -> Observation {
    Observation::new(
        screen,
        revision,
        CaptureReason::Manual,
        TermState::default(),
        CaptureProvenance::new(0, None, None, 0),
    )
}

fn screen_with(text_rows: &[&str], cols: u16) -> Screen {
    let rows = text_rows.len() as u16;
    let mut cells = blank(cols, rows);
    for (y, row) in text_rows.iter().enumerate() {
        put(&mut cells, cols, 0, y as u16, row);
    }
    finish(cells, cols, rows)
}

/// Scripted observer: yields each observation in turn, repeating the last.
fn script(mut script: Vec<Observation>) -> impl FnMut() -> Observation {
    move || {
        if script.len() > 1 {
            script.remove(0)
        } else {
            script[0].clone()
        }
    }
}

fn long_span(s: &Span) -> bool {
    s.text.chars().count() > 3
}

// ---------------------------------------------------------------------------
// Q01: text match modes
// ---------------------------------------------------------------------------

#[test]
fn text_substring_reports_viewport_coords_origin_and_revision() {
    let screen = screen_with(&["hello world", "second line"], 12);
    let spans = Locator::text("world").resolve(&screen, 7).unwrap();
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
    let screen = screen_with(&["hello", "hello world"], 12);
    let spans = Locator::text("hello")
        .mode(TextMode::Exact)
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (0, 0));
    assert_eq!(spans[0].text, "hello");
}

#[test]
fn text_case_insensitive_folds_ascii() {
    let screen = screen_with(&["Hello World"], 12);
    let spans = Locator::text("hello")
        .mode(TextMode::CaseInsensitive)
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "Hello");
    // Sensitive default does not match.
    assert!(Locator::text("hello")
        .resolve(&screen, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn text_case_insensitive_non_ascii_is_unsupported() {
    let mut cells = blank(6, 1);
    put_wide(&mut cells, 6, 0, 0, "好");
    let screen = finish(cells, 6, 1);
    let err = Locator::text("x")
        .mode(TextMode::CaseInsensitive)
        .resolve(&screen, 0)
        .unwrap_err();
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
}

#[test]
fn text_normalized_collapses_whitespace_runs() {
    let screen = screen_with(&["a   b\t\tc"], 10);
    let spans = Locator::text("a b c")
        .mode(TextMode::Normalized)
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].end_x), (0, 8));
    assert_eq!(spans[0].text, "a   b\t\tc");
}

#[test]
fn text_empty_or_multiline_pattern_is_usage_error() {
    let screen = screen_with(&["hi"], 4);
    assert!(matches!(
        Locator::text("").resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    assert!(matches!(
        Locator::text("a\nb").resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
}

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

#[test]
fn region_resolves_one_span_per_row() {
    let screen = screen_with(&["abcdef", "ghijkl"], 6);
    let spans = Locator::region(1, 0, 3, 2).resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].text, "bcd");
    assert_eq!((spans[0].x, spans[0].y), (1, 0));
    assert_eq!(spans[1].text, "hij");
    assert_eq!((spans[1].x, spans[1].y), (1, 1));
}

#[test]
fn region_out_of_bounds_and_zero_size_are_usage_errors() {
    let screen = screen_with(&["abcd"], 4);
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
    let screen = finish(cells, 6, 1);
    assert!(matches!(
        Locator::region(3, 0, 2, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    assert!(matches!(
        Locator::region(1, 0, 2, 1).resolve(&screen, 0),
        Err(LocateError::Usage(_))
    ));
    // Whole grapheme is fine.
    let ok = Locator::region(1, 0, 3, 1).resolve(&screen, 0).unwrap();
    assert_eq!(ok[0].text, " 好");
}

// ---------------------------------------------------------------------------
// Q02: combinators
// ---------------------------------------------------------------------------

#[test]
fn within_before_after_scope_correctly() {
    let screen = screen_with(&["alpha beta gamma", "delta beta"], 16);
    let scope = Locator::region(0, 0, 16, 1);
    let inner = Locator::within(scope, Locator::text("beta"));
    let spans = inner.resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (6, 0));

    let before = Locator::before(Locator::text("beta"), Locator::text("gamma"));
    let spans = before.resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].y, 0);

    let after = Locator::after(Locator::text("beta"), Locator::text("alpha"));
    let spans = after.resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 2);

    let after_none = Locator::after(Locator::text("alpha"), Locator::text("gamma"));
    assert!(after_none.resolve(&screen, 0).unwrap().is_empty());
}

#[test]
fn nth_first_last_select_in_order() {
    let screen = screen_with(&["x one x two x"], 13);
    let q = || Locator::text("x");
    assert_eq!(Locator::first(q()).resolve(&screen, 0).unwrap()[0].x, 0);
    assert_eq!(Locator::last(q()).resolve(&screen, 0).unwrap()[0].x, 12);
    assert_eq!(Locator::nth(q(), 1).resolve(&screen, 0).unwrap()[0].x, 6);
    assert!(Locator::nth(q(), 3).resolve(&screen, 0).unwrap().is_empty());
}

#[test]
fn and_or_filter_combine_sets() {
    let mut cells = blank(9, 1);
    put(&mut cells, 9, 0, 0, "foo bar  ");
    for cell in cells.iter_mut().take(3) {
        cell.mods.bold = true;
    }
    let screen = finish(cells, 9, 1);

    let both = Locator::and(
        Locator::text("o"),
        Locator::style(StyleQuery::new().bold(true)),
    );
    let spans = both.resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 2); // both "o"s of bold "foo"

    let either = Locator::or(Locator::text("foo"), Locator::text("bar"));
    let spans = either.resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 2);

    // or deduplicates identical spans.
    let dup = Locator::or(Locator::text("foo"), Locator::text("foo"));
    assert_eq!(dup.resolve(&screen, 0).unwrap().len(), 1);

    let filtered = Locator::filter(either, long_span);
    assert_eq!(filtered.resolve(&screen, 0).unwrap().len(), 0);
    let filtered = Locator::filter(Locator::text("foo"), long_span);
    assert_eq!(filtered.resolve(&screen, 0).unwrap().len(), 0);
    let kept = Locator::filter(Locator::text("bar"), |_s: &Span| true);
    assert_eq!(kept.resolve(&screen, 0).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Q03: strict uniqueness + scrollback non-clickability
// ---------------------------------------------------------------------------

#[test]
fn resolve_unique_ok_not_found_and_ambiguous() {
    let screen = screen_with(&["solo", "pair pair"], 9);
    let one = Locator::text("solo").resolve_unique(&screen, 3).unwrap();
    assert_eq!((one.x, one.y), (0, 0));

    assert!(matches!(
        Locator::text("missing").resolve_unique(&screen, 3),
        Err(LocateError::NotFound { .. })
    ));

    let err = Locator::text("pair")
        .resolve_unique(&screen, 3)
        .unwrap_err();
    match err {
        LocateError::Ambiguous { matches } => {
            assert_eq!(matches.len(), 2);
            assert_eq!((matches[0].x, matches[0].y), (0, 1));
            assert_eq!((matches[1].x, matches[1].y), (5, 1));
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn scrollback_matches_flagged_and_ordered_first() {
    let screen = screen_with(&["here target"], 12);
    let scrollback = vec!["old target".to_string(), "older".to_string()];
    let spans = Locator::text("target")
        .resolve_with_scrollback(&screen, 1, &scrollback)
        .unwrap();
    assert_eq!(spans.len(), 2);
    // Scrollback (oldest first) sorts before viewport.
    assert!(spans[0].scrollback);
    assert_eq!(spans[0].scrollback_index, Some(0));
    assert_eq!(spans[0].text, "target");
    assert_eq!(spans[0].click_point(), None);
    assert!(!spans[1].scrollback);
    assert_eq!((spans[1].x, spans[1].y), (5, 0));
}

#[test]
fn scrollback_target_is_never_clickable() {
    let screen = screen_with(&["viewport"], 8);
    let scrollback = vec!["scrollback hit".to_string()];
    let span = Locator::text("hit")
        .resolve_unique_with_scrollback(&screen, 1, &scrollback)
        .unwrap();
    assert!(span.scrollback);
    let err = tuiscotti::locate::PendingAction::from_span(Locator::text("hit"), span, 1).unwrap_err();
    assert!(matches!(err, LocateError::ViewportOnly { .. }), "{err:?}");
}

// ---------------------------------------------------------------------------
// Wrapped lines + wide cells + region origins (Q02/Q10)
// ---------------------------------------------------------------------------

#[test]
fn wrapped_rows_join_into_one_logical_span() {
    // Row 0 runs to the edge (full) so it wraps into row 1.
    let screen = screen_with(&["abcde", "fg"], 5);
    let spans = Locator::text("defg").resolve(&screen, 0).unwrap();
    assert_eq!(spans.len(), 1);
    let s = &spans[0];
    assert_eq!((s.x, s.y), (3, 0));
    assert_eq!((s.end_x, s.end_y), (2, 1));
    assert_eq!(s.text, "defg");
    assert_eq!(s.width_cols, 4);

    // Opt out: physical rows never match across the boundary.
    let spans = Locator::text("defg")
        .physical_rows()
        .resolve(&screen, 0)
        .unwrap();
    assert!(spans.is_empty());

    // Non-full rows do not join even by default.
    let screen = screen_with(&["abc", "def"], 5);
    assert!(Locator::text("cdef")
        .resolve(&screen, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn regex_matches_across_wrapped_rows() {
    let screen = screen_with(&["ab12", "34cd"], 4);
    let spans = Locator::regex("[0-9]+")
        .unwrap()
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "1234");
    assert_eq!((spans[0].y, spans[0].end_y), (0, 1));
}

#[test]
fn wide_cells_skipped_and_counted_as_two_columns() {
    let mut cells = blank(7, 1);
    put(&mut cells, 7, 0, 0, "a");
    put_wide(&mut cells, 7, 1, 0, "好"); // cols 1-2
    put(&mut cells, 7, 3, 0, "b");
    let screen = finish(cells, 7, 1);

    let b = Locator::text("b").resolve_unique(&screen, 0).unwrap();
    assert_eq!((b.x, b.end_x), (3, 4));

    let wide = Locator::text("好b").resolve_unique(&screen, 0).unwrap();
    assert_eq!((wide.x, wide.end_x), (1, 4));
    assert_eq!(wide.width_cols, 3);
    assert_eq!(wide.text, "好b");

    let run = Locator::style(StyleQuery::new())
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(run.len(), 1); // one run: continuation contributes no break
    assert_eq!(run[0].text, "a好b   ");
    assert_eq!(run[0].width_cols, 7);
}

#[test]
fn region_screens_keep_origin_with_local_coords() {
    let mut cells = blank(10, 4);
    put(&mut cells, 10, 3, 2, "hi");
    let screen = finish(cells, 10, 4);
    let region = screen
        .region(2, 1, 5, 2, tuiscotti::screen::RegionPolicy::Clip)
        .unwrap();
    assert_eq!(region.screen().origin(), (2, 1));
    let spans = Locator::text("hi").resolve(region.screen(), 9).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (1, 1)); // region-local
    assert_eq!(spans[0].origin, (2, 1)); // crop origin retained
}

// ---------------------------------------------------------------------------
// Q04: retryable assertions with ONE deadline
// ---------------------------------------------------------------------------

#[test]
fn expect_visible_succeeds_and_times_out() {
    let ready = obs(screen_with(&["go"], 4), 1);
    let mut immediate = script(vec![ready.clone()]);
    let spans = Locator::text("go")
        .expect_visible(&mut immediate, Duration::from_secs(1))
        .unwrap();
    assert_eq!(spans.len(), 1);

    let blank_screen = obs(screen_with(&["--"], 4), 1);
    let mut never = script(vec![blank_screen]);
    let err = Locator::text("go")
        .expect_visible(&mut never, Duration::from_millis(60))
        .unwrap_err();
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn expect_text_waits_for_exact_unique_text() {
    let o1 = obs(screen_with(&["--"], 4), 1);
    let o2 = obs(screen_with(&["go"], 4), 2);
    let mut appear = script(vec![o1, o2]);
    let span = Locator::text("go")
        .expect_text(&mut appear, "go", Duration::from_secs(2))
        .unwrap();
    assert_eq!(span.text, "go");

    // Unique match but wrong text keeps retrying, then times out.
    let wrong = obs(screen_with(&["gone"], 4), 1);
    let mut stuck = script(vec![wrong]);
    let err = Locator::text("go")
        .expect_text(&mut stuck, "gone", Duration::from_millis(60))
        .unwrap_err();
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn expect_count_waits_for_exact_count() {
    let o1 = obs(screen_with(&["a"], 4), 1);
    let o2 = obs(screen_with(&["a a"], 4), 2);
    let mut appear = script(vec![o1, o2]);
    let spans = Locator::text("a")
        .expect_count(&mut appear, 2, Duration::from_secs(2))
        .unwrap();
    assert_eq!(spans.len(), 2);

    let one = obs(screen_with(&["a"], 4), 1);
    let mut stuck = script(vec![one]);
    let err = Locator::text("a")
        .expect_count(&mut stuck, 2, Duration::from_millis(60))
        .unwrap_err();
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn usage_and_unsupported_fail_immediately_without_waiting() {
    let screen = obs(screen_with(&["hi"], 4), 1);
    // Generous deadline: immediate errors must return far earlier.
    let timeout = Duration::from_secs(10);

    let mut o = script(vec![screen.clone()]);
    let start = Instant::now();
    let err = Locator::text("")
        .expect_visible(&mut o, timeout)
        .unwrap_err();
    assert!(matches!(err, LocateError::Usage(_)), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));

    let mut cells = blank(4, 1);
    put_wide(&mut cells, 4, 0, 0, "好");
    let wide = obs(finish(cells, 4, 1), 1);
    let mut o = script(vec![wide]);
    let start = Instant::now();
    let err = Locator::text("x")
        .mode(TextMode::CaseInsensitive)
        .expect_count(&mut o, 1, timeout)
        .unwrap_err();
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

// ---------------------------------------------------------------------------
// Q08: temporal assertions
// ---------------------------------------------------------------------------

#[test]
fn present_now_and_not_present_now_are_single_shot() {
    let o = obs(screen_with(&["here"], 4), 1);
    assert!(Locator::text("here").present_now(&o).unwrap());
    assert!(!Locator::text("gone").present_now(&o).unwrap());
    assert!(Locator::text("gone").not_present_now(&o).unwrap());
    assert!(!Locator::text("here").not_present_now(&o).unwrap());
}

#[test]
fn eventually_absent_passes_and_times_out() {
    let o1 = obs(screen_with(&["busy"], 4), 1);
    let o2 = obs(screen_with(&["idle"], 4), 2);
    let mut clears = script(vec![o1, o2]);
    Locator::text("busy")
        .eventually_absent(&mut clears, Duration::from_secs(2))
        .unwrap();

    let stuck = obs(screen_with(&["busy"], 4), 1);
    let mut never = script(vec![stuck]);
    let err = Locator::text("busy")
        .eventually_absent(&mut never, Duration::from_millis(60))
        .unwrap_err();
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn remains_absent_watches_full_window_and_catches_appearance() {
    let clear = obs(screen_with(&["idle"], 4), 1);
    let mut stays = script(vec![clear]);
    Locator::text("busy")
        .remains_absent(&mut stays, Duration::from_millis(50))
        .unwrap();

    let o1 = obs(screen_with(&["idle"], 4), 1);
    let o2 = obs(screen_with(&["busy"], 4), 2);
    let mut appears = script(vec![o1, o2]);
    let err = Locator::text("busy")
        .remains_absent(&mut appears, Duration::from_secs(2))
        .unwrap_err();
    match err {
        LocateError::UnexpectedlyPresent { matches } => {
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].text, "busy");
        }
        other => panic!("expected UnexpectedlyPresent, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Q05: actions execute once, never stale
// ---------------------------------------------------------------------------

#[test]
fn click_and_submit_deliver_exactly_once_with_coords() {
    let o = obs(screen_with(&["press me"], 8), 5);
    let pending = Locator::text("press").prepare_action(&o).unwrap();
    assert_eq!(pending.revision(), 5);

    let mut got = Vec::new();
    pending.click(&o, &mut |a| got.push(a)).unwrap();
    assert_eq!(got, vec![Action::Click { x: 0, y: 0 }]);

    let mut got = Vec::new();
    pending.submit(&o, &mut |a| got.push(a)).unwrap();
    assert_eq!(got, vec![Action::Submit { x: 0, y: 0 }]);
}

#[test]
fn stale_revision_never_delivers() {
    let o1 = obs(screen_with(&["press"], 6), 5);
    let pending = Locator::text("press").prepare_action(&o1).unwrap();
    let o2 = obs(screen_with(&["press"], 6), 6);
    let mut calls = 0;
    let err = pending.click(&o2, &mut |_| calls += 1).unwrap_err();
    assert!(
        matches!(
            err,
            LocateError::StaleTarget {
                expected: 5,
                current: 6
            }
        ),
        "{err:?}"
    );
    assert_eq!(calls, 0);
}

#[test]
fn readiness_retries_never_touch_the_sink() {
    let o1 = obs(screen_with(&["--"], 4), 1);
    let o2 = obs(screen_with(&["go"], 4), 2);
    let polls = std::cell::Cell::new(0usize);
    let mut script_obs = vec![o1, o2];
    let mut observe = || {
        polls.set(polls.get() + 1);
        if script_obs.len() > 1 {
            script_obs.remove(0)
        } else {
            script_obs[0].clone()
        }
    };
    // Readiness takes no sink: several polls happen before the target exists.
    let pending = Locator::text("go")
        .prepare_action_retry(&mut observe, Duration::from_secs(2))
        .unwrap();
    assert!(
        polls.get() >= 2,
        "expected retries, got {} polls",
        polls.get()
    );
    assert_eq!(pending.revision(), 2);

    // Only the explicit click delivers, exactly once.
    let current = observe();
    let mut calls = 0;
    pending.click(&current, &mut |_| calls += 1).unwrap();
    assert_eq!(calls, 1);
}

#[test]
fn ambiguous_target_blocks_action_preparation() {
    let o = obs(screen_with(&["dup dup"], 8), 1);
    let err = Locator::text("dup").prepare_action(&o).unwrap_err();
    assert!(matches!(err, LocateError::Ambiguous { .. }), "{err:?}");

    // Explicit disambiguation unblocks it.
    let pending = Locator::nth(Locator::text("dup"), 1)
        .prepare_action(&o)
        .unwrap();
    let mut got = Vec::new();
    pending.click(&o, &mut |a| got.push(a)).unwrap();
    assert_eq!(got, vec![Action::Click { x: 4, y: 0 }]);
}
