//! HTML: escaped static offline evidence, without JavaScript.
//!
//! [`html_static`] renders a [`Frame`] as one self-contained document:
//! inline CSS only, the PNG evidence as a `data:` URI `<img>`, a selectable
//! SVG overlay with transparent fills (copy/select only — the PNG stays
//! authoritative), and the plain-text projection in a `<pre>`. There is
//! deliberately **no `<script>` element at all** — not even an inert
//! `type="application/json"` data block — so the document cannot execute
//! anything regardless of viewer quirks.
//!
//! Every untrusted string (title, cell text, text projection) crosses the
//! [`escape_html`](crate::render::escape_html) /
//! [`escape_html_attr`](crate::render::escape_html_attr) chokepoints.
//! [`assert_static_offline`] rejects script elements, event-handler
//! attributes, `javascript:` URLs, external references, and embedded
//! frames/objects.
//!
//! [`Frame`]: tuiscotti_core::frame::Frame

use crate::formats::FormatError;
use crate::profile::Profile;
use crate::render::{escape_html, escape_html_attr, render_svg};
use tuiscotti_core::frame::Frame;

/// Injected elements rejected anywhere in the document.
const FORBIDDEN_ELEMENTS: &[&str] = &["<script", "<link", "<iframe", "<object", "<embed", "<form"];

/// Handler attributes, `javascript:` URLs, and external references rejected
/// inside tag spans only (as escaped text they are inert content).
const FORBIDDEN_IN_TAGS: &[&str] = &[
    "javascript:",
    "onload=",
    "onerror=",
    "onclick=",
    "onmouseover=",
    "onfocus=",
    "src=\"http",
    "src='http",
    "href=\"http",
    "href='http",
    "url(http",
    "src=\"//",
    "href=\"//",
];

/// Render `frame` as static offline HTML. `png` (when given) is embedded as
/// a `data:` URI image; `generation` labels the capture in an inert HTML
/// comment so every capture stays identifiable in every format.
#[must_use]
pub fn html_static(
    frame: &Frame,
    profile: &Profile,
    title: &str,
    png: Option<&[u8]>,
    generation: &str,
) -> String {
    let img = png.map_or(String::new(), |bytes| {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let (w, h) = profile.image_size(frame.cols, frame.rows);
        format!(
            "<img src=\"data:image/png;base64,{b64}\" alt=\"{}\" width=\"{w}\" height=\"{h}\">",
            escape_html_attr(title)
        )
    });
    // The generation label sits in an HTML comment: admit hex only so the
    // label can never break out of the comment, whatever the caller passes.
    let gen_label: String = generation.chars().filter(char::is_ascii_hexdigit).collect();
    let gen_label = if gen_label.is_empty() {
        "none".to_string()
    } else {
        gen_label
    };
    let svg = render_svg(frame, profile);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{background:#141414;margin:24px;color:#eee;font-family:monospace}}.shot{{position:relative;display:inline-block;line-height:0}}.shot>img{{display:block;image-rendering:pixelated}}.shot>svg{{position:absolute;inset:0;width:100%;height:100%}}.shot>svg rect,.shot>svg text{{fill:transparent!important}}pre{{white-space:pre-wrap;word-break:break-all}}</style></head><body><!-- generation: {gen_label} --><div class=\"shot\">{img}{svg}</div><details open><summary>text</summary><pre>{}</pre></details></body></html>",
        escape_html(title),
        escape_html(&frame.text())
    )
}

/// Fail when `html` is not static offline evidence.
///
/// Two layers: injected *elements* (`<script`, `<iframe`, …) are rejected
/// anywhere in the document — template output never contains them and
/// escaped text cannot spell them (its `<` becomes `&lt;`). Handler
/// attributes, `javascript:` URLs, and external references are rejected
/// inside tag spans only: as escaped text (a `<pre>` showing a payload)
/// they are inert content, not markup.
///
/// # Errors
///
/// Returns `FormatError` naming the first forbidden token found.
pub fn assert_static_offline(html: &str) -> Result<(), FormatError> {
    let lower = html.to_lowercase();
    for token in FORBIDDEN_ELEMENTS {
        if lower.contains(token) {
            return Err(FormatError(format!(
                "HTML evidence carries forbidden element {token:?}"
            )));
        }
    }
    for span in tag_spans(&lower) {
        for token in FORBIDDEN_IN_TAGS {
            if span.contains(token) {
                return Err(FormatError(format!(
                    "HTML evidence tag carries forbidden {token:?}"
                )));
            }
        }
    }
    Ok(())
}

/// Raw `<…>` spans of `lower`: every `<` that starts markup through its
/// closing `>`. Escaped text contributes no spans (its brackets are
/// entities), and template tags carry only static attributes.
fn tag_spans(lower: &str) -> Vec<&str> {
    let bytes = lower.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && tag_start(bytes[i + 1]) {
            if let Some(end) = lower[i..].find('>') {
                spans.push(&lower[i..=(i + end)]);
                i += end + 1;
                continue;
            }
            break;
        }
        i += 1;
    }
    spans
}

/// First character after `<` for markup (tags, closes, comments, doctype).
fn tag_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'/' || b == b'!' || b == b'?'
}
