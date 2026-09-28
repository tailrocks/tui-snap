# tuisnap — Rust TUI visual-regression toolkit

Two capture paths share one canonical frame; both produce full approved
frames, readable PNGs, and portable HTML expected/actual/diff reports.

```text
Fixture model + view state + viewport + theme
        └─▶ actual production Ratatui view ──▶ frame        (no PTY, no subprocess)

Real executable ──▶ PTY + terminal-state engine ──▶ frame   (keyboard/mouse/resize)
```

A changed snapshot requires explicit review (`Store::accept` /
`GroupedStore::accept_all`, or `cargo insta review` for the macro gates).
Equality only validates the fixtures covered — never every app state.

## Quick start: pure view tests

```rust
use ratatui::widgets::Paragraph;
use tuisnap::{Profile, Provenance, VENDORED_FACES};
use tuisnap::snapshot::Store;

#[test]
fn home_screen() {
    let store = Store::new(std::path::Path::new("tests/visual"));
    let profile = Profile::default_profile();
    // Render the ACTUAL production view from fixture data:
    let frame = tuisnap::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "home", vec![]),
        |f| f.render_widget(Paragraph::new("home"), f.area()),
    );
    // Actual artifacts are written BEFORE the assertion, so a failure still
    // leaves reviewable evidence (actual/*.frame.json + *.png + report.html).
    let outcome = store.check("home", &frame, &profile, &VENDORED_FACES, 1.0).unwrap();
    outcome.ensure_matched().unwrap();
}
```

Bulk suites reuse one cached renderer per thread and can rebuild the HTML
report without the CLI:

```rust
fn bulk(
    store: &tuisnap::snapshot::Store,
    profile: &tuisnap::Profile,
    frame: &tuisnap::Frame,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = profile.renderer(&tuisnap::VENDORED_FACES)?; // fonts parsed once
    let outcome = store.check_with(&mut renderer, "home", frame, 1.0)?;
    outcome.ensure_matched()?;
    let report = store.report_with(&mut renderer, 1.0, "my suite")?; // re-verify + report.html
    assert_eq!(report.failed(), 0);
    Ok(())
}
```

First run fails with `missing-approval` (fail-closed). Inspect
`actual/*.png` + `report.html`, then accept explicitly from Rust:

```rust
fn accept_reviewed(store: &tuisnap::snapshot::Store) -> Result<(), tuisnap::snapshot::SnapshotError> {
    store.accept("home")?; // one snapshot
    for name in store.actual_names()? { // everything reviewed
        store.accept(&name)?;
    }
    Ok(())
}
```

There is deliberately **no** `BLESS=1` / auto-accept: CI must never approve
snapshots by itself (see `docs/CI.md`).

## Interactive tests (feature `pty`, on by default)

```rust
use std::time::{Duration, Instant};
use tuisnap::tui::{CancelToken, Tui};

let mut s = Tui::new(["./my-tui"]).size(120, 40).spawn()?;
let cancel = CancelToken::new();
let obs = s.wait_predicate(
    |o| tuisnap::proto::screen_text(&o.screen).contains("Ready"),
    Instant::now() + Duration::from_secs(5),
    &cancel,
)?; // timeout fails WITH the screen
s.press("ctrl+Up")?; // modifiers + special keys, `+`-joined
s.send_text("hello")?; // literal input (bracketed paste: `paste`)
let settled = s.wait_stable(Instant::now() + Duration::from_secs(5), &cancel)?;
let frame = tuisnap::assert::frame_from_screen(&s.snapshot()?);
s.close()?;
```

Mouse input (`click`, `mouse_wheel`, …) requires the app to enable mouse
reporting first; otherwise it fails with `ModeNotEnabled` instead of
silently dropping.

Pure view tests build without the PTY engine: `cargo test --no-default-features`.
The `tuisnap` CLI binary itself requires the default `pty` feature.

## API map (examples 01–08)

The library facade (`src/lib.rs`) centers on `Screen`/`Frame` plus:

- gates: `tuisnap::assert_snapshot!` / `tuisnap::assert_screenshot!` (Insta
  review), `snapshot::Store`, `grouped::GroupedStore`
- capture: `ratatui::render_screen` (pure views), `tui::{Tui, Session}`
  (interactive, feature `pty`), `command::Command` (piped CLI)
- queries: `locate::Locator` (`text`/`regex`/`style` + `expect_*` waits),
  `observe`, `screen::Observation`
- agents/CI: `proto::{Op, execute, run_machine_line}` (typed ops +
  `--machine` JSON), `runner::TestContext`, `export`, `mcp`
- rendering: `Profile::default_profile`, `render::Renderer`,
  `VENDORED_FACES`, `VENDORED_FALLBACK_FACES`

`examples/01-pure-view.rs` … `examples/08-agent-workflow.rs` each run end to
end (`cargo run --example 01-pure-view`); `tests/examples_lane.rs` keeps
them green.

