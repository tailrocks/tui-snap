//! Performance collection: toolchain facts and optional build timings.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "perf";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask perf [--build-timings]\n\
\n\
writes a JSON perf report to target/xtask-perf/report-<unix>.json with\n\
toolchain versions, cpu count, member/file/lock counts. With\n\
--build-timings, also runs cargo build --workspace --timings and\n\
copies the newest cargo-timing report next to it.\n\
\n\
For the reproducible benchmark suite and budget scoreboard, run\n\
`cargo xtask bench` (results in benches/results/).\n\
";

/// Collect performance facts.
///
/// # Errors
///
/// Returns an error on bad flags, failed child runs, or unwritable reports.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    let mut build_timings = false;
    for arg in args {
        if arg == "--build-timings" {
            build_timings = true;
        } else {
            return Err(util::fail(format!("{NAME}: unexpected arg: {arg}")));
        }
    }
    let dir = root.join("target").join("xtask-perf");
    fs::create_dir_all(&dir).map_err(|e| util::fail(format!("create {}: {e}", dir.display())))?;
    let stamp = unix_secs();
    let report = collect(root)?;
    let path = dir.join(format!("report-{stamp}.json"));
    fs::write(&path, &report).map_err(|e| util::fail(format!("write {}: {e}", path.display())))?;
    println!("perf: wrote {}", util::display(root, &path));
    println!("{report}");
    if build_timings {
        copy_timings(root, &dir, stamp)?;
    }
    println!("perf: PASS");
    Ok(Status::Pass)
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn collect(root: &Path) -> Result<String> {
    let rustc = program_version("rustc")?;
    let cargo = program_version("cargo")?;
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
    let files = util::walk_files(root)?;
    let rs_files = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .count();
    let lock_packages = util::read_lines(&root.join("Cargo.lock"))?
        .iter()
        .filter(|l| l.trim() == "[[package]]")
        .count();
    Ok(format!(
        "{{\n  \"unix_secs\": {},\n  \"rustc\": \"{}\",\n  \"cargo\": \"{}\",\n  \
         \"cpus\": {cpus},\n  \"rs_files\": {rs_files},\n  \"lock_packages\": {lock_packages}\n}}\n",
        unix_secs(),
        json_escape(&rustc),
        json_escape(&cargo),
    ))
}

fn program_version(program: &str) -> Result<String> {
    let exe = OsString::from(program);
    let out = util::run_program(&std::env::temp_dir(), &exe, &["--version"])?;
    Ok(out.lines().next().unwrap_or("unknown").trim().to_string())
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn copy_timings(root: &Path, dir: &Path, stamp: u64) -> Result<()> {
    util::run_cargo(root, &["build", "--workspace", "--timings"])?;
    let timings_dir = root.join("target").join("cargo-timings");
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    let entries = fs::read_dir(&timings_dir)
        .map_err(|e| util::fail(format!("read {}: {e}", timings_dir.display())))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_timing = std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if !name.starts_with("cargo-timing-") || !is_timing {
            continue;
        }
        let modified = entry.metadata()?.modified().unwrap_or(UNIX_EPOCH);
        let replace = newest.as_ref().is_none_or(|(t, _)| modified > *t);
        if replace {
            newest = Some((modified, path));
        }
    }
    let Some((_, src)) = newest else {
        return Err(util::fail("no cargo-timing report produced"));
    };
    let dest = dir.join(format!("timing-{stamp}.json"));
    fs::copy(&src, &dest).map_err(|e| util::fail(format!("copy {}: {e}", dest.display())))?;
    println!("perf: wrote {}", util::display(root, &dest));
    Ok(())
}
