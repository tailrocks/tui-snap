//! Visual gates: every fixture screen × theme × size, headless.
//!
//! First run writes actuals and fails missing-approval; inspect
//! `tests/visual/actual/*.png` + `report.html`, then accept explicitly:
//! `cargo run -q -- accept --store tests/visual --all`

use std::path::PathBuf;
use tuiscotti::snapshot::{Status, Store};
use tuiscotti::{Profile, Provenance, VENDORED_FACES};
use tuiscotti_fixtures::views::matrix::{Model, Screen, render};

fn store() -> Store {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/visual");
    Store::new(&root)
}

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        profile: "tuisnap-default".into(),
        source: "fixture".into(),
        argv: vec![],
        created_unix: 0,
    }
}

#[test]
fn fixture_visual_gates() {
    let st = store();
    let profile = Profile::default_profile();
    // One renderer for the whole matrix: faces parsed once, glyphs cached.
    let mut renderer = profile.renderer(&VENDORED_FACES).expect("renderer");
    let screens = [Screen::Home, Screen::Table, Screen::Dialog, Screen::Glyphs];
    let themes = [(true, "dark"), (false, "light")];
    let sizes = [(80u16, 24u16), (120, 40), (160, 50)];
    let mut outcomes = Vec::new();
    for screen in screens {
        for (dark, theme) in themes {
            for (cols, rows) in sizes {
                let name = format!("{screen:?}-{theme}-{cols}x{rows}").to_lowercase();
                let model = Model::new(screen, dark);
                let frame =
                    tuiscotti::ratatui::draw_frame(cols, rows, prov(), |f| render(f, &model));
                let outcome = st
                    .check_with(&mut renderer, &name, &frame, 1.0)
                    .expect("snapshot check");
                outcomes.push((name, outcome));
            }
        }
    }
    // Report always written (reviewable even on failure).
    let entries: Vec<_> = outcomes
        .iter()
        .map(|(_, o)| st.report_entry(o, &profile).expect("report entry"))
        .collect();
    tuiscotti::snapshot::write_report(&st, "fixture visual gates", &entries).expect("report");
    let mut failures = Vec::new();
    for (name, o) in &outcomes {
        if let Err(e) = o.ensure_matched() {
            failures.push(format!("{name}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} snapshot(s) require review:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Corrupt approvals fail closed in a scratch store (committed bytes
/// untouched): garbage or truncated approved frames report
/// `CorruptApproval` (never a match, never a panic), and undecodable
/// approved PNG bytes are a hard check error.
#[test]
fn corrupt_approvals_are_rejected_not_matched() {
    let scratch = std::env::temp_dir().join(format!(
        "tuiscotti-g5-visual-corrupt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos()
    ));
    let st = Store::new(&scratch);
    let profile = Profile::default_profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).expect("renderer");
    let model = Model::new(Screen::Home, true);
    let frame = tuiscotti::ratatui::draw_frame(40, 10, prov(), |f| render(f, &model));
    let approved = scratch.join("approved");
    std::fs::create_dir_all(&approved).expect("approved dir");

    // Garbage and truncated approved frames: explicit CorruptApproval.
    let json = frame.to_json();
    let mut cut = json.len() / 2;
    while !json.is_char_boundary(cut) {
        cut -= 1;
    }
    for (name, bytes) in [
        ("garbage", "{not json".to_string()),
        ("truncated", json[..cut].to_string()),
    ] {
        std::fs::write(approved.join(format!("{name}.frame.json")), bytes)
            .expect("stage corrupt approval");
        let outcome = st
            .check_with(&mut renderer, name, &frame, 1.0)
            .expect("snapshot check");
        assert_eq!(outcome.status, Status::CorruptApproval, "{name}");
        let err = outcome.ensure_matched().expect_err("must not match");
        assert!(err.to_string().contains("corrupt-approval"), "{err}");
    }

    // Matching frame but undecodable PNG: the check itself errors.
    std::fs::write(
        approved.join("badpng.frame.json"),
        frame.to_json().as_bytes(),
    )
    .expect("stage frame");
    std::fs::write(approved.join("badpng.png"), b"definitely not a png").expect("stage png");
    let err = st
        .check_with(&mut renderer, "badpng", &frame, 1.0)
        .expect_err("undecodable approved PNG must fail the check");
    assert!(err.to_string().contains("cannot decode"), "{err}");
}
