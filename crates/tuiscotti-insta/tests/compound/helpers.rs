//! Shared fixtures for the compound approval tests.

use std::fs;
use std::path::Path;

use tuiscotti_core::screen::{Screen, ScreenError};
use tuiscotti_insta::assert::{generation_id, png_generation, render_sample};

pub(super) fn styled_screen() -> Result<Screen, ScreenError> {
    use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods};
    let cells = vec![
        Cell {
            x: 0,
            y: 0,
            symbol: "A".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Indexed(1),
            bg: Color::Default,
            mods: Mods {
                bold: true,
                ..Mods::default()
            },
            underline_color: Color::Default,
        },
        Cell {
            x: 1,
            y: 0,
            symbol: "b".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: Mods::default(),
            underline_color: Color::Default,
        },
    ];
    Screen::validate(
        2,
        1,
        0,
        0,
        cells,
        Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
            blinking: false,
        },
    )
}

pub(super) fn write_text_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    body: &str,
) -> std::io::Result<()> {
    let content = format!(
        "---\nsource: tests/compound.rs\ndescription: tuiscotti generation {generation}\nexpression: canonical\n---\n{body}"
    );
    fs::write(dir.join(format!("{name}.snap")), content)
}

pub(super) fn write_binary_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    sidecar: &[u8],
) -> std::io::Result<()> {
    let meta = format!(
        "---\nsource: tests/compound.rs\ndescription: tuiscotti generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    fs::write(dir.join(format!("{name}.snap.png")), sidecar)
}

/// Whether Insta would bless in place (then the failure-ordering test has no
/// failure to order). Mirrors `tuiscotti/tests/common` logic.
pub(super) fn insta_updates_in_place() -> bool {
    matches!(
        std::env::var("INSTA_UPDATE").ok().as_deref(),
        Some("always" | "1" | "unseen" | "force")
    )
}

/// Whether Insta writes `.snap.new` pendings (and fails) on mismatch.
/// Mirrors `tuiscotti/tests/common` + insta 1.48 resolution.
pub(super) fn insta_writes_new_files() -> bool {
    match std::env::var("INSTA_UPDATE").ok().as_deref() {
        Some("new") => true,
        Some("auto" | "") | None => !is_ci(),
        Some(_) => false,
    }
}

pub(super) fn is_ci() -> bool {
    match std::env::var("CI").ok().as_deref() {
        Some("false" | "0" | "") => false,
        None => std::env::var("TF_BUILD").is_ok(),
        Some(_) => true,
    }
}

/// Recursively collect files under `dir` as `(relative, absolute)` pairs.
pub(super) fn collect_files(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries: Vec<std::path::PathBuf> = fs::read_dir(&d)
            .map_err(|e| format!("read {}: {e}", d.display()))?
            .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|e| format!("strip {}: {e}", path.display()))?
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The single published bundle directory under `evidence` (exactly one
/// `complete.json` must exist).
pub(super) fn single_bundle(evidence: &Path) -> Result<std::path::PathBuf, String> {
    let files = collect_files(evidence)?;
    let completes: Vec<_> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with("/complete.json"))
        .collect();
    assert_eq!(
        completes.len(),
        1,
        "exactly one complete bundle expected, files: {:?}",
        files.iter().map(|(r, _)| r).collect::<Vec<_>>()
    );
    completes[0]
        .1
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "bundle parent".to_string())
}

/// Assert the bundle at `bundle` carries the sample rendered from `screen`
/// under `scenario`/`png_stem`, with a manifest binding matching the tagged
/// image. Returns the manifest binding for approval crafting.
pub(super) fn assert_bundle_matches_sample(
    bundle: &Path,
    screen: &Screen,
    scenario: &str,
    png_stem: &str,
) -> Result<String, String> {
    for name in [
        "canonical.txt",
        "image.png",
        "sample.ansi",
        "sample.txt",
        "sample.html",
        "manifest.json",
        "complete.json",
    ] {
        assert!(bundle.join(name).is_file(), "missing bundle file {name}");
    }
    let sample = render_sample(screen).map_err(|e| e.to_string())?;
    assert_eq!(
        fs::read_to_string(bundle.join("canonical.txt")).map_err(|e| e.to_string())?,
        sample.canonical
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.ansi")).map_err(|e| e.to_string())?,
        sample.ansi
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.txt")).map_err(|e| e.to_string())?,
        sample.txt
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.html")).map_err(|e| e.to_string())?,
        sample.html
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(manifest["schema"], "tuiscotti-evidence-bundle/1");
    let binding = manifest["binding"]
        .as_str()
        .ok_or_else(|| "manifest binding".to_string())?
        .to_string();
    assert!(binding.starts_with("v2-"), "{binding}");
    assert_eq!(
        manifest["generation"].as_str(),
        Some(generation_id(&sample.canonical).as_str())
    );
    let image = fs::read(bundle.join("image.png")).map_err(|e| e.to_string())?;
    assert_eq!(png_generation(&image).as_deref(), Some(binding.as_str()));
    assert_eq!(manifest["snapshot"]["canonical"].as_str(), Some(scenario));
    assert_eq!(manifest["snapshot"]["png"].as_str(), Some(png_stem));
    Ok(binding)
}

pub(super) fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}
