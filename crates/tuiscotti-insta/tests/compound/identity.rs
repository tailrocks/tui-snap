//! F10: resolved identity, bindings, and evidence partitioning (pure units).

use std::fs;
use std::path::Path;

use super::helpers::{styled_screen, write_binary_snap, write_text_snap};
use tuiscotti_core::screen::canonical_string;
use tuiscotti_insta::assert::{
    AttemptIdentity, EvidenceId, Location, check_consistent, png_tag_generation, render_sample,
    resolve_snapshot_identity, resolve_snapshot_identity_in, sample_binding, sanitize_segment,
};

#[test]
fn snapshot_identity_is_caller_relative_and_suffixed() {
    let loc = Location {
        file: "tests/compound.rs",
        line: 1,
    };
    // Default: <manifest>/<caller-parent>/snapshots, absolute.
    let id = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
    assert_eq!(id.dir, Path::new("/pkg-a/tests/snapshots"));
    assert_eq!(id.canonical, "shot");
    assert_eq!(id.png_base, "shot-img");
    // Same name in another package or module file: a different identity.
    let other_pkg = resolve_snapshot_identity("/pkg-b", loc, "shot", None);
    assert_ne!(id.dir, other_pkg.dir);
    let other_mod = resolve_snapshot_identity(
        "/pkg-a",
        Location {
            file: "tests/sub/other.rs",
            line: 1,
        },
        "shot",
        None,
    );
    assert_ne!(id.dir, other_mod.dir);
    // Same directory, different file: Insta's native default shares the dir
    // (prepend is off), so same-name snapshots there are one snapshot.
    let same_dir = resolve_snapshot_identity(
        "/pkg-a",
        Location {
            file: "tests/other.rs",
            line: 1,
        },
        "shot",
        None,
    );
    assert_eq!(id.dir, same_dir.dir);
    // Absolute override wins verbatim; relative override joins the caller dir.
    let abs = resolve_snapshot_identity("/pkg-a", loc, "shot", Some(Path::new("/snaps")));
    assert_eq!(abs.dir, Path::new("/snaps"));
    let rel = resolve_snapshot_identity("/pkg-a", loc, "shot", Some(Path::new("custom")));
    assert_eq!(rel.dir, Path::new("/pkg-a/tests/custom"));
    // Explicit-dir form honors the same rules.
    let explicit = resolve_snapshot_identity_in("/pkg-a", loc, Path::new("/e"), "shot");
    assert_eq!(explicit.dir, Path::new("/e"));
    // Active suffix applies to BOTH stems, exactly like Insta.
    let mut settings = insta::Settings::new();
    settings.set_snapshot_suffix("v2");
    settings.bind(|| {
        let suffixed = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
        assert_eq!(suffixed.canonical, "shot@v2");
        assert_eq!(suffixed.png_base, "shot-img@v2");
    });
    // No suffix leaks out of the bind scope.
    let plain = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
    assert_eq!(plain.canonical, "shot");
}

#[test]
fn sample_binding_covers_canonical_render_and_payload() {
    let screen = styled_screen().expect("valid test screen");
    let sample = render_sample(&screen).expect("render sample");
    let base = sample_binding(
        &sample.canonical,
        "tuiscotti-default/rv1/straight-rgba",
        &sample.png,
    );
    assert!(base.starts_with("v2-"), "{base}");
    // Deterministic.
    assert_eq!(
        base,
        sample_binding(
            &sample.canonical,
            "tuiscotti-default/rv1/straight-rgba",
            &sample.png
        )
    );
    // Canonical change moves it.
    assert_ne!(
        base,
        sample_binding("other", "tuiscotti-default/rv1/straight-rgba", &sample.png)
    );
    // Profile/renderer-only change moves it (same canonical text).
    assert_ne!(
        base,
        sample_binding(&sample.canonical, "custom/rv1/straight-rgba", &sample.png)
    );
    assert_ne!(
        base,
        sample_binding(
            &sample.canonical,
            "tuiscotti-default/rv2/straight-rgba",
            &sample.png
        )
    );
    // Payload-only change moves it.
    let mut png = sample.png.clone();
    let last = png.len() - 1;
    png[last] ^= 0xFF;
    assert_ne!(
        base,
        sample_binding(
            &sample.canonical,
            "tuiscotti-default/rv1/straight-rgba",
            &png
        )
    );
}

