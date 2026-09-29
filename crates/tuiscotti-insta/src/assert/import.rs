//! Read-only importer for classic/grouped four-file frozen trees.

use std::path::{Path, PathBuf};

/// One imported four-artifact scenario.
#[derive(Debug, Clone)]
pub struct ImportedScenario {
    /// Scenario name (relative `/`-separated stem).
    pub name: String,
    /// `.ansi` bytes as UTF-8.
    pub ansi: String,
    /// `.txt` bytes as UTF-8.
    pub txt: String,
    /// `.html` bytes as UTF-8.
    pub html: String,
    /// `.png` bytes (decode-validated).
    pub png: Vec<u8>,
}

/// A read-only imported frozen tree: scenarios plus reported-but-tolerated entries.
#[derive(Debug, Clone, Default)]
pub struct FrozenTree {
    /// Fully validated scenarios, sorted by name.
    pub scenarios: Vec<ImportedScenario>,
    /// Reported, non-fatal entries: extra files (`extra file: <rel>`) and
    /// unknown embedded-frame fields (`unsupported field in <name>.html: <key>`).
    pub unsupported: Vec<String>,
}

/// Frozen-tree import failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// A scenario stem fails name validation.
    InvalidName(String),
    /// A scenario stem lacks one or more of the four artifacts.
    Incomplete {
        /// Scenario stem.
        name: String,
        /// Missing artifact extensions (without dot).
        missing: Vec<String>,
    },
    /// An artifact is present but unparsable (non-UTF-8 text, undecodable PNG).
    Corrupt {
        /// Artifact at fault.
        path: PathBuf,
        /// Why it is unusable.
        reason: String,
    },
    /// Filesystem failure (path context included).
    Io(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::InvalidName(e) => write!(f, "invalid scenario name: {e}"),
            ImportError::Incomplete { name, missing } => {
                write!(
                    f,
                    "scenario {name:?} incomplete, missing: {}",
                    missing.join(", ")
                )
            }
            ImportError::Corrupt { path, reason } => {
                write!(f, "artifact corrupt: {}: {reason}", path.display())
            }
            ImportError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Scenario-name validation, mirroring the `grouped` layout rules without
/// depending on its runtime: relative `/`-separated paths, no absolute paths,
/// no `.`/`..`/empty segments, no backslashes.
pub(crate) fn check_scenario_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("empty name".to_string());
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return Err(format!("{name:?}: absolute paths are not allowed"));
    }
    if name.contains('\\') {
        return Err(format!(
            "{name:?}: backslashes are not allowed (use `/` separators)"
        ));
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err(format!("{name:?}: empty path segment"));
        }
        if seg == ".." {
            return Err(format!("{name:?}: `..` segments are not allowed"));
        }
        if seg == "." {
            return Err(format!("{name:?}: `.` segments are not allowed"));
        }
    }
    Ok(())
}

fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| ImportError::Io(format!("read {}: {e}", d.display())))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Io(format!("read {}: {e}", d.display())))?
            .into_iter()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Known top-level keys of the frame JSON embedded in `.html` renders.
const KNOWN_FRAME_KEYS: &[&str] = &["version", "cols", "rows", "cells", "cursor", "provenance"];

