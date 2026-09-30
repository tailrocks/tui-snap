# PR #6 Closure Ledger — v1

Single source of truth for PR #6 closure. Every row: ID, impact, owner, reproducer/evidence, fixing commit, verifier, disposition. Unlisted items do not exist as findings. PENDING = not yet revalidated at current HEAD.

HEAD: `f89d522d0ec36765cc360ec7ebb422374d8bc3b5` · base `main@9dc86da` · PR OPEN/MERGEABLE · 15/15 checks green · reviewDecision empty (no approval) · 0 unresolved threads.

## Verified entries

| ID | Impact | Owner | Reproducer / evidence | Fixing commit | Verifier | Disposition |
|----|--------|-------|----------------------|---------------|----------|-------------|
| F1 legacy-importer | Was: blocks merge if shims/execution present | ledger | `import_compat` metadata+read only; canary test `imports_never_write_nor_execute_canary`; no vendored engine/shims; packets bless read-only import | n/a (no defect) | ledger writer | VERIFIED-KEEP-RESOLVED |
| NEXTEST-BASELINE | Baseline gate | ledger | nextest 618/618 GREEN, 42s wall at `f89d522` | n/a | ledger writer | VERIFIED |
| TERMPANE-MAIN-API | Swap-plan input | ledger | termpane main `dc40286`: process/pty/session modules, Unix-only, MSRV 1.97, unsafe forbid | n/a | ledger writer | VERIFIED-QUALIFIED (API only) |
| TERMPANE-REGISTRY | Merge gate: registry UNPUBLISHED (sparse index 404) | TBD (external: termpane owner) | sparse index 404 | n/a | ledger writer | VERIFIED-OPEN (blocks any termpane-dep merge) |
| VELNOR-PR1 | External reference only | external owner | Velnor PR #1 head `c8b3a89`, gates red, NOT qualified, no termpane workflows | n/a | ledger writer | VERIFIED-OUT-OF-SCOPE |
| SWAP-PLAN-DOC | Doc incomplete | TBD | `TERMPANE-SWAP-PLAN.md` truncated at line 328, rewrite in progress | pending | ledger writer | VERIFIED-OPEN |
| PERF-NUMBERS | Stale data | TBD | `PERFORMANCE.md` holds prior-head (`0f14262`/`75ff479`) numbers only — re-measurement required | pending | ledger writer | VERIFIED-OPEN |

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
