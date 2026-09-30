# PR #6 Closure Ledger — v3

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
| C1-REGISTRY-SAFETY | Concurrency audit | ledger | registry concurrent access race-free | n/a (no defect) | ledger writer | VERIFIED |
| C2-XPROC-LIFECYCLE | Concurrency audit | ledger | cross-process session/child lifecycle sound | n/a (no defect) | ledger writer | VERIFIED |
| C3-IDENTITY-SEPARATION | Concurrency audit | ledger | baseline/attempt/session identities never conflated | n/a (no defect) | ledger writer | VERIFIED |
| C4-ESCAPING | Concurrency audit | ledger | shell/arg/env escaping correct, no injection | n/a (no defect) | ledger writer | VERIFIED |
| C5-RUNNER-NEUTRAL-CTX | Concurrency audit | ledger | contexts runner-neutral (nextest/cargo/standalone) | n/a (no defect) | ledger writer | VERIFIED |
| C6-QUEUES | Concurrency audit | ledger | internal queues bounded + race-free | n/a (no defect) | ledger writer | VERIFIED |
| C7-LOGS-DIMS | Concurrency audit | ledger | logs + terminal dims consistent under concurrency | n/a (no defect) | ledger writer | VERIFIED |
| C8-FDS | Concurrency audit | ledger | FD ownership sound (see C-R2 macOS residual) | n/a (no defect) | ledger writer | VERIFIED |
| C9-NO-SERIALIZATION | Concurrency audit | ledger | no hidden global serialization bottleneck | n/a (no defect) | ledger writer | VERIFIED |
| C-R1-PID-REUSE | Residual: pid-reuse signaling on orphan path | TBD | documented residual: orphan-path kill may signal recycled pid | pending | ledger writer | VERIFIED-RESIDUAL |
| C-R2-MACOS-FD-LEAK | Residual: macOS fd leak, SPAWN_LOCK mitigation | TBD | documented residual: leak contained by SPAWN_LOCK, not eliminated | pending | ledger writer | VERIFIED-RESIDUAL |
| C-G1-RENDERCACHE-CAPS | GAP: RenderCache/glyph total caps + eviction | TBD (concurrency fixer in flight) | no total caps, no eviction policy | pending | ledger writer | GAP-OPEN |
| C-G2-ADMISSION | GAP: session/child admission control | TBD (concurrency fixer in flight) | unbounded session/child spawn, no admission gate | pending | ledger writer | GAP-OPEN |
| C-X1-NEXTEST-GROUP | FALSE: nextest terminal-e2e group mis-scoped + unmeasured max-threads | TBD (concurrency fixer in flight) | group scope claim wrong; max-threads effect unmeasured | pending | ledger writer | FALSE-OPEN |
| S1-FINGERPRINTS | Snapshot audit: fingerprints incl underline color | ledger | fingerprint covers underline color + all tracked attrs | n/a (no defect) | ledger writer | VERIFIED |
| S2-VALIDATION | Snapshot audit: validation | ledger | snapshot validation rejects malformed inputs | n/a (no defect) | ledger writer | VERIFIED |
| S3-ATOMIC-PUBLISH | Snapshot audit: atomic publish | ledger | publish atomic, no torn snapshots | n/a (no defect) | ledger writer | VERIFIED |
| S4-CORRUPT-APPROVAL-FAILS | Snapshot audit: corrupt approval fails | ledger | corrupt approval data fails closed | n/a (no defect) | ledger writer | VERIFIED |
| S5-SUFFIX-IDENTITY | Snapshot audit: suffix identity | ledger | suffix scheme uniquely identifies variants | n/a (no defect) | ledger writer | VERIFIED |
| S6-BUNDLE-BEFORE-FAILURE | Snapshot audit: bundle before failure | ledger | evidence bundle written before failure surfaces | n/a (no defect) | ledger writer | VERIFIED |
| S7-STRICT-PIXEL-COMPARE | Snapshot audit: strict pixel compare | ledger | pixel compare strict, no silent tolerance | n/a (no defect) | ledger writer | VERIFIED |
| SNAP-A3 | F09 gap: no positive key-move test for face-pin/fallback-sha/version | TBD (snapshot fixer in flight) | only refusal tests; needs 2nd font fixture, two valid profiles differing in face bytes → keys differ | pending | ledger writer | GAP-OPEN |
| SNAP-B3 | F09 gap: huge/zero-dim IHDR path untested | TBD (snapshot fixer in flight) | code bounds pre-decode (entry.rs:31) but no test feeds 20000×20000 IHDR → put-err/get-miss+evict | pending | ledger writer | GAP-OPEN |
| SNAP-C | F09 gap: symlink-plant exclusivity + non-exclusive write_atomic | TBD (snapshot fixer in flight) | cache symlink-plant test missing; snapshot write_atomic uses non-exclusive fs::write (types.rs:220); evidence failure cleanup untested | pending | ledger writer | GAP-OPEN |
| SNAP-E | F09 gap: cache on/off verdict agreement unproven, cache unwired | TBD (snapshot fixer in flight) | RenderCache has zero production callers; no gate-verdict comparison no_cache on/off; RENDER_NO_CACHE path untested | pending | ledger writer | GAP-OPEN |
| SNAP-F | F10 gap: no default-placement macro e2e | TBD (snapshot fixer in flight) | all macro tests use explicit EvolvingIn dirs; None-override caller snapshots/ landing unproven | pending | ledger writer | GAP-OPEN |
| SNAP-G | F10 gap: Insta source: never asserted on real pendings | TBD (snapshot fixer in flight) | description tokens asserted; source:-names-caller missing | pending | ledger writer | GAP-OPEN |
| SNAP-I | F10 gap: render_identity() is a constant, profile leg untested | TBD (snapshot fixer in flight) | face hashes/geometry/palette/fallbacks unrepresented; same screen scale-2-vs-1 strings identical; Insta path skips primary-hash verification | pending | ledger writer | GAP-OPEN |
| SNAP-K | F10 gap: first-run accept path uses synthesized headers | TBD (snapshot fixer in flight) | pending headers/tags never parsed; rerun helpers hardcode approvals instead of rename-then-rerun | pending | ledger writer | GAP-OPEN |
| SNAP-L | F10 gap: frozen tag checks v1 id, not v2 binding | TBD (snapshot fixer in flight) | frozen.rs:227 uses canonical-only generation_id; no profile/face pins; drift cause unnamed (pixels still catch) | pending | ledger writer | GAP-OPEN |
| PERF-SEC7 | Perf audit: section-7 NOT ready | TBD (benchmark builder in flight) | 8/8 budgets GAP, 7 tooling gaps | pending | ledger writer | GAP-OPEN |

