//! Workspace migration-shape checks (G3/G7): layout, manifests, independence.

use std::path::Path;

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "migrate";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask migrate\n\
\n\
checks the migrated workspace shape:\n\
  - root Cargo.toml lists exactly the 8 expected members and is virtual\n\
  - every member manifest inherits edition/rust-version/license and lints\n\
  - xtask has no product-graph dependencies (no tuiscotti/path deps)\n\
  - legacy first-party tools/ directory is gone\n\
";

/// Expected workspace members, relative to the root.
const MEMBERS: &[&str] = &[
    "crates/tuiscotti-core",
    "crates/tuiscotti-render",
    "crates/tuiscotti-runtime",
    "crates/tuiscotti-insta",
    "crates/tuiscotti",
    "crates/tuiscotti-cli",
    "crates/tuiscotti-fixtures",
    "crates/xtask",
];

/// Run the migration-shape checks.
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
    status = status.join(check_members(root)?);
    status = status.join(check_virtual(root)?);
    status = status.join(check_inheritance(root)?);
    status = status.join(check_xtask_independence(root)?);
    status = status.join(check_no_tools_dir(root));
    println!(
        "migrate: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn check_members(root: &Path) -> Result<Status> {
    let lines = util::read_lines(&root.join("Cargo.toml"))?;
    let mut in_members = false;
    let mut found: Vec<String> = Vec::new();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed.starts_with(']') {
                break;
            }
            let member = trimmed.trim_matches(|c| c == '"' || c == ',' || c == ' ');
            if !member.is_empty() {
                found.push(member.to_string());
            }
        }
    }
    let mut status = Status::Pass;
    for member in MEMBERS {
        if !found.iter().any(|f| f == member) {
            println!("migrate: FAIL member missing: {member}");
            status = Status::Fail;
        }
    }
    for member in &found {
        if !MEMBERS.contains(&member.as_str()) {
            println!("migrate: FAIL unexpected member: {member}");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("migrate: PASS members ({} expected)", MEMBERS.len());
    }
    Ok(status)
}

fn check_virtual(root: &Path) -> Result<Status> {
    let lines = util::read_lines(&root.join("Cargo.toml"))?;
    if lines.iter().any(|l| l.trim() == "[package]") {
        println!("migrate: FAIL root Cargo.toml is not a virtual manifest");
        return Ok(Status::Fail);
    }
    println!("migrate: PASS root manifest is virtual");
    Ok(Status::Pass)
}

fn check_inheritance(root: &Path) -> Result<Status> {
    let mut status = Status::Pass;
    for member in MEMBERS {
        let path = root.join(member).join("Cargo.toml");
        let rel = format!("{member}/Cargo.toml");
        let lines = util::read_lines(&path)?;
        for key in [
            "edition.workspace",
            "rust-version.workspace",
            "license.workspace",
        ] {
            if !lines.iter().any(|l| l.contains(key)) {
                println!("migrate: FAIL {rel} missing {key}");
                status = Status::Fail;
            }
        }
        if !has_lints_workspace(&lines) {
            println!("migrate: FAIL {rel} missing [lints] workspace = true");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!(
            "migrate: PASS manifest inheritance ({} members)",
            MEMBERS.len()
        );
    }
    Ok(status)
}

fn has_lints_workspace(lines: &[String]) -> bool {
    let mut section = String::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_string();
        } else if section == "[lints]" && trimmed == "workspace = true" {
            return true;
        }
    }
    false
}

fn check_xtask_independence(root: &Path) -> Result<Status> {
    let path = root.join("crates/xtask/Cargo.toml");
    let lines = util::read_lines(&path)?;
    let mut status = Status::Pass;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.contains("tuiscotti") && !trimmed.contains("name = ") {
            println!("migrate: FAIL xtask depends on product graph: {trimmed}");
            status = Status::Fail;
        }
        if trimmed.contains("path =") {
            println!("migrate: FAIL xtask uses a path dependency: {trimmed}");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("migrate: PASS xtask independent of product graph");
    }
    Ok(status)
}

fn check_no_tools_dir(root: &Path) -> Status {
    if root.join("tools").exists() {
        println!("migrate: FAIL legacy tools/ directory still present");
        return Status::Fail;
    }
    println!("migrate: PASS no legacy tools/ directory");
    Status::Pass
}
