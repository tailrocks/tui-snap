use super::*;
use tuiscotti::VENDORED_FACES;
use tuiscotti::grouped::GroupedStore;
use tuiscotti::snapshot::Status;

#[test]
fn accept_all_walks_nested_names_recursively() {
    let (_dir, st) = tmp_store("acceptall");
    let names = [
        "showcase/pages/overview_120x40_truecolor",
        "showcase/pages/detail_120x40_truecolor",
        "showcase/modals/confirm",
        "home",
    ];
    for (i, name) in names.iter().enumerate() {
        st.check(
            name,
            &frame_with(&format!("screen {i}")),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    }
    let mut listed = st.actual_names().unwrap();
    assert_eq!(listed.len(), 4, "{listed:?}");
    let accepted = st.accept_all().unwrap();
    assert_eq!(accepted, listed);
    assert_eq!(st.approved_names().unwrap(), listed);
    for name in &names {
        let outcome = st
            .check(
                name,
                &frame_with("placeholder"),
                &profile(),
                &VENDORED_FACES,
                1.0,
            )
            .unwrap();
        assert_eq!(outcome.status(), Status::CellsDiffer, "{name} was approved");
    }
    listed.sort();
    let mut sorted = names.to_vec();
    sorted.sort();
    assert_eq!(listed, sorted);
}

#[test]
fn report_reverifies_nested_actuals_outside_approved_tree() {
    let (_dir, st) = tmp_store("report");
    st.check(
        "suite/one",
        &frame_with("alpha"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    st.accept("suite/one").unwrap();
    st.check(
        "suite/nested/two",
        &frame_with("beta"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "grouped suite")
        .unwrap();
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.failed(), 1, "`suite/nested/two` has no approval");
    assert!(report.path.exists());
    // Default report path: under the actual scratch root, NEVER approved/.
    assert!(report.path.starts_with(st.actual_root()));
    assert!(!report.path.starts_with(st.approved_root()));
    let html = std::fs::read_to_string(&report.path).unwrap();
    assert!(html.contains("suite/one — matched"), "{html}");
    assert!(
        html.contains("suite/nested/two — missing-approval"),
        "{html}"
    );
    assert!(
        !html.contains("data:image/png;base64,"),
        "suite report must not embed PNGs"
    );
    // The approved tree is still exactly the four artifacts of `suite/one`.
    assert_eq!(
        tree_files(st.approved_root()),
        vec![
            "suite/one.ansi".to_string(),
            "suite/one.html".to_string(),
            "suite/one.png".to_string(),
            "suite/one.txt".to_string(),
        ]
    );
}

#[test]
fn report_path_is_configurable() {
    let (dir, st) = tmp_store("reportcfg");
    let custom = dir.path().join("target").join("grouped-report.html");
    let st = st.with_report_path(&custom);
    st.check("a/b", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "custom path")
        .unwrap();
    assert_eq!(report.path, custom);
    assert!(custom.exists());
}

#[test]
fn custom_actual_and_diff_roots_are_honored() {
    let dir = tempfile::tempdir().unwrap();
    let approved = dir.path().join("approved-tree");
    let st = GroupedStore::new(&approved)
        .with_actual_root(&dir.path().join("scratch/actual"))
        .with_diff_root(&dir.path().join("scratch/diff"));
    let name = "deep/nested/name";
    let outcome = st
        .check(name, &frame_with("roots"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(
        outcome
            .actual
            .ansi
            .starts_with(dir.path().join("scratch/actual"))
    );
    assert_eq!(outcome.status(), Status::MissingApproval);
    st.accept(name).unwrap();
    assert!(approved.join(format!("{name}.ansi")).exists());
    let outcome = st
        .check(
            name,
            &frame_with("changed"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    let diff = outcome.outcome.diff_png.unwrap();
    assert!(
        diff.starts_with(dir.path().join("scratch/diff")),
        "{}",
        diff.display()
    );
}

#[test]
fn default_roots_are_siblings_of_approved() {
    let dir = tempfile::tempdir().unwrap();
    let st = GroupedStore::new(&dir.path().join("snapshots"));
    assert_eq!(st.actual_root(), &dir.path().join("snapshots.actual"));
    assert_eq!(st.diff_root(), &dir.path().join("snapshots.diff"));
    assert_eq!(
        st.report_path(),
        dir.path().join("snapshots.actual").join("report.html")
    );
}

#[test]
fn accept_without_actuals_is_an_error() {
    let (_dir, st) = tmp_store("noactual");
    let err = st.accept("never/checked").unwrap_err().to_string();
    assert!(err.contains("nothing to accept"), "{err}");
}

#[test]
fn check_with_reuses_one_renderer_across_nested_checks() {
    let (_dir, st) = tmp_store("checkwith");
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    let frame = frame_with("cached grouped");
    let o1 = st.check_with(&mut renderer, "g/s", &frame, 1.0).unwrap();
    assert_eq!(o1.status(), Status::MissingApproval);
    st.accept("g/s").unwrap();
    let o2 = st.check_with(&mut renderer, "g/s", &frame, 1.0).unwrap();
    assert_eq!(o2.status(), Status::Matched);
    assert_eq!(o2.outcome.pixel_score, Some(1.0));
    o2.ensure_matched().unwrap();
}

#[test]
fn html_artifact_is_a_standalone_colored_render() {
    let name = "docs/preview";
    let (_dir, st) = tmp_store("htmlview");
    let outcome = st
        .check(
            name,
            &frame_with("standalone"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    let html = std::fs::read_to_string(&outcome.actual.html).unwrap();
    assert!(html.contains("<svg"), "{html}");
    assert!(html.contains("data:image/png;base64,"), "{html}");
    assert!(
        html.contains("<script type=\"application/json\">"),
        "{html}"
    );
    assert!(html.contains("<title>docs/preview</title>"), "{html}");
    let body = html.split("<body>").nth(1).unwrap();
    let img = body.find("<img ").expect("png img");
    assert!(
        !body[..img].contains("<details"),
        "PNG must be the primary visual, not nested under details: {html}"
    );
}