## Pending revalidation

| ID | Impact | Owner | Reproducer / evidence | Fixing commit | Verifier | Disposition |
|----|--------|-------|----------------------|---------------|----------|-------------|
| F02 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F03 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F04 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F05 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F06 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F07 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F08 | Concurrency audit fallout (C-G1/G2 gaps, C-X1 false) | TBD (concurrency fixer in flight) | C1–C9 VERIFIED, C-R1/R2 residuals documented; C-G1/G2 + C-X1 open | pending | ledger writer | OPEN |
| F09 | Cache fallout (SNAP-A3/B3/C/E) + caps gap (C-G1) | TBD (snapshot + concurrency fixers in flight) | fingerprints/validation/publish/fail-closed VERIFIED; A3/B3/C/E + total-caps/eviction open | pending | ledger writer | OPEN |
| F10 | Insta fallout (SNAP-F/G/I/K/L) | TBD (snapshot fixer in flight) | suffix/bundle/compare VERIFIED; F/G/I/K/L open | pending | ledger writer | OPEN |
| SEC7 | Benchmark suite + 8 budgets | TBD (benchmark builder in flight) | section-7 NOT ready; 8/8 budgets GAP; 7 tooling gaps (PERF-SEC7) | pending | ledger writer | OPEN |
| F11 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F12 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| F13 | TBD | TBD | PENDING revalidation | — | — | PENDING |
| ACCEPTANCE-BUDGETS | TBD | TBD | PENDING revalidation | — | — | PENDING |

## Changelog

- v1: seeded verified entries (1)–(7) at `f89d522`; F02–F13 + acceptance budgets PENDING. No commit.
- v2: added L1–L8 synthesis seeds (dep boundary, direct-view, piped-CLI, real-PTY, CLI+proto, nextest ids+resolve, journal fail-closed, cargo-bin+fixtures), TERMPANE-SRCQUAL-6/6 (source-only), EXTCONSUM-ABC (cargo test + nextest, repo untouched); all verified at `b409d71`; HEAD advanced to `aed37eb` by a docs-only sibling commit (swap plan, no code impact). F02–F13 + acceptance budgets stay PENDING. No commit.
- v3: concurrency audit — C1–C9 VERIFIED (registry safety, xproc lifecycle, identity separation, escaping, runner-neutral contexts, queues, logs+dims, FDs, no-serialization), C-R1/R2 residuals documented (pid-reuse orphan signal; macOS fd leak w/ SPAWN_LOCK), C-G1/G2 GAP (RenderCache/glyph caps+eviction; session/child admission), C-X1 FALSE (nextest terminal-e2e mis-scope + unmeasured max-threads) — fixer in flight. Snapshot audit — 16 checks, no FALSE; S1–S7 VERIFIED (fingerprints incl underline color, validation, atomic publish, corrupt-approval-fails, suffix identity, bundle-before-failure, strict pixel compare), SNAP-A3/B3/C/E/F/G/I/K/L gaps — fixer in flight. Perf audit — PERF-SEC7 NOT ready, 8/8 budgets GAP, 7 tooling gaps — benchmark builder in flight. F08/F09/F10 → OPEN with fixers assigned (not closed). No commit.
