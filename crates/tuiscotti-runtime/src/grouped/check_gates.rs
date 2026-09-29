use super::{ArtifactPaths, GroupedOutcome};
use crate::snapshot::{CompareOutcome, Status};
use tuiscotti_core::frame::Frame;

/// Locate the first differing byte of two blobs, for human diagnostics.
fn first_difference(approved: &[u8], actual: &[u8]) -> String {
    let n = approved.len().min(actual.len());
    let mut i = 0;
    while i < n && approved[i] == actual[i] {
        i += 1;
    }
    let mut line = 1;
    for &b in &approved[..i] {
        if b == b'\n' {
            line += 1;
        }
    }
    format!(
        "first difference at byte {i} (approved line {line}); approved {} bytes, actual {} bytes",
        approved.len(),
        actual.len()
    )
}

/// Validated cheap actuals plus both artifact path sets.
pub(crate) struct CheapActuals {
    pub(crate) ansi: String,
    pub(crate) txt: String,
    pub(crate) actual: ArtifactPaths,
    pub(crate) approved: ArtifactPaths,
}

/// Fresh outcome skeleton: `MissingApproval` until a gate says otherwise.
pub(crate) fn fresh_grouped(name: &str, cheap: &CheapActuals, actual: &Frame) -> GroupedOutcome {
    let outcome = CompareOutcome {
        name: name.to_string(),
        status: Status::MissingApproval,
        cell_diffs: Vec::new(),
        cell_diff_total: 0,
        pixel_score: None,
        approved_png_regenerated: false,
        digest_expected: None,
        digest_actual: format!("{:016x}", actual.digest()),
        actual_frame: cheap.actual.frame_json.clone(),
        actual_png: cheap.actual.png.clone(),
        // Never exists in a conforming approved tree (four artifacts
        // only); reports simply omit the expected-frame panel.
        expected_frame: cheap.approved.frame_json.clone(),
        expected_png: None,
        expected_png_bytes: None,
        diff_png: None,
        note: String::new(),
    };
    GroupedOutcome {
        outcome,
        ansi_match: None,
        txt_match: None,
        html_match: None,
        actual: cheap.actual.clone(),
        approved: cheap.approved.clone(),
    }
}

/// Which approved artifacts are absent (empty = all present). `approved`
/// holds `.ansi`/`.txt`/`.html`/`.png` in that order (`None` = missing).
pub(crate) fn missing_approved_names(approved: [Option<&[u8]>; 4]) -> Vec<&'static str> {
    const LABELS: [&str; 4] = [".ansi", ".txt", ".html", ".png"];
    approved
        .iter()
        .zip(LABELS)
        .filter_map(|(slot, label)| slot.is_none().then_some(label))
        .collect()
}

/// ANSI + TXT byte gates: cell-exact, then content-only.
pub(crate) fn run_byte_gates(
    grouped: &mut GroupedOutcome,
    approved_ansi: &[u8],
    actual_ansi: &str,
    approved_txt: &[u8],
    actual_txt: &str,
    notes: &mut Vec<String>,
) {
    let outcome = &mut grouped.outcome;
    // ANSI byte gate: the cell-exact comparison (symbol+fg+bg+mods).
    let ansi_equal = approved_ansi == actual_ansi.as_bytes();
    grouped.ansi_match = Some(ansi_equal);
    if !ansi_equal {
        outcome.status = Status::CellsDiffer;
        notes.push(format!(
            "ansi differs (cell-exact gate): {}",
            first_difference(approved_ansi, actual_ansi.as_bytes())
        ));
    }

    // TXT byte gate: content only (style-only changes keep txt equal).
    let txt_equal = approved_txt == actual_txt.as_bytes();
    grouped.txt_match = Some(txt_equal);
    if !txt_equal {
        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::CellsDiffer;
        }
        notes.push(format!(
            "txt differs: {}",
            first_difference(approved_txt, actual_txt.as_bytes())
        ));
    }
}

/// HTML byte gate: identical cells with a changed renderer/font fail
/// here — a render-level event, reported as `PixelsDiffer`.
pub(crate) fn run_html_gate(
    grouped: &mut GroupedOutcome,
    approved_html: &[u8],
    actual_html_bytes: &[u8],
    notes: &mut Vec<String>,
) {
    let html_equal = approved_html == actual_html_bytes;
    grouped.html_match = Some(html_equal);
    if !html_equal {
        if matches!(grouped.outcome.status, Status::MissingApproval) {
            grouped.outcome.status = Status::PixelsDiffer;
        }
        notes.push(format!(
            "html differs (render-level gate): {}",
            first_difference(approved_html, actual_html_bytes)
        ));
    }
}
