use super::*;
use std::fs;
use tuiscotti::assert::{
    FrozenError, assert_frozen_screenshot, assert_frozen_snapshot, check_frozen_screenshot,
    check_frozen_snapshot, frozen_accept, png_tag_generation, render_sample,
};
use tuiscotti::insta_proto::insta_string;

#[test]
fn frozen_missing_fails() {
    let root = frozen_dir();
    let screen = fixture();
    let err = check_frozen_snapshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    let err = check_frozen_screenshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_frozen_snapshot(root.path(), "shot", &screen);
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_frozen_screenshot(root.path(), "shot", &screen);
        }))
        .is_err()
    );
}

#[test]
fn frozen_corrupt_fails_and_never_heals() {
    let root = frozen_dir();
    let screen = fixture();
    write_frozen(root.path(), "shot", &screen, false);
    fs::write(root.path().join("shot.png"), b"not a png").unwrap();
    let before = list_files(root.path());
    let err = check_frozen_screenshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()),
        before,
        "frozen failure must not write"
    );
    // Non-UTF-8 canonical is corrupt too.
    fs::write(root.path().join("shot.canonical.txt"), b"\xff\xfe invalid").unwrap();
    let err = check_frozen_snapshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()),
        before,
        "frozen failure must not write"
    );
}

#[test]
fn frozen_accept_always_errors() {
    let root = frozen_dir();
    write_frozen(root.path(), "shot", &fixture(), true);
    // Even with valid state present...
    let err = frozen_accept(root.path(), "shot").unwrap_err();
    assert!(matches!(err, FrozenError::AcceptRejected { .. }), "{err}");
    // ...and on an empty root.
    let empty = frozen_dir();
    assert!(matches!(
        frozen_accept(empty.path(), "x"),
        Err(FrozenError::AcceptRejected { .. })
    ));
}

#[test]
fn frozen_passes_when_matching() {
    let root = frozen_dir();
    let screen = fixture();
    // Untagged legacy PNG: pixel verdict stands.
    write_frozen(root.path(), "plain", &screen, false);
    check_frozen_snapshot(root.path(), "plain", &screen).unwrap();
    check_frozen_screenshot(root.path(), "plain", &screen).unwrap();
    // Tagged PNG with the right generation: full gate green.
    write_frozen(root.path(), "tagged", &screen, true);
    check_frozen_screenshot(root.path(), "tagged", &screen).unwrap();
    // Tagged PNG with the WRONG generation: pixels match, binding fails.
    let sample = render_sample(&screen).unwrap();
    fs::write(
        root.path().join("mistag.canonical.txt"),
        insta_string(&screen),
    )
    .unwrap();
    fs::write(
        root.path().join("mistag.png"),
        png_tag_generation(&sample.png, "gen-b"),
    )
    .unwrap();
    let err = check_frozen_screenshot(root.path(), "mistag", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    assert!(err.to_string().contains("generation"), "{err}");
}
