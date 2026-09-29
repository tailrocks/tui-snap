//! SVG views, HTML evidence shell, and escaping chokepoints.

use crate::profile::BlinkPhase;
use crate::profile::Profile;
use tuiscotti_core::frame::Frame;

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Attribute-context escape: [`esc_xml`] plus quotes so a value cannot
/// break out of `alt="..."` / `title="..."`. SVG text content stays on
/// [`esc_xml`] so cell `"` is not rewritten to `&quot;` (HTML snapshots
/// with quoted cell text must keep their bytes).
fn esc_attr(s: &str) -> String {
    esc_xml(s).replace('"', "&quot;").replace('\'', "&#39;")
}

// ---------------------------------------------------------------------------
// Safe-export audit (V06): every untrusted string crossing into HTML/JSON is
// escaped at exactly one of these chokepoints. Cell symbols, titles, and the
// embedded canonical JSON all arrive here; raw interpolation anywhere else is
// a bug. Trace journals (backlog A04) must route cell text through the same
// JSON chokepoint — never `format!` it into a hand-built envelope.
// ---------------------------------------------------------------------------

/// Escape untrusted text for HTML element content (SVG `<text>`, `<title>`).
#[must_use]
pub fn escape_html(s: &str) -> String {
    esc_xml(s)
}

/// Escape untrusted text for a double-quoted HTML attribute (`alt`, `title`).
#[must_use]
pub fn escape_html_attr(s: &str) -> String {
    esc_attr(s)
}

/// Escape canonical JSON for `<script type="application/json">`: `<` becomes
/// `\u003c` so a cell symbol like `</script>` cannot terminate the element.
/// Still valid JSON — `\u003c` re-parses to `<`, keeping lossless re-import.
#[must_use]
pub fn escape_json_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// Selectable-text SVG (secondary evidence: viewer fonts apply, so the PNG
/// stays authoritative for pixel gates). Blinking cells sample
/// [`BlinkPhase::On`] (frozen-visible); use [`render_svg_phased`] to sample
/// the off phase.
pub fn render_svg(frame: &Frame, profile: &Profile) -> String {
    render_svg_phased(frame, profile, BlinkPhase::On)
}

/// [`render_svg`] sampling a declared blink phase (V07): off-phase blinking
/// cells contribute blank space, like concealed cells. Blink intent stays in
/// canonical state; only the still is sampled.
pub fn render_svg_phased(frame: &Frame, profile: &Profile, phase: BlinkPhase) -> String {
    let cw = profile.cell_w;
    let ch = profile.cell_h;
    let pad = profile.pad;
    let w = frame.cols as u32 * cw + pad * 2;
    let h = frame.rows as u32 * ch + pad * 2;
    let bg = profile.default_bg.to_hex();
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" font-family=\"'JetBrainsMono Nerd Font Mono','JetBrains Mono',monospace\" font-size=\"{}\">\n<rect width=\"100%\" height=\"100%\" fill=\"{bg}\"/>\n",
        profile.font_px as u32
    );
    for y in 0..frame.rows {
        let mut x = 0u16;
        while x < frame.cols {
            let Some(cell) = frame.get(x, y) else {
                x += 1;
                continue;
            };
            if cell.continuation {
                x += 1;
                continue;
            }
            let (fg0, bg0) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let key = style_key(cell, fg0, bg0);
            let (run, nx) = coalesce_run(frame, profile, phase, x, y, &key);
            emit_span(&mut s, frame, profile, cell, &run, x, y, nx, fg0, bg0);
            x = nx;
        }
    }
    s.push_str("</svg>\n");
    s
}

type StyleKey = (
    tuiscotti_core::frame::Rgb,
    tuiscotti_core::frame::Rgb,
    bool,
    bool,
    tuiscotti_core::frame::UnderlineStyle,
    tuiscotti_core::frame::Color,
    bool,
);

fn style_key(
    cell: &tuiscotti_core::frame::Cell,
    fg: tuiscotti_core::frame::Rgb,
    bg: tuiscotti_core::frame::Rgb,
) -> StyleKey {
    (
        fg,
        bg,
        cell.mods.bold,
        cell.mods.italic,
        cell.mods.effective_underline_style(),
        cell.underline_color,
        cell.mods.strikethrough,
    )
}

/// Coalesce the maximal run of identical-style cells so words stay selectable
/// as one `<text>` element. Spaces join the run (same advance in monospace);
/// continuations break it (the lead's wide advance is handled by cell
/// geometry, not font metrics). Returns the run text and the first column
/// past the run.
fn coalesce_run(
    frame: &Frame,
    profile: &Profile,
    phase: BlinkPhase,
    x: u16,
    y: u16,
    key: &StyleKey,
) -> (String, u16) {
    let mut run = String::new();
    let mut nx = x;
    while nx < frame.cols {
        let Some(c) = frame.get(nx, y) else { break };
        if c.continuation {
            break;
        }
        let (fg, bg) = Frame::resolve_cell(c, profile.default_fg, profile.default_bg);
        if style_key(c, fg, bg) != *key {
            break;
        }
        if c.mods.hidden || (c.mods.blink && phase == BlinkPhase::Off) {
            run.push_str(&" ".repeat(usize::from(c.width.max(1))));
        } else {
            run.push_str(&c.symbol);
        }
        // Advance by display width (wide cells occupy 2 columns but hold one
        // grapheme in the lead cell).
        nx += u16::from(c.width.max(1));
    }
    (run, nx)
}

