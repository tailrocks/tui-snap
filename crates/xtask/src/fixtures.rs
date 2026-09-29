//! Fixture builds/exports: compile fixtures once, resolve real artifacts.

use std::fs;
use std::path::{Path, PathBuf};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "fixtures";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask fixtures [--export DIR]\n\
\n\
builds tuiscotti-fixtures binaries and prints the authoritative artifact\n\
paths parsed from cargo --message-format=json (never first-found guesses).\n\
With --export DIR, copies each built executable into DIR.\n\
";

/// Build fixtures and resolve authoritative artifacts.
///
/// # Errors
///
/// Returns an error on bad flags, a failed build, or a failed export copy.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    let mut export: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--export" {
            let Some(dir) = args.get(i + 1) else {
                return Err(util::fail(format!("{NAME}: --export needs a directory")));
            };
            export = Some(dir.as_str());
            i += 2;
        } else {
            return Err(util::fail(format!("{NAME}: unexpected arg: {}", args[i])));
        }
    }
    let stdout = util::run_cargo(
        root,
        &[
            "build",
            "-p",
            "tuiscotti-fixtures",
            "--bins",
            "--message-format=json",
        ],
    )?;
    let artifacts = parse_artifacts(&stdout);
    if artifacts.is_empty() {
        println!("fixtures: FAIL tuiscotti-fixtures defines no binary targets");
        return Ok(Status::Fail);
    }
    for artifact in &artifacts {
        println!("fixtures: artifact {artifact}");
    }
    if let Some(dir) = export {
        export_artifacts(&artifacts, &root.join(dir))?;
    }
    println!("fixtures: PASS ({} binaries)", artifacts.len());
    Ok(Status::Pass)
}

/// Extract unique executable paths from cargo JSON `compiler-artifact` lines.
#[must_use]
pub fn parse_artifacts(stdout: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if !line.contains("compiler-artifact") {
            continue;
        }
        if let Some(exe) = executable_of(line)
            && !out.contains(&exe)
        {
            out.push(exe);
        }
    }
    out.sort();
    out
}

/// Read the `"executable"` value from one cargo JSON message line.
fn executable_of(line: &str) -> Option<String> {
    let key = line.find("\"executable\"")?;
    let after = line.get(key + "\"executable\"".len()..)?;
    let colon = after.find(':')?;
    let value = after.get(colon + 1..)?.trim_start();
    if !value.starts_with('"') {
        return None;
    }
    let mut text = String::new();
    let mut chars = value[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(text),
            '\\' => text.push(chars.next()?),
            _ => text.push(c),
        }
    }
    None
}

fn export_artifacts(artifacts: &[String], dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| util::fail(format!("create {}: {e}", dir.display())))?;
    for artifact in artifacts {
        let src = PathBuf::from(artifact);
        let Some(name) = src.file_name() else {
            return Err(util::fail(format!("artifact has no file name: {artifact}")));
        };
        let dest = dir.join(name);
        fs::copy(&src, &dest).map_err(|e| util::fail(format!("copy {}: {e}", dest.display())))?;
        println!("fixtures: exported {}", dest.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_artifacts;

    #[test]
    fn parses_executable_artifacts() {
        let stdout = concat!(
            "{\"reason\":\"compiler-artifact\",\"package_id\":\"x\",\"target\":{\"kind\":[\"bin\"]},",
            "\"profile\":{},\"features\":[],\"filenames\":[\"/t/a\"],\"executable\":\"/t/a\",\"fresh\":false}\n",
            "{\"reason\":\"compiler-artifact\",\"package_id\":\"y\",\"target\":{\"kind\":[\"rlib\"]},",
            "\"profile\":{},\"features\":[],\"filenames\":[\"/t/b.rlib\"],\"executable\":null,\"fresh\":true}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n",
        );
        assert_eq!(parse_artifacts(stdout), vec!["/t/a".to_string()]);
    }
}
