use super::*;
use tuiscotti::profile::{MissingGlyphPolicy, RENDERER_VERSION, RenderProfile};
use tuiscotti::render::{
    BundleManifest, Renderer, check_contract_bytes, escape_html, escape_html_attr,
    escape_json_for_script, frame_from_screen, redact_frame, redact_screen, render_frame_strict,
    render_screen,
};
use tuiscotti::{Cell, Mods, Screen};

// ---------------------------------------------------------------------------
// V06: safe export — escaping, concealment vs redaction.
// ---------------------------------------------------------------------------
#[test]
fn export_escapes_untrusted_content() {
    assert_eq!(escape_html("<a>&\""), "&lt;a&gt;&amp;\"");
    assert_eq!(escape_html_attr("x\" onload=\""), "x&quot; onload=&quot;");
    let evil = "{\"s\":\"</script><img src=x onerror=y>\"}";
    let safe = escape_json_for_script(evil);
    assert!(!safe.contains("</script>"), "{safe}");
    assert!(safe.contains("\\u003c/script>"), "{safe}");
    // Still valid JSON, re-parses to the identical value.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&safe).unwrap(),
        serde_json::from_str::<serde_json::Value>(evil).unwrap()
    );
    // End to end: hostile title cannot break out of the HTML document.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(4, 1, vec![cell(0, 0, "<", 1)]);
    let html = Renderer::for_render_profile(&rp)
        .unwrap()
        .render_html(&frame, "x\" onload=\"y")
        .unwrap();
    assert!(!html.contains("alt=\"x\" onload=\""), "{html}");
    let body = html.split("<body>").nth(1).unwrap();
    assert!(!body.split("<script").next().unwrap().contains("</script>"));
}

#[test]
fn concealment_hides_pixels_not_canonical_data() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut hiddens: Vec<Cell> = "SECRET"
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let mut c = cell(i as u16, 0, &ch.to_string(), 1);
            c.mods.hidden = true;
            c
        })
        .collect();
    let screen = screen_from_leads(8, 2, hiddens.clone());
    for c in &mut hiddens {
        c.mods.hidden = false;
        c.symbol = " ".to_string();
    }
    let blanks = screen_from_leads(8, 2, hiddens);
    // Pixels: concealed SECRET == blank spaces.
    assert_eq!(
        render_screen(&screen, &rp).unwrap().png,
        render_screen(&blanks, &rp).unwrap().png
    );
    // Visible HTML/SVG carry no trace of the concealed text.
    let frame = frame_from_screen(&screen, "qual");
    let svg = tuiscotti::render::render_svg(&frame, &rp.to_profile());
    assert!(!svg.contains("SECRET"), "{svg}");
    let html = Renderer::for_render_profile(&rp)
        .unwrap()
        .render_html(&frame, "t")
        .unwrap();
    let visible = html.split("<script").next().unwrap();
    assert!(!visible.contains("SECRET"), "{visible}");
    // Canonical data DOES retain it: concealment is not redaction.
    // (Per-cell JSON holds single symbols, so check each concealed cell.)
    let json = frame.to_json();
    for ch in "SECRET".chars() {
        assert!(json.contains(&format!("\"symbol\":\"{ch}\"")), "{ch}");
    }
    assert!(json.contains("\"hidden\":true"));
    assert!(tuiscotti::render::ansi_dump(&frame).contains("SECRET"));
}

