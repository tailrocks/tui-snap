# tuisnap — Rust TUI visual-regression toolkit

Two capture paths share one canonical frame (`tuiscotti::Frame`,
schema v3). Both produce full approved frames, readable PNGs, and
portable HTML expected/actual/diff reports.

```text
Fixture model + view state + viewport + theme
        └─▶ actual production Ratatui view ──▶ frame        (no PTY, no subprocess)

Real executable ──▶ PTY + terminal-state engine ──▶ frame   (keyboard/mouse/resize)
```

A changed snapshot requires explicit review (`Store::accept` /
`GroupedStore::accept_all`, or `cargo insta review` for the macro
gates). There is deliberately **no** `BLESS=1` / auto-accept: CI must
never approve snapshots by itself. Equality only validates the
fixtures covered — never every app state.

Crates and binaries keep their current names: facade `tuiscotti`,
binary `tuisnap` (from `tuiscotti-cli`), config `tui-snap.toml`.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Workflow 1: pure view test

Render the ACTUAL production view from fixture data, gate it with
`snapshot::Store`. First run fails with `missing-approval`
(fail-closed) and still writes reviewable evidence
(`actual/*.frame.json` + `*.png` + `report.html`).

```rust
use ratatui::widgets::Paragraph;
use tuiscotti::snapshot::Store;
use tuiscotti::{Profile, Provenance, VENDORED_FACES};

#[test]
fn home_screen() {
    let store = Store::new(std::path::Path::new("tests/visual"));
    let profile = Profile::default_profile();
    let frame = tuiscotti::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "home", vec![]),
        |f| f.render_widget(Paragraph::new("home"), f.area()),
    );
    let outcome = store.check("home", &frame, &profile, &VENDORED_FACES, 1.0).unwrap();
    outcome.ensure_matched().unwrap();
}
```

After reviewing the actuals, accept explicitly from Rust:

```rust
store.accept("home")?; // one snapshot
for name in store.actual_names()? { // everything reviewed
    store.accept(&name)?;
}
```

Runnable end to end: `cargo run -p tuiscotti --example 01-pure-view`
(exit 0, prints `EXAMPLE-01-OK`). Pure view tests build without the
PTY engine: `cargo test -p tuiscotti --no-default-features`.

## Workflow 2: interactive PTY test (feature `pty`, on by default)

```rust
use std::time::{Duration, Instant};
use tuiscotti::tui::{CancelToken, Tui};

let mut s = Tui::new(["./my-tui"]).size(120, 40).spawn()?;
let cancel = CancelToken::new();
let obs = s.wait_predicate(
    |o| tuiscotti::proto::screen_text(&o.screen).contains("Ready"),
    Instant::now() + Duration::from_secs(5),
    &cancel,
)?; // timeout fails WITH the screen
s.press("ctrl+Up")?; // modifiers + special keys, `+`-joined
s.send_text("hello")?; // literal input (bracketed paste: `paste`)
let settled = s.wait_stable(Instant::now() + Duration::from_secs(5), &cancel)?;
let frame = tuiscotti::assert::frame_from_screen(&s.snapshot()?);
s.close()?;
```

Mouse input (`click`, `mouse_wheel`, …) requires the app to enable
mouse reporting first; otherwise it fails with `ModeNotEnabled`
instead of silently dropping.

Runnable end to end: `cargo run -p tuiscotti --example 04-interactive-tui`
(exit 0, prints `EXAMPLE-04-OK`).

## Workflow 3: CLI capture and offline review

```sh
tuisnap doctor                                        # toolchain / fonts / profile / env
tuisnap capture --out shots/demo -- ./my-tui --flag  # run + collect artifacts
tuisnap inspect --dir shots/demo                     # offline view; never executes
tuisnap render --input shot.frame.json --format png --out shot
tuisnap diff --expected a.png --actual b.png         # exit 4 on mismatch
tuisnap accept home --store shots                    # one reviewed snapshot, explicit
echo '{"type":"capabilities"}' | tuisnap --machine   # typed op protocol over stdio
```

Exit statuses: 0 ok; 2 CLI usage error; 3 op error; 4 verification
disagreement. `capture`/`record` preserve the child's exit code.
Full grammar: [docs/CLI.md](docs/CLI.md) (transcribed from `--help`).

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, dependency graph, data flow
- [docs/API.md](docs/API.md) — public Rust API design and status labels
- [docs/CLI.md](docs/CLI.md) — CLI reference (from the implemented Clap grammar)
- [docs/SNAPSHOTS.md](docs/SNAPSHOTS.md) — snapshot/approval semantics
- [docs/TESTING.md](docs/TESTING.md) — tests, fixtures, examples lane, CI wiring
- [docs/COMPARISON.md](docs/COMPARISON.md) — vs tui-test, terminal-control, termlens
- [docs/PERFORMANCE.md](docs/PERFORMANCE.md) — measured build/test/render numbers
- [docs/MIGRATION.md](docs/MIGRATION.md) — schema and layout migrations
- [docs/LIMITATIONS.md](docs/LIMITATIONS.md) — platform matrix, known gaps
- [docs/DECISIONS.md](docs/DECISIONS.md) — durable decisions and rationale
- [CONTRIBUTING.md](CONTRIBUTING.md) — tooling, gates, workflow
- `assets/fonts/FONTS.md` — font licensing and coverage

## Status labels

Docs use five truthful labels, reconciled from code + tests at the
current head: **implemented** (shipped, tested), **partial** (works
with documented gaps), **unsupported** (explicitly rejected, fails
closed), **future** (planned, not present), and per-platform
**tested / compiles-only / not run**. No legacy shims: removed APIs
stay removed.

## License

Apache-2.0 — see [LICENSE](LICENSE).