## CLI

```text
tuisnap init --dir .                              # scaffold tui-snap.toml + nextest config + example
tuisnap doctor                                     # toolchain / fonts / profile / env report
tuisnap schema                                     # print the op-protocol JSON schema
tuisnap capture --out shots/home -- ./my-tui --flag  # run + collect artifacts
tuisnap inspect --dir shots/home                  # offline view; never executes
tuisnap render --input shot.frame.json --format png --format svg --out shot
tuisnap diff --expected a.png --actual b.png      # exit 4 on mismatch
tuisnap review --dir verdicts                     # list verdicts; exit 4 on any fail
tuisnap accept home --store shots/home            # accept one reviewed candidate (explicit)
tuisnap report --dir verdicts --out report.html   # standalone HTML report
tuisnap import --dir frozen                       # read-only frozen-tree import
tuisnap session start --name demo -- ./my-tui     # + stop / list / prune / attach
tuisnap record --out trace.jsonl -- ./my-tui      # bounded event recording
tuisnap trace --input trace.jsonl                 # offline journal view
tuisnap --machine < ops.jsonl                     # typed op protocol over stdio
```

Exit statuses: 0 ok; 2 CLI usage error; 3 op error; 4 verification
disagreement. `capture`/`record` preserve the child's exit code instead.
Full reference: `tuisnap --help` (per command: `tuisnap <cmd> --help`).

`render` accepts `--font-file` (hash recorded in the profile); the fallback
chain below still applies on top of an override. Offline `frame.json`
re-renders byte-identical PNGs (pinned by tests).

The PTY engine is `portable-pty` 0.9 (PTY owner) + `alacritty_terminal` 0.26
(terminal state) from crates.io — no git/path dependencies, no Cargo
patches. Convert captures with `tuisnap::assert::frame_from_screen` (or
`Screen::from_frame` the other way). See `docs/MIGRATION.md` for schema 3
history.

Pinned toolchain: 1.98.1 (`rust-toolchain.toml`). Schema v4: underline
styles (single/double/curly/dotted/dashed) and underline color are
canonical; blink stays frozen-visible with slow/rapid combined, hidden is
conceal, overline stays dropped.

Consumer and migration gates:

```text
cargo run --locked --manifest-path tests/fixtures/consumer/Cargo.toml
python3 tools/test_migration.py
```

## Layout of a store

```text
<store>/approved/<name>.frame.json   # committed: canonical cells + approved PNG below
<store>/approved/<name>.png          # + <name>.png.fidelity.json (missing/fallback glyphs)
<store>/actual/<name>.frame.json     # local evidence (gitignored)
<store>/actual/<name>.png            # + <name>.png.fidelity.json + <name>.manifest.json (candidate seal)
<store>/diff/<name>.png              # red-overlay diff, on mismatch
<store>/report.html                  # review index: links PNGs/frames on disk, never embeds
```

A missing approved PNG fails closed (`missing-approval`); nothing
regenerates approvals except explicit `accept`. The PNG gate compares exact
decoded pixels (re-encoding passes, one changed channel fails); review
leniency lives only on a validated threshold (`PerceptualPolicy` rejects
NaN/out-of-range). `CompareOutcome` is `#[must_use]` — dropping one without
`ensure_matched()` warns instead of silently passing.

## Grouped multi-artifact store

`tuisnap::grouped::GroupedStore` is an alternative store for suites that
want nested scenario names and committed, human-reviewable artifacts. A
scenario `<group>/<sub_group>/<name>` commits EXACTLY four files under the
approved root — no `.frame.json`, no sidecars:

```text
snapshots/showcase/pages/overview_120x40_truecolor.ansi   # normalized SGR dump (cell-exact gate)
snapshots/showcase/pages/overview_120x40_truecolor.txt    # plain black-and-white text
snapshots/showcase/pages/overview_120x40_truecolor.png    # colored image (pixel gate)
snapshots/showcase/pages/overview_120x40_truecolor.html   # standalone colored HTML render
```

```rust
fn check_page(
    store: &tuisnap::grouped::GroupedStore,
    profile: &tuisnap::Profile,
    frame: &tuisnap::Frame,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = profile.renderer(&tuisnap::VENDORED_FACES)?;
    let outcome = store.check_with(&mut renderer, "pages/overview", frame, 1.0)?;
    outcome.ensure_matched()?;
    store.report_with(&mut renderer, 1.0, "my suite")?; // HTML report, outside approved/
    Ok(())
}
```

Actuals (`snapshots.actual/`), diff PNGs (`snapshots.diff/`) and the report
(`snapshots.actual/report.html` by default) live OUTSIDE the approved tree
— override with `with_actual_root` / `with_diff_root` / `with_report_path`
(e.g. under `target/`). Gates: `.ansi`/`.txt`/`.html` byte-compares (the
ansi dump is the cell-exact gate; html catches renderer changes) plus the
same exact decoded-pixel PNG gate as the classic store. Actual PNG/HTML
always render fresh from the candidate frame — never copied from approved.
Missing approvals fail closed; names with absolute paths, `..`, empty
segments or backslashes are rejected. Bless recursively from Rust:

