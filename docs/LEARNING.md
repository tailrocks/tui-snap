# Learning path

Eight small programs, each one idea. Run them in order; every one exits 0 and
prints an `EXAMPLE-NN-OK` marker. Nothing writes outside temp dirs (snapshot
and evidence dirs point at `tempdir`s, `INSTA_UPDATE=no` is set in-process).

| # | Example | Idea |
|---|---|---|
| 01 | `examples/01-pure-view.rs` | Production draw closure → `Screen` → `assert_snapshot!` (pre-approved) |
| 02 | `examples/02-styled-shot.rs` | `assert_screenshot!`: canonical + PNG as one sample, evidence on disk |
| 03 | `examples/03-piped-cli.rs` | `Command` error cases: `SpawnError` vs exit code, split streams |
| 04 | `examples/04-interactive-tui.rs` | PTY journey: spawn → `wait_predicate` → snapshot → close |
| 05 | `examples/05-locators-waits.rs` | `Locator::text` + `expect_visible` to one deadline |
| 06 | `examples/06-artifacts-review.rs` | `emit_four` + frozen root: pin two artifacts, reject acceptance |
| 07 | `examples/07-advanced-profiles.rs` | Strict `RenderProfile`, `Strict` vs `Placeholder` missing policy |
| 08 | `examples/08-agent-workflow.rs` | `proto::execute` + `--machine` JSON envelopes, no PTY |

Commands:

```sh
cargo run --example 01-pure-view        # any single step
cargo test --test examples_lane        # the executed lane: runs all 8, asserts exit 0 + markers
cargo nextest run                      # full suite, same lane included
cargo test --doc                       # doctest lane (5 doctests)
```

Suggested route: 01→02 for snapshot mechanics, 03→04 for process/PTY
capture, 05 for querying grids, 06→07 for review and rendering policy,
08 for driving it all from an agent.
