# Qualification fork: schema 3

Canonical schema 3 adds `Mods.hidden` and `Mods.blink`; version-2 imports fail
instead of guessing lost attributes. Blink presence survives, but blink phase
is frozen visible and slow/rapid rates remain combined. HIDDEN suppresses
PNG/SVG glyph paint, while canonical JSON retains the original symbol: hidden
text is not redacted. Never capture real secrets.

`python3 tools/migrate_fixture_v3.py --out DIR` exports only the audited
schema-2 approvals read directly from Git revision
`5036cf87e621e6beb66deffe3224abdbefc955cb`, tied to the exact fixture source hash.
Run from a checkout containing that revision; current schema-3 approvals and
actual captures are never read. `--continuation-styles` additionally repairs old continuation
colors/modifiers from each original wide lead; its ledger lists these changes
separately. Outputs require review before replacing approved files. This is
not a general migration of arbitrary schema-2 snapshots: unknown hidden/blink
state cannot be recovered from that format.

# Engine swap: vt100 → termpane (schema 3 unchanged)

Schema v3 is unchanged: blink stays frozen-visible with slow/rapid combined,
hidden is conceal, overline/underline-styles stay dropped. MSRV is now 1.97
(termpane floor) — a breaking change for consumers.

The PTY engine and raw-ANSI replay now run on `termpane` v0.1.0 (pinned git
tag via termlens) instead of the vendored vt100 0.16.2 fork. Attr mapping:
hidden=conceal, blink=slow||rapid, all others 1:1; colors Default/Idx/Rgb 1:1;
DECAWM, bell events, `?12`, serialization round-trips, DEC 2026 and DECRQM
coverage per termpane CHANGELOG 0.1.0. The pending-wrap phantom column
(`cursor_position` == cols) is clamped to `cols-1` in `ansi::replay_raw`,
matching the prior fork behavior; `validate()` still rejects out-of-grid
cursors loudly. The `termlens` shadow parser is deleted (attributes are native
now). `pub use termlens` re-export and consumer type-identity stay intact.

Historical note: the vendored vt100 0.16.2 fork (independent bold/DIM,
DECAWM, wide continuation attributes, hidden/blink/strike) is removed. Its
archive hashes were in `vendor/UPSTREAM.json` (deleted). termlens 0.9 remains
vendored; its engine dependency is now the termpane git tag. Upstream source
hashes and licenses are retained in `vendor/TERMLENS-UPSTREAM.json` and
`vendor/termlens`.
Code using `frame_from_screen` should construct its screen through the
`tuisnap::termlens` re-export so the engine's Rust type identity matches.
Qualify ordinary consumption with `cargo run --locked --manifest-path
tests/fixtures/consumer/Cargo.toml` and inspect `cargo tree -i termpane` (one
version) and `cargo tree -i vt100` (empty) in that
consumer.

`Session::paste` retains termlens's simulated terminal behavior (LF→CR plus
paste-marker sanitization). `Session::paste_literal` preserves literal line
breaks, requires bracketed-paste mode, and rejects embedded delimiters so
payload text cannot escape into ordinary input. This separates deterministic
payload testing from simulated terminal behavior.

Raw-ANSI replay still cannot observe cursor appearance; its block/steady
cursor is a limitation, not equivalence to the PTY path. Use PTY captures for
cursor shape/blinking assertions. Raster output remains a pinned approximation
of terminal font rendering, not pixel identity with a terminal emulator.

## Differential evidence (vt100 oracle, pre-cutover)

Oracle: temporary feature-gated harness fed deleted `Vt100Emulator` vs
`TermpaneEmulator` identical bytes at rows=4, cols=10, scrollback=100 via
`feed_all` helper looping `process` across frame/query stops. Removed with
oracle at cutover; results preserved here because oracle no longer exists
in-tree.

Compared 16 streams:
- combined-attr wide SGR+RGB: `b"\x1b[1;2;7;8;5;9mX\x1b[0m\x1b[2;1H\x1b[38;2;1;2;3;48;2;4;5;6m\xe7\x95\x8c"`
- autowrap-off overwrite+reenable: `b"\x1b[?7l\x1b[1;8HABC\x1b[?7hDE\x1b[3;1H"`
- autowrap-off wide suppression: `b"\x1b[?7l\x1b[2;8H\xe7\x95\x8c\x1b[3;1H"`
- `?7l+A`: `b"\x1b[?7l\x1b[1;8HA"`
- `?7h+A`: `b"\x1b[?7h\x1b[1;8HA"`
- intensity 1;2/22: `b"\x1b[1;2mX\x1b[22;1mB\x1b[22;2mD\x1b[0mN"`
- blink/conceal/strike separate cells: `b"\x1b[5mB\x1b[0m\x1b[8mC\x1b[0m\x1b[9mS\x1b[0mp"`
- 256-color vs truecolor: `b"\x1b[38;5;196mX\x1b[0m\x1b[38;2;0;8;9mY"`
- colon-form RGB: `b"\x1b[38:2:10:20:30mA\x1b[0m\x1b[38:2::10:20:30mB"`
- DEC Special Graphics box: `b"\x1b(0lqqqk\x1b(B"`
- 7-line scrollback: `b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven"`
- alt-screen enter/exit: `b"\x1b[?1049hALT\x1b[?1049lBACK"`
- mode-set 2004/1/1002: `b"\x1b[?2004h\x1b[?1h\x1b[?1002h"`
- 2026 frame: `b"\x1b[?2026hframe1\x1b[?2026lnext"`
- U+FFFD replacement: `b"caf\xef\xbf\xbd done"`
- bold/color/blink mix: `b"\x1b[1;31mA\x1b[0m\x1b[5mB\x1b[0m\x1b[1;5;4mC"`