#[test]
fn redaction_destroys_content_and_validates() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut c = cell(0, 0, "S", 1);
    c.mods.hidden = true;
    let mut frame = frame_from_leads(6, 2, vec![c, cell(1, 0, "東", 2), cont(2, 0)]);
    frame.provenance.argv = vec!["tuisnap".into(), "--password=s3cret".into()];
    let red = redact_frame(&frame);
    red.validate().unwrap();
    assert!(!red.to_json().contains('S'));
    assert!(!red.to_json().contains('東'));
    // Defense-in-depth: launch arguments may carry secrets.
    assert!(red.provenance.argv.is_empty());
    assert!(!red.to_json().contains("s3cret"));
    // Geometry, colors, cursor survive; content and mods do not.
    assert_eq!((red.cols, red.rows), (frame.cols, frame.rows));
    assert_eq!(red.get(1, 0).unwrap().width, 2);
    assert!(red.get(2, 0).unwrap().continuation);
    assert_eq!(red.get(0, 0).unwrap().symbol, "█");
    assert_eq!(red.get(1, 0).unwrap().symbol, "██");
    assert_eq!(red.get(0, 0).unwrap().mods, Mods::default());
    assert_ne!(
        render_frame_strict(&frame, &rp).unwrap().png,
        render_frame_strict(&red, &rp).unwrap().png
    );
    // Screen variant preserves the origin.
    let screen = Screen::from_frame(&frame).unwrap();
    let red_screen = redact_screen(&screen).unwrap();
    assert_eq!(red_screen.origin(), screen.origin());
    assert_eq!(red_screen.cols(), screen.cols());
    let red_frame = frame_from_screen(&red_screen, "qual");
    assert!(!red_frame.to_json().contains('東'));
}

// ---------------------------------------------------------------------------
// V10: opt-in byte contracts.
// ---------------------------------------------------------------------------
#[test]
fn contract_bytes_opt_in_and_exact() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(6, 2, vec![cell(0, 0, "h", 1), cell(1, 0, "i", 1)]);
    let mut r = Renderer::for_render_profile(&rp).unwrap();
    let a = r.render_artifacts(&frame, "t").unwrap().contract();
    let b = r.render_artifacts(&frame, "t").unwrap().contract();
    check_contract_bytes(&a, &b).unwrap();
    // One changed cell breaks the contract with field + byte offset.
    let mutated = frame_from_leads(6, 2, vec![cell(0, 0, "H", 1), cell(1, 0, "i", 1)]);
    let c = r.render_artifacts(&mutated, "t").unwrap().contract();
    let err = check_contract_bytes(&c, &a).unwrap_err();
    assert!(err.to_string().contains("ansi"), "{err}");
    assert!(err.to_string().contains("byte"), "{err}");
    // Contracts serialize for storage.
    let round: tuiscotti::render::ContractBytes =
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(round, a);
}

// ---------------------------------------------------------------------------
// V09: portable offline bundle.
// ---------------------------------------------------------------------------
#[test]
fn bundle_is_offline_and_self_describing() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(6, 2, vec![cell(0, 0, "A", 1)]);
    let mut r = Renderer::for_render_profile(&rp).unwrap();
    let artifacts = r.render_artifacts(&frame, "bundle").unwrap();
    let manifest = BundleManifest::for_render(&rp, &artifacts.fidelity);
    let dir = tempfile::tempdir().unwrap();
    let paths = artifacts.write_bundle(dir.path(), &manifest).unwrap();
    assert_eq!(paths.len(), 6);
    for name in [
        "screen.ansi",
        "screen.txt",
        "screen.png",
        "screen.html",
        "fidelity.json",
        "manifest.json",
    ] {
        assert!(dir.path().join(name).is_file(), "{name}");
    }
    let html = std::fs::read_to_string(dir.path().join("screen.html")).unwrap();
    assert!(html.contains("data:image/png;base64,"), "PNG embedded");
    assert!(!html.contains("src=\"http"), "no external refs");
    assert!(!html.contains("href=\"http"), "no external refs");
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(m["renderer_version"], RENDERER_VERSION);
    assert_eq!(m["profile_hash"], serde_json::Value::String(rp.hash()));
    assert_eq!(m["face_hashes"].as_array().unwrap().len(), 4);
    assert_eq!(m["fallback_faces"].as_array().unwrap().len(), 3);
    assert_eq!(m["approximate"], serde_json::Value::Bool(false));
}
