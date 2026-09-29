//! Font maintenance: verify exact font bytes against recorded hashes.
//!
//! Pure-Rust SHA-256 only; no Python/fonttools. Byte changes require a
//! separately qualified `record` run plus review, never silent drift.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::sha256;
use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "fonts";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask fonts [verify | record]\n\
\n\
verify (default): hashes every assets/fonts/*.ttf|*.otf with pure-Rust\n\
SHA-256 and compares against crates/xtask/fonts.sha256. Any mismatch,\n\
missing entry, or unrecorded file fails.\n\
record: rewrites crates/xtask/fonts.sha256 from current bytes. Byte\n\
changes must be separately qualified and reviewed.\n\
";

/// Directory holding the upstream font binaries.
const FONT_DIR: &str = "assets/fonts";

/// Recorded-hashes file, coreutils `sha256sum` format.
const HASH_FILE: &str = "crates/xtask/fonts.sha256";

/// Verify or record font hashes.
///
/// # Errors
///
/// Returns an error on bad subcommand args or unreadable font/hash files.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    match args.first().map(String::as_str) {
        None | Some("verify") => {
            if args.len() > 1 {
                return Err(util::fail(format!(
                    "{NAME}: unexpected args: {}",
                    args.join(" ")
                )));
            }
            verify(root)
        }
        Some("record") => {
            if args.len() > 1 {
                return Err(util::fail(format!(
                    "{NAME}: unexpected args: {}",
                    args.join(" ")
                )));
            }
            record(root)
        }
        Some(other) => Err(util::fail(format!("{NAME}: unknown action '{other}'"))),
    }
}

/// Font binary paths under `assets/fonts/`, sorted.
fn font_files(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join(FONT_DIR);
    let entries =
        fs::read_dir(&dir).map_err(|e| util::fail(format!("read {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if entry.file_type()?.is_file() && matches!(ext, "ttf" | "otf") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Parse the recorded-hashes file into `filename -> hex` entries.
fn recorded(root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join(HASH_FILE);
    let lines = util::read_lines(&path)?;
    let mut map = BTreeMap::new();
    for line in &lines {
        let mut parts = line.split_whitespace();
        let (Some(hex), Some(name)) = (parts.next(), parts.next()) else {
            return Err(util::fail(format!("malformed hash line: {line}")));
        };
        map.insert(name.to_string(), hex.to_string());
    }
    Ok(map)
}

fn verify(root: &Path) -> Result<Status> {
    let fonts = font_files(root)?;
    let expected = recorded(root)?;
    let mut status = Status::Pass;
    for path in &fonts {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let bytes =
            fs::read(path).map_err(|e| util::fail(format!("read {}: {e}", path.display())))?;
        let actual = sha256::hexdigest(&bytes);
        match expected.get(name) {
            Some(want) if *want == actual => {
                println!("fonts: OK {name} ({bytes} bytes)", bytes = bytes.len());
            }
            Some(want) => {
                println!("fonts: FAIL {name} byte drift:\n  want {want}\n  have {actual}");
                status = Status::Fail;
            }
            None => {
                println!("fonts: FAIL {name} has no recorded hash");
                status = Status::Fail;
            }
        }
    }
    for name in expected.keys() {
        let known = fonts
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some(name.as_str()));
        if !known {
            println!("fonts: FAIL recorded {name} no longer present");
            status = Status::Fail;
        }
    }
    println!(
        "fonts: {}",
        if status == Status::Pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    Ok(status)
}

fn record(root: &Path) -> Result<Status> {
    let fonts = font_files(root)?;
    let mut lines = Vec::new();
    for path in &fonts {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let bytes =
            fs::read(path).map_err(|e| util::fail(format!("read {}: {e}", path.display())))?;
        lines.push(format!("{}  {name}", sha256::hexdigest(&bytes)));
    }
    lines.sort();
    let dest = root.join(HASH_FILE);
    let text = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    fs::write(&dest, &text).map_err(|e| util::fail(format!("write {}: {e}", dest.display())))?;
    println!("fonts: recorded {} hashes to {HASH_FILE}", lines.len());
    println!("fonts: review required: byte changes must be separately qualified");
    println!("fonts: PASS");
    Ok(Status::Pass)
}
