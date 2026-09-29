//! README grouped store + fallback chain (split from `readme_lock.rs`; shared helpers live in the root).

use super::home_frame;
use tuiscotti::{Profile, VENDORED_FACES};

// ---------------------------------------------------------------------------
// Grouped fences: check_with + report_with + accept_all + root overrides.
// ---------------------------------------------------------------------------

#[test]
fn readme_grouped_store() {
    fn check_page(
        store: &tuiscotti::grouped::GroupedStore,
        profile: &tuiscotti::Profile,
        frame: &tuiscotti::Frame,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = profile.renderer(&tuiscotti::VENDORED_FACES)?;
        let outcome = store.check_with(&mut renderer, "pages/overview", frame, 1.0)?;
        outcome.ensure_matched()?;
        store.report_with(&mut renderer, 1.0, "my suite")?;
        Ok(())
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = tuiscotti::grouped::GroupedStore::new(&tmp.path().join("snapshots"));
    // Documented defaults live outside the approved tree.
    assert_eq!(store.actual_root(), tmp.path().join("snapshots.actual"));
    assert_eq!(store.diff_root(), tmp.path().join("snapshots.diff"));
    assert_eq!(
        store.report_path(),
        tmp.path().join("snapshots.actual").join("report.html")
    );
    // Overrides resolve.
    let custom = tuiscotti::grouped::GroupedStore::new(&tmp.path().join("s2"))
        .with_actual_root(&tmp.path().join("a"))
        .with_diff_root(&tmp.path().join("d"))
        .with_report_path(&tmp.path().join("r.html"));
    assert_eq!(custom.actual_root(), tmp.path().join("a"));
    assert_eq!(custom.diff_root(), tmp.path().join("d"));
    assert_eq!(custom.report_path(), tmp.path().join("r.html"));

    let profile = Profile::default_profile();
    let frame = home_frame();
    let first = store
        .check("pages/overview", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(first.status(), tuiscotti::snapshot::Status::MissingApproval);

    // Bless recursively from Rust (the documented accept_all fence).
    let accepted = store.accept_all().unwrap();
    assert_eq!(accepted, vec!["pages/overview".to_string()]);
    // Approved tree holds exactly the four artifacts.
    for ext in ["ansi", "txt", "png", "html"] {
        assert!(
            tmp.path()
                .join(format!("snapshots/pages/overview.{ext}"))
                .is_file(),
            "missing approved .{ext}"
        );
    }
    assert!(
        !tmp.path()
            .join("snapshots/pages/overview.frame.json")
            .exists(),
        "approved tree must not hold .frame.json"
    );
    check_page(&store, &profile, &frame).unwrap();
    assert!(store.report_path().is_file());
}

// ---------------------------------------------------------------------------
// Font-fallback fence: FallbackFace + Renderer::with_fallbacks.
// ---------------------------------------------------------------------------

#[test]
fn readme_fallback_chain() {
    fn custom_chain(
        profile: &tuiscotti::Profile,
    ) -> Result<tuiscotti::Renderer, Box<dyn std::error::Error>> {
        let chain = [tuiscotti::FallbackFace {
            bytes: tuiscotti::VENDORED_SYMBOLS2_FONT,
            sha256: tuiscotti::VENDORED_SYMBOLS2_FONT_SHA256,
            desc: "my extra symbols",
        }];
        Ok(tuiscotti::render::Renderer::with_fallbacks(
            profile,
            &tuiscotti::VENDORED_FACES,
            &chain,
        )?)
    }

    let profile = Profile::default_profile();
    let mut r = custom_chain(&profile).unwrap();
    let rendered = r.render(&home_frame()).unwrap();
    assert!(!rendered.png.is_empty());
    // A wrong pin refuses to render.
    let bad = tuiscotti::render::Renderer::with_fallbacks(
        &profile,
        &VENDORED_FACES,
        &[tuiscotti::FallbackFace {
            bytes: tuiscotti::VENDORED_SYMBOLS2_FONT,
            sha256: &"0".repeat(64),
            desc: "bad pin",
        }],
    );
    assert!(bad.is_err());
}
