# Architecture

Head: `75ff479` (`redesign/rust-first-testing-platform`), 2026-09-29.
Graph below is normal-dependency edges from `cargo tree --edges
normal` at head; dev-dependencies are noted separately.

## Workspace

```text
crates/
  tuiscotti           facade: re-exports + examples/01–08 (no logic of its own)
  tuiscotti-core      pure models: frame, screen, locate, semant, names, ratatui
  tuiscotti-render    profiles, render (PNG/SVG/ANSI/HTML), diff, export
  tuiscotti-runtime   tui + tui_shell + waits (pty), command, runner, observe,
                      proto, mcp, snapshot, grouped, import_compat
  tuiscotti-insta     assert_snapshot! / assert_screenshot! over Insta
  tuiscotti-cli       binary `tuiscotti` (thin arg parsing over the facade)
  tuiscotti-fixtures  fixture app + 3 fixture binaries + committed approvals (publish = false)
  xtask               repo automation: docs, fonts, fixtures, perf, brand, deps (publish = false)
```

## Dependency graph (from `cargo tree --edges normal` at head)

Internal edges only; external deps per crate follow.

Backbone (the edge list below is authoritative):

```text
tuiscotti-cli ──▶ tuiscotti ──┬──▶ tuiscotti-runtime ──▶ tuiscotti-render ──▶ tuiscotti-core
                              └──▶ tuiscotti-insta ──────▶ tuiscotti-core
```

Internal edges, exhaustively (normal deps):

- `tuiscotti-cli` → `tuiscotti`
- `tuiscotti` → `tuiscotti-runtime`, `tuiscotti-render`,
  `tuiscotti-insta`, `tuiscotti-core`
- `tuiscotti-runtime` → `tuiscotti-render`, `tuiscotti-core`
  (`tuiscotti-insta` is still listed in the runtime manifest but
  unused by any runtime code since F12 — a dead edge whose removal
  belongs to the dep-graph owner once `Cargo.lock` settles)
- `tuiscotti-insta` → `tuiscotti-render`, `tuiscotti-core`
- `tuiscotti-render` → `tuiscotti-core`

`tuiscotti-fixtures` (normal: anyhow, crossterm, ratatui only;
tuiscotti/core/render are DEV-dependencies) and `xtask`
(standalone binary) have no internal normal edges.

The facade depends on all four leaf crates directly; the runtime
additionally depends on render (stores + op protocol render PNGs —
load-bearing, not incidental). The runtime → insta edge is dead
since F12: the only thing the runtime took from insta was the pure
canonical projection, which now lives in core next to `Screen`
(`screen::canonical_string` / `canonical_value`); no runtime module
names `tuiscotti_insta` anymore, and the manifest line is left for the
dep-graph owner to delete with the lockfile. The CLI reaches
fixtures only through the facade — there is no direct CLI →
fixtures edge.

External dependency shape (workspace-pinned, `=x.y.z` in root
`Cargo.toml`):

| Crate | External deps |
|---|---|
| tuiscotti-core | ratatui, serde, serde_json, unicode-width |
| tuiscotti-render | + base64, swash, image, image-compare, serde, serde_json, sha2 |
| tuiscotti-insta | image, insta, serde_json, sha2 |
| tuiscotti-runtime | + base64, serde, serde_json, sha2; pty-gated: portable-pty, alacritty_terminal, libc |
| tuiscotti-cli | clap, serde_json |
| tuiscotti-fixtures | anyhow, crossterm, ratatui (+ tuiscotti/core/render as dev-deps) |

Feature flags (`pty`, default on): `tuiscotti-cli/pty` →
`tuiscotti/pty` → `tuiscotti-runtime/pty` →
`dep:portable-pty, dep:alacritty_terminal, dep:libc`.
`tuiscotti-fixtures` has the same default. The facade pins
`tuiscotti-runtime` with `default-features = false` and re-adds the
terminal runtime only via its own `pty` feature, so
`--no-default-features` builds pure-view tests without PTY or
native deps. The runtime also defines `test-overrides` (F12, never
default): it gates the test-only `set_runtime_dir_override` so the
function cannot exist in production builds; the facade forwards it
and only the CLI dev-dependencies enable it.

