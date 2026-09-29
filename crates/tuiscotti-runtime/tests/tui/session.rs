//! Session lifecycle: spawn, observe, input, exit, waits (split from `tui.rs`; shared helpers live in the root).

use super::{cancel, contains, deadline};
use tuiscotti_runtime::tui::{Key, KeyMods, Tui, WaitError, parse_chord, process_exists};

#[test]
fn spawn_and_observe() {
    let mut s = Tui::new(["/bin/cat"])
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let obs = s.observe_now().expect("observe_now succeeds");
    assert_eq!(obs.screen.cols(), 40);
    assert_eq!(obs.screen.rows(), 10);
    assert!(obs.screen.cursor().visible);
    // Modes always known; title unknown until the app sets one.
    assert!(obs.state.modes.is_known());
    assert!(obs.state.palette.is_known());
    assert!(obs.state.bells.is_known());
    s.close().expect("close succeeds");
}

#[test]
fn typed_input_echoes() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("hello-tui\n").expect("send_text succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "hello-tui"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "hello-tui"));
    s.close().expect("close succeeds");
}

#[test]
fn chord_press_sends_key() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("hi").expect("send_text succeeds");
    s.press("Enter").expect("press succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "hi"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "hi"));
    s.close().expect("close succeeds");
}

#[test]
fn chord_parser_shapes() {
    let (k, m) = parse_chord("Ctrl+P").expect("parse_chord succeeds");
    assert_eq!(k, Key::Char('P'));
    assert_eq!(m, KeyMods::CTRL);
    let (k, m) = parse_chord("alt+Enter").expect("parse_chord succeeds");
    assert_eq!(k, Key::Enter);
    assert_eq!(m, KeyMods::ALT);
    let (k, m) = parse_chord("Shift+F5").expect("parse_chord succeeds");
    assert_eq!(k, Key::F(5));
    assert_eq!(m, KeyMods::SHIFT);
    let (k, m) = parse_chord("a").expect("parse_chord succeeds");
    assert_eq!(k, Key::Char('a'));
    assert_eq!(m, KeyMods::NONE);
    assert!(parse_chord("Ctrl+").is_err());
    assert!(parse_chord("Nope+X").is_err());
    assert!(parse_chord("F13").is_err());
}

#[test]
fn resize_changes_grid() {
    let mut s = Tui::new(["/bin/cat"])
        .size(80, 24)
        .spawn()
        .expect("spawn succeeds");
    s.resize(40, 10).expect("resize succeeds");
    let snap = s.snapshot().expect("snapshot succeeds");
    assert_eq!((snap.cols(), snap.rows()), (40, 10));
    assert!(s.resize(1, 10).is_err());
    assert!(s.resize(40, 0).is_err());
    s.close().expect("close succeeds");
}

#[test]
fn predicate_timeout_yields_evidence() {
    let mut s = Tui::new(["/bin/cat"])
        .size(50, 8)
        .spawn()
        .expect("spawn succeeds");
    let err = s
        .wait_predicate(|_| false, deadline(1), &cancel())
        .expect_err("wait_predicate must fail");
    match err {
        WaitError::Timeout { evidence, .. } => {
            assert_eq!((evidence.screen.cols(), evidence.screen.rows()), (50, 8));
        }
        other => panic!("expected timeout, got {other}"),
    }
    s.close().expect("close succeeds");
}

#[test]
fn exit_codes_observed() {
    let s = Tui::new(["/bin/sh", "-c", "exit 3"])
        .spawn()
        .expect("spawn succeeds");
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    assert!(!w.status.success());
    w.code(3).expect("code succeeds");
    // `w` moved; re-check via a fresh failing assertion on another session.
    let s = Tui::new(["/bin/sh", "-c", "exit 3"])
        .spawn()
        .expect("spawn succeeds");
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    assert!(w.success().is_err());
}

#[test]
fn printf_app_output_and_success() {
    let s = Tui::new(["/bin/sh", "-c", "printf 'out-42\\n'"])
        .spawn()
        .expect("spawn succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "out-42"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "out-42"));
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    w.success().expect("success succeeds");
}

#[test]
fn close_reaps_child_no_leak() {
    let mut s = Tui::new(["/bin/sleep", "30"])
        .spawn()
        .expect("spawn succeeds");
    let pid = s.pid().expect("child pid");
    assert!(process_exists(pid));
    s.close().expect("close succeeds");
    assert!(!process_exists(pid), "child {pid} leaked after close");
}

#[test]
fn drop_reaps_child_no_leak() {
    let pid = {
        let s = Tui::new(["/bin/sleep", "30"])
            .spawn()
            .expect("spawn succeeds");
        s.pid().expect("child pid")
    };
    assert!(!process_exists(pid), "child {pid} leaked after drop");
}

#[test]
fn finish_eofs_cat_to_success() {
    let s = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("bye\n").expect("send_text succeeds");
    let _ = s.wait_predicate(|o| contains(&o.screen, "bye"), deadline(5), &cancel());
    let status = s.finish(deadline(5)).expect("finish succeeds");
    assert!(status.success());
}

#[test]
fn two_sessions_independent() {
    let mut a = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    let mut b = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    a.send_text("alpha-1\n").expect("send_text succeeds");
    b.send_text("beta-2\n").expect("send_text succeeds");
    let oa = a
        .wait_predicate(|o| contains(&o.screen, "alpha-1"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    let ob = b
        .wait_predicate(|o| contains(&o.screen, "beta-2"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&oa.screen, "alpha-1"));
    assert!(!contains(&oa.screen, "beta-2"));
    assert!(contains(&ob.screen, "beta-2"));
    assert!(!contains(&ob.screen, "alpha-1"));
    a.close().expect("close succeeds");
    b.close().expect("close succeeds");
}