/// Read-only import of a classic/grouped four-file tree (I07).
///
/// Groups `<stem>.{ansi,txt,png,html}` files (recursively, so nested `a/b`
/// scenarios work) into scenarios: stems are name-validated, every stem needs
/// all four artifacts, text parses as UTF-8, PNGs must decode. Anything else —
/// extra files, unknown embedded-frame JSON fields — is REPORTED in
/// [`FrozenTree::unsupported`], never fatal. Reads only; the tree is untouched.
pub fn import_frozen_v1(dir: &Path) -> Result<FrozenTree, ImportError> {
    const MEMBER_EXTS: [&str; 4] = ["ansi", "txt", "png", "html"];
    let mut members: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, PathBuf>,
    > = std::collections::BTreeMap::new();
    let mut unsupported = Vec::new();
    for path in collect_files(dir)? {
        let rel = path
            .strip_prefix(dir)
            .map_err(|e| ImportError::Io(format!("prefix {}: {e}", path.display())))?;
        // Join components with `/`: a literal backslash inside a filename stays a
        // backslash (and fails name validation) instead of becoming a separator.
        let rel: String = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let file = rel.rsplit('/').next().unwrap_or(&rel);
        if file.ends_with(".png.fidelity.json") {
            unsupported.push(format!("extra file: {rel}"));
            continue;
        }
        let (stem, ext) = match rel.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), e.to_string()),
            None => {
                unsupported.push(format!("extra file: {rel}"));
                continue;
            }
        };
        if !MEMBER_EXTS.contains(&ext.as_str()) {
            unsupported.push(format!("extra file: {rel}"));
            continue;
        }
        members.entry(stem).or_default().insert(ext, path);
    }
    let mut scenarios = Vec::new();
    for (stem, got) in &members {
        check_scenario_name(stem).map_err(ImportError::InvalidName)?;
        let missing: Vec<String> = MEMBER_EXTS
            .iter()
            .filter(|e| !got.contains_key(**e))
            .map(|e| (*e).to_string())
            .collect();
        if !missing.is_empty() {
            return Err(ImportError::Incomplete {
                name: stem.clone(),
                missing,
            });
        }
        let read_text = |ext: &str| -> Result<String, ImportError> {
            let p = &got[ext];
            let bytes = std::fs::read(p)
                .map_err(|e| ImportError::Io(format!("read {}: {e}", p.display())))?;
            String::from_utf8(bytes).map_err(|e| ImportError::Corrupt {
                path: p.clone(),
                reason: format!("not UTF-8: {e}"),
            })
        };
        let png_path = &got["png"];
        let png = std::fs::read(png_path)
            .map_err(|e| ImportError::Io(format!("read {}: {e}", png_path.display())))?;
        image::load_from_memory(&png).map_err(|e| ImportError::Corrupt {
            path: png_path.clone(),
            reason: format!("PNG does not decode: {e}"),
        })?;
        let html = read_text("html")?;
        report_embedded_fields(stem, &html, &mut unsupported);
        scenarios.push(ImportedScenario {
            name: stem.clone(),
            ansi: read_text("ansi")?,
            txt: read_text("txt")?,
            html,
            png,
        });
    }
    unsupported.sort();
    Ok(FrozenTree {
        scenarios,
        unsupported,
    })
}

/// Report unknown fields of the frame JSON embedded in an `.html` render.
/// Findings are non-fatal notes in `unsupported`.
fn report_embedded_fields(stem: &str, html: &str, unsupported: &mut Vec<String>) {
    const OPEN: &str = "<script type=\"application/json\">";
    const CLOSE: &str = "</script>";
    let Some(after) = html.split_once(OPEN).map(|(_, tail)| tail) else {
        unsupported.push(format!("no embedded frame JSON in {stem}.html"));
        return;
    };
    let Some((json_text, _)) = after.split_once(CLOSE) else {
        unsupported.push(format!("truncated embedded frame JSON in {stem}.html"));
        return;
    };
    let value: serde_json::Value = match serde_json::from_str(json_text) {
        Ok(v) => v,
        Err(e) => {
            unsupported.push(format!(
                "unparsable embedded frame JSON in {stem}.html: {e}"
            ));
            return;
        }
    };
    let serde_json::Value::Object(map) = value else {
        unsupported.push(format!(
            "embedded frame JSON in {stem}.html is not an object"
        ));
        return;
    };
    let mut unknown: Vec<&str> = map
        .keys()
        .filter(|k| !KNOWN_FRAME_KEYS.contains(&k.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort();
    for key in unknown {
        unsupported.push(format!("unsupported field in {stem}.html: {key}"));
    }
}
