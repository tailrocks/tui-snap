# tuisnap — Rust TUI visual-regression toolkit

Two capture paths share one canonical frame; both produce full approved
frames, readable PNGs, and portable HTML expected/actual/diff reports.

```text
Fixture model + view state + viewport + theme
        └─▶ actual production Ratatui view ──▶ frame        (no PTY, no subprocess)

Real executable ──▶ PTY + terminal-state engine ──▶ frame   (keyboard/mouse/resize)
```

A changed snapshot requires explicit review (`tuisnap accept`). Equality only
validates the fixtures covered — never every app state.

## Quick start: pure view tests

```rust
use tuisnap::{Profile, Provenance, VENDORED_FACES};
use tuisnap::snapshot::Store;

#[test]
fn home_screen() {
    let store = Store::new(std::path::Path::new("tests/visual"));
    let profile = Profile::default_profile();
    // Render the ACTUAL production view from fixture data:
    let frame = tuisnap::ratatui::draw_frame(120, 40, prov(), |f| {
        myapp::render_home(f, &fixture_model())
    });
    // Actual artifacts are written BEFORE the assertion, so a failure still
    // leaves reviewable evidence (actual/*.frame.json + *.png + report.html).
    let outcome = store.check("home", &frame, &profile, &VENDORED_FACES, 1.0).unwrap();
    outcome.ensure_matched().unwrap();
}
```

Bulk suites reuse one cached renderer per thread and can rebuild the HTML
report without the CLI:

```rust
let mut renderer = profile.renderer(&VENDORED_FACES)?;      // fonts parsed once
let outcome = store.check_with(&mut renderer, "home", &frame, 1.0)?;
let report = store.report_with(&mut renderer, 1.0, "my suite")?; // re-verify + report.html
```

First run fails with `missing-approval` (fail-closed). Inspect
`actual/*.png` + `report.html`, then accept explicitly:

```text
cargo run -q -- accept --store tests/visual --name home   # one snapshot
cargo run -q -- accept --store tests/visual --all         # everything reviewed
```

There is deliberately **no** `BLESS=1` / auto-accept: CI must never approve
snapshots by itself (see `docs/CI.md`).

## Interactive tests (feature `pty`, on by default)

```rust
let opts = tuisnap::pty::PtyOptions::default()
    .without_env("NO_COLOR")                    // strip inherited vars from the child
    .with_env("HOLLA_NO_HISTORY", "1");         // set app-specific ones
let mut s = tuisnap::pty::Session::spawn(&["./my-tui".into()], &opts)?;
s.wait_for_text("Ready")?;                 // timeout fails WITH the screen
s.wait_until(|sc| sc.cursor() == (0, 4, true))?;  // any predicate on the live screen
s.send_key("ctrl-up")?;                    // ctrl/alt/shift + special keys, too
s.scroll(10, 5, tuisnap::pty::Scroll::Down)?;     // wheel + non-left clicks: click_with
let frame = s.wait_stable(Duration::from_millis(300))?;  // style-aware settle
```

Pure view tests build without the PTY engine: `cargo test --no-default-features`.

## CLI

```text
tuisnap render --input shot.frame.json --format png --format svg --out shot
tuisnap check  --store tests/visual --name home --input actual.frame.json
tuisnap accept --store tests/visual --name home        # or --all
tuisnap report --store tests/visual                    # re-verify + rewrite report.html
tuisnap run --cols 120 --rows 40 --send enter --wait-for Ready \
  --store shots --name home -- ./my-tui                # capture + gate
```

`render` also accepts `--font-file` (hash recorded); all gates accept it too
(the fallback chain below still applies on top of an override).
Offline `frame.json` re-renders byte-identical PNGs (proven by tests).

Git/path library dependencies include the pinned PTY engine (termpane v0.1.0
via termlens) and need no Cargo patches. Use `tuisnap::termlens` when
constructing engine types for `frame_from_screen`. See `docs/MIGRATION.md`
for schema 3, the vt100 → termpane engine swap, and fixture migration.

MSRV: 1.97 (termpane floor). Schema v3 unchanged: blink is frozen-visible
with slow/rapid combined, hidden is conceal, overline/underline-styles stay
dropped.

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
NaN/out-of-range). Outcomes are `#[must_use]` — dropping one without
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
let store = tuisnap::grouped::GroupedStore::new(std::path::Path::new("tests/snapshots"));
let mut renderer = profile.renderer(&VENDORED_FACES)?;
let outcome = store.check_with(&mut renderer, "pages/overview", &frame, 1.0)?;
outcome.ensure_matched()?;
store.report_with(&mut renderer, 1.0, "my suite")?;   // HTML report, outside approved/
```

Actuals (`snapshots.actual/`), diff PNGs (`snapshots.diff/`) and the report
(`snapshots.actual/report.html` by default) live OUTSIDE the approved tree
— override with `with_actual_root` / `with_diff_root` / `with_report_path`
(e.g. under `target/`). Gates: `.ansi`/`.txt`/`.html` byte-compares (the
ansi dump is the cell-exact gate; html catches renderer changes) plus the
same exact decoded-pixel PNG gate as the classic store. Actual PNG/HTML
always render fresh from the candidate frame — never copied from approved.
Missing approvals fail closed; names with absolute paths, `..`, empty
segments or backslashes are rejected. Bless recursively from the CLI:

```text
tuisnap accept --grouped --store snapshots --all
tuisnap report --grouped --store snapshots --report-path target/report.html
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
let chain = [tuisnap::FallbackFace {
    bytes: MY_FONT,
    sha256: MY_FONT_SHA256,   // verified at load; mismatch refuses to render
    desc: "my extra symbols",
}];
let mut r = tuisnap::render::Renderer::with_fallbacks(&profile, &faces, &chain)?;
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
