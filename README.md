# tuiscotti — Rust TUI visual-regression toolkit

Two capture paths share one canonical frame (`tuiscotti::Frame`,
schema v3). Both produce approved frames, readable PNGs, and
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

Names: facade `tuiscotti`, binary `tuiscotti` (from
`tuiscotti-cli`), config `tuiscotti.toml`.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Workflow 1: pure view + `assert_screenshot!`

Render the ACTUAL production view from fixture data, gate it with
the compound macro (canonical text + generation-tagged PNG as one
sample). Approvals are explicit: seed the reviewed sample first, as
below — a first run with no approval fails closed.

```rust
use ratatui::widgets::Paragraph;
use tuiscotti::assert::{Policy, generation_id, png_tag_generation, render_sample};
use tuiscotti::ratatui::{EdgePolicy, render_screen};

#[test]
fn styled_shot() {
    let screen = render_screen(
        24,
        4,
        |f| f.render_widget(Paragraph::new("styled shot"), f.area()),
        EdgePolicy::default(),
    )
    .unwrap()
    .into_screen();
    let tmp = tempfile::tempdir().unwrap();
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps).unwrap();
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: tmp.path().join("evidence"),
    };
    // Review-then-accept, made explicit: seed the approved sample first.
    let sample = render_sample(&screen).unwrap();
    let generation = generation_id(&sample.canonical);
    std::fs::write(
        snaps.join("styled-shot.snap"),
        format!(
            "---\nsource: readme\ndescription: tuiscotti generation {generation}\n\
             expression: canonical\n---\n{}",
            sample.canonical
        ),
    )
    .unwrap();
    std::fs::write(
        snaps.join("styled-shot-img.snap"),
        format!(
            "---\nsource: readme\ndescription: tuiscotti generation {generation}\n\
             expression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
        ),
    )
    .unwrap();
    std::fs::write(
        snaps.join("styled-shot-img.snap.png"),
        png_tag_generation(&sample.png, &generation),
    )
    .unwrap();
    tuiscotti::assert_screenshot!("styled-shot", &screen, &policy);
}
```

Runnable end to end: `cargo run -p tuiscotti --example 02-styled-shot`
(exit 0, prints `EXAMPLE-02-OK`). Pure view tests build without the
PTY engine: `cargo test -p tuiscotti --no-default-features`.

## Workflow 2: live spawn + locators + snapshot (feature `pty`, on by default)

```rust
use std::time::{Duration, Instant};
use tuiscotti::tui::{CancelToken, Tui};

#[test]
fn live_menu() {
    let mut s = Tui::new([
        "/bin/sh",
        "-c",
        "printf 'menu: alpha\\nmenu: beta\\n'; sleep 30",
    ])
    .size(40, 8)
    .spawn()
    .unwrap();
    let cancel = CancelToken::new();
    s.wait_predicate(
        |o| {
            tuiscotti::proto::screen_text(&o.screen).contains("menu: beta")
        },
        Instant::now() + Duration::from_secs(5),
        &cancel,
    )
    .unwrap();
    let span = s.get_by_text("menu: beta").expect_visible().unwrap();
    assert_eq!(span.text, "menu: beta");
    let screen = s.snapshot().unwrap();
    assert!(
        tuiscotti::observe::screen_text(&screen).contains("menu: alpha")
    );
    s.close().unwrap();
}
```

Timeouts fail WITH the last screen, never a bare deadline error.
Mouse input without app-enabled reporting fails closed
(`ModeNotEnabled`), never silently drops.

Runnable end to end: `cargo run -p tuiscotti --example 04-interactive-tui`
(exit 0, prints `EXAMPLE-04-OK`).

## Workflow 3: CLI capture + frozen review

```sh
tuiscotti capture --out /tmp/shots/demo -- echo hello
# captured Exit(0) -> /tmp/shots/demo
tuiscotti inspect --dir /tmp/shots/demo
# artifacts in /tmp/shots/demo (3 files, offline view): manifest + stdout/stderr
tuiscotti render --input crates/tuiscotti-fixtures/tests/visual/approved/dialog-dark-80x24.frame.json --format png --out /tmp/shots/shot
# wrote /tmp/shots/shot.png
tuiscotti diff --expected crates/tuiscotti-fixtures/tests/visual/approved/dialog-dark-80x24.png --actual /tmp/shots/shot.png
# pixels_equal=true dims_equal=true score=1 (exit 0: offline re-render is byte-identical)
tuiscotti render --input crates/tuiscotti-fixtures/tests/visual/approved/dialog-dark-80x24.frame.json --format ansi --format txt --format png --format html --out /tmp/frozen/shot
tuiscotti import --dir /tmp/frozen
# scenarios: 1 (shot); read-only — frozen trees never accept
# (also reports "unsupported: 1" for shot.png.fidelity.json: extras are listed, never fatal)
echo '{"type":"capabilities"}' | tuiscotti machine
# {"ok":true,"result":{"type":"capabilities","capabilities":{"protocol":"2.0.0",…}}}
```

Run the binary via `cargo run -q -p tuiscotti-cli -- <args>` (or
`cargo build -p tuiscotti-cli`, then `./target/debug/tuiscotti`).
Frozen roots are read-only by construction (`frozen_accept` always
errors); the Rust side is `cargo run -p tuiscotti --example
06-artifacts-review` (exit 0, prints `EXAMPLE-06-OK`).

Exit statuses: 0 ok; 2 CLI usage error; 3 op error; 4 verification
disagreement. `capture`/`record` preserve the child's exit code.
Full grammar: [docs/CLI.md](docs/CLI.md) (transcribed from `--help`).

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, dependency graph, data flow
- [docs/API.md](docs/API.md) — public Rust API design and status labels
- [docs/CLI.md](docs/CLI.md) — CLI reference (from the implemented Clap grammar)
- [crates/tuiscotti-cli/SYNTAX.md](crates/tuiscotti-cli/SYNTAX.md) — Rust + CLI syntax matrix (every surface, one table)
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
