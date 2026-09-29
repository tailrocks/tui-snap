use super::*;
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::{Frame, FrameError};
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render;

/// Assemble one report row from a check outcome. Free-function form of
/// [`Store::report_entry`] so non-classic stores can build rows without a
/// [`Store`]. Does not read PNG bytes.
pub fn report_entry(
    outcome: &CompareOutcome,
    profile: &Profile,
) -> Result<ReportEntry, SnapshotError> {
    Ok(ReportEntry {
        outcome: outcome.clone(),
        profile_desc: profile.name.clone(),
        font_sha256: profile.font_sha256.clone(),
    })
}

/// Result of [`Store::report`]/[`Store::report_with`]: the rewritten report
/// plus every outcome it embeds.
#[derive(Debug)]
pub struct StoreReport {
    /// Path of the rewritten `report.html`.
    pub path: PathBuf,
    /// One outcome per re-verified actual, in name order.
    pub outcomes: Vec<CompareOutcome>,
}

impl StoreReport {
    /// Outcomes that did not match (the CLI turns this into a non-zero exit).
    #[must_use]
    pub fn failed(&self) -> usize {
        self.outcomes.iter().filter(|o| !o.status.matched()).count()
    }
}

/// One row of the review HTML report. Images are files on disk; the HTML
/// only stores relative `href`s so hundreds of captures stay browser-usable.
pub struct ReportEntry {
    pub outcome: CompareOutcome,
    pub profile_desc: String,
    pub font_sha256: String,
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JSON embedded in `<script type="application/json">`: escape `<` so a cell
/// symbol like `</script>` cannot terminate the element (still valid JSON —
/// `\u003c` re-parses to `<`, keeping lossless re-import).
pub fn json_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// Write a review index: PNGs linked from disk (never base64-embedded),
/// failed captures first, frame JSON linked not inlined.
pub fn write_report(
    store: &Store,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    write_report_at(&store.root.join("report.html"), title, entries)
}

/// [`write_report`] with an explicit output path, for stores whose report
/// does not live at a fixed location (e.g. [`crate::grouped::GroupedStore`],
/// which keeps its report out of the approved tree).
pub fn write_report_at(
    path: &Path,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SnapshotError(format!("cannot create {}: {e}", parent.display())))?;
    }
    let report_dir = path.parent().unwrap_or(Path::new("."));
    let failed_n = entries
        .iter()
        .filter(|e| !e.outcome.status.matched())
        .count();
    let mut ordered: Vec<&ReportEntry> = Vec::with_capacity(entries.len());
    ordered.extend(entries.iter().filter(|e| !e.outcome.status.matched()));
    ordered.extend(entries.iter().filter(|e| e.outcome.status.matched()));

    let mut body = format!(
        "<p>{} captures · {} matched · {} failed</p>\n",
        entries.len(),
        entries.len() - failed_n,
        failed_n
    );
    for e in ordered {
        append_entry_section(&mut body, report_dir, e)?;
    }
    let profile_line = entries
        .first()
        .map(|e| {
            format!(
                "<p>profile: {} · font sha256: {}</p>",
                esc_html(&e.profile_desc),
                esc_html(&e.font_sha256)
            )
        })
        .unwrap_or_default();
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:system-ui,sans-serif;background:#141414;color:#eee;margin:24px}}section{{border:1px solid #444;margin:16px 0;padding:16px}}img{{max-width:100%;image-rendering:pixelated}}table{{border-collapse:collapse}}td,th{{border:1px solid #555;padding:2px 8px;font-family:monospace}}</style></head><body><h1>{}</h1>{profile_line}{body}</body></html>",
        esc_html(title),
        esc_html(title)
    );
    write_atomic(path, html.as_bytes())?;
    Ok(path.to_path_buf())
}

