use super::*;
use std::time::{Duration, Instant};
use tuiscotti::locate::{Action, LocateError, Locator, StyleQuery, TextMode};

#[test]
fn scrollback_matches_flagged_and_ordered_first() {
    let screen = screen_with(&["here target"], 12).expect("screen_with succeeds");
    let scrollback = vec!["old target".to_string(), "older".to_string()];
    let spans = Locator::text("target")
        .resolve_with_scrollback(&screen, 1, &scrollback)
        .expect(
            "Locator::text(\"target\") .resolve_with_scrollback(&screen, 1, &scrollback) succeeds",
        );
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
    let screen = screen_with(&["viewport"], 8).expect("screen_with succeeds");
    let scrollback = vec!["scrollback hit".to_string()];
    let span = Locator::text("hit")
        .resolve_unique_with_scrollback(&screen, 1, &scrollback)
        .expect("Locator::text(\"hit\") .resolve_unique_with_scrollback(&screen, 1, &scrollback) succeeds");
    assert!(span.scrollback);
    let err =
        tuiscotti::locate::PendingAction::from_span(Locator::text("hit"), span, 1).expect_err("tuiscotti::locate::PendingAction::from_span(Locator::text(\"hit\"), span, 1) is an error");
    assert!(matches!(err, LocateError::ViewportOnly { .. }), "{err:?}");
}

// ---------------------------------------------------------------------------
// Wrapped lines + wide cells + region origins (Q02/Q10)
// ---------------------------------------------------------------------------
#[test]
fn wrapped_rows_join_into_one_logical_span() {
    // Row 0 runs to the edge (full) so it wraps into row 1.
    let screen = screen_with(&["abcde", "fg"], 5).expect("screen_with succeeds");
    let spans = Locator::text("defg")
        .resolve(&screen, 0)
        .expect("Locator::text(\"defg\").resolve(&screen, 0) succeeds");
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
        .expect("Locator::text(\"defg\") .physical_rows() .resolve(&screen, 0) succeeds");
    assert!(spans.is_empty());

    // Non-full rows do not join even by default.
    let screen = screen_with(&["abc", "def"], 5).expect("screen_with succeeds");
    assert!(
        Locator::text("cdef")
            .resolve(&screen, 0)
            .expect("Locator::text(\"cdef\") .resolve(&screen, 0) succeeds")
            .is_empty()
    );
}

#[test]
fn regex_matches_across_wrapped_rows() {
    let screen = screen_with(&["ab12", "34cd"], 4).expect("screen_with succeeds");
    let spans = Locator::regex("[0-9]+")
        .expect("Locator::regex(\"[0-9]+\") succeeds")
        .resolve(&screen, 0)
        .expect("resolve succeeds");
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
    let screen = finish(cells, 7, 1).expect("finish succeeds");

    let b = Locator::text("b")
        .resolve_unique(&screen, 0)
        .expect("Locator::text(\"b\").resolve_unique(&screen, 0) succeeds");
    assert_eq!((b.x, b.end_x), (3, 4));

    let wide = Locator::text("好b")
        .resolve_unique(&screen, 0)
        .expect("Locator::text(\"好b\").resolve_unique(&screen, 0) succeeds");
    assert_eq!((wide.x, wide.end_x), (1, 4));
    assert_eq!(wide.width_cols, 3);
    assert_eq!(wide.text, "好b");

    let run = Locator::style(StyleQuery::new())
        .resolve(&screen, 0)
        .expect("Locator::style(StyleQuery::new()) .resolve(&screen, 0) succeeds");
    assert_eq!(run.len(), 1); // one run: continuation contributes no break
    assert_eq!(run[0].text, "a好b   ");
    assert_eq!(run[0].width_cols, 7);
}

#[test]
fn region_screens_keep_origin_with_local_coords() {
    let mut cells = blank(10, 4);
    put(&mut cells, 10, 3, 2, "hi");
    let screen = finish(cells, 10, 4).expect("finish succeeds");
    let region = screen
        .region(2, 1, 5, 2, tuiscotti::screen::RegionPolicy::Clip)
        .expect("screen .region(2, 1, 5, 2, tuiscotti::screen::RegionPolicy::Clip) succeeds");
    assert_eq!(region.screen().origin(), (2, 1));
    let spans = Locator::text("hi")
        .resolve(region.screen(), 9)
        .expect("Locator::text(\"hi\").resolve(region.screen(), 9) succeeds");
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].y), (1, 1)); // region-local
    assert_eq!(spans[0].origin, (2, 1)); // crop origin retained
}