Compared per stream: text, styled text, cursor, size, mouse mode,
bracketed paste, app cursor, alt screen, scrollback text, `mid_sequence`,
`in_sync_update`, `mode_state` for 1/7/25/47/1049/2004/1004/1000/1002/
1003/1005/1006/2026; plus split-half re-feed agreement (feed `n/2`, then
rest, compare to one-shot).

Outcome: zero diffs one-shot on all 16; zero diffs split-half on 15/16.
Exception: U+FFFD split inside its 3-byte sequence — old `STAND_IN` hack
(`stand_in_for_replacement`/`restore_replacement` in deleted
`vendor/termlens/src/emu/vt100.rs`) needed all 3 bytes in one feed and
dropped it; termpane buffers incomplete UTF-8 across calls via `pending_utf8`
+ ground-state tracker in termpane crate (`src/grid.rs` `process` +
`src/grid/midseq.rs`), surfaced as `DamageGrid::mid_sequence()` and
OR-composed by `mid_sequence()` in `vendor/termlens/src/emu/termpane.rs`,
and draws it; history-text path (`capture_scrolled_rows`/`row_text`) agrees.
Judged vt100 limitation, termpane correct, not bent to match.

Re-verify today without oracle: ported pins in `tests/tool_qualification.rs`
(`serialized_contents_restore_wrap_before_painting`,
`formatted_intensity_roundtrip_clears_each_independent_flag`,
`formatted_terminal_modes_preserve_autowrap_disable_and_restore`) plus PTY
matrix in `tests/pty.rs`, all green on new engine.

---

# Migration: v0.1 → v0.2

v0.2 records the breaking migration that removed the old APIs; it is not the
current redesign plan. The current implementation review is in
[docs/RESEARCH.md](RESEARCH.md), and the proposed Rust-first direction is in
[docs/REDESIGN-PLAN.md](REDESIGN-PLAN.md). Breaking changes remain intentional;
the old APIs are gone, not deprecated.

## Removed (no shims)

| v0.1 | v0.2 |
|---|---|
| `tuisnap::ratatui_shot::{widget_frame, draw_frame}` | `tuisnap::ratatui::{widget_frame, draw_frame, capture}` — new signatures take `Provenance`; `widget_frame` hides the cursor, `draw_frame` preserves it |
| `tuisnap::Baseline` + `BLESS=1` / `UPDATE_SNAPSHOT=1` | `tuisnap::snapshot::Store` + explicit `tuisnap accept`. **Rationale:** ambient approval violates "CI must never auto-bless" (a test proves no env var accepts) |
| `digest` CLI subcommand | `check` / `report` subcommands |
| `render --input *.ansi` | `render` reads canonical `frame.json` only; raw streams replay via `ansi::replay_raw` (feature `pty`) |
| `tuisnap::render::write_format`, `to_png_bytes`, block-glyph PNG | `render_png` (real fontdue glyphs), `render_svg`, `ansi_dump`; formats via CLI `--format` |
| Hand-rolled SGR replay parser | Deleted; `termpane` is the established emulator |
| `PtySession` / `run_once(argv, opts, sends)` | `pty::Session` (termlens engine) / `run_once(argv, opts, sends, settle)`; waits now fail on timeout instead of returning `false` |

## Schema

- `Frame` is now versioned (`version: 2`), with positioned cells, explicit
  widths/continuations, `Default|Indexed|Rgb` colors, six modifiers,
  cursor state, and provenance. v0.1 JSON does not import — re-capture.
- `Frame::digest` now mixes the schema version and the cursor. Old digests
  are incomparable by design.

## Workflow

```text
# v0.1
UPDATE_BASELINE=1 cargo test   # ambient bless

# v0.2
cargo test                     # writes actuals, fails missing/changed
cargo run -q -- accept --store <dir> --all   # explicit, after review
cargo test                     # green
```

## Dependencies

- Added (all latest, pure-cargo): `fontdue`, `image-compare`, `base64`,
  `sha2`, `termlens` (optional via `pty` feature), `termpane` (optional).
- Removed direct use of `portable-pty` (termlens owns PTY lifetime now).
- Pure view tests: `cargo build/test --no-default-features` excludes
  `termlens` + `termpane` entirely.
