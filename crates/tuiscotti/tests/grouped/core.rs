use super::*;
use ratatui::widgets::Paragraph;
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

#[test]
fn missing_accept_match_round_trip_nested_name() {
    let name = "showcase/pages/overview_120x40_truecolor";
    let (_dir, st) = tmp_store("snapshots");
    let frame = frame_with("hello grouped");

    let o1 = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(o1.status(), Status::MissingApproval);
    assert!(o1.ensure_matched().is_err());
    assert!(o1.outcome.note.contains(".ansi"), "{}", o1.outcome.note);
    // All four actual artifacts + debug sidecars written BEFORE the gate ran,
    // in nested directories.
    assert!(o1.actual.ansi.exists());
    assert!(o1.actual.txt.exists());
    assert!(o1.actual.png.exists());
    assert!(o1.actual.html.exists());
    assert!(o1.actual.frame_json.exists());
    assert!(o1.actual.png.with_extension("png.fidelity.json").exists());
    // Nothing approved yet (fail-closed).
    assert!(!o1.approved.ansi.exists());

    st.accept(name).unwrap();
    // The approved tree holds EXACTLY the four artifacts: no .frame.json,
    // no .fidelity.json, nothing else.
    assert_eq!(
        tree_files(st.approved_root()),
        vec![
            format!("{name}.ansi"),
            format!("{name}.html"),
            format!("{name}.png"),
            format!("{name}.txt"),
        ]
    );

    let o2 = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(o2.status(), Status::Matched);
    assert_eq!(o2.outcome.pixel_score, Some(1.0));
    assert_eq!(o2.ansi_match, Some(true));
    assert_eq!(o2.txt_match, Some(true));
    assert_eq!(o2.html_match, Some(true));
    o2.ensure_matched().unwrap();
}

#[test]
fn ansi_and_txt_and_html_are_byte_deterministic() {
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    // Same screen, different capture timestamps: provenance time must not
    // leak into any artifact (the HTML embed normalizes it).
    let mut p2 = prov();
    p2.created_unix = 1_700_000_000;
    let a = tuiscotti::ratatui::widget_frame(Paragraph::new("deterministic ╔═╗"), 30, 6, prov());
    let b = tuiscotti::ratatui::widget_frame(Paragraph::new("deterministic ╔═╗"), 30, 6, p2);
    assert_eq!(
        tuiscotti::render::ansi_dump(&a),
        tuiscotti::render::ansi_dump(&b)
    );
    assert_eq!(a.text(), b.text());
    let ha = renderer.render_html(&a, "t").unwrap();
    let hb = renderer.render_html(&b, "t").unwrap();
    assert_eq!(ha, hb, "html must not embed the capture timestamp");
    // Re-rendered twice through render_artifacts: identical bytes.
    let r1 = renderer.render_artifacts(&a, "t").unwrap();
    let r2 = renderer.render_artifacts(&a, "t").unwrap();
    assert_eq!(r1.ansi, r2.ansi);
    assert_eq!(r1.txt, r2.txt);
    assert_eq!(r1.html, r2.html);
    assert_eq!(r1.png, r2.png);
    // The embedded frame JSON still re-imports losslessly (timestamp zeroed).
    let start = ha.find("<script type=\"application/json\">").unwrap()
        + "<script type=\"application/json\">".len();
    let end = ha[start..].find("</script>").unwrap() + start;
    let back = tuiscotti::Frame::from_json(&ha[start..end]).unwrap();
    assert_eq!(back.digest(), a.digest(), "cells/cursor survive the embed");
    assert_eq!(back.provenance.created_unix, 0);
}

#[test]
fn check_seals_manifest_and_verdict_that_report_reuses_verbatim() {
    let name = "sealed/one";
    let (_dir, st) = tmp_store("sealed");
    let frame = frame_with("sealed verdict");
    st.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    let checked = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(checked.status(), Status::Matched);

    // C08-grouped: candidate seal written after all candidate writes.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(st.actual_root().join(format!("{name}.manifest.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["name"], name);
    assert!(manifest["profile"].is_string());
    for key in [
        "ansi_sha256",
        "txt_sha256",
        "png_sha256",
        "html_sha256",
        "frame_sha256",
    ] {
        assert_eq!(manifest[key].as_str().unwrap().len(), 64, "{key}");
    }
    // C05: the ONE persisted verdict.
    let verdict: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(st.actual_root().join(format!("{name}.verdict.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(verdict["status"], "matched");
    assert_eq!(verdict["pixel_threshold"], 1.0);
    assert!(
        verdict["checks_performed"]
            .as_array()
            .unwrap()
            .contains(&serde_json::Value::String("png-pixel-gate".into()))
    );

    // C05: report on the same inputs reuses the verdict — same status.
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "sealed suite")
        .unwrap();
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, checked.status());
    assert_eq!(report.outcomes[0].pixel_score, Some(1.0));
}

#[test]
fn stale_verdict_after_accept_recomputes_instead_of_reuse() {
    let name = "stale/one";
    let (_dir, st) = tmp_store("stale");
    let frame = frame_with("stale verdict");
    let first = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(first.status(), Status::MissingApproval);
    // Accept WITHOUT re-checking: the sealed MissingApproval verdict is now
    // stale (the approved side appeared). Report must recompute via check —
    // never silently reuse the stale verdict.
    st.accept(name).unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "stale suite")
        .unwrap();
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, Status::Matched);
    // The recompute re-sealed a fresh verdict for the next report.
    let verdict: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(st.actual_root().join(format!("{name}.verdict.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(verdict["status"], "matched");
}

#[test]
fn interrupted_candidate_reports_missing_approval_never_pixel_verdict() {
    let name = "broken/one";
    let (_dir, st) = tmp_store("broken");
    let frame = frame_with("interrupted");
    st.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    let matched = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status(), Status::Matched);

    // Simulate interruption: frame survived, PNG write lost.
    std::fs::remove_file(&matched.actual.png).unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "broken suite")
        .unwrap();
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, Status::MissingApproval);
    assert!(
        report.outcomes[0].note.contains("incomplete"),
        "{}",
        report.outcomes[0].note
    );
}
