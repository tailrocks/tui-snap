# termpane registry release — handoff contract

Consumer: tui-snap PR #6 (`termpane` backend swap). Verified 2026-09-30.
Source of API truth: [TERMPANE-SWAP-PLAN.md](TERMPANE-SWAP-PLAN.md) (pinned to
`dc40286`); this file adds only release/registry facts.

## 1. Required API (all refs = swap-plan sections)

- Transport (§2 T1–T10): `pty::spawn_pty(&SpawnParams, cols, rows)`,
  `Master` reader/writer/resize/size, `PtyChild` pid/try_wait/wait/kill,
  `process::ExitStatus/signal/pid_alive` + `SIG*` consts.
- `SpawnParams` (§2 T1/E1): verbatim argv, overrides-only env,
  `env_clear`/`env_remove`/`current_dir`/`detached`. Raw-`spawn_pty`
  path required — `PtySession::spawn` injects `COLORTERM=truecolor`.
- Grid/observe (§2 O1–O4/O8–O12/R1, D2): `DamageGrid::process`,
  `drain_passthrough`, `dump() -> GridSnapshot`, mode/cursor getters.
- `wait_frame` (§2 O15/D6): follow-up only; swap keeps
  `WaitError::Unsupported` (two-phase).
- Input encodings (§2 I1–I5): `kitty_kb_flags`,
  `application_cursor`, `mouse_protocol_mode/encoding`,
  `bracketed_paste`, `focus_events`, `PtyWriter`.
- Gaps NOT required for release: O5 (modes 4/6/20), O6 (OSC 4
  palette), O7 (OSC 10/11 set), O13 (CSI 14/16/18t) — see §5 handoff.

## 2. Candidate commit

- `dc40286` → `dc40286f9a8942f19f3a4cdebaaf1e18b7321709` (VERIFIED via
  `gh api repos/tailrocks/termpane/commits/dc40286`).
- Release MUST contain the §1 surface; re-run the §2 map row-by-row
  against the released source (§6). No yanked/pre-release.

## 3. Package version

- **TBD-UNPUBLISHED** — `curl -A tui-snap-ci-check/1.0
  https://crates.io/api/v1/crates/termpane` returns HTTP 404
  `{"errors":[{"detail":"crate \`termpane\` does not exist"}]}`;
  sparse index `https://index.crates.io/te/rm/termpane` also 404.
- `Cargo.toml` on main says `0.1.0` (swap plan D10) — watch value
  only, not a pin.

## 4. Supported targets

- Unix-only per swap plan E2 (`process`/`pty`/`session` carry
  `compile_error!` off-Unix). Consumer needs: Linux + macOS native;
  swap Unix-gates the `pty` feature and keeps non-Unix stubs.

## 5. Registry identity

- crates.io crate `termpane`, sparse index
  (`https://index.crates.io/te/rm/termpane`). Exact `=X.Y.Z` pin at
  swap time; git/path/`[patch]`/ranges forbidden (§7).

## 6. Release workflow + provenance

- Owned by the Velnor parallel team (upstream `tailrocks` owners
  hold publish rights; this project cannot self-publish — §0).
- Velnor PR #1 "Velnor Actions V1 implementation (under
  qualification)" (`docs/velnor-actions-spec` → `main`): head
  `b2a0c56`, state OPEN/MERGEABLE, mergeStateStatus UNSTABLE
  (`Plan` check FAILURE, rest SUCCESS/SKIPPED) — NOT qualified.
- Tag / dry-run / CI-green ≠ registry proof. Proof is ONLY: HTTP 200
  from both URLs in §3 for the released version.

## 7. Consumer smoke tests (re-run registry-only post-release)

Ledger v3 `TERMPANE-SRCQUAL-6/6` (source-only, NOT registry proof):
spawn_true/outcome, stty-size+resize, stdin-close-EOFs-cat,
observe-revision, finish-kills-sleep, close-idempotent. Must re-run
6/6 against the registry release before the swap lands.

## Gate status

**BLOCKED ON REGISTRY RELEASE** (fresh curl 2026-09-30: crates.io 404).
