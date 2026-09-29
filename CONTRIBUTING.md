# Contributing

Pinned toolchain: Rust **1.98.1** (`rust-toolchain.toml`), edition 2024.
`cargo` on PATH may be a cache shim; behavior is identical.

## Setup

```sh
cargo fetch --locked        # warm the registry (offline builds after this)
cargo build --locked --offline
cargo nextest run --locked --offline   # full suite (preferred runner)
```

Pure-view-only build (no PTY engine): append `--no-default-features`
to `cargo build` / `cargo test`. The `tuisnap` binary requires the
default `pty` feature.

## Gates (run before every push)

```sh
cargo fmt --check
cargo clippy --locked --offline --all-targets --all-features -D warnings
cargo nextest run --locked --offline --all-features
cargo test --locked --offline --doc   # doctests ride separately
```

Workspace lints forbid `unwrap_used`, `expect_used`, `panic!`,
`todo!`, `dbg!` in shipped code; tests use the standard helpers.
`unsafe_code` is forbidden. DCO signoff required: `git commit -s`.

## Layout

- `crates/tuiscotti` — public facade (re-exports only, plus examples).
  New public API goes in the owning leaf crate and is re-exported here.
- `crates/tuiscotti-core` — pure models: `frame`, `screen`, `locate`,
  `semant`, `names`, `ratatui` adapter. No rendering, PTY, or I/O.
- `crates/tuiscotti-render` — profiles, PNG/SVG/ANSI/HTML rendering,
  decoded-pixel diff, evidence export.
- `crates/tuiscotti-runtime` — PTY sessions (`tui`, `tui_shell`),
  piped `command`, `runner`, op `proto`, `snapshot`/`grouped` stores.
- `crates/tuiscotti-insta` — `assert_snapshot!` / `assert_screenshot!`
  gates over Insta.
- `crates/tuiscotti-cli` — the `tuisnap` binary. Thin: parse args,
  call the facade.
- `crates/tuiscotti-fixtures` — shared fixture app + committed approvals.
- `crates/xtask` — repo automation entry point (`cargo xtask …`).

Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Rules that bite

- **No legacy code.** Remove old paths completely — no shims, aliases,
  or deprecation periods. Breaking changes are preferred.
- **CI must never auto-accept.** There is no bless flag or variable;
  do not add one. Approval is `Store::accept` / `GroupedStore::accept`
  / `tuisnap accept` / `cargo insta review`, on a workstation only.
  See [docs/SNAPSHOTS.md](docs/SNAPSHOTS.md).
- **Fail closed.** Missing or corrupt approvals fail the gate; unknown
  input to a guard fails rather than passes. New statuses go through
  `snapshot::Status` and both stores.
- **Determinism.** Same frame + same profile + same vendored font
  bytes = byte-identical PNGs. No system fonts, no wall-clock in
  artifacts (provenance timestamps excepted), no unordered maps in
  canonical output.
- **Docs match code.** `crates/tuiscotti-cli/tests/readme_lock.rs`
  mirrors every README fence statement-for-statement and asserts the
  documented CLI flags exist in `--help`. If you change a documented
  behavior, update the README and the lock test together.
- **Link check.** Every `docs/*.md` + `README.md` link must resolve;
  run the check in [docs/TESTING.md](docs/TESTING.md) before pushing
  doc edits.

## Reviews and merges

One working branch (`redesign/rust-first-testing-platform`); small
PRs, merged promptly after all gates pass. Before merge, read every
review/comment/thread; verify findings against code, tests, and docs;
fix + verify + commit + push + reply with the fixing commit URL, or
reply with evidence before resolving. Merge only with zero
unaddressed feedback, zero unresolved threads, and all required
checks green at the final head SHA.
