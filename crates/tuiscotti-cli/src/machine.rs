//! Explicit machine interface: `tuisnap machine < ops.jsonl`.
//!
//! Op JSON per line on stdin, one envelope JSON per line on stdout. Exit 0
//! when every op succeeded, else [`EXIT_OP_ERROR`]. This subcommand replaces
//! the old hidden `--machine` pre-scan: machine mode is now a documented,
//! discoverable (`--help`-listed) part of the CLI grammar, and the parent
//! parser can no longer mistake a child's `--machine` argument for its own.

use std::io::BufRead;

use tuiscotti::proto::{self, EXIT_OP_ERROR};

/// Run machine mode over stdio; return the process exit code.
pub fn machine_main() -> i32 {
    let stdin = std::io::stdin();
    let mut all_ok = true;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin: {e}");
                return EXIT_OP_ERROR;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let (out, ok) = proto::run_machine_line(&line);
        if let Some(code) = crate::write_line(&out) {
            return code;
        }
        all_ok &= ok;
    }
    if all_ok { 0 } else { EXIT_OP_ERROR }
}