Layering rules:

- `tuiscotti-core` is pure: no rendering, no PTY, no filesystem
  beyond parsing, no internal deps. Everything depends on it; it
  depends on nothing internal.
- `tuiscotti-render` adds pixels: profiles + renderer (swash
  raster backend) + diff + export. Depends only on core.
- `tuiscotti-insta` adds review gates over Insta. Depends on core
  + render only — never on the runtime.
- `tuiscotti-runtime` owns all execution (PTY, processes, sessions)
  and all artifact stores. It is the only crate that may spawn.
- `tuiscotti` is a facade: `pub use` re-exports, zero logic.
- `tuiscotti-cli` is thin: Clap structs + `run()` dispatch onto the
  facade. No business logic in `main.rs` beyond arg shaping.

## Data flow

```text
PURE PATH (no subprocess):
  fixture model + view state + viewport + theme
    ─▶ production Ratatui draw closure
    ─▶ core::ratatui::{draw_frame, render_screen} ─▶ Frame / Screen
    ─▶ Store::check / GroupedStore::check / assert_snapshot!
    ─▶ approved frame.json + PNG (+ fidelity sidecar), diff PNG, report.html

INTERACTIVE PATH (feature pty):
  real executable ─▶ portable-pty ─▶ alacritty_terminal state
    ─▶ tui::Session observations ─▶ Screen
    ─▶ assert::frame_from_screen ─▶ Frame ─▶ same gates as above

AGENT PATH (no PTY required):
  JSON ops ─▶ proto::execute / run_machine_line ─▶ envelopes
  tuiscotti machine │ mcp::serve │ proto session start/stop/list/prune
```

## Key types

- `core::frame::Frame` — canonical grid: positioned cells, explicit
  widths/continuations, `Default|Indexed|Rgb` colors, modifiers,
  cursor, provenance. `FRAME_VERSION = 3`. JSON-serializable,
  `validate()` rejects out-of-grid data loudly.
- `core::screen::Screen` — validated observation model built from a
  `Frame` (`Screen::from_frame`) or a live session. `Observation`
  pairs a screen with revision, capture reason, terminal state, and
  provenance. `canonical_string`/`canonical_value` are the
  deterministic state projections every gate binds (F12: moved from
  the insta spike into core, next to the type they project).
- `render::profile::Profile` — pinned render contract: font bytes
  (SHA-256), 10×21 cells at 16px, palette, scale ×2, cursor policy.
  `tuiscotti-default` is the one shipping profile.
- `render::Renderer` — `Frame`/`Screen` → PNG/SVG/ANSI/HTML +
  fidelity sidecar. Per-glyph fallback chain; never system fonts.
  One-shot callers go through the thread-local shared instances
  (`with_profile`/`with_strict`, F12): faces parsed once per thread,
  glyph caches shared; custom profiles still construct per call.
- `runtime::snapshot::Store` — classic store: `approved/` +
  `actual/` + `diff/` + `report.html` under one root.
- `runtime::grouped::GroupedStore` — nested scenario names,
  exactly four committed artifacts per scenario
  (`.ansi`/`.txt`/`.png`/`.html`).
- `runtime::tui::Tui`/`Session` — owned PTY sessions: spawn, key
  chords, mouse, resize, waits that fail with evidence on timeout.
  The reader→worker op channel is bounded with PTY backpressure and
  the worker coalesces pending `Feed` batches (F12); `Session::meta`
  serves revision + geometry without a screen clone; bound locators
  (`get_by*`) click atomically in the owning worker (F11).
- `runtime::proto::{Op, execute}` — typed op protocol (15 ops) +
  named sessions + bounded recording; the agent control plane.

Public API design: [API.md](API.md). Snapshot semantics:
[SNAPSHOTS.md](SNAPSHOTS.md). Durable rationale: [DECISIONS.md](DECISIONS.md).
