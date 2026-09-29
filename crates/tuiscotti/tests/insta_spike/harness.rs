use super::fixtures::*;
use super::*;
use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use tuiscotti::Screen;
use tuiscotti::diff::AlphaPolicy;
use tuiscotti::insta_proto::{PngPixelComparator, insta_string};

/// Pending-dependent simulations (accept/reject/interrupted) need failing
/// assertions to write `.snap.new` pendings AND fail — the `new` behavior —
/// while approvals stay byte-identical (asserted in the reject test, so
/// accidental blessing is still impossible). `INSTA_UPDATE` is ambient-only
/// (`set_var` is an `unsafe fn` in edition 2024), so guard callers skip unless
/// the effective mode writes pendings (see `common`).
pub(crate) fn require_pending_mode() -> bool {
    if common::insta_writes_new_files() {
        return true;
    }
    eprintln!("skip: ambient INSTA_UPDATE does not write `.snap.new` pendings");
    false
}

pub(crate) fn settings_for(dir: &Path, generation: &str) -> insta::Settings {
    let mut s = insta::Settings::new();
    s.set_snapshot_path(dir);
    s.set_prepend_module_to_snapshot(false);
    s.set_description(format!("tuiscotti generation {generation}"));
    s
}

pub(crate) fn payload_to_string(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}

/// Run a canonical-text assertion; Ok = insta passed, Err = insta failed.
pub(crate) fn run_canonical(
    dir: &Path,
    name: &str,
    screen: &Screen,
    generation: &str,
) -> Result<(), String> {
    let settings = settings_for(dir, generation);
    let name = name.to_string();
    let text = insta_string(screen);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings.bind(|| {
            insta::assert_snapshot!(name, text);
        });
    })) {
        Ok(()) => Ok(()),
        Err(p) => Err(payload_to_string(&*p)),
    }
}

/// Run a PNG assertion under the decoded-pixel comparator.
pub(crate) fn run_png(
    dir: &Path,
    name: &str,
    png: Vec<u8>,
    generation: &str,
    alpha: AlphaPolicy,
) -> Result<(), String> {
    let mut settings = settings_for(dir, generation);
    settings.set_comparator(Box::new(PngPixelComparator::new(alpha)));
    let full = format!("{name}.png");
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings.bind(|| {
            insta::assert_binary_snapshot!(full.as_str(), png);
        });
    })) {
        Ok(()) => Ok(()),
        Err(p) => Err(payload_to_string(&*p)),
    }
}

pub(crate) fn write_text_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!(
        "---\nsource: tests/insta_spike.rs\ndescription: tuiscotti generation {generation}\nexpression: insta_string\n---\n{body}"
    );
    Ok(fs::write(dir.join(format!("{name}.snap")), content)?)
}

pub(crate) fn write_binary_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    sidecar: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let meta = format!(
        "---\nsource: tests/insta_spike.rs\ndescription: tuiscotti generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    Ok(fs::write(dir.join(format!("{name}.snap.png")), sidecar)?)
}

pub(crate) fn snap_description(snap_path: &Path) -> Option<String> {
    let text = fs::read_to_string(snap_path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("description:") {
            let v = v.trim().trim_matches('"');
            return v
                .strip_prefix("tuiscotti generation ")
                .map(ToString::to_string);
        }
    }
    None
}

/// Compound consistency gate (I04/C08): the approved canonical `.snap`, the
/// approved PNG `.snap`, and the PNG sidecar bytes must all carry the SAME
/// generation. Any mismatch (or missing binding) is an error.
pub(crate) fn check_consistent(dir: &Path, canonical: &str, png: &str) -> Result<(), String> {
    let c = snap_description(&dir.join(format!("{canonical}.snap")))
        .ok_or_else(|| format!("{canonical}.snap: missing generation binding"))?;
    let p = snap_description(&dir.join(format!("{png}.snap")))
        .ok_or_else(|| format!("{png}.snap: missing generation binding"))?;
    let sidecar = fs::read(dir.join(format!("{png}.snap.png")))
        .map_err(|e| format!("{png}.snap.png unreadable: {e}"))?;
    let t = png_find_text(&sidecar, PNG_GEN_KEYWORD)
        .ok_or_else(|| format!("{png}.snap.png: missing tEXt generation"))?;
    if c == p && p == t {
        Ok(())
    } else {
        Err(format!(
            "mixed compound baseline: canonical={c} png-meta={p} png-bytes={t}"
        ))
    }
}

/// Simulate `cargo insta accept` for ONE artifact: rename `.snap.new` (plus
/// binary sidecar `.snap.new.png`) into place. Refuses incomplete binary
/// pendings (torn write): metadata without pixels is never blessed.
pub(crate) fn accept_sim(dir: &Path, base: &str) -> Result<(), String> {
    let new = dir.join(format!("{base}.snap.new"));
    if !new.exists() {
        return Err(format!("{base}: no pending .snap.new to accept"));
    }
    let meta = fs::read_to_string(&new).map_err(|e| format!("{base}: {e}"))?;
    let is_binary = meta.lines().any(|l| l.trim() == "snapshot_kind: binary");
    let sidecar_new = dir.join(format!("{base}.snap.new.png"));
    if is_binary && !sidecar_new.exists() {
        return Err(format!(
            "{base}: incomplete binary pending (sidecar missing), refusing accept"
        ));
    }
    fs::rename(&new, dir.join(format!("{base}.snap"))).map_err(|e| format!("{base}: {e}"))?;
    if sidecar_new.exists() {
        fs::rename(&sidecar_new, dir.join(format!("{base}.snap.png")))
            .map_err(|e| format!("{base}: {e}"))?;
    }
    Ok(())
}

/// Simulate `cargo insta reject` for ONE artifact.
pub(crate) fn reject_sim(dir: &Path, base: &str) {
    drop(fs::remove_file(dir.join(format!("{base}.snap.new"))));
    drop(fs::remove_file(dir.join(format!("{base}.snap.new.png"))));
}

/// Copy approved state to fresh names (re-run phases; see header).
pub(crate) fn copy_approved(
    dir: &Path,
    from: &str,
    to: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::copy(
        dir.join(format!("{from}.snap")),
        dir.join(format!("{to}.snap")),
    )?;
    let sidecar = dir.join(format!("{from}.snap.png"));
    if sidecar.exists() {
        fs::copy(sidecar, dir.join(format!("{to}.snap.png")))?;
    }
    Ok(())
}

pub(crate) fn fresh_dir(
    test: &str,
) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("spike-{test}-"))
        .tempdir()?;
    let dir = tmp.path().join("snaps");
    fs::create_dir(&dir)?;
    Ok((tmp, dir))
}
