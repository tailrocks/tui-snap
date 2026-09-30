# PR #6 Closure Ledger — v2

Single source of truth for PR #6 closure. Every row: ID, impact, owner, reproducer/evidence, fixing commit, verifier, disposition. Unlisted items do not exist as findings. PENDING = not yet revalidated at current HEAD.

HEAD: `aed37ebfaa65c2cc2a3e69f8cff121fef1cacad2` · base `main@9dc86da` · PR OPEN/MERGEABLE · reviewDecision empty (no approval) · 0 unresolved threads · checks: 15/15 green at `f89d522`; Policy generated-tree red at `b409d71`/`aed37eb` (docs-only commits, regen pending).

## Verified entries

| ID | Impact | Owner | Reproducer / evidence | Fixing commit | Verifier | Disposition |
|----|--------|-------|----------------------|---------------|----------|-------------|
| F1 legacy-importer | Was: blocks merge if shims/execution present | ledger | `import_compat` metadata+read only; canary test `imports_never_write_nor_execute_canary`; no vendored engine/shims; packets bless read-only import | n/a (no defect) | ledger writer | VERIFIED-KEEP-RESOLVED |
| NEXTEST-BASELINE | Baseline gate | ledger | nextest 618/618 GREEN, 42s wall at `f89d522` | n/a | ledger writer | VERIFIED |
| TERMPANE-MAIN-API | Swap-plan input | ledger | termpane main `dc40286`: process/pty/session modules, Unix-only, MSRV 1.97, unsafe forbid | n/a | ledger writer | VERIFIED-QUALIFIED (API only) |
| TERMPANE-REGISTRY | Merge gate: registry UNPUBLISHED (sparse index 404) | TBD (external: termpane owner) | sparse index 404 | n/a | ledger writer | VERIFIED-OPEN (blocks any termpane-dep merge) |
| VELNOR-PR1 | External reference only | external owner | Velnor PR #1 head `c8b3a89`, gates red, NOT qualified, no termpane workflows | n/a | ledger writer | VERIFIED-OUT-OF-SCOPE |
| SWAP-PLAN-DOC | Doc rewritten + verified | ledger | `TERMPANE-SWAP-PLAN.md` rewritten 328→446 lines vs merged termpane main `dc40286` (D1–D10 drift; gaps O5/O6/O7/O13/E2 open); drift spot-confirmed in upstream source | aed37eb | coordinator | VERIFIED |
| PERF-NUMBERS | Stale data | TBD | `PERFORMANCE.md` holds prior-head (`0f14262`/`75ff479`) numbers only — re-measurement required | pending | ledger writer | VERIFIED-OPEN |
| L1-DEP-BOUNDARY | Swap precondition: pty isolated, git deps denied | ledger | sole pty consumer `tuiscotti-runtime` (`portable-pty` optional behind `pty` feature, workspace pin `=0.9.0`); zero termpane entries in any `Cargo.toml`/`Cargo.lock` (one comment in root `Cargo.toml` only); zero `[patch]` sections; `deny.toml`: `unknown-git=deny`, `allow-git=[]` | n/a (no defect) | ledger writer | VERIFIED |
| L2-DIRECT-VIEW | Facade contract: direct-view surface | ledger | `tuiscotti-core/src/ratatui/mod.rs` re-exports: `capture/draw_frame/from_buffer/widget_frame`, `CURSOR_SENTINEL/plant_cursor_sentinel`, `ClippedCell/EdgePolicy/REPLACEMENT/ScreenCapture`, `render/render_screen/screen_from_buffer/screen_from_test_backend/stateful_screen/widget_screen`; `EdgePolicy` enum in `ratatui/edge.rs` | n/a (no defect) | ledger writer | VERIFIED |
| L3-PIPED-CLI | Facade contract: piped-CLI surface | ledger | `Command` builder (`new/cargo_bin/from_std`, `arg/args`, `env/envs/env_remove/env_clear`, `current_dir`, `stdin`, `timeout`, `output_limit`, `drain_deadline`, `shell`, `std_command`, `run`); `Termination::{Exit,Signal,Timeout,OutputLimit,SpawnError}` (never conflated); `IsolatedEnv` (`new/root/home/cwd/tmp/envs/apply/into_path`) + `isolated_env()` | n/a (no defect) | ledger writer | VERIFIED |
| L4-REAL-PTY | Facade contract: real-PTY surface | ledger | `Tui` builder (`new/cargo_bin` eager resolve via `cargo_bin_path`, `arg/args`, `size`, child-only env/cwd, profile, `spawn`; secret-safe `Debug`); waits incl `wait_frame` → always `WaitError::Unsupported` + evidence snapshot, `wait_exit`, `wait_predicate`; input `send_text/send_bytes`; teardown `close` | n/a (no defect) | ledger writer | VERIFIED |
| L5-CLI-PROTO | Facade contract: CLI+proto grammar | ledger | `Cmd` 16 variants (`Init/Doctor/Schema/Capture/Inspect/Render/Diff/Review/Accept/Report/Import/Session/Record/Trace/Machine/DaemonInternal` + `Session` subops `Start/Stop/List/Prune/Attach/Input`); exits 0 ok / 2 usage (`EXIT_USAGE`) / 3 op error (`EXIT_OP_ERROR`) / 4 verify-fail (`EXIT_VERIFY_FAIL`); full matrix in `crates/tuiscotti-cli/SYNTAX.md` | n/a (no defect) | ledger writer | VERIFIED |
| L6-NEXTEST-IDS-RESOLVE | Runner contract: stable ids, no-guess resolve | ledger | `BaselineId`/`AttemptId` (`from_env/from_map`, `stable_key`, `with_variant/with_shard`); `resolve_bin[_with_map]` consults `NEXTEST_BIN_EXE_*` + `cargo_bin_env_names`, dedupes identical paths, `Missing`/`Ambiguous` typed errors — never probes `target/` nor nests `cargo build` | n/a (no defect) | ledger writer | VERIFIED |
| L7-JOURNAL-FAIL-CLOSED | Runner contract: incomplete until proven complete | ledger | `Journal::complete` writes `COMPLETE` marker; `Journal::status` = `Complete` only when marker exists AND journal tail is a `complete` event; missing/unreadable/non-complete-tail → `Incomplete` with reason | n/a (no defect) | ledger writer | VERIFIED |
| L8-CARGO-BIN-FIXTURES | Resolver contract + fixture inventory | ledger | `cargo_bin_path` order: `CARGO_BIN_EXE_<name>` exact → normalized → next-to-exe → `deps/` parent → cwd `target/debug`+`target/release` (`is_file` only, typed `SpawnError` lists all searched); `Tui::cargo_bin` delegates to it; 3 fixture bins: `menu_fixture`, `streams_fixture`, `protocol_fixture` | n/a (no defect) | ledger writer | VERIFIED |
| TERMPANE-SRCQUAL-6/6 | Source qual: upstream rev works, registry still unqualified | ledger | `/tmp/termpane-srcqual` (git `rev=dc40286f9a8942f19f3a4cdebaaf1e18b7321709`, feature `pty`): `cargo test` 6/6 PASS — spawn_true/outcome, stty-size+resize, stdin-close-EOFs-cat, observe-revision, finish-kills-sleep, close-idempotent. SOURCE-ONLY, NOT registry (per crate README) | n/a (no defect) | ledger writer | VERIFIED-QUALIFIED (source only) |
| EXTCONSUM-ABC | External proof: facade works from outside the repo | ledger | `/tmp/tuiscotti-extconsum` (path-dep on `crates/tuiscotti`, repo untouched — `git status` unchanged): (A) pure-view canonical state, (B) piped echo exit+output+env, (C) real-PTY menu journey — 3/3 PASS under BOTH `cargo test` and `cargo nextest run` | n/a (no defect) | ledger writer | VERIFIED |

## Pending revalidation

| ID | Impact | Owner | Reproducer / evidence | Fixing commit | Verifier | Disposition |
|----|--------|-------|----------------------|---------------|----------|-------------|
| F02 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F03 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F04 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F05 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F06 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F07 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F08 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F09 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F10 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F11 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F12 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F13 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| ACCEPTANCE-BUDGETS | TBD | TBD | PENDING revalidation | — | — | PENDING |

## Changelog

- v1: seeded verified entries (1)–(7) at `f89d522`; F02–F13 + acceptance budgets PENDING. No commit.
- v2: added L1–L8 synthesis seeds (dep boundary, direct-view, piped-CLI, real-PTY, CLI+proto, nextest ids+resolve, journal fail-closed, cargo-bin+fixtures), TERMPANE-SRCQUAL-6/6 (source-only), EXTCONSUM-ABC (cargo test + nextest, repo untouched); all verified at `b409d71`; HEAD advanced to `aed37eb` by a docs-only sibling commit (swap plan, no code impact). F02–F13 + acceptance budgets stay PENDING. No commit.
