//! Source-policy checks: crates-only Rust, no foreign scripts, line limits.

use std::fs;
use std::path::Path;

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "policy";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask policy\n\
\n\
enforces source policies (mirrors .alint.yml intent):\n\
  - every first-party .rs file lives under crates/\n\
  - no first-party Python/JS/TS files or manifests\n\
  - no python/node shebangs in extensionless files or shell scripts\n\
  - .rs files stay within 400 lines (150 for src/lib.rs and src/main.rs)\n\
";

/// Forbidden first-party extensions (executable foreign code + manifests).
const FOREIGN_EXTS: &[&str] = &[
    "py", "pyi", "pyc", "ipynb", "js", "mjs", "cjs", "jsx", "ts", "tsx",
];

/// Forbidden first-party manifest/lock filenames.
const FOREIGN_FILES: &[&str] = &[
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "pyproject.toml",
    "uv.lock",
];

/// Ordinary-file line limit; roots (`src/lib.rs`, `src/main.rs`) use 150.
const MAX_LINES: usize = 400;
const MAX_ROOT_LINES: usize = 150;

/// Run the source-policy checks.
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
    status = status.join(check_crates_only(root, &files));
    status = status.join(check_foreign(root, &files));
    status = status.join(check_shebangs(root, &files));
    status = status.join(check_line_limits(root, &files)?);
    println!(
        "policy: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn check_crates_only(root: &Path, files: &[std::path::PathBuf]) -> Status {
    let mut status = Status::Pass;
    for path in files {
        let is_rs = path.extension().and_then(|e| e.to_str()) == Some("rs");
        if !is_rs {
            continue;
        }
        let rel = util::display(root, path);
        if !rel.starts_with("crates/") {
            println!("policy: FAIL rust outside crates/: {rel}");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("policy: PASS crates-only rust");
    }
    status
}

fn check_foreign(root: &Path, files: &[std::path::PathBuf]) -> Status {
    let mut status = Status::Pass;
    for path in files {
        let rel = util::display(root, path);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if FOREIGN_FILES.contains(&name) {
            println!("policy: FAIL foreign manifest: {rel}");
            status = Status::Fail;
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if FOREIGN_EXTS.contains(&ext) {
            println!("policy: FAIL foreign script: {rel}");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("policy: PASS no foreign scripts");
    }
    status
}

fn check_shebangs(root: &Path, files: &[std::path::PathBuf]) -> Status {
    let mut status = Status::Pass;
    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let candidate = !name.contains('.') || ext == "sh";
        if !candidate {
            continue;
        }
        let first = first_line(path);
        let Some(line) = first else { continue };
        if line.starts_with("#!") && (line.contains("python") || line.contains("node")) {
            println!(
                "policy: FAIL foreign shebang: {}",
                util::display(root, path)
            );
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("policy: PASS no foreign shebangs");
    }
    status
}

fn first_line(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0_u8; 512];
    let count = file.read(&mut buf).ok()?;
    let head = buf[..count].split(|b| *b == b'\n').next().unwrap_or(&[]);
    String::from_utf8(head.to_vec()).ok()
}

fn check_line_limits(root: &Path, files: &[std::path::PathBuf]) -> Result<Status> {
    let mut status = Status::Pass;
    let mut checked = 0_usize;
    for path in files {
        let is_rs = path.extension().and_then(|e| e.to_str()) == Some("rs");
        if !is_rs {
            continue;
        }
        let rel = util::display(root, path);
        if rel.contains("/fixtures/") || rel.contains("/testdata/") {
            continue;
        }
        checked += 1;
        let limit = if is_crate_root(&rel) {
            MAX_ROOT_LINES
        } else {
            MAX_LINES
        };
        let lines = util::count_lines(path)?;
        if lines > limit {
            println!("policy: FAIL {rel} has {lines} lines (limit {limit})");
            status = Status::Fail;
        }
    }
    if status == Status::Pass {
        println!("policy: PASS line limits ({checked} files)");
    }
    Ok(status)
}

fn is_crate_root(rel: &str) -> bool {
    rel.ends_with("/src/lib.rs") || rel.ends_with("/src/main.rs")
}
