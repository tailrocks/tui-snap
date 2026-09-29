//! Branding inspection: old-brand scan over active paths.

use std::path::{Path, PathBuf};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "brand";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask brand\n\
\n\
scans active paths (crates/, root configs, README, examples) for old-brand\n\
tokens (tui-snap spellings). Historical docs, font notices, .github, and the\n\
lockfile are out of scope: docs/ research may cite history, and .github is\n\
velnor-owned. Repository-identity strings (the tailrocks/tui-snap GitHub\n\
path) are not brand usage and are scrubbed before matching. Any other hit\n\
in active paths fails.\n\
";

/// Old-brand spellings that must not appear in active paths.
const OLD_TOKENS: &[&str] = &[
    "tui-snap", "tuisnap", "TuiSnap", "tuiSnap", "tui_snap", "TUI_SNAP",
];

/// This module's own token table is the one allowed self-match.
const SELF_FILE: &str = "crates/xtask/src/brand.rs";

/// Repository-identity strings: the code brand moved to tuiscotti while
/// the repo still lives at this GitHub path, so these are scrubbed before
/// matching (a CODEOWNERS owner proof is not brand usage).
const REPO_IDENTITY: &[&str] = &["tailrocks/tui-snap", "donbeave/tui-snap"];

/// Max hits printed before truncation.
const MAX_HITS: usize = 50;

/// Run the branding inspection.
///
/// # Errors
///
/// Returns an error on unexpected args or unreadable files.
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
    let files = util::walk_files(root)?;
    let mut status = Status::Pass;
    status = status.join(check_members_brand(root));
    status = status.join(scan_active(root, &files));
    println!(
        "brand: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn check_members_brand(root: &Path) -> Status {
    let path = root.join("Cargo.toml");
    let lines = util::read_lines(&path).unwrap_or_default();
    let mut status = Status::Pass;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("\"crates/") {
            let member = trimmed.trim_matches(|c| c == '"' || c == ',' || c == ' ');
            let ok = member.starts_with("crates/tuiscotti") || member == "crates/xtask";
            if !ok {
                println!("brand: FAIL non-tuiscotti member: {member}");
                status = Status::Fail;
            }
        }
    }
    if status == Status::Pass {
        println!("brand: PASS workspace members use tuiscotti names");
    }
    status
}

fn scan_active(root: &Path, files: &[PathBuf]) -> Status {
    let mut hits: Vec<String> = Vec::new();
    for path in files {
        let rel = util::display(root, path);
        if !is_active(&rel) {
            continue;
        }
        let Ok(lines) = util::read_lines(path) else {
            continue;
        };
        for (index, line) in lines.iter().enumerate() {
            let scrubbed = scrub_repo_identity(line);
            if let Some(token) = OLD_TOKENS.iter().find(|t| scrubbed.contains(*t)) {
                hits.push(format!("{}:{}: {token}", rel, index + 1));
            }
        }
    }
    if hits.is_empty() {
        println!("brand: PASS no old-brand tokens in active paths");
        return Status::Pass;
    }
    for hit in hits.iter().take(MAX_HITS) {
        println!("brand: FAIL {hit}");
    }
    if hits.len() > MAX_HITS {
        println!("brand: FAIL ... and {} more", hits.len() - MAX_HITS);
    }
    Status::Fail
}

/// Remove repository-identity substrings before old-brand matching.
fn scrub_repo_identity(line: &str) -> String {
    let mut out = line.to_string();
    for id in REPO_IDENTITY {
        out = out.replace(id, "");
    }
    out
}

/// Active paths are everything except documented historical/foreign scopes.
fn is_active(rel: &str) -> bool {
    if rel == SELF_FILE {
        return false;
    }
    if rel.starts_with("docs/")
        || rel.starts_with("assets/fonts/")
        || rel.starts_with(".github/")
        || rel.starts_with(".github-gen/")
    {
        return false;
    }
    !matches!(
        rel,
        "Cargo.lock" | "IMPLEMENTATION-GOAL.md" | "IMPLEMENTATION-GOAL.txt" | "REFERENCE-SPEC.md"
    )
}

#[cfg(test)]
mod tests {
    use super::{OLD_TOKENS, scrub_repo_identity};

    #[test]
    fn repo_identity_scrubs_but_brand_still_matches() {
        let scrubbed = scrub_repo_identity("# committer of all 82 commits on tailrocks/tui-snap");
        assert!(
            OLD_TOKENS.iter().all(|t| !scrubbed.contains(t)),
            "repo path must not read as brand usage: {scrubbed}"
        );
        let kept = scrub_repo_identity("binary `tuisnap` (from `tuiscotti-cli`)");
        assert!(
            OLD_TOKENS.iter().any(|t| kept.contains(t)),
            "real brand usage must still match: {kept}"
        );
    }
}