#[test]
fn missing_or_partial_bindings_fail_the_strict_gate() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = canonical_string(&screen);
    let binding = sample_binding(&canonical, "prof/rv1/straight-rgba", b"png-bytes");
    let sample = render_sample(&screen).expect("render sample");
    let tagged = png_tag_generation(&sample.png, &binding);
    let untagged = sample.png.clone();

    // Nothing at all: missing.
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("empty dir must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Canonical only: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("canonical-only must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // PNG sidecar without a tag: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "m-img", &binding, &untagged).expect("write binary snap");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("untagged sidecar must fail");
    assert!(err.to_string().contains("tEXt"), "{err}");

    // PNG metadata without a binding: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    fs::write(
        tmp.path().join("m-img.snap"),
        "---\nsource: t\nexpression: png\n---\n",
    )
    .expect("write png meta");
    fs::write(tmp.path().join("m-img.snap.png"), &tagged).expect("write sidecar");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("unbound png meta must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Complete and agreeing: passes.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "m-img", &binding, &tagged).expect("write binary snap");
    check_consistent(tmp.path(), "m", "m-img").expect("complete compound passes");
}

#[test]
fn attempt_identity_parses_nextest_shape() {
    use std::collections::HashMap;
    let mut env = HashMap::new();
    env.insert("NEXTEST_RUN_ID".to_string(), "run-1".to_string());
    env.insert("NEXTEST_ATTEMPT".to_string(), "2".to_string());
    env.insert(
        "NEXTEST_ATTEMPT_ID".to_string(),
        "run-1:bin$test".to_string(),
    );
    env.insert("NEXTEST_STRESS_CURRENT".to_string(), "3".to_string());
    env.insert("TUISCOTTI_SHARD".to_string(), "0/4".to_string());
    let id = AttemptIdentity::from_map(&env);
    assert_eq!(id.run, "run-1");
    assert_eq!(id.attempt, 2);
    assert_eq!(id.attempt_uid.as_deref(), Some("run-1:bin$test"));
    assert_eq!(id.stress_iter, Some(3));
    assert_eq!(id.shard.as_deref(), Some("0/4"));
    assert_eq!(id.run_dir(), "run-run-1");
    assert_eq!(id.attempt_dir(), "attempt-2-stress3-shard-0_4");

    // Garbage attempt is lossy-zero; "none" stress is None.
    let mut env = HashMap::new();
    env.insert("NEXTEST_ATTEMPT".to_string(), "bogus".to_string());
    env.insert("NEXTEST_STRESS_CURRENT".to_string(), "none".to_string());
    let id = AttemptIdentity::from_map(&env);
    assert_eq!(id.attempt, 0);
    assert_eq!(id.stress_iter, None);

    // Absent env: process-unique local run, attempt 0.
    let id = AttemptIdentity::from_map(&HashMap::new());
    assert!(id.run.starts_with("local-"), "{}", id.run);
    assert_eq!(id.attempt, 0);
    assert_eq!(id.shard, None);
}

#[test]
fn evidence_identity_partitions_and_never_escapes() {
    use std::collections::HashMap;
    let attempt = AttemptIdentity::from_map(&HashMap::from([
        ("NEXTEST_RUN_ID".to_string(), "r".to_string()),
        ("NEXTEST_ATTEMPT".to_string(), "1".to_string()),
    ]));
    let root = Path::new("/root");
    let id = EvidenceId {
        package: "my-pkg".to_string(),
        test: "a::b".to_string(),
        scenario: "shot".to_string(),
        variant: Some("dark".to_string()),
        attempt,
    };
    assert_eq!(
        id.bundle_dir(root),
        Path::new("/root/my-pkg/a__b/shot@dark/run-r/attempt-1")
    );
    // Hostile segments are sanitized, never escaped.
    assert_eq!(sanitize_segment("../../etc"), "_.._etc");
    assert_eq!(sanitize_segment(""), "unknown");
    assert_eq!(sanitize_segment("..."), "unknown");
    let evil = EvidenceId {
        package: "../../p".to_string(),
        test: "t/t".to_string(),
        scenario: "shot".to_string(),
        variant: None,
        attempt: AttemptIdentity::from_map(&HashMap::new()),
    };
    let dir = evil.bundle_dir(root);
    assert!(dir.starts_with(root), "{}", dir.display());
    for comp in dir.strip_prefix(root).expect("strip root").components() {
        let s = comp.as_os_str().to_string_lossy();
        assert!(!s.contains('/') && !s.contains('\\'), "no separators: {s}");
        assert!(s != ".." && s != ".", "no dot segments: {s}");
    }
}
