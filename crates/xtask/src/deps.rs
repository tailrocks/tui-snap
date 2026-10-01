//! Dependency inspection: banned deps, git sources, wildcards, lockfile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "deps";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask deps\n\
\n\
inspects manifests and Cargo.lock:\n\
  - removed-backend deps (portable-pty, alacritty_terminal, libc) fail\n\
    everywhere (G1 swap landed; no temporary holder remains)\n\
  - git sources, wildcard versions, and [patch] sections fail\n\
  - lockfile git sources fail; duplicate locked versions warn\n\
";

/// Backend deps removed by G1; no temporary holder remains after the swap.
const BANNED: &[&str] = &["portable-pty", "alacritty_terminal", "libc"];

/// Run the dependency inspection.
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
    let manifests = member_manifests(root);
    let mut status = Status::Pass;
    for manifest in &manifests {
        status = status.join(check_manifest(root, manifest)?);
    }
    status = status.join(check_lockfile(root)?);
    println!(
        "deps: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn member_manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("Cargo.toml")];
    for member in [
        "tuiscotti-core",
        "tuiscotti-render",
        "tuiscotti-runtime",
        "tuiscotti-insta",
        "tuiscotti",
        "tuiscotti-cli",
        "tuiscotti-fixtures",
        "xtask",
    ] {
        out.push(root.join("crates").join(member).join("Cargo.toml"));
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

fn check_manifest(root: &Path, path: &Path) -> Result<Status> {
    let lines = util::read_lines(path)?;
    let rel = util::display(root, path);
    let mut status = Status::Pass;
    let mut section = String::new();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_string();
            if trimmed.starts_with("[patch") {
                println!("deps: FAIL {rel} has [patch] section");
                status = Status::Fail;
            }
            if let Some(name) = table_dep_name(trimmed) {
                status = status.join(check_dep(&rel, &name));
            }
            continue;
        }
        if trimmed.contains("git =") {
            println!("deps: FAIL {rel} has git source: {trimmed}");
            status = Status::Fail;
        }
        if trimmed.contains("version") && trimmed.contains("\"*\"") {
            println!("deps: FAIL {rel} has wildcard version: {trimmed}");
            status = Status::Fail;
        }
        if is_dep_section(&section)
            && let Some(name) = inline_dep_name(trimmed)
        {
            status = status.join(check_dep(&rel, &name));
        }
    }
    if status == Status::Pass {
        println!("deps: PASS {rel}");
    }
    Ok(status)
}

/// Dependency name from a `[dependencies.name]`-style table header.
fn table_dep_name(header: &str) -> Option<String> {
    let inner = header.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.rsplit('.');
    let name = parts.next()?;
    let parent: Vec<&str> = parts.collect();
    let in_deps = parent.iter().any(|p| {
        matches!(
            *p,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    });
    if in_deps && !name.contains(' ') && !name.starts_with('"') {
        return Some(name.to_string());
    }
    None
}

/// Dependency name from an inline `name = ...` line.
fn inline_dep_name(trimmed: &str) -> Option<String> {
    let (name, _) = trimmed.split_once('=')?;
    let name = name.trim().trim_matches('"');
    if name.is_empty() || name.contains(' ') || name.contains('.') {
        return None;
    }
    Some(name.to_string())
}

fn is_dep_section(section: &str) -> bool {
    matches!(
        section,
        "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
    ) || (section.starts_with("[target.") && section.contains("dependencies]"))
}

fn check_dep(rel: &str, name: &str) -> Status {
    if !BANNED.contains(&name) {
        return Status::Pass;
    }
    println!("deps: FAIL {rel} directly depends on banned {name}");
    Status::Fail
}

fn check_lockfile(root: &Path) -> Result<Status> {
    let path = root.join("Cargo.lock");
    let lines = util::read_lines(&path)?;
    let mut status = Status::Pass;
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.contains("git+") {
            println!("deps: FAIL Cargo.lock has git source: {trimmed}");
            status = Status::Fail;
        }
        if let Some(name) = trimmed.strip_prefix("name = ") {
            current = Some(name.trim_matches('"').to_string());
        } else if let Some(version) = trimmed.strip_prefix("version = ")
            && let Some(name) = current.take()
        {
            versions
                .entry(name)
                .or_default()
                .push(version.trim_matches('"').to_string());
        }
    }
    let mut dupes = 0_usize;
    for (name, mut vers) in versions {
        vers.sort();
        vers.dedup();
        if vers.len() > 1 {
            println!(
                "deps: WARN {name} locked {} versions: {}",
                vers.len(),
                vers.join(", ")
            );
            dupes += 1;
        }
    }
    if status == Status::Pass {
        println!(
            "deps: PASS Cargo.lock (no git sources, {dupe_note})",
            dupe_note = if dupes == 0 {
                "no duplicate versions".to_string()
            } else {
                format!("{dupes} duplicate versions warned")
            }
        );
    }
    Ok(status)
}
