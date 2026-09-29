//! Docs/examples checks: stale script refs, relative links, example builds.

use std::path::{Path, PathBuf};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "docs";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask docs [--compile-examples]\n\
\n\
checks user-facing docs (README, AGENTS, CONTRIBUTING, USAGE, MIGRATION, CI):\n\
  - no removed-client install/run commands (pip/npm/node/python tooling)\n\
  - every relative markdown link target exists\n\
other docs/** research hits are notes, not failures. With\n\
--compile-examples, also runs cargo check --workspace --examples.\n\
";

/// Docs whose stale script references fail the check.
const ACTIVE_DOCS: &[&str] = &[
    "README.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "docs/USAGE.md",
    "docs/MIGRATION.md",
    "docs/CI.md",
];

/// Removed-client install/run tokens that must not appear in active docs.
const FORBIDDEN: &[&str] = &[
    "pip install",
    "pip3 install",
    "npm install",
    "npm i ",
    "npm run",
    "npx ",
    "node -e",
    "node ",
    "python -c",
    "python3 tools/",
    "python tools/",
    "python3 ",
    "pyproject.toml",
    "package.json",
];

/// Run the docs/examples checks.
///
/// # Errors
///
/// Returns an error on bad flags or when the example build fails to run.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    let mut compile_examples = false;
    for arg in args {
        if arg == "--compile-examples" {
            compile_examples = true;
        } else {
            return Err(util::fail(format!("{NAME}: unexpected arg: {arg}")));
        }
    }
    let mut status = Status::Pass;
    status = status.join(check_tokens(root)?);
    status = status.join(check_links(root)?);
    status = status.join(check_examples(root, compile_examples)?);
    println!(
        "docs: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in util::walk_files(root)? {
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(out)
}

fn check_tokens(root: &Path) -> Result<Status> {
    let mut status = Status::Pass;
    for path in markdown_files(root)? {
        let rel = util::display(root, &path);
        let active = ACTIVE_DOCS.contains(&rel.as_str());
        let Ok(lines) = util::read_lines(&path) else {
            continue;
        };
        for (index, line) in lines.iter().enumerate() {
            if let Some(token) = FORBIDDEN.iter().find(|t| line.contains(*t)) {
                if active {
                    println!("docs: FAIL {rel}:{}: {token}", index + 1);
                    status = Status::Fail;
                } else {
                    println!("docs: note {rel}:{}: {token}", index + 1);
                }
            }
        }
    }
    if status == Status::Pass {
        println!("docs: PASS no stale script refs in active docs");
    }
    Ok(status)
}

fn check_links(root: &Path) -> Result<Status> {
    let mut status = Status::Pass;
    for path in markdown_files(root)? {
        let rel = util::display(root, &path);
        let active = ACTIVE_DOCS.contains(&rel.as_str());
        let Ok(lines) = util::read_lines(&path) else {
            continue;
        };
        let Some(dir) = path.parent() else { continue };
        for (index, line) in lines.iter().enumerate() {
            for target in link_targets(line) {
                if !link_exists(root, dir, &target) {
                    if active {
                        println!("docs: FAIL {rel}:{}: missing {target}", index + 1);
                        status = Status::Fail;
                    } else {
                        println!("docs: note {rel}:{}: missing {target}", index + 1);
                    }
                }
            }
        }
    }
    if status == Status::Pass {
        println!("docs: PASS relative links resolve in active docs");
    }
    Ok(status)
}

/// Extract `](target)` link targets from one line.
fn link_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j] != b')' {
                j += 1;
            }
            if j < bytes.len() {
                if let Ok(target) = std::str::from_utf8(&bytes[i + 2..j]) {
                    out.push(target.to_string());
                }
                i = j;
            }
        }
        i += 1;
    }
    out
}

fn link_exists(root: &Path, dir: &Path, target: &str) -> bool {
    if target.contains("://") || target.starts_with("mailto:") {
        return true;
    }
    let path_part = target.split('#').next().unwrap_or("");
    if path_part.is_empty() {
        return true;
    }
    if let Some(rest) = path_part.strip_prefix('/') {
        return root.join(rest).exists();
    }
    dir.join(path_part).exists()
}

fn check_examples(root: &Path, compile: bool) -> Result<Status> {
    let mut count = 0_usize;
    for path in util::walk_files(root)? {
        let rel = util::display(root, &path);
        let is_rs = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
        let in_examples = rel.starts_with("examples/") || rel.contains("/examples/");
        if is_rs && in_examples {
            count += 1;
        }
    }
    if !compile {
        println!("docs: PASS examples listed ({count} files, build skipped)");
        return Ok(Status::Pass);
    }
    let output = util::run_cargo(root, &["check", "--workspace", "--examples"])?;
    let warnings = output.lines().filter(|l| l.contains("warning")).count();
    println!("docs: PASS examples compile ({count} files, {warnings} warnings)");
    Ok(Status::Pass)
}
