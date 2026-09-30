# Snapshot and approval semantics

Three gates, one rule: **actuals are written before any assertion,
approvals change only by explicit review.** CI must never approve
snapshots by itself — there is no bless flag, env var, or accepting
CI mode, and a test proves no env var accepts.

## Classic store (`snapshot::Store`)

Layout under one root:

```text
<store>/approved/<name>.frame.json   # committed: canonical cells
<store>/approved/<name>.png          # committed: approved pixels (required)
<store>/actual/<name>.frame.json     # per-run evidence (gitignored)
<store>/actual/<name>.png            # + <name>.png.fidelity.json
<store>/actual/<name>.manifest.json  # candidate seal (sha256 trio + profile)
<store>/diff/<name>.png              # red-overlay diff, on mismatch
<store>/report.html                  # review index; links files, never embeds
```

`check` / `check_with` (caller-owned `Renderer` for suite-wide glyph
cache reuse) always write the actual trio + manifest first, sealed
last: a crash between writes leaves an absent or disagreeing
manifest, and `verify_candidate` reports `capture-incomplete`
instead of letting a later report silently re-render the gap.

Gate order per name: name validation (rejects escapes before any
write) → threshold validation (`PerceptualPolicy` rejects
NaN/out-of-range) → actual validate + render + write trio + seal →
approved frame read (missing → `missing-approval`) → cell compare
(exact; cursor-only changes reported as cursor summaries) →
approved PNG read **from disk** (missing → `missing-approval`,
never regenerated in memory: a renderer upgrade must fail loudly,
not re-render the expectation it gates) → decoded-pixel compare.

Statuses (`snapshot::Status`): `matched`, `cells-differ`,
`pixels-differ`, `dimension-mismatch`, `missing-approval`,
`corrupt-approval`, `capture-incomplete`, `not-checked`
(`verify_candidate` only: trio intact, no verdict yet).
`CompareOutcome` is `#[must_use]` — dropping one without
`ensure_matched()` warns instead of silently passing.

Acceptance is explicit and per-name: `Store::accept(name)` (Rust)
or `tuiscotti accept --store <dir> <name>` (CLI). `actual_names()`
lists reviewable candidates. `report` / `report_with` re-verify and
rewrite `report.html`; `StoreReport::failed()` counts failures.

## Grouped store (`grouped::GroupedStore`)

For suites that want nested scenario names and committed,
human-reviewable artifacts. A scenario `<group>/…/<name>` commits
EXACTLY four files under the approved root — no `.frame.json`, no
sidecars:

```text
snapshots/<group>/<name>_<cols>x<rows>_<profile>.ansi   # normalized SGR dump (cell-exact gate)
snapshots/<group>/<name>_<cols>x<rows>_<profile>.txt    # plain black-and-white text
snapshots/<group>/<name>_<cols>x<rows>_<profile>.png    # colored image (pixel gate)
snapshots/<group>/<name>_<cols>x<rows>_<profile>.html   # standalone colored HTML render
```

Actuals (`<root>.actual/`), diff PNGs (`<root>.diff/`), and the
report (`<root>.actual/report.html`) live OUTSIDE the approved tree
by default; override with `with_actual_root` / `with_diff_root` /
`with_report_path`. Gates: `.ansi`/`.txt`/`.html` byte-compares
plus the same exact decoded-pixel PNG gate as the classic store.
Actual PNG/HTML always render fresh from the candidate frame —
never copied from approved. Missing approvals fail closed; names
with absolute paths, `..`, empty segments, or backslashes are
rejected (`InvalidName`). `accept(name)` blesses one scenario,
`accept_all()` blesses every reviewed actual recursively and
returns the accepted names.

The classic `Store` is unaffected; both share statuses, report
machinery, and the renderer.

## Generations (same-sample binding)

Every artifact of one capture belongs to one `formats::Generation`
(frame + profile name). For the macro gates,
`assert::generation_id` is the hex SHA-256 of the canonical text,
embedded in each `.snap` description (`tuiscotti generation <hex>
render <profile>/rv<N>/straight-rgba at <file>:<line>`) AND in the
PNG `tEXt` chunk (`png_tag_generation`). `check_consistent` reads
all three bindings (canonical `.snap`, PNG `.snap`, sidecar bytes)
and fails on any mix — never half-passes. A changed screen yields a
new generation; a changed renderer names itself in the description
via the render identity.

## Frozen vs evolving review

Evolving roots accept: `Store::accept`, `GroupedStore::accept` /
`accept_all`, `tuiscotti accept <name>`, `cargo insta review` —
always explicit, per-name (or per-reviewed-actual), on a
workstation. Frozen roots never accept:

- Layout: `<root>/<name>.canonical.txt` + `<root>/<name>.png`
  (generation-tagged bytes).
- Gates: `check_frozen_snapshot` / `check_frozen_screenshot` /
  `assert_frozen_snapshot` / `assert_frozen_screenshot`, or
  `Policy::Frozen{root}`. Read-only: missing/corrupt files fail,
  never self-heal.
- `frozen_accept` always errors (`FrozenError::AcceptRejected`).
- CLI: `tuiscotti import --dir <tree>` is the read-only frozen view
  (`<stem>.{ansi,txt,png,html}` → scenarios; extras reported, never
  fatal). `tuiscotti accept` against a frozen root rejects.

## SHA256SUMS (fixture approvals)

`crates/tuiscotti-fixtures/tests/SHA256SUMS` pins every committed
approval byte. The `sha256sums_manifest_pins_every_approval` test
fails on any drift; after a qualified approval change, re-record
with `cargo xtask fixtures --bless-manifest`. This is tooling
(`xtask`), not product code — no snapshot gate reads the manifest
at runtime.

## Macro gates (`assert_snapshot!` / `assert_screenshot!`)

- `assert_snapshot!(name, screen)` — Insta text gate over the
  canonical projection (`screen::canonical_string`).
- `assert_screenshot!(name, screen)` — compound gate: canonical
  text + generation-tagged PNG as one sample. Evidence lands on
  disk BEFORE failure; mixed generations fail, never half-pass
  (`generation_id`, `png_tag_generation`, `check_consistent`).
- `Policy::EvolvingIn { snapshots, evidence }` pins explicit dirs;
  frozen roots (`check_frozen_*`) are read-only and never
  self-heal — `frozen_accept` always errors.
- `emit_four` exports one generation as `.ansi`/`.txt`/`.png`/`.html`;
  `import_frozen_v1` reads a frozen tree back read-only.

Review with `cargo insta review`. `INSTA_UPDATE=no` keeps suites
fail-closed in automation.

## Pixel gate

The PNG gate compares exact decoded pixels: re-encoding passes,
one changed channel fails. Review leniency lives only on a
validated `PerceptualPolicy` threshold passed to `check`. Fidelity
sidecars (`<name>.png.fidelity.json`) record missing/fallback
glyphs next to every PNG; fallback-served cells are listed in
`fallback_glyphs` (omitted when empty, so sidecars stay
byte-stable). Same frame + same profile + same vendored font bytes
= byte-identical PNGs — no system fonts anywhere on the PNG path.