/// One `<section>` of the report: header, images, cell table, scores,
/// and frame links for a single entry.
fn append_entry_section(
    body: &mut String,
    report_dir: &Path,
    e: &ReportEntry,
) -> Result<(), SnapshotError> {
    let o = &e.outcome;
    body.push_str(&format!(
        "<section id=\"{}\"><h2>{} — {}</h2>\n",
        esc_attr(&o.name),
        esc_html(&o.name),
        o.status.as_str()
    ));
    append_entry_images(body, report_dir, o)?;
    if o.cell_diff_total > 0 {
        body.push_str(&format!(
            "<p>{} differing cell(s):</p><table><tr><th>x</th><th>y</th><th>expected</th><th>actual</th></tr>",
            o.cell_diff_total
        ));
        for d in &o.cell_diffs {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                d.x,
                d.y,
                esc_html(&d.expected),
                esc_html(&d.actual)
            ));
        }
        body.push_str("</table>");
    }
    if let Some(s) = o.pixel_score {
        body.push_str(&format!("<p>pixel similarity: {s:.6}</p>"));
    }
    if o.actual_frame.exists() {
        body.push_str(&format!(
            "<p><a href=\"{}\">actual frame.json</a></p>",
            rel_href(report_dir, &o.actual_frame)
        ));
    }
    if o.expected_frame.exists() {
        body.push_str(&format!(
            "<p><a href=\"{}\">expected frame.json</a></p>",
            rel_href(report_dir, &o.expected_frame)
        ));
    }
    body.push_str("</section>");
    Ok(())
}

/// The expected/actual/diff image row of one entry section. Expected PNG
/// bytes that live only in memory spill to a `report-media/` sidecar.
fn append_entry_images(
    body: &mut String,
    report_dir: &Path,
    o: &CompareOutcome,
) -> Result<(), SnapshotError> {
    body.push_str("<div class=\"imgs\">");
    let expected_src = match &o.expected_png {
        Some(p) if p.exists() => Some(rel_href(report_dir, p)),
        _ => match &o.expected_png_bytes {
            Some(bytes) => Some(rel_href(
                report_dir,
                &write_report_sidecar(report_dir, &o.name, "expected", bytes)?,
            )),
            None => None,
        },
    };
    if let Some(src) = expected_src {
        body.push_str(&format!(
            "<figure><figcaption>expected</figcaption><img src=\"{src}\" alt=\"expected {}\"></figure>",
            esc_attr(&o.name)
        ));
    } else {
        body.push_str("<figure><figcaption>expected</figcaption><p>missing approval</p></figure>");
    }
    if o.actual_png.exists() {
        let src = rel_href(report_dir, &o.actual_png);
        body.push_str(&format!(
            "<figure><figcaption>actual</figcaption><img src=\"{src}\" alt=\"actual {}\"></figure>",
            esc_attr(&o.name)
        ));
    }
    if let Some(p) = o.diff_png.as_ref().filter(|p| p.exists()) {
        let src = rel_href(report_dir, p);
        body.push_str(&format!(
            "<figure><figcaption>diff</figcaption><img src=\"{src}\" alt=\"diff {}\"></figure>",
            esc_attr(&o.name)
        ));
    }
    body.push_str("</div>");
    Ok(())
}

fn esc_attr(s: &str) -> String {
    esc_html(s).replace('"', "&quot;")
}

fn rel_href(from_dir: &Path, to: &Path) -> String {
    let from = from_dir.components().collect::<Vec<_>>();
    let to_c = to.components().collect::<Vec<_>>();
    let mut i = 0;
    while i < from.len() && i < to_c.len() && from[i] == to_c[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..from.len() {
        out.push("..");
    }
    for c in &to_c[i..] {
        out.push(*c);
    }
    if out.as_os_str().is_empty() {
        return to
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| to.display().to_string());
    }
    out.to_string_lossy().replace('\\', "/")
}

fn write_report_sidecar(
    report_dir: &Path,
    name: &str,
    kind: &str,
    bytes: &[u8],
) -> Result<PathBuf, SnapshotError> {
    let dir = report_dir.join("report-media");
    std::fs::create_dir_all(&dir)
        .map_err(|e| SnapshotError(format!("cannot create {}: {e}", dir.display())))?;
    let safe = name.replace('/', "__");
    let path = dir.join(format!("{safe}-{kind}.png"));
    write_atomic(&path, bytes)?;
    Ok(path)
}
