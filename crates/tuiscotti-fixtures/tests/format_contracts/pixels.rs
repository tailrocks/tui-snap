//! PNG pixels + HTML static offline (split from `format_contracts.rs`; shared helpers live in the root).

use super::common::{self, menu_frame, protocol_frame, renderer, streams_frame};
use super::menu_bundle;
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::formats::{
    assert_opaque_rgb, assert_static_offline, capture_all, changed_pixels, generation_for,
    html_static,
};

// --- PNG: opaque pixels -----------------------------------------------------

#[test]
fn png_is_opaque_rgb_and_deterministic() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let mut first = renderer();
    let a = first.render_png(&frame).expect("render");
    let info = assert_opaque_rgb(&a).expect("opaque RGB evidence");
    assert_eq!(
        (info.width, info.height),
        common::profile().image_size(40, 10)
    );
    let mut second = renderer();
    let b = second.render_png(&frame).expect("render");
    assert_eq!(a, b, "deterministic bytes for identical frame + profile");
}

#[test]
fn changed_pixels_beats_reencode_assumptions() {
    let a = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let mut model_b = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    model_b.selected = 1;
    let b = tuiscotti::ratatui::draw_frame(40, 10, common::prov("menu-view"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model_b);
    });
    let pa = renderer().render_png(&a).expect("render");
    let pa2 = renderer().render_png(&a).expect("render");
    assert!(
        changed_pixels(&pa, &pa2).expect("diff").is_empty(),
        "re-encode of identical pixels: zero changed pixels"
    );
    let pb = renderer().render_png(&b).expect("render");
    let changed = changed_pixels(&pa, &pb).expect("diff");
    assert!(!changed.is_empty(), "selection move changes decoded pixels");
    let total = common::profile().image_size(40, 10);
    assert!(
        changed.len() < (total.0 * total.1) as usize / 2,
        "change is localized, not a full repaint: {} px",
        changed.len()
    );
    // Dimension mismatch is an error, never a diff.
    let tiny = renderer()
        .render_png(&menu_frame(10, 4, Theme::Dark, Scenario::Empty))
        .expect("render");
    assert!(changed_pixels(&pa, &tiny).is_err());
}

// --- HTML: static offline, no JavaScript ------------------------------------

#[test]
fn html_is_static_offline_with_png_embed() {
    let bundle = menu_bundle();
    assert_static_offline(&bundle.html).expect("static offline");
    assert!(
        !bundle.html.to_lowercase().contains("<script"),
        "no script at all"
    );
    assert!(
        bundle.html.contains("data:image/png;base64,"),
        "offline PNG embed"
    );
    assert!(bundle.html.contains("autosave"), "selectable text present");
    assert!(
        bundle.html.contains(&bundle.generation.id),
        "generation labeled"
    );
}

#[test]
fn html_matches_committed_approvals_for_all_views() {
    for (name, frame) in [
        (
            "menu-demo-40x10",
            menu_frame(40, 10, Theme::Dark, Scenario::Demo),
        ),
        (
            "streams-demo-60x12",
            streams_frame(60, 12, Theme::Dark, false),
        ),
        (
            "protocol-demo-50x12",
            protocol_frame(50, 12, Theme::Dark, false),
        ),
    ] {
        let bundle = capture_all(&mut renderer(), &frame, name).expect("capture");
        let approved = common::read_expected(&format!("{name}.html"));
        assert_static_offline(&approved).expect("committed HTML stays static offline");
        assert_eq!(bundle.html, approved, "{name}: HTML drifted");
    }
}

#[test]
fn html_injection_is_escaped_not_executed() {
    let mut model = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    model.error = Some("</script><script>alert(1)</script>".to_string());
    let frame = tuiscotti::ratatui::draw_frame(48, 14, common::prov("inject"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model);
    });
    let generation = generation_for(&frame, "test").id;
    let html = html_static(
        &frame,
        &common::profile(),
        "\"><img src=x onerror=alert(1)>",
        None,
        &generation,
    );
    assert_static_offline(&html).expect("injection neutralized");
    assert!(
        !html.to_lowercase().contains("<script"),
        "no script element smuggled"
    );
    assert!(html.contains("&lt;script&gt;"), "payload escaped");
    // The validator itself bites: raw smuggled markup is rejected.
    assert_static_offline("<p>x</p><script>alert(1)</script>").expect_err("raw script caught");
    assert_static_offline("<img src=\"x\" onerror=\"alert(1)\">").expect_err("handler caught");
    assert_static_offline("<a href=\"http://evil.example\">x</a>").expect_err("external caught");
}
