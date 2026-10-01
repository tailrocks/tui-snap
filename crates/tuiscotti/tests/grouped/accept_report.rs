use super::*;
use tuiscotti::VENDORED_FACES;
use tuiscotti::grouped::GroupedStore;
use tuiscotti::snapshot::Status;

#[test]
fn accept_all_walks_nested_names_recursively() {
    let (_dir, st) = tmp_store("acceptall").expect("tmp_store succeeds");
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
        .expect("st.check( name, &frame_with(&format!(\"screen {i}\")), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    }
    let mut listed = st.actual_names().expect("st.actual_names() succeeds");
    assert_eq!(listed.len(), 4, "{listed:?}");
    let accepted = st.accept_all().expect("st.accept_all() succeeds");
    assert_eq!(accepted, listed);
    assert_eq!(
        st.approved_names().expect("st.approved_names() succeeds"),
        listed
    );
    for name in &names {
        let outcome = st
            .check(
                name,
                &frame_with("placeholder"),
                &profile(),
                &VENDORED_FACES,
                1.0,
            )
            .expect("st .check( name, &frame_with(\"placeholder\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
        assert_eq!(outcome.status(), Status::CellsDiffer, "{name} was approved");
    }
    listed.sort();
    let mut sorted = names.to_vec();
    sorted.sort_unstable();
    assert_eq!(listed, sorted);
}

#[test]
fn report_reverifies_nested_actuals_outside_approved_tree() {
    let (_dir, st) = tmp_store("report").expect("tmp_store succeeds");
    st.check(
        "suite/one",
        &frame_with("alpha"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .expect("st.check( \"suite/one\", &frame_with(\"alpha\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    st.accept("suite/one")
        .expect("st.accept(\"suite/one\") succeeds");
    st.check(
        "suite/nested/two",
        &frame_with("beta"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .expect("st.check( \"suite/nested/two\", &frame_with(\"beta\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "grouped suite")
        .expect("st .report(&profile(), &VENDORED_FACES, 1.0, \"grouped suite\") succeeds");
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.failed(), 1, "`suite/nested/two` has no approval");
    assert!(report.path.exists());
    // Default report path: under the actual scratch root, NEVER approved/.
    assert!(report.path.starts_with(st.actual_root()));
    assert!(!report.path.starts_with(st.approved_root()));
    let html = std::fs::read_to_string(&report.path)
        .expect("std::fs::read_to_string(&report.path) succeeds");
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
        tree_files(st.approved_root()).expect("tree_files succeeds"),
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
    let (dir, st) = tmp_store("reportcfg").expect("tmp_store succeeds");
    let custom = dir.path().join("target").join("grouped-report.html");
    let st = st.with_report_path(&custom);
    st.check("a/b", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect("st.check(\"a/b\", &frame_with(\"x\"), &profile(), &VENDORED_FACES, 1.0) succeeds");
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "custom path")
        .expect("st .report(&profile(), &VENDORED_FACES, 1.0, \"custom path\") succeeds");
    assert_eq!(report.path, custom);
    assert!(custom.exists());
}

#[test]
fn custom_actual_and_diff_roots_are_honored() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let approved = dir.path().join("approved-tree");
    let st = GroupedStore::new(&approved)
        .with_actual_root(&dir.path().join("scratch/actual"))
        .with_diff_root(&dir.path().join("scratch/diff"));
    let name = "deep/nested/name";
    let outcome = st
        .check(name, &frame_with("roots"), &profile(), &VENDORED_FACES, 1.0)
        .expect(
            "st .check(name, &frame_with(\"roots\"), &profile(), &VENDORED_FACES, 1.0) succeeds",
        );
    assert!(
        outcome
            .actual
            .ansi
            .starts_with(dir.path().join("scratch/actual"))
    );
    assert_eq!(outcome.status(), Status::MissingApproval);
    st.accept(name).expect("st.accept(name) succeeds");
    assert!(approved.join(format!("{name}.ansi")).exists());
    let outcome = st
        .check(
            name,
            &frame_with("changed"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("st .check( name, &frame_with(\"changed\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    let diff = outcome
        .outcome
        .diff_png
        .expect("outcome.outcome.diff_png is some");
    assert!(
        diff.starts_with(dir.path().join("scratch/diff")),
        "{}",
        diff.display()
    );
}

#[test]
fn default_roots_are_siblings_of_approved() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
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
    let (_dir, st) = tmp_store("noactual").expect("tmp_store succeeds");
    let err = st
        .accept("never/checked")
        .expect_err("st.accept(\"never/checked\") is an error")
        .to_string();
    assert!(err.contains("nothing to accept"), "{err}");
}

#[test]
fn check_with_reuses_one_renderer_across_nested_checks() {
    let (_dir, st) = tmp_store("checkwith").expect("tmp_store succeeds");
    let profile = profile();
    let mut renderer = profile
        .renderer(&VENDORED_FACES)
        .expect("profile.renderer(&VENDORED_FACES) succeeds");
    let frame = frame_with("cached grouped");
    let o1 = st
        .check_with(&mut renderer, "g/s", &frame, 1.0)
        .expect("st.check_with(&mut renderer, \"g/s\", &frame, 1.0) succeeds");
    assert_eq!(o1.status(), Status::MissingApproval);
    st.accept("g/s").expect("st.accept(\"g/s\") succeeds");
    let o2 = st
        .check_with(&mut renderer, "g/s", &frame, 1.0)
        .expect("st.check_with(&mut renderer, \"g/s\", &frame, 1.0) succeeds");
    assert_eq!(o2.status(), Status::Matched);
    assert_eq!(o2.outcome.pixel_score, Some(1.0));
    o2.ensure_matched().expect("o2.ensure_matched() succeeds");
}

#[test]
fn html_artifact_is_a_standalone_colored_render() {
    let name = "docs/preview";
    let (_dir, st) = tmp_store("htmlview").expect("tmp_store succeeds");
    let outcome = st
        .check(
            name,
            &frame_with("standalone"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("st .check( name, &frame_with(\"standalone\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    let html = std::fs::read_to_string(&outcome.actual.html)
        .expect("std::fs::read_to_string(&outcome.actual.html) succeeds");
    assert!(html.contains("<svg"), "{html}");
    assert!(html.contains("data:image/png;base64,"), "{html}");
    assert!(
        html.contains("<script type=\"application/json\">"),
        "{html}"
    );
    assert!(html.contains("<title>docs/preview</title>"), "{html}");
    let body = html
        .split("<body>")
        .nth(1)
        .expect("html.split(\"<body>\").nth(1) is some");
    let img = body.find("<img ").expect("png img");
    assert!(
        !body[..img].contains("<details"),
        "PNG must be the primary visual, not nested under details: {html}"
    );
}
