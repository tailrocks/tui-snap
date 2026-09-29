//! Read-only compat importers (backlog A10).
//!
//! - [`import_cast`]: asciinema v2 `.cast` (also what our own
//!   [`export::cast_v2`](tuiscotti_render::export::cast_v2) writes, and what
//!   `microsoft/tui-test` emits — see finding below).
//! - [`import_termctrl`]: `anomalyco/terminal-control` versioned `.termctrl`
//!   JSON Lines recordings (schema v1 + v2).
//!
//! ## Competitor finding (2026-09-28, inspected in `/tmp` only)
//!
//! - `microsoft/tui-test` @ `7afb14b` has NO own trace format. Its
//!   `RecordingFormat` is `{Apng, Gif, Mp4, Cast}` (`crates/tui-test/src/api.rs`)
//!   and the `Cast` writer emits standard asciinema v2, already covered by
//!   [`import_cast`]. Nothing else to import; no format invented here.
//! - `anomalyco/terminal-control` @ `c1d4f95` HAS a versioned, schema-documented
//!   trace: `.termctrl` JSON Lines with `schemas/recording-entry-v{1,2}.schema.json`
//!   and `FORMAT_VERSION = 2` (`src/recording.rs`). Imported by
//!   [`import_termctrl`] with an explicit [`LossReport`].
//!
//! ## Guarantees
//!
//! - READ-ONLY: importers only `read` the source. They never write beside it
//!   and never execute anything recorded in it (commands, markers, titles are
//!   data; input bytes are marked non-executable and are never fed as output).
//! - OUTPUT-ONLY direction: [`CastTrace::screens_via`] /
//!   [`TermctrlTrace::screens_via`] feed caller replay functions with terminal
//!   OUTPUT bytes only. No emulator coupling lives here; the caller supplies
//!   replay.
//! - BOUNDED: [`ImportLimits`] caps line length, event count, and total bytes.
//!   Violations fail with [`CompatError::TooLarge`], never truncation.
//! - Errors carry byte offsets: header problems are
//!   [`CompatError::Version`], bad event lines [`CompatError::Content`].

mod errors;
mod cast;
mod termctrl_types;
mod termctrl_parse;

pub use errors::*;
pub use cast::*;
pub use termctrl_types::*;
pub use termctrl_parse::*;