// ---------------------------------------------------------------------------
// Q04: retryable assertions with ONE deadline
// ---------------------------------------------------------------------------
#[test]
fn expect_visible_succeeds_and_times_out() {
    let ready = obs(screen_with(&["go"], 4).expect("screen_with succeeds"), 1);
    let mut immediate = script(vec![ready.clone()]);
    let spans = Locator::text("go")
        .expect_visible(&mut immediate, Duration::from_secs(1))
        .expect("Locator::text(\"go\") .expect_visible(&mut immediate, Duration::from_secs(1)) succeeds");
    assert_eq!(spans.len(), 1);

    let blank_screen = obs(screen_with(&["--"], 4).expect("screen_with succeeds"), 1);
    let mut never = script(vec![blank_screen]);
    let err = Locator::text("go")
        .expect_visible(&mut never, Duration::from_millis(60))
        .expect_err("Locator::text(\"go\") .expect_visible(&mut never, Duration::from_millis(60)) is an error");
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn expect_text_waits_for_exact_unique_text() {
    let o1 = obs(screen_with(&["--"], 4).expect("screen_with succeeds"), 1);
    let o2 = obs(screen_with(&["go"], 4).expect("screen_with succeeds"), 2);
    let mut appear = script(vec![o1, o2]);
    let span = Locator::text("go")
        .expect_text(&mut appear, "go", Duration::from_secs(2))
        .expect("Locator::text(\"go\") .expect_text(&mut appear, \"go\", Duration::from_secs(2)) succeeds");
    assert_eq!(span.text, "go");

    // Unique match but wrong text keeps retrying, then times out.
    let wrong = obs(screen_with(&["gone"], 4).expect("screen_with succeeds"), 1);
    let mut stuck = script(vec![wrong]);
    let err = Locator::text("go")
        .expect_text(&mut stuck, "gone", Duration::from_millis(60))
        .expect_err("Locator::text(\"go\") .expect_text(&mut stuck, \"gone\", Duration::from_millis(60)) is an error");
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn expect_count_waits_for_exact_count() {
    let o1 = obs(screen_with(&["a"], 4).expect("screen_with succeeds"), 1);
    let o2 = obs(screen_with(&["a a"], 4).expect("screen_with succeeds"), 2);
    let mut appear = script(vec![o1, o2]);
    let spans = Locator::text("a")
        .expect_count(&mut appear, 2, Duration::from_secs(2))
        .expect(
            "Locator::text(\"a\") .expect_count(&mut appear, 2, Duration::from_secs(2)) succeeds",
        );
    assert_eq!(spans.len(), 2);

    let one = obs(screen_with(&["a"], 4).expect("screen_with succeeds"), 1);
    let mut stuck = script(vec![one]);
    let err = Locator::text("a")
        .expect_count(&mut stuck, 2, Duration::from_millis(60))
        .expect_err("Locator::text(\"a\") .expect_count(&mut stuck, 2, Duration::from_millis(60)) is an error");
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn usage_and_unsupported_fail_immediately_without_waiting() {
    let screen = obs(screen_with(&["hi"], 4).expect("screen_with succeeds"), 1);
    // Generous deadline: immediate errors must return far earlier.
    let timeout = Duration::from_secs(10);

    let mut o = script(vec![screen.clone()]);
    let start = Instant::now();
    let err = Locator::text("")
        .expect_visible(&mut o, timeout)
        .expect_err("Locator::text(\"\") .expect_visible(&mut o, timeout) is an error");
    assert!(matches!(err, LocateError::Usage(_)), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));

    let mut cells = blank(4, 1);
    put_wide(&mut cells, 4, 0, 0, "好");
    let wide = obs(finish(cells, 4, 1).expect("finish succeeds"), 1);
    let mut o = script(vec![wide]);
    let start = Instant::now();
    let err = Locator::text("x")
        .mode(TextMode::CaseInsensitive)
        .expect_count(&mut o, 1, timeout)
        .expect_err("Locator::text(\"x\") .mode(TextMode::CaseInsensitive) .expect_count(&mut o, 1, timeout) is an error");
    assert!(matches!(err, LocateError::Unsupported(_)), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

// ---------------------------------------------------------------------------
// Q08: temporal assertions
// ---------------------------------------------------------------------------
#[test]
fn present_now_and_not_present_now_are_single_shot() {
    let o = obs(screen_with(&["here"], 4).expect("screen_with succeeds"), 1);
    assert!(
        Locator::text("here")
            .present_now(&o)
            .expect("Locator::text(\"here\").present_now(&o) succeeds")
    );
    assert!(
        !Locator::text("gone")
            .present_now(&o)
            .expect("Locator::text(\"gone\").present_now(&o) succeeds")
    );
    assert!(
        Locator::text("gone")
            .not_present_now(&o)
            .expect("Locator::text(\"gone\").not_present_now(&o) succeeds")
    );
    assert!(
        !Locator::text("here")
            .not_present_now(&o)
            .expect("Locator::text(\"here\").not_present_now(&o) succeeds")
    );
}

#[test]
fn eventually_absent_passes_and_times_out() {
    let o1 = obs(screen_with(&["busy"], 4).expect("screen_with succeeds"), 1);
    let o2 = obs(screen_with(&["idle"], 4).expect("screen_with succeeds"), 2);
    let mut clears = script(vec![o1, o2]);
    Locator::text("busy")
        .eventually_absent(&mut clears, Duration::from_secs(2))
        .expect("Locator::text(\"busy\") .eventually_absent(&mut clears, Duration::from_secs(2)) succeeds");

    let stuck = obs(screen_with(&["busy"], 4).expect("screen_with succeeds"), 1);
    let mut never = script(vec![stuck]);
    let err = Locator::text("busy")
        .eventually_absent(&mut never, Duration::from_millis(60))
        .expect_err("Locator::text(\"busy\") .eventually_absent(&mut never, Duration::from_millis(60)) is an error");
    assert!(matches!(err, LocateError::Timeout { .. }), "{err:?}");
}

#[test]
fn remains_absent_watches_full_window_and_catches_appearance() {
    let clear = obs(screen_with(&["idle"], 4).expect("screen_with succeeds"), 1);
    let mut stays = script(vec![clear]);
    Locator::text("busy")
        .remains_absent(&mut stays, Duration::from_millis(50))
        .expect("Locator::text(\"busy\") .remains_absent(&mut stays, Duration::from_millis(50)) succeeds");

    let o1 = obs(screen_with(&["idle"], 4).expect("screen_with succeeds"), 1);
    let o2 = obs(screen_with(&["busy"], 4).expect("screen_with succeeds"), 2);
    let mut appears = script(vec![o1, o2]);
    let err = Locator::text("busy")
        .remains_absent(&mut appears, Duration::from_secs(2))
        .expect_err("Locator::text(\"busy\") .remains_absent(&mut appears, Duration::from_secs(2)) is an error");
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
    let o = obs(
        screen_with(&["press me"], 8).expect("screen_with succeeds"),
        5,
    );
    let pending = Locator::text("press")
        .prepare_action(&o)
        .expect("Locator::text(\"press\").prepare_action(&o) succeeds");
    assert_eq!(pending.revision(), 5);

    let mut got = Vec::new();
    pending
        .click(&o, &mut |a| got.push(a))
        .expect("pending.click(&o, &mut |a| got.push(a)) succeeds");
    assert_eq!(got, vec![Action::Click { x: 0, y: 0 }]);

    let mut got = Vec::new();
    pending
        .submit(&o, &mut |a| got.push(a))
        .expect("pending.submit(&o, &mut |a| got.push(a)) succeeds");
    assert_eq!(got, vec![Action::Submit { x: 0, y: 0 }]);
}

#[test]
fn stale_revision_never_delivers() {
    let o1 = obs(screen_with(&["press"], 6).expect("screen_with succeeds"), 5);
    let pending = Locator::text("press")
        .prepare_action(&o1)
        .expect("Locator::text(\"press\").prepare_action(&o1) succeeds");
    let o2 = obs(screen_with(&["press"], 6).expect("screen_with succeeds"), 6);
    let mut calls = 0;
    let err = pending
        .click(&o2, &mut |_| calls += 1)
        .expect_err("pending.click(&o2, &mut |_| calls += 1) is an error");
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
    let o1 = obs(screen_with(&["--"], 4).expect("screen_with succeeds"), 1);
    let o2 = obs(screen_with(&["go"], 4).expect("screen_with succeeds"), 2);
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
        .expect("Locator::text(\"go\") .prepare_action_retry(&mut observe, Duration::from_secs(2)) succeeds");
    assert!(
        polls.get() >= 2,
        "expected retries, got {} polls",
        polls.get()
    );
    assert_eq!(pending.revision(), 2);

    // Only the explicit click delivers, exactly once.
    let current = observe();
    let mut calls = 0;
    pending
        .click(&current, &mut |_| calls += 1)
        .expect("pending.click(&current, &mut |_| calls += 1) succeeds");
    assert_eq!(calls, 1);
}

#[test]
fn ambiguous_target_blocks_action_preparation() {
    let o = obs(
        screen_with(&["dup dup"], 8).expect("screen_with succeeds"),
        1,
    );
    let err = Locator::text("dup")
        .prepare_action(&o)
        .expect_err("Locator::text(\"dup\").prepare_action(&o) is an error");
    assert!(matches!(err, LocateError::Ambiguous { .. }), "{err:?}");

    // Explicit disambiguation unblocks it.
    let pending = Locator::nth(Locator::text("dup"), 1)
        .prepare_action(&o)
        .expect("Locator::nth(Locator::text(\"dup\"), 1) .prepare_action(&o) succeeds");
    let mut got = Vec::new();
    pending
        .click(&o, &mut |a| got.push(a))
        .expect("pending.click(&o, &mut |a| got.push(a)) succeeds");
    assert_eq!(got, vec![Action::Click { x: 4, y: 0 }]);
}
