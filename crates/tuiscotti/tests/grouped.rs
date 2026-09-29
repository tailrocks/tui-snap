//! Grouped multi-artifact store behaviors: nested names, four-artifact
//! approvals, byte gates (ansi/txt/html), pixel gate, recursive accept and
//! report, name validation, determinism.

use ratatui::widgets::Paragraph;
use tuiscotti::grouped::GroupedStore;
use tuiscotti::{Profile, Provenance};

#[path = "grouped/accept_report.rs"]
mod accept_report;
#[path = "grouped/core.rs"]
mod core;
#[path = "grouped/gates.rs"]
mod gates;

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn profile() -> Profile {
    Profile::default_profile()
}

fn frame_with(text: &str) -> tuiscotti::Frame {
    tuiscotti::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_store(tag: &str) -> (tempfile::TempDir, GroupedStore) {
    let dir = tempfile::tempdir().unwrap();
    let st = GroupedStore::new(&dir.path().join(tag));
    (dir, st)
}

/// Every file below `dir`, relative paths sorted (for approved-tree audits).
fn tree_files(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out.sort();
    out
}
