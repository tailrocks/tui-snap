//! Named session ops (start/stop/input/observe/list/prune).
//!
//! Split into one module per op so each file stays under the repo line
//! gate; behavior is unchanged.

use std::path::PathBuf;

use super::{OpError, checked_aux_path, runtime_dir};

mod input;
mod list;
mod prune;
mod start;
mod stop;

pub use input::{session_input, session_input_bytes, session_observe};
pub use list::session_list;
pub use prune::session_prune;
pub use start::{session_start, session_start_os, session_start_pty};
pub use stop::session_stop;

/// Containment-checked session log path for `session attach`.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name or an unusable runtime dir.
pub fn session_log_path(name: &str) -> Result<PathBuf, OpError> {
    let dir = runtime_dir()?;
    checked_aux_path(&dir, name, "log")
}
