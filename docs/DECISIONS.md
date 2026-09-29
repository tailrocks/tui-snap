# Durable decisions

Decisions that must survive refactors. Each names the rule, the
rationale, and where it is pinned in code/tests.

## 1. CI must never auto-accept

There is no bless flag, env var, or accepting CI mode — and there
must never be. Approval is `Store::accept` /
`GroupedStore::accept(_all)` / `tuiscotti accept` (per-name) /
`cargo insta review`, on a workstation only, after reviewing
actuals. A test proves no env var accepts. Rationale: ambient
approval lets a broken renderer bless its own breakage.

## 2. Fail closed everywhere

Missing or corrupt approvals fail the gate; unknown input to a
guard fails rather than passes. Concretes: missing approved PNG is
`missing-approval` (never regenerated in memory — a renderer
upgrade must fail loudly, not re-render the expectation it gates);
interrupted candidate writes are `capture-incomplete` via the
sealed manifest; mouse input without app-enabled reporting is
`ModeNotEnabled`, never silently dropped; `PerceptualPolicy`
rejects NaN/out-of-range thresholds; `#[must_use]` on outcomes
turns dropped gates into warnings.

## 3. Actuals before assertions

Every gate writes reviewable evidence (actual frame/PNG/fidelity,
diff PNG, report) BEFORE the assertion runs, so a failure still
leaves the full story on disk. Timeouts fail WITH the last
screen/observation, not a bare deadline error.

## 4. Exact decoded-pixel gate

The PNG gate compares decoded pixels: re-encoding passes, one
changed channel fails. Review leniency lives only on a validated
perceptual threshold. Rationale: fuzzy-by-default gates hide
renderer regressions; strict-by-default with explicit leniency
keeps every exception visible at the call site.

## 5. Deterministic rendering, no system fonts

Same frame + same profile + same vendored font bytes =
byte-identical PNGs on any machine. The renderer tries styled
primary → regular primary → pinned fallback chain (SHA-256
verified at load; mismatch refuses to render) → deterministic tofu
with correct advance + fidelity record. Offline `frame.json`
re-renders are byte-identical (pinned by tests). Runners need no
system fonts.

## 6. Pure-cargo PTY engine: portable-pty + alacritty_terminal

The PTY engine is `portable-pty` 0.9 (PTY owner) + `alacritty_
terminal` 0.26 (terminal state) from crates.io — no git/path deps,
no patches, no vendored forks. Replaces, in order: the vendored
vt100 fork, then the `termpane` git crate over vendored termlens.
The adapter owns input encoding and query answering (the emulator
parses modes but ships no encoder or query callbacks); that
behavior is qualified by `tool_qualification` + PTY matrix tests.
Known gaps (silent unknown sequences, dropped cell blink, no
scrollback reflow) are documented in LIMITATIONS.md, not hidden.

## 7. Typed op protocol over stdio, no daemon

Agents drive `proto::{Op, execute}` / `tuiscotti machine`: 17 typed
ops, JSON envelopes, versioned protocol (`2.0.0`) with a printed
schema (`tuiscotti schema`). Named sessions use versioned endpoint
files in an owner-only runtime dir — no daemon process, no sockets
to leak. The MCP bridge (`mcp::serve`) speaks the same ops.

## 8. Config responsibilities are split by owner

`tuiscotti.toml` (capture + assertion policy, owned by tuiscotti) vs
`.config/nextest.toml` (scheduling only, owned by cargo-nextest,
never parsed here) vs Insta config (review behaviour, owned by
Insta). Printed by `tuiscotti init` and pinned in `proto::CONFIG_DOCS`.
Rationale: each tool owns its file; no cross-parsing, no drift.

## 9. Layering: pure core, thin edges

`tuiscotti-core` is pure (no rendering, PTY, or I/O) and every
crate depends on it; `tuiscotti` is re-exports only; `tuiscotti`
`main.rs` is arg parsing over the facade. Only `tuiscotti-runtime`
may spawn. New public API goes in the owning leaf crate and is
re-exported. Rationale: the dependency graph in ARCHITECTURE.md is
a reviewable contract — a new edge is a design decision, not an
accident.

## 10. No legacy code

Migrations finish: old paths are removed completely, never shimmed
or aliased. Breaking changes are preferred over compatibility
layers. This file, MIGRATION.md, and git history are the record —
not the source tree.

## 11. Superseded ledger requirements (old 82-item ledger retired)

The old `docs/REDESIGN-LEDGER.md` (82 items, frozen at `d6b2574`) was
retired with the crates/ restructure; full text survives in git history
(e.g. `git show 2509bbf:docs/REDESIGN-LEDGER.md`). Disposition:

- SUPERSEDED — A09 (thin JS/TS + Python clients): foreign-language SDKs
  are out of scope. Clients, manifests, and client tests were removed;
  transport stays Rust-only (`tuiscotti` + `tuiscotti machine`).
- SUPERSEDED — R04 (registry portable-pty + alacritty_terminal backend):
  replaced by the termpane-only backend boundary (G1). No direct,
  renamed, or target-specific dependency on portable-pty,
  alacritty_terminal, or libc remains permitted in product code.
- SUPERSEDED — R05 (Ghostty binding qualification): moot once the
  backend decision became termpane-only; no alternative emulator is
  evaluated.
- RETAINED (re-mechanized) — M09 (pure-view builds without PTY/native
  deps): still required as workspace policy (pure view/screenshot
  configuration must not compile the terminal runtime); enforced via
  crate layering and feature isolation instead of the old root
  `pty` feature.

Every other still-relevant correctness, three-mode testing, fidelity,
lifecycle, Insta, nextest, and Rust/CLI requirement from the old ledger
is retained and covered by the current suite; nothing was deleted merely
because the ledger file was removed.
