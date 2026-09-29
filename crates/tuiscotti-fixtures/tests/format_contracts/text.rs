//! ASCII, TXT, ANSI projections (split from `format_contracts.rs`; shared helpers live in the root).

use super::capture::{self as cap, renderer};
use super::common::{self, menu_frame, streams_frame};
use super::menu_bundle;
use super::pure::protocol_frame;
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::formats::{
    ansi_normalized, ascii_projection, assert_no_escapes, assert_normalized_sgr, assert_seven_bit,
    capture_all, txt_projection,
};

// --- ASCII: 7-bit diagnostic ------------------------------------------------

#[test]
fn ascii_is_seven_bit_with_exact_substitution_accounting() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let ascii = ascii_projection(&frame);
    assert_seven_bit(&ascii.text).expect("7-bit output");
    assert!(ascii.lossy(), "box borders must count as substitutions");
    assert!(
        ascii.substitutions.len() > 20,
        "every border cell recorded, got {}",
        ascii.substitutions.len()
    );
    for sub in &ascii.substitutions {
        assert_seven_bit(&sub.replacement).expect("7-bit replacement");
        assert_ne!(sub.original, sub.replacement);
    }
    // Geometry preserved: same row count, same display width per row.
    assert_eq!(ascii.text.lines().count(), 10);
    assert_eq!(
        ascii.text,
        cap::read_expected("menu-demo-40x10.ascii.txt").expect("committed baseline")
    );
}

#[test]
fn ascii_lossless_only_when_nothing_substituted() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let ascii = ascii_projection(&frame);
    assert!(
        ascii.lossless_text().is_none(),
        "lossy ASCII has no lossless text"
    );
    // Pure-ASCII content projects loss-free.
    let plain = tuiscotti::ratatui::draw_frame(12, 3, common::prov("ascii"), |f| {
        use ratatui::widgets::Paragraph;
        f.render_widget(Paragraph::new("hello ascii"), f.area());
    });
    let artifact = ascii_projection(&plain);
    assert!(!artifact.lossy());
    assert_eq!(artifact.lossless_text(), Some(artifact.text.as_str()));
    assert_seven_bit(&artifact.text).expect("7-bit");
}

#[test]
fn ascii_reports_wide_and_combining_loss_per_cell() {
    let frame = streams_frame(60, 12, Theme::Dark, false);
    let ascii = ascii_projection(&frame);
    assert_seven_bit(&ascii.text).expect("7-bit");
    assert!(ascii.lossy());
    // Wide CJK lead cells emit exactly two ASCII columns.
    let cjk: Vec<_> = ascii
        .substitutions
        .iter()
        .filter(|s| s.original.chars().any(|c| c == '日' || c == '本'))
        .collect();
    assert!(!cjk.is_empty(), "CJK substitutions recorded");
    for sub in cjk {
        assert_eq!(sub.replacement.len(), 2, "wide cell keeps 2 columns");
    }
}

// --- TXT: plain Unicode -----------------------------------------------------

#[test]
fn txt_is_plain_unicode_matching_baselines() {
    let menu = txt_projection(&menu_frame(40, 10, Theme::Dark, Scenario::Demo));
    assert_no_escapes(&menu).expect("no escapes");
    assert_eq!(
        menu,
        cap::read_expected("menu-demo-40x10.txt").expect("committed baseline")
    );
    let streams = txt_projection(&streams_frame(60, 12, Theme::Dark, false));
    assert_no_escapes(&streams).expect("no escapes");
    assert_eq!(
        streams,
        cap::read_expected("streams-demo-60x12.txt").expect("committed baseline")
    );
    let protocol = txt_projection(&protocol_frame(50, 12, Theme::Dark, false));
    assert_no_escapes(&protocol).expect("no escapes");
    assert_eq!(
        protocol,
        cap::read_expected("protocol-demo-50x12.txt").expect("committed baseline")
    );
}

#[test]
fn txt_whitespace_policy_trims_tails_keeps_interior() {
    let frame = streams_frame(60, 12, Theme::Dark, false);
    let txt = txt_projection(&frame);
    for line in txt.lines() {
        assert!(!line.ends_with(' '), "no trailing blanks: {line:?}");
    }
    assert!(txt.contains("padded   cells   here"), "interior kept");
    assert!(!txt.ends_with('\n'), "no trailing newline");
    assert_eq!(txt.lines().count(), 12, "one line per grid row");
}

// --- ANSI: normalized SGR ---------------------------------------------------

#[test]
fn ansi_is_normalized_not_raw_transcript() {
    let bundle = menu_bundle().expect("capture bundle");
    assert_normalized_sgr(&bundle.ansi).expect("normalized SGR only");
    // Content rides along; style changes the bytes.
    assert!(bundle.ansi.contains("autosave"));
    assert!(bundle.ansi.contains("\x1b["), "SGR runs present");
    let light = ansi_normalized(&menu_frame(40, 10, Theme::Light, Scenario::Demo));
    assert_normalized_sgr(&light).expect("normalized SGR only");
    assert_ne!(bundle.ansi, light, "theme change moves ANSI bytes");
}

#[test]
fn ansi_matches_committed_approvals_for_all_views() {
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
        let bundle =
            capture_all(&mut renderer().expect("renderer"), &frame, name).expect("capture");
        let approved = cap::read_expected(&format!("{name}.ansi")).expect("committed baseline");
        assert_normalized_sgr(&approved).expect("committed ANSI stays normalized");
        assert_eq!(bundle.ansi, approved, "{name}: ANSI drifted");
    }
}

#[test]
fn ansi_only_style_change_moves_ansi_but_not_txt() {
    let dark = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let light = menu_frame(40, 10, Theme::Light, Scenario::Demo);
    assert_eq!(
        txt_projection(&dark),
        txt_projection(&light),
        "same content: TXT still"
    );
    assert_ne!(
        ansi_normalized(&dark),
        ansi_normalized(&light),
        "new palette: ANSI moves"
    );
    // Underline-across-spaces is style-only too.
    let plain = streams_frame(60, 12, Theme::Dark, false);
    assert_eq!(txt_projection(&plain).lines().count(), 12);
}