```rust
let accepted = store.accept_all()?; // every reviewed actual → approved
```

The classic `Store` above is fully unaffected; both share statuses, the
report machinery and the renderer. See `docs/USAGE.md` for gate semantics
in detail.


## Fidelity contract

- Layout from frame widths (CJK keeps 2 cells even as tofu); real glyphs via
  `fontdue` from a pinned vendored font — never placeholder blocks. Glyphs
  rasterize at the final scale (`font_px × scale`), HiDPI-crisp with no
  post upscale.
- Profile pins font bytes (SHA-256), 10×21 cells at 16px (JetBrains Mono
  metrics), palette, scale ×2, cursor policy. `verify_geometry` fails loudly
  on drift.
- Bold / italic / bold-italic render with the REAL faces of the vendored
  JetBrainsMono Nerd Font Mono family; the faux double-strike / shear survive
  only when a face fails to load or `--font-file` overrides with one face.
- Covered by the primary family: box drawing, blocks, Braille, Nerd icons,
  combining marks. What it lacks is served per-glyph by the vendored fallback
  chain (below). Codepoints NO face covers (color emoji, Hangul, JIS level-2
  kanji) render as deterministic tofu with correct advance AND are reported
  in `<name>.png.fidelity.json` next to every PNG output (documented in
  `assets/fonts/FONTS.md`).

## Font fallback

The PNG path never uses system fonts (determinism across machines). Per
glyph the renderer tries: styled primary face → regular primary face →
pinned fallback faces in order → tofu + fidelity record. The default chain
([`VENDORED_FALLBACK_FACES`]) is three vendored Noto subsets, sha256-pinned
and verified at load (SIL OFL 1.1, `assets/fonts/LICENSE-Noto.txt`):

| Face | Covers | Size |
|---|---|---|
| Noto Sans Symbols 2 subset | ◐ ★ ☕ ❤ ✔ ⬤ — Geometric Shapes, Misc Symbols, Dingbats, Misc Symbols & Arrows | 87 KB |
| Noto Sans Symbols subset | ⚷ ⚙ ♻ — misc symbols unique to v1 (U+2600–U+26FF) | 27 KB |
| Noto Sans CJK JP subset | 東京 — kana, JIS X 0208 level-1 kanji, fullwidth forms | 679 KB |

`Renderer::new` loads this chain; primary-covered frames render
BYTE-IDENTICAL with or without it (pinned by
`tests/render.rs::primary_covered_fixtures_match_pre_fallback_render_bytes`).
Fallback glyphs draw centered and clipped inside the primary cell box; the
cell grid never moves. Cells served by a fallback face are listed in the
sidecar's `fallback_glyphs` (omitted when empty, so existing sidecars stay
byte-stable). Register your own faces (or render primary-only) with
[`Renderer::with_fallbacks`]; each face carries its own sha256 pin:

```rust
fn custom_chain(profile: &tuisnap::Profile) -> Result<tuisnap::Renderer, Box<dyn std::error::Error>> {
    let chain = [tuisnap::FallbackFace {
        bytes: tuisnap::VENDORED_SYMBOLS2_FONT,
        sha256: tuisnap::VENDORED_SYMBOLS2_FONT_SHA256, // verified at load; mismatch refuses to render
        desc: "my extra symbols",
    }];
    Ok(tuisnap::render::Renderer::with_fallbacks(
        profile,
        &tuisnap::VENDORED_FACES,
        &chain,
    )?)
}
```

The subsets are reproducible and extensible (JIS level-2, Hangul, more
blocks): `python3 tools/subset_fonts.py` re-downloads commit-pinned upstreams,
re-subsets, and prints the new hashes to pin — see `assets/fonts/FONTS.md`.
- Terminal-like, measured fidelity — NOT pixel-identity with any terminal
  emulator; cell data stays authoritative for styles.

## Docs

- `docs/USAGE.md` — patterns, CLI reference, approval workflow
- `docs/MIGRATION.md` — v0.1 → v0.2 (breaking), BLESS removal
- `docs/CI.md` — CI wiring that cannot auto-accept
- `assets/fonts/FONTS.md` — font licensing and coverage
- `docs/RESEARCH.md` — current implementation review and proposed direction
- `docs/ALTERNATIVES-REVIEW.md`, `docs/SIMILAR-PROJECTS.md` — verified competitor landscape

- [docs/REDESIGN-PLAN.md](docs/REDESIGN-PLAN.md) — proposed next direction; not shipped behavior
- [docs/REDESIGN-BACKLOG.md](docs/REDESIGN-BACKLOG.md) — work items and acceptance criteria

## License

Apache-2.0 — see [LICENSE](LICENSE).
