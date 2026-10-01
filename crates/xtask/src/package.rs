//! Packaging dry-run: metadata plus `cargo package --list` per member.

use std::path::{Path, PathBuf};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "package";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask package\n\
\n\
for every publishable member (no publish = false): verifies description\n\
metadata exists, runs cargo package --list (online: fresh runners have no\n\
cargo cache), and refuses junk paths in the packaged set (target/ output,\n\
.DS_Store, *.swp, *.snap.new). Any failure, junk path, or missing\n\
metadata fails.\n\
";

/// Run the packaging dry-run.
///
/// # Errors
///
/// Returns an error on unexpected args or unreadable manifests.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    if !args.is_empty() {
        return Err(util::fail(format!(
            "{NAME}: unexpected args: {}",
            args.join(" ")
        )));
    }
    let mut status = Status::Pass;
    for manifest in publishable_manifests(root)? {
        status = status.join(check_member(root, &manifest)?);
    }
    println!(
        "package: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn member_manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return out;
    };
    for entry in entries.flatten() {
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            out.push(manifest);
        }
    }
    out.sort();
    out
}

fn publishable_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for manifest in member_manifests(root) {
        let lines = util::read_lines(&manifest)?;
        let unpublished = lines.iter().any(|l| l.trim() == "publish = false");
        if unpublished {
            println!(
                "package: skip {} (publish = false)",
                util::display(root, &manifest)
            );
        } else {
            out.push(manifest);
        }
    }
    Ok(out)
}

fn check_member(root: &Path, manifest: &Path) -> Result<Status> {
    let lines = util::read_lines(manifest)?;
    let rel = util::display(root, manifest);
    let mut status = Status::Pass;
    if !lines.iter().any(|l| l.trim().starts_with("description")) {
        println!("package: FAIL {rel} missing description metadata");
        status = Status::Fail;
    }
    let Some(name) = package_name(&lines) else {
        println!("package: FAIL {rel} has no [package] name");
        return Ok(Status::Fail);
    };
    match util::run_cargo(root, &["package", "--list", "--allow-dirty", "-p", &name]) {
        Ok(list) => {
            let junk: Vec<&str> = list.lines().filter(|p| is_junk(p)).collect();
            if junk.is_empty() {
                println!("package: PASS {name} ({} files)", list.lines().count());
            } else {
                for path in junk {
                    println!("package: FAIL {name} ships junk path: {path}");
                }
                status = Status::Fail;
            }
        }
        Err(err) => {
            println!("package: FAIL {name}: {err}");
            status = Status::Fail;
        }
    }
    Ok(status)
}

/// Junk that must never ship inside a published crate.
///
/// Cargo's default include set is exactly the tracked source (verified: every
/// tracked file ships, nothing else does), so manifests carry no
/// include/exclude lists and this deny-list is the wiring that keeps it that
/// way. `Cargo.toml.orig` / `.cargo_vcs_info.json` are cargo-generated and
/// legitimate, so only real stray classes are refused.
fn is_junk(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("target/")
        || base == ".DS_Store"
        || Path::new(base)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("swp"))
        || base.ends_with(".snap.new")
}

/// Package name from the `[package]` section of a manifest.
fn package_name(lines: &[String]) -> Option<String> {
    let mut section = String::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_string();
        } else if section == "[package]"
            && let Some(rest) = trimmed.strip_prefix("name")
        {
            let value = rest.trim().strip_prefix('=')?.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
