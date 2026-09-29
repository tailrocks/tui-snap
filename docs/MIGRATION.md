# Migration

No shims, no aliases, no deprecation periods: removed items stay
removed. This file records what changed and what to do about it.

## Current layout (head `0f14262`)

The workspace moved from a single crate (`src/`, `tests/`,
`examples/`) to `crates/`:

| Before | After |
|---|---|
| `src/*` (crate `tuisnap`) | `crates/tuiscotti-{core,render,runtime,insta}/src/*` (facade `tuiscotti`) |
| `src/main.rs` (binary `tuisnap`) | `crates/tuiscotti-cli/src/main.rs` (same binary name) |
| `tests/visual/approved` | `crates/tuiscotti-fixtures/tests/visual/approved` |
| `tests/fixtures/*` | `crates/tuiscotti-fixtures/tests/fixtures/*` |
| `tests/snapshots/*` | `crates/tuiscotti-fixtures/tests/snapshots/*` |
| `examples/01–08` | `crates/tuiscotti/examples/01–08` |
| `tuisnap::…` paths | `tuiscotti::…` paths (binary and `tui-snap.toml` unchanged) |

Rust imports change (`tuisnap::` → `tuiscotti::`); the `tuisnap`
binary name, `tui-snap.toml`, and CLI grammar are unchanged.

## Schema versions

- `Frame` is versioned; current `FRAME_VERSION = 3` (`UnderlineStyle`
  + underline color canonical; blink frozen-visible with slow/rapid
  combined; hidden is conceal; overline dropped). Version-2 JSON
  does not import — re-capture.
- `Frame::digest` mixes the schema version and the cursor; old
  digests are incomparable by design.
- Grouped scenarios have no `.frame.json`: the `.ansi` dump is the
  cell-exact gate; renderer changes are caught by the `.html` byte
  gate plus the PNG pixel gate.

## Removed surfaces (do not look for them)

| Removed | Replacement |
|---|---|
| `BLESS=1` / `UPDATE_SNAPSHOT=1` ambient approval | Explicit `Store::accept` / `GroupedStore::accept` / `tuisnap accept` / `cargo insta review` |
| CLI `check`, `run`, `digest`; `accept --all`; `report --store`; `render` of raw `*.ansi` | Current grammar in [CLI.md](CLI.md); scriptable surface is `tuisnap machine` + `proto::execute` |
| `tuisnap::ratatui_shot::{widget_frame, draw_frame}` | `tuiscotti::ratatui::{widget_frame, draw_frame, capture}` (signatures take `Provenance`) |
| `tuisnap::Baseline` | `tuiscotti::snapshot::Store` / `grouped::GroupedStore` |
| `PtySession` / `run_once(argv, opts, sends)` | `tui::{Tui, Session}`; waits fail on timeout instead of returning `false` |
| `tools/*.py` helpers (`subset_fonts.py`, `test_migration.py`, `migrate_fixture_v3.py`) | Gone with `tools/`; font subsets are committed under `assets/fonts/` (see `assets/fonts/FONTS.md`) |
| Vendored `vt100` fork, `vendor/termlens`, `termpane` git dependency | PTY engine is `portable-pty` 0.9 (PTY owner) + `alacritty_terminal` 0.26 (terminal state) from crates.io — no git/path deps, no patches |
| Hand-rolled SGR replay parser, `ansi::replay_raw`, `tuisnap::pty`, `tuisnap::termlens` re-export | Deleted; raw-ANSI replay is `observe::Replay` / `tui_shell::Recording` over the current engine |

## Engine history (why the old docs mention other emulators)

1. Vendored `vt100` 0.16.2 fork → replaced by `termpane` v0.1.0
   (git-only) over vendored `termlens` 0.9, schema 3 unchanged.
   Differential evidence (16 streams, vt100 oracle, pre-cutover)
   lived in the old migration doc; the oracle was removed at
   cutover and its ported pins live in `tool_qualification` tests.
2. `termpane` git + vendored `termlens` → `portable-pty` 0.9 +
   `alacritty_terminal` 0.26 from crates.io (current). Rationale:
   no vendored forks, no git-only backends, pure-cargo build.
   See [DECISIONS.md](DECISIONS.md).

MSRV history: 1.97 during the termpane window (its floor); current
pin is 1.98.1 (`rust-toolchain.toml`).

## Workflow then and now

```text
# v0.1
UPDATE_BASELINE=1 cargo test   # ambient bless — gone

# now
cargo nextest run --locked --offline --all-features   # writes actuals, fails missing/changed
tuisnap accept --store <dir> <name>                   # explicit, per-name, after review
cargo nextest run --locked --offline --all-features   # green
```
