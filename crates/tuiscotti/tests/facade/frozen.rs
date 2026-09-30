use super::*;
use std::fs;
use tuiscotti::assert::{
    FrozenError, assert_frozen_screenshot, assert_frozen_snapshot, check_frozen_screenshot,
    check_frozen_snapshot, frozen_accept, png_tag_generation, render_sample,
};
use tuiscotti::screen::canonical_string;

#[test]
fn frozen_missing_fails() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    let err = check_frozen_snapshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_snapshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    let err = check_frozen_screenshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"shot\", &screen) is an error");
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
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    write_frozen(root.path(), "shot", &screen, false).expect("write_frozen succeeds");
    fs::write(root.path().join("shot.png"), b"not a png")
        .expect("fs::write(root.path().join(\"shot.png\"), b\"not a png\") succeeds");
    let before = list_files(root.path()).expect("list_files succeeds");
    let err = check_frozen_screenshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()).expect("list_files succeeds"),
        before,
        "frozen failure must not write"
    );
    // Non-UTF-8 canonical is corrupt too.
    fs::write(root.path().join("shot.canonical.txt"), b"\xff\xfe invalid").expect(
        "fs::write(root.path().join(\"shot.canonical.txt\"), b\"\\xff\\xfe invalid\") succeeds",
    );
    let err = check_frozen_snapshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_snapshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()).expect("list_files succeeds"),
        before,
        "frozen failure must not write"
    );
}

#[test]
fn frozen_accept_always_errors() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    write_frozen(
        root.path(),
        "shot",
        &fixture().expect("fixture succeeds"),
        true,
    )
    .expect("write_frozen succeeds");
    // Even with valid state present...
    let err = frozen_accept(root.path(), "shot")
        .expect_err("frozen_accept(root.path(), \"shot\") is an error");
    assert!(matches!(err, FrozenError::AcceptRejected { .. }), "{err}");
    // ...and on an empty root.
    let empty = frozen_dir().expect("frozen_dir succeeds");
    assert!(matches!(
        frozen_accept(empty.path(), "x"),
        Err(FrozenError::AcceptRejected { .. })
    ));
}

#[test]
fn frozen_passes_when_matching() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    // Untagged legacy PNG: pixel verdict stands.
    write_frozen(root.path(), "plain", &screen, false).expect("write_frozen succeeds");
    check_frozen_snapshot(root.path(), "plain", &screen)
        .expect("check_frozen_snapshot(root.path(), \"plain\", &screen) succeeds");
    check_frozen_screenshot(root.path(), "plain", &screen)
        .expect("check_frozen_screenshot(root.path(), \"plain\", &screen) succeeds");
    // Tagged PNG with the right generation: full gate green.
    write_frozen(root.path(), "tagged", &screen, true).expect("write_frozen succeeds");
    check_frozen_screenshot(root.path(), "tagged", &screen)
        .expect("check_frozen_screenshot(root.path(), \"tagged\", &screen) succeeds");
    // Tagged PNG with the WRONG generation: pixels match, binding fails.
    let sample = render_sample(&screen).expect("render_sample(&screen) succeeds");
    fs::write(
        root.path().join("mistag.canonical.txt"),
        canonical_string(&screen),
    )
    .expect(
        "fs::write( root.path().join(\"mistag.canonical.txt\"), canonical_string(&screen), ) succeeds",
    );
    fs::write(
        root.path().join("mistag.png"),
        png_tag_generation(&sample.png, "gen-b"),
    )
    .expect("fs::write( root.path().join(\"mistag.png\"), png_tag_generation(&sample.png, \"gen-b\"), ) succeeds");
    let err = check_frozen_screenshot(root.path(), "mistag", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"mistag\", &screen) is an error");
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    assert!(err.to_string().contains("generation"), "{err}");
}