#[allow(
    clippy::too_many_arguments,
    reason = "span emission shares one call site; grouping would obscure the SVG contract"
)]
fn emit_span(
    s: &mut String,
    frame: &Frame,
    profile: &Profile,
    cell: &tuiscotti_core::frame::Cell,
    run: &str,
    x: u16,
    y: u16,
    nx: u16,
    fg0: tuiscotti_core::frame::Rgb,
    bg0: tuiscotti_core::frame::Rgb,
) {
    let cw = profile.cell_w;
    let ch = profile.cell_h;
    let pad = profile.pad;
    {
        let span_cols = nx - x;
        let px = pad + x as u32 * cw;
        let py = pad + y as u32 * ch;
        if bg0 != profile.default_bg {
            s.push_str(&format!(
                "<rect x=\"{px}\" y=\"{py}\" width=\"{}\" height=\"{ch}\" fill=\"{}\"/>\n",
                span_cols as u32 * cw,
                bg0.to_hex()
            ));
        }
        let weight = if cell.mods.bold {
            " font-weight=\"bold\""
        } else {
            ""
        };
        let style = if cell.mods.italic {
            " font-style=\"italic\""
        } else {
            ""
        };
        // text-decoration paints across the whole run, spaces included —
        // the same contract the PNG path follows for whitespace cells.
        let mut deco = Vec::new();
        if cell.mods.underline {
            deco.push("underline");
        }
        if cell.mods.strikethrough {
            deco.push("line-through");
        }
        let decoration = if deco.is_empty() {
            String::new()
        } else {
            format!(" text-decoration=\"{}\"", deco.join(" "))
        };
        // Non-single styles map to the SVG decoration style. The style
        // applies to every decoration on the run (a combined
        // double-underline + strike doubles both); single underlines
        // emit nothing, keeping existing SVG byte-identical.
        let deco_style = match cell.mods.effective_underline_style() {
            tuiscotti_core::frame::UnderlineStyle::Double => " text-decoration-style=\"double\"",
            tuiscotti_core::frame::UnderlineStyle::Curly => " text-decoration-style=\"wavy\"",
            tuiscotti_core::frame::UnderlineStyle::Dotted => " text-decoration-style=\"dotted\"",
            tuiscotti_core::frame::UnderlineStyle::Dashed => " text-decoration-style=\"dashed\"",
            tuiscotti_core::frame::UnderlineStyle::None
            | tuiscotti_core::frame::UnderlineStyle::Single => "",
        };
        let attrs = format!("{weight}{style}{decoration}{deco_style}");
        s.push_str(&format!(
            "<text xml:space=\"preserve\" x=\"{px}\" y=\"{}\" fill=\"{}\"{}>{}</text>\n",
            py + ch - 4,
            fg0.to_hex(),
            attrs,
            esc_xml(run)
        ));
    }
}

/// Build the standalone HTML document for a frame from an already-rendered
/// PNG (shared by [`Renderer::render_html`](super::Renderer::render_html) and [`Renderer::render_artifacts`](super::Renderer::render_artifacts)
/// so the PNG is rasterized once). See [`Renderer::render_html`](super::Renderer::render_html) for the
/// determinism contract of the embedded frame JSON.
pub(crate) fn html_document(frame: &Frame, profile: &Profile, title: &str, png: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let (png_w, png_h) = profile.image_size(frame.cols, frame.rows);
    // SVG is a selectable overlay only: its fills are forced transparent so
    // viewer fonts cannot tofu-over the PNG. Copy/select still works.
    let svg = render_svg(frame, profile);
    let mut embedded = frame.clone();
    embedded.provenance.created_unix = 0;
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{background:#141414;margin:24px}}.shot{{position:relative;display:inline-block;line-height:0}}.shot>img{{display:block;image-rendering:pixelated}}.shot>svg{{position:absolute;inset:0;width:100%;height:100%}}.shot>svg rect,.shot>svg text{{fill:transparent!important}}</style></head><body><div class=\"shot\"><img src=\"data:image/png;base64,{b64}\" alt=\"{}\" width=\"{png_w}\" height=\"{png_h}\">{svg}</div><script type=\"application/json\">{}</script></body></html>",
        esc_attr(title),
        esc_attr(title),
        escape_json_for_script(&embedded.to_json())
    )
}
