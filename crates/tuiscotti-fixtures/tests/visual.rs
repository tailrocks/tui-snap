//! Visual gates: every fixture screen × theme × size, headless.
//!
//! First run writes actuals and fails missing-approval; inspect
//! `tests/visual/actual/*.png` + `report.html`, then accept explicitly:
//! `cargo run -q -- accept --store tests/visual --all`

use tuiscotti_fixtures::fixture_app::{render_model, Model, Screen};
use std::path::PathBuf;
use tuiscotti::snapshot::Store;
use tuiscotti::{Profile, Provenance, VENDORED_FACES};

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
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
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
                    tuiscotti::ratatui::draw_frame(cols, rows, prov(), |f| render_model(f, &model));
                let outcome = st.check_with(&mut renderer, &name, &frame, 1.0).unwrap();
                outcomes.push((name, outcome));
            }
        }
    }
    // Report always written (reviewable even on failure).
    let entries: Vec<_> = outcomes
        .iter()
        .map(|(_, o)| st.report_entry(o, &profile).unwrap())
        .collect();
    tuiscotti::snapshot::write_report(&st, "fixture visual gates", &entries).unwrap();
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
