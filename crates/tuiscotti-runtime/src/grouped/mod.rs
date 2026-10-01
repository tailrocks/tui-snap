//! Grouped multi-artifact snapshot store: one directory tree per suite,
//! nested scenario names, four committed artifacts per scenario.
//!
//! A scenario name is a slash-separated path like
//! `showcase/pages/overview_120x40_truecolor`. Each scenario commits exactly
//! these four artifacts under the approved root:
//!
//! ```text
//! <approved>/<name>.ansi   colored terminal text (normalized SGR dump)
//! <approved>/<name>.txt    plain black-and-white text
//! <approved>/<name>.png    colored image (authoritative pixel gate)
//! <approved>/<name>.html   standalone colored HTML render
//! ```
//!
//! The approved tree holds NOTHING else — no `.frame.json`, no `.cursor`
//! sidecars. Scratch state lives outside the approved root:
//!
//! ```text
//! <actual>/<name>.{ansi,txt,png,html}   latest capture (written BEFORE any assertion)
//! <actual>/<name>.frame.json            debug sidecar (report re-verification)
//! <actual>/<name>.png.fidelity.json     missing-glyph sidecar
//! <actual>/<name>.manifest.json         candidate seal (hashes, profile, complete)
//! <actual>/<name>.verdict.json          persisted check verdict (report reuses)
//! <actual>/report.html                  review index (file links, not embeds)
//! <diff>/<name>.png                     red-overlay diff, on mismatch
//! ```
//!
//! Gates ([`GroupedStore::check_with`]):
//! - `.ansi` byte-compare — the cell-exact gate (symbol + fg + bg + mods per
//!   cell, deterministic);
//! - `.txt` byte-compare — content gate (a style-only change shows as
//!   ansi=false/txt=true);
//! - `.html` byte-compare — the render-level gate (identical cells with a
//!   changed renderer/font fail here; the embedded frame JSON normalizes the
//!   provenance timestamp, see [`tuiscotti_render::render::Renderer::render_html`]);
//! - `.png` decoded-pixel compare with the same threshold semantics as
//!   [`crate::snapshot::Store::check_with`] — dimensions must match exactly,
//!   `score >= pixel_threshold` to pass, diff PNG written on any
//!   sub-1.0 score.
//!
//! Statuses reuse [`Status`](crate::snapshot::Status): cell-gate
//! failures read as
//! [`Status::CellsDiffer`](crate::snapshot::Status::CellsDiffer),
//! render-level/pixel failures as
//! [`Status::PixelsDiffer`](crate::snapshot::Status::PixelsDiffer), any
//! missing approved artifact as
//! [`Status::MissingApproval`](crate::snapshot::Status::MissingApproval)
//! (fail-closed, never silently). Approvals
//! change solely through explicit [`GroupedStore::accept`] /
//! [`GroupedStore::accept_all`] — there is no env-var auto-bless.
//!
//! Reports never re-evaluate with their own rule (C05): every check seals a
//! `<name>.manifest.json` (candidate completeness) and a
//! `<name>.verdict.json` (the one persisted verdict: status, pixel policy,
//! artifact hashes, checks performed). [`GroupedStore::report_with`] reuses a
//! verdict only when its hashes still match the artifacts on disk and the
//! threshold matches; anything stale is recomputed via `check`, never
//! silently reused — so report status equals check status on the same inputs.
//! A candidate with a missing manifest member (e.g. frame present but PNG
//! write lost) reports
//! [`Status::MissingApproval`](crate::snapshot::Status::MissingApproval)
//! (C08-grouped), never a pixel verdict or a pass.
//!
//! Default roots for an approved root `snapshots/`: actual `snapshots.actual/`,
//! diff `snapshots.diff/`, report `snapshots.actual/report.html` — siblings,
//! so the committed tree stays clean. Override with
//! [`GroupedStore::with_actual_root`], [`GroupedStore::with_diff_root`] and
//! [`GroupedStore::with_report_path`] (e.g. under `target/`).

mod check;
mod check_gates;
mod mutate;
mod seal;
mod types;

pub(crate) use check::*;
pub(crate) use check_gates::*;
pub use types::*;

pub use tuiscotti_core::names::{InvalidName, validate_name};
