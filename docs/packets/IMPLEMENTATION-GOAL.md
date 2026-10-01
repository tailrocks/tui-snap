# Tuiscotti: corrective implementation /goal

Copy from `/goal` below. All proposed interfaces and policy inputs require implementation and execution verification.

/goal

Correct and complete PR #6 as Tuiscotti: a Rust-only TUI visual-regression and CLI testing toolkit, with termpane as its only terminal backend boundary, native Ratatui/Insta/nextest workflows, strict Rust governance, a deliberately small public API, and one coherent CLI.

This is an implementation goal, not a request for another proposal. Deliver the corrected code, upstream termpane improvements, tests, fixture applications, documentation, tooling, and consistent branding. Preserve working features and immutable historical evidence; remove the wrong backend, foreign-language clients/scripts, obsolete APIs, and architectural duplication. Breaking changes are preferred to maintaining the wrong abstraction.

REPOSITORIES AND INPUTS

Primary repository: https://github.com/tailrocks/tui-snap
Implementation PR: https://github.com/tailrocks/tui-snap/pull/6
Observed branch: redesign/rust-first-testing-platform
Observed PR head: 7e8272bc08dd3241d731f832c573fd4c6de3fe1e
Observed base: 9dc86daff1dcbf20805b145916e8f04e9515f929
Upstream terminal library: https://github.com/tailrocks/termpane
Observed termpane head: 8ff87fe1795b5a246214e9dc8a2a000c8746dab5
Review date: 2026-09-29

Read the current PR, changed source, all reviews/comments/replies, root and nested AGENTS.md, manifests, fixtures, tooling, CI generator inputs, and docs. Read REFERENCE-SPEC.md and config-reference/ from the accompanying packet when available. This prompt is self-contained about mandatory scope; a missing packet is not permission to stop. The PR's REDESIGN-PLAN/BACKLOG/LEDGER and previous research are context, not authority to override this correction.

Compare current source and public Rust/CLI workflows with:
- https://github.com/microsoft/tui-test
- https://github.com/anomalyco/terminal-control
- https://github.com/vyncint/termlens
- https://github.com/mitsuhiko/insta
- https://insta.rs/docs/
- https://nexte.st/docs/
- https://rust-lang.github.io/api-guidelines/
- https://mr-boxington.jdx.dev
- https://docs.renovatebot.com/
- https://alint.org/docs/

Refresh refs and pin the revisions actually analyzed. Never reset a branch to the observed SHA or discard newer work merely to reproduce this review. PR-reported test counts and “82/82” are claims to verify, not acceptance evidence.

PRECEDENCE AND CORRECTED SCOPE

The latest eight user goals supersede conflicting earlier research or implementation directions. Specifically:
- Do not select Alacritty, Ghostty, tui-test, Terminal Control, or termlens as an alternative product backend. Use termpane; extend it upstream where required.
- Remove Python and JavaScript/TypeScript clients and scripts. Foreign-language SDKs are OUT OF SCOPE, not an optional feature to complete later in this implementation.
- Keep the useful Rust API, CLI, CLI machine output, native Insta, nextest, safe offline evidence, traces and Rust-owned session features. Do not delete them merely because foreign SDKs are removed.
- All first-party Rust source belongs under crates/. Requested tests/fixtures paths are crate-relative.
- Proposed new identity is Tuiscotti, with descriptor “Rust TUI visual-regression toolkit.” Verify collisions before making the coordinated rename. Do not preserve live old-name aliases.
- Preserve historical approved artifact bytes and source provenance. Moving an artifact with a verified path/hash receipt is not regenerating it. Historical references may contain the old name; active APIs, commands, config and docs must not.
- Reconcile the old 82-item ledger: mark superseded backend/client requirements explicitly superseded; retain every still-relevant correctness, three-mode testing, fidelity, lifecycle, Insta, nextest and Rust/CLI capability requirement. Do not mechanically reimplement rejected features or delete remaining coverage.

MANDATORY EXECUTION RULES

Use subagents aggressively for all work.

Always delegate work to subagents whenever delegation is possible. Treat subagents as the default execution mechanism, not an optional optimization.

Your execution strategy must:
- decompose the goal into independent or partially independent workstreams;
- spawn subagents for each workstream;
- parallelize all work that can safely run concurrently;
- use additional subagents for research, implementation, review, testing, verification, and cross-checking;
- avoid doing work serially in the parent agent when it can be delegated;
- keep spawning useful subagents as new independent tasks are discovered;
- use independent subagents to verify important conclusions and completed changes;
- coordinate and synthesize subagent results into the final implementation.

Do not merely recommend parallelization—actually execute the goal through subagents.

The parent agent should primarily orchestrate, resolve dependencies/conflicts, integrate results, run final deterministic checks, and ensure the complete goal is finished.

Default rule: **delegate first, parallelize aggressively, verify independently, then integrate.**

Always commit changes frequently while working.

Prefer small, incremental, logically scoped commits instead of keeping a large dirty working tree for a long time and committing everything at the end. As soon as a meaningful unit of work is complete and verified, commit it.

Push progress to the remote repository regularly so work is continuously propagated, recoverable, reviewable, and easy to bisect or revert.

At the same time, avoid unnecessary branches. Prefer doing as much work as possible on a single working branch and keep committing to that branch throughout the task.

Create additional branches only when there is a clear technical or workflow reason that makes working safely on the existing branch impractical or impossible.

In short: **commit often, push regularly, and minimize branch proliferation.**

Never ask the user questions or wait for clarification. Work fully autonomously.

If anything is ambiguous, uncertain, conflicting, incomplete, or requires a decision:
- Spawn subagents to investigate it independently.
- Analyze the available context, repository, documentation, code, history, external references, and relevant best practices.
- Research alternative approaches where necessary.
- Compare multiple options and their tradeoffs.
- Verify important assumptions and findings independently.
- Re-verify critical decisions before acting.
- Make the best reasonable decision yourself and continue execution.

Do not stop because information is imperfect. Infer intent from the goal, existing architecture, conventions, documentation, and surrounding context. Prefer making a well-researched, reversible decision over asking the user.

When uncertainty is significant, use multiple independent subagents to challenge the proposed solution and resolve disagreements through evidence.

Your responsibility is to unblock yourself. Questions that would normally be sent to the user should instead become internal research, analysis, verification, or subagent tasks.

Continue working until the goal is fully completed, verified, and no meaningful actionable work remains.

EXECUTION INTEGRITY AND GOVERNANCE

Honor any actual mandatory model/effort policy; do not override or pretend to verify unavailable controls. Do not fabricate agents, independent reviews, tests, performance measurements, package releases, or approvals. When the execution environment genuinely lacks a required facility, attempt supported alternatives, finish independent work, and record the exact blocker without converting it into success.

Use one working branch for this product and one focused upstream branch for necessary termpane work. Extra branches are justified only for a real repository/worktree reason. Give agents disjoint file ownership and explicit interfaces; serialize shared Git index/checkout/commit operations. Use isolated patch workspaces where overlapping changes cannot safely share a checkout. Preserve unrelated dirty work. Do not force-push shared history, broadly clean worktrees, or reset other agents' changes. Merge current main rather than rebasing shared work.

Use bounded parallel compilation, not redundant workspace builds fighting over one target directory. Keep logical commits verified and push regularly. No committed scratch agent logs, invented progress evidence, secret files, or unrelated product changes.

Generated .github behavior remains Velnor-owned where current repository policy says so. Change canonical inputs, improve the generic generator upstream when needed, and regenerate. Never hand-edit generated YAML or add a repository-specific script bypass. Review instructions independently before changing them.

Read all feedback again at the final head. Fix accepted findings with regression tests and reply linking the commit; reject incorrect feedback only with evidence before resolving. A running bot review is not an approval. Merge, publish a release, or perform hosted repository administration only within actual authorization and permissions; missing permissions remain explicitly incomplete external actions, not a reason to fake delivery.

0. AUDIT AND IMPLEMENTATION ORDER

Start parallel source audits for: termpane migration, Rust-only cleanup/xtask, public API/CLI, docs/comparisons, fixtures/format evidence, strict workspace/build policy, branding/licensing, and independent verification. Each returns source paths, identified contract, tests, risk, and proposed changes. Separate implementers from final reviewers of their own critical work.

Create an eight-goal acceptance matrix, linked to still-relevant original backlog IDs. Record existing approved hashes, effective dependency graphs, public API inventory, format/schema versions, CLI grammar, first-party scripts, and real failing tests. Keep durable product evidence, not transient coordination, in the repository.

Verify these source-derived findings against the refreshed head:
- Root pty feature directly enables portable-pty, alacritty_terminal, libc.
- Backend rejects per-cell blink and synchronized-output profiles.
- Root package remains edition 2021 and source is not a real crates/ workspace.
- Foreign SDKs and Python migration tooling remain.
- Main strips --machine from all arguments before Clap, including child arguments after --.
- Assertion implementation reports src/assert.rs as native Insta source rather than the caller.
- generation_id hashes canonical text only: reproduce whether same-screen/different-render samples can mix before describing the risk as a confirmed failure.
- cargo_bin heuristics can choose the first existing debug/release candidate rather than an authoritative selected target.
- Schema/API/completion claims conflict between source, README and PR body.

Write focused regressions for real findings. Preserve corrected pixel/approval behavior already present; do not blindly reapply outdated bug descriptions. Fix roots enabling related failures, not tests that happen to expose a symptom.

Recommended ordering: audit and negative tests; upstream termpane contract and optional transport; workspace/xtask/strict policy; backend replacement; public API/CLI; fixtures/all formats; docs/comparison and coordinated rename; final independent gate. Parallelize independent work throughout. Do not wait for unrelated upstream review before finishing available work.

G1. TERMPANE-ONLY BACKEND; EXTEND UPSTREAM FIRST

The product must have no direct, renamed, build/dev/target-specific dependency or source import/re-export of portable-pty, alacritty_terminal or libc. No second emulator, shadow parser, copied termpane/termlens source, hidden Cargo patch, or optional fallback. Keep termpane::DamageGrid as the terminal-state engine.

Important distinction: current termpane is a deterministic screen model, not a PTY/process library. Do not pretend replacing an import supplies transport. Add required reusable behavior in tailrocks/termpane itself, through reviewed upstream PRs:
- Preserve its model-only default = [] behavior, zero host-side effects, safe Rust and no native terminal engine requirement for passive users.
- Add feature-gated process supervision and PTY transport, for example process and pty features. Names must reflect verified implemented APIs, not assumptions.
- Expose safe launch/read/write/resize, owned child identity, exit/reap/terminate, bounded cancellation/shutdown, trailing-output drain, and supported descendant containment.
- Reuse safe official OS/PTY primitives privately inside termpane as necessary. First-party unsafe remains forbidden. A private portable-pty transport dependency is not the consumer's direct dependency; document it transparently. Do not move Alacritty inside termpane under another name. Do not promise zero transitive libc throughout unrelated upstream dependencies.
- Inventory existing native termpane cells, colors, underline style/color, hyperlink, cursor, replies, damage and snapshot APIs before adding duplicates.
- Add missing public observation state and mode-aware input encoding upstream: independent bold/dim, blink, styled continuations, default/indexed/RGB colors, cursor intent, palettes, keyboard/mouse/paste/focus modes, synchronized update completion and capability reporting.
- Preserve raw VT replay without process launch. One state/revision must coherently contain all required non-cell and cell observations.
- Keep assertions, rendering, Insta, nextest-specific metadata and product branding out of termpane.

Create minimal upstream reproducers and tests for each missing capability. Qualify stdio/PTY/environment/process operations on macOS and Linux, including the disclosed macOS guardian identity issue. Do not assume a platform-specific ps format or RAII protects against parent SIGKILL. Use safe process identity checks and never kill unrelated processes. Test panic, blocked read/write, cancellation, child exit with late output, resize races and unsupported boundaries.

After upstream acceptance and official release, use that released version from crates.io and lock it. The user-supplied deny policy rejects Git dependencies with an empty allowlist. A Git SHA or persistent external path override is NOT a compliant final dependency. Do not invent a version, silently relax deny.toml, publish to an account without permission, or substitute another backend while waiting. Complete other work and report a genuine release permission blocker if one exists.

Remove the superseded backend adapter only after the termpane-based path passes the same required behavioral and visual tests. Run external downstream consumers of the published termpane package. Inspect cargo metadata and source for disguised aliases, optional old backends and target-specific leaks. Freeze original approved evidence during the switch.

G2. RUST API AND CLI ONLY

Delete Python and TypeScript/JavaScript client implementations, package manifests/locks, bindings, generators, examples, tests, build/release jobs, setup instructions and publishing hooks. Search the entire repository, not just files changed in PR #6.

Replace active cross-language promises with at most one concise future-directions note stating that foreign SDKs are currently out of scope. Do not retain empty packages or renamed SDK scaffolding.

Retain useful Rust API and CLI machine/schema/session/trace capabilities. Rust-owned MCP or structured CLI output is not a foreign-language SDK. All transport interfaces reuse one Rust operation/result contract, not a duplicate engine.

Mark original A09 and related foreign-client requirements superseded by this request. “Full implementation” now means every retained requirement plus all eight corrections, not reviving removed scope.

G3. REMOVE PYTHON/JS/TS SCRIPTS; USE RUST XTASK

Remove all first-party Python/JS/TS executable content, including scripts outside the PR diff, extensionless shebangs, inline CI snippets, generated scripts, embedded python/node command strings and report JavaScript. Do not bypass this rule with python -c, node -e, Rust strings, TypeScript renamed as data, or a large shell rewrite.

Replace maintenance automation with crates/xtask, using focused safe Rust modules. Cover migration checks, fixture builds/exports, source policies, branding/dependency inspection, docs/examples, performance collection, packaging and font maintenance. Preserve exact existing font bytes unless a separately qualified change is intended; do not introduce Python/fonttools as a hidden dependency. Use a qualified Rust solution or unchanged upstream assets.

Keep xtask independent of the product dependency graph. Use the conventional cargo xtask pattern and a Mise/mbx-wrapped execution path. Verify custom subcommand forwarding rather than assuming flags. Avoid recursion into the same xtask invocation.

Use static HTML/CSS for portable reports; pre-render expected/actual/overlay/diff images and use native HTML disclosure/navigation. A richer review UI can be a Rust TUI. No inline JS or Node build is needed.

JSON/TOML/YAML configuration is permitted. Renovate and GitHub Actions are external tools whose implementation language does not authorize first-party script packages. Do not add local Node/Python tooling merely to validate them; use supported external pinned distributions or the configured service. Replace shell fixture applications with real Rust fixtures rather than building an alternate scripting system.

G4. RESTRUCTURE DOCUMENTATION AND COMPARISONS

Rewrite the active documentation around the actual product: concise README with three runnable workflows, short AGENTS, CONTRIBUTING/tooling, architecture and dependency graph, tests/fixtures, snapshot/approval semantics, public API design, CLI reference, comparisons, performance, migration, limitations and durable decisions. Consolidate duplicate root/docs research. Historical studies may remain clearly marked, but must not compete with current guidance.

Document truthfully which behavior is implemented, tested, partially supported, unsupported or future scope. Reconcile actual schema versions, fixture counts, visible versus canonical state, report portability, backend ownership and platform claims from code and tests. Delete stale “82/82” claims or replace them with current evidence; do not merely rename them.

Create a detailed comparison with microsoft/tui-test, anomalyco/terminal-control and vyncint/termlens. Pin the actual inspected revisions and cite source functions/tests. Compare Rust API and CLI separately: program launch, views, pipes, PTY, native arguments, configuration, waits, input, locators, state, snapshots, screenshots, all formats, Insta, nextest, lifecycle, diagnostics, safety, extensibility and compile cost.

For every meaningful public operation provide current syntax, competitor syntax, the semantic tradeoff, proposed syntax, reason and an executed contract test. Preserve competitors' strengths honestly: tui-test session-bound lazy locators, termlens compact builder/waits and native snapshot workflow, Terminal Control owned frames/detached rendering. Count documented-but-unexecuted behavior as unqualified, not proven. Do not score API quality solely by line count.

Use compiled/executed examples and doc tests. Generate low-level CLI reference from the implemented Clap grammar. Keep architectural explanations in docs, not lengthy agent instructions. Validate links and ensure shipped docs contain no removed-client or script commands.

G5. RUST FIXTURE APPLICATIONS AND ALL FORMATS

All .rs files must be under crates/, including examples, benches, tests, fixtures and xtask. Use crate-relative tests/fixtures, for example:

crates/tuiscotti-fixtures/
  Cargo.toml                    # publish = false; explicit Rust binary targets
  src/lib.rs                    # production-style views and deterministic models
  src/views/
  tests/fixtures/apps/menu.rs
  tests/fixtures/apps/streams.rs
  tests/fixtures/apps/protocol.rs
  tests/fixtures/data/
  tests/fixtures/expected/
  tests/format_contracts.rs
  tests/view_contracts.rs
  tests/interaction_contracts.rs

Compile binaries once through the outer build/nextest workflow; resolve authoritative built artifacts. No nested Cargo build per test, shell-only menu substitute, or first-found stale binary. Pure-view and live-TUI fixtures call the same actual rendering function; controller/action tests remain separate.

Support and test all these distinct formats:
- ASCII: explicit 7-bit diagnostic projection with documented substitution/loss policy.
- TXT: plain Unicode text, no escapes, explicit whitespace handling.
- ANSI: normalized VT/SGR screen output, not the original raw transcript.
- PNG: independently rendered pixels with strict explicit alpha/profile behavior.
- HTML: escaped static offline evidence, without JavaScript.
- Canonical JSON: complete versioned state and provenance for verification.

ASCII is not ANSI. Preserve both rather than silently interpreting the user's request as one. Lossy ASCII cannot stand in for Unicode/canonical equality.

Test formats on pure views, real PTY journeys and appropriate piped-output projections. Include sizes/themes, RGB/indexed/default colors, independent modifiers, styled spaces, wide continuations, combining characters, CJK/icons, clipped views, one-row/column static cases, cursor states, errors/empty/focus/selection, resize/paste/input and raw invalid UTF-8 pipe output.

Every capture exports one identifiable generation. Add negative tests for format content, loss/truncation reporting, changed pixels versus re-encoding, ANSI-only style changes, whitespace, hidden source data, HTML injection, missing/corrupt approvals and mixed generations. Preserve frozen artifact bytes; path moves require before/after hash mappings. New intentional baseline approvals require review, not a mass blessing command.

G6. REBUILD THE PUBLIC API AND CLI FOR IDIOMATIC RUST

Audit every public item and CLI operation. Export a small deliberate facade: Tui, Command, Screen, Error/Result and the necessary snapshot/render types. Stop requiring ordinary users to enter proto, insta_proto, worker, provenance and conversion internals. Keep advanced primitives available through purposeful documented modules, not blanket pub modules.

Conventions:
- One program plus args; native OsStr/OsString and Path/PathBuf.
- Std-like reusable command/launch builders with consistent setter ownership; explicit fallible side-effect boundaries. Validate cross-field configuration before launching/rendering. Do not force every harmless setter to return Result.
- Typed errors retaining sources/context, not Result<_, String> and erased auxiliary messages.
- Spawn/resolution/IO failures are errors; nonzero child exit is truthful ProcessOutput. Preserve exit/signal/timeout/output-limit distinctions, raw streams and incomplete drain metadata.
- Fallible UTF-8 views and explicitly named lossy access; no lossy conversion in equality checks.
- One invariant-preserving public Screen with renderers accepting &Screen. Historical wire DTOs stay isolated in read-only import.
- Simple waits use configured Duration defaults; explicit advanced deadlines/cancellation remain available without required token boilerplate.
- Live session-bound fresh locators with scoped/relative composition; pure detached query evaluation remains available for advanced usage. No manually fabricated revision/Observation in the basic example.
- Unique actions and stale-target checks; never click scrollback or retry destructive input because an assertion has not passed.
- Typed Key/modifier APIs and parsed strings through FromStr; advanced raw bytes/paste explicit.
- Distinct immediate observation, stable snapshot, synchronized-frame and exit waits; negative temporal assertions have explicit semantics.
- Meaningful Debug that does not leak secrets; validated constructors and accessors; From for infallible and TryFrom for fallible conversion. No speculative trait/plugin framework or public terminal-backend selector.
- Blocking Rust tests remain first-class; no mandatory async executor for pure views. Any async adapter needs a demonstrated use case and feature isolation.

Ordinary examples should resemble the following proposed shapes, adjusted only for verified idiomatic correctness:

    let screen = tuiscotti::ratatui::render((100, 30), |frame| {
        fixtures::render_settings(frame, &model);
    })?;
    tuiscotti::assert_screenshot!("settings", &screen);

    let mut app = tuiscotti::Tui::cargo_bin("menu-fixture")?
        .size(100, 30)
        .spawn()?;
    app.get_by_text("Ready").expect_visible()?;
    app.press("Ctrl+P")?;
    app.get_by_text("Settings").click()?;
    let screen = app.snapshot()?;
    tuiscotti::assert_screenshot!("settings-open", &screen);
    app.press("q")?;
    app.expect_exit().success()?;

Keep advanced renderer/comparison/policy options without returning to manual setup for every test. Tests use actual public API from external consumer crates, not private shortcuts.

Fix native Insta macro metadata at the caller, not just a location string in a description. Verify file/module/test identity, scoped settings, suffixes, hygienic expansion and single evaluation from an out-of-workspace consumer. Use public Insta APIs; no source fork/private internal clone.

Keep compound canonical/image approval tied to the same sample and relevant render profile/font/policy identity. Reproduce canonical-identical/render-different cases and partial acceptance. Evidence must be produced before failure. Never infer pixel equality from cells or reuse approved output as candidate evidence. Keep evolving review separate from immutable reference verification.

Eliminate global mutable test environment/CWD. Edition 2024 makes some environment mutation unsafe; solve architecture rather than weakening forbid unsafe. Configure child environments and isolated test subprocesses explicitly.

Refactor CLI using Clap-native typed subcommands/options. Replace hidden pre-scanned --machine with an explicit documented machine interface. Use args_os and OsString child argv. The parent's parser must never consume an argument after --; add the exact regression where a child receives --machine. Support typed formats, paths, useful help, consistent option names and explicitly documented tool-versus-child exit policies. Offline render/diff/report/import must not require enabling PTY.

Provide a syntax matrix spanning setup, pure render, piped launch, live launch, binary selection, waits, input, locators, capture, assertions, profile customization, all exports, frozen review, machine operations and cleanup. Do not remove advanced operations simply to make the table shorter.

G7. STRICT WORKSPACE, COMPILATION, TOOLING AND DEPENDENCIES

Make the root a virtual workspace. Start with separate core, render, runtime, Insta integration, facade, CLI, fixture and xtask crates; merge/split only for measured dependency/build benefits. All code under crates/, no runtime logic hidden in fixture exclusions. Centralize shared versions in workspace.dependencies and inherit package metadata/lints in every member. Keep tests in separate files.

Recommended dependencies:
- core: passive validated models/queries, no renderer, PTY, CLI or async runtime.
- render: core plus image/font/shaping functionality, no process launch.
- runtime: core plus feature-gated upstream termpane transport/model APIs; pipe-mode std primitives as appropriate; no alternative emulator/direct removed dependencies.
- Insta adapter: core/render and public Insta APIs; no PTY.
- facade: normal Rust API/Ratatui with deliberate optional features. Pure view/screenshot configuration does not compile the terminal runtime.
- CLI: depends on required library capabilities; normal library use never builds CLI.
- xtask: typed maintenance only; no facade/runtime dependency.
- fixtures: nonpublished consumers for tests, not production graph.

Use the exact policy appendix below. Every member declares edition.workspace, rust-version.workspace, license.workspace and [lints] workspace = true. Preserve all user-selected deny levels and Clippy settings. Do not weaken rules to get green builds, add broad allow attributes, hide implementation in fixtures, or use dummy cfgs/generated files to evade enforcement. Narrow justified expect attributes require a real false-positive and are checked for staleness.

Use manifest rust-version = "1.98" and edition/style_edition 2024. The verified normal toolchain patch is 1.98.1; recheck official stable documentation at execution, preserve the requested API floor, and keep Rust/Mise pins consistent. Do not release with a known-defective patch for the sake of a floor demonstration.

Use 80 Clippy-counted lines per function, 400 physical lines per ordinary Rust file, and 150 per lib.rs/main.rs. Adapt alint globs to Tuiscotti AND cover the facade, CLI and xtask. Prove gates fail using miniature negative repositories: 81 counted function lines, 401 physical lines, 151 root lines, missing files, wrong locations, forbidden scripts and missing lint inheritance. Ensure the glob matches real files; a successful empty match is not compliance.

Keep cargo-deny advisory ignores empty, wildcard versions denied, unknown registries/Git denied and allow-git empty. Do not broaden license allowances as a shortcut. Own workspace path+version dependencies are allowed; external termpane source copies are not. Pin a real registry release and lockfile. Audit normal/build/dev/target graphs and aliases.

The desired MIT OR Apache-2.0 license applies only to source whose rights support it. Audit authorship/copy history before relicensing. Preserve third-party and font notices and licenses; package metadata does not relicense assets or Apache-only upstream termpane. Resolve rights through available authorized routes, not invented permission. A genuine unresolved issue blocks a false license claim.

Use Mise to pin reproducible tools. Latest mbx observed here is 1.20.0 (2026-09-28); verify latest stable at execution and pin it, not a floating latest. Every compilation entry point runs through supported mbx commands, including xtask startup, check/test/Clippy/docs/examples/bench/package. Verify exact CLI forwarding for cargo extensions before documenting commands. Do not recursively wrap wrappers or invoke nested Cargo from individual tests.

Coordinate parallel builds with separate mutable target directories where appropriate and mbx shared resource/cache controls. Measure cold/no-cache and warm-cache builds; a cache hit alone is not the CI success criterion. Keep untrusted PRs from writing trusted remote caches. Preserve required checks while targeting the existing under-120-second fast-feedback goal under declared hardware/cache assumptions.

Add Renovate best practices through supported Cargo and Mise managers, workspace/lockfile updates, dependency dashboard, explicit bounded concurrency and maintenance. Separate termpane, renderer/font/codec, Rust toolchain/MSRV and other pre-1.0 API changes from low-risk batches. Do not auto-accept snapshots. Begin without automerge until gates prove safe. Security updates cannot remain indefinitely behind ordinary batches. Validate selected options and any custom toolchain manager against official current documentation; do not invent manager names or regex pins.

Use external Renovate configuration (JSON/JSON5), not a Node project. Respect generated CI: change authoritative Velnor inputs or its generic implementation and regenerate. CODEOWNERS must reflect actual known maintainers, not fabricated teams.

G8. CONSISTENT RENAME TO TUISCOTTI

The intended brand is Tuiscotti, lowercase tuiscotti. It combines TUI and biscotti, evoking a crisp snapshot while fitting the memorable food-word tradition of Ratatui. Descriptor: “Rust TUI visual-regression toolkit.” Optional tagline: “Crisp snapshots. Real terminal tests.”

Before irreversible namespace actions, check exact/case-insensitive GitHub, crates.io/package and executable/config conflicts and obvious related usage. Current research found no exact GitHub repository match, but registry publication/availability is unverified and nothing is reserved. Do not claim global uniqueness or trademark clearance. If a concrete blocking collision is found, independently research and choose one comparably distinctive alternative, record it once, then consistently substitute that single choice; do not leave a menu of active names.

Rename facade/crates, Rust imports, binary, help/version output, profiles, environment-variable prefix, config filename, artifact runtime roots, protocol branding, examples, docs, badges, package descriptions, generated inputs, release metadata and source-policy globs. Preferred identifiers: tuiscotti, tuiscotti-core/render/runtime/insta/cli/fixtures, tuiscotti.toml, TUISCOTTI_*.

Do not rename the separate termpane project. Do not change immutable historical snapshot bytes, font names/notices, old source URLs or factual citation provenance merely to remove a string. Isolate those historical occurrences in explicit read-only compatibility/evidence handling. New active output must use only the new brand; no old command/API/env aliases or deprecated wrapper crate.

Rename the existing hosted repository under the same owner only through available authorized administration and after checking implications for PR, redirects, remotes, CI, releases and package metadata. Do not create a duplicate repository or reset history. If administration is unavailable, finish the source rename, record the exact external action and do not claim hosted rename completed.

Do not publish or occupy a public package name without actual authorization. Build/package/clean-install validation is required regardless; publication permission is distinct from correctness.

FINAL VERIFICATION AND DELIVERY

Use independent agents to review eight-goal coverage, upstream boundary, API ergonomics, artifact/approval correctness, nextest/cleanup, safety/licensing, tool enforcement and branding. Critically verify every completion claim against final source and tests. Resolve disagreements through reproductions, not averaging opinions.

Required final evidence:
1. Official termpane version/PRs and downstream tests; no prohibited direct deps/source uses or fallback; pure model and pure view dependency isolation.
2. No foreign clients/scripts, including embedded scripts; all maintenance through Rust xtask and static report assets.
3. Virtual workspace inheritance, exact policy configuration and deliberately failing enforcement probes; no silent config-key skips.
4. Public Rust/CLI syntax matrix and executed simple/advanced external examples, including correct -- argument preservation and native Insta call sites.
5. Real fixture apps and ASCII/TXT/ANSI/PNG/HTML/canonical format contracts; original approval hashes unchanged with move receipts.
6. Native Insta review, partial-generation rejection, frozen verification, normal Cargo-test and nextest filtered/sharded/retry/stress/remapped/timeout runs with attempt-safe evidence.
7. Linux/macOS runtime conformance and explicit Windows support evidence; no cross-compilation mistaken for runtime testing.
8. Measured cold/warm compile and test costs, mbx/Mise/Renovate setup, reproducible packaging, authoritative docs and one active brand.

Read all final PR feedback and generated-CI status. Address accepted comments with fixing-commit links. Work on the existing PR branch unless state requires its successor, and keep upstream termpane work separate and focused. Commit and push all task-owned changes; leave unrelated changes intact. Do not merge or publish beyond authorization.

The final report must list exact commits/PRs, upstream release dependency, each goal's tests and result, profile/platform coverage, immutable hash results, measurements, removal inventory, licensing/name checks, and any genuine external blocker. Do not claim 8/8 or “best in class” merely because code compiles or docs say done. Continue until every accessible implementation action is complete and every required unmet gate is either resolved or specifically evidenced as externally blocked.

POLICY APPENDIX — APPLY WITHOUT WEAKENING

The workspace policy below is a structural starting point, not permission to invent dependency versions. Populate workspace.dependencies with verified real versions after graph design and termpane release qualification. Member declarations must inherit this policy.

Root Cargo.toml workspace policy

```toml
# Reference policy fragment, not a complete implemented workspace.
# Merge into the real virtual workspace; resolve registry versions during implementation.
[workspace]
members = [
    "crates/tuiscotti-core",
    "crates/tuiscotti-render",
    "crates/tuiscotti-runtime",
    "crates/tuiscotti-insta",
    "crates/tuiscotti",
    "crates/tuiscotti-cli",
    "crates/tuiscotti-fixtures",
    "crates/xtask",
]
resolver = "3"

[workspace.dependencies]
# Centralize all actual dependencies here, including dev/build dependencies.
# Use real compatible released versions; termpane must come from the official registry release.

[workspace.package]
edition = "2024"
rust-version = "1.98"
license = "MIT OR Apache-2.0"

[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
unexpected_cfgs = "deny"
unfulfilled_lint_expectations = "deny"
missing_docs = "warn"
missing_debug_implementations = "warn"
unreachable_pub = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
too_many_lines = "deny"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
dbg_macro = "deny"
mem_forget = "deny"
await_holding_lock = "deny"
await_holding_refcell_ref = "deny"
let_underscore_future = "deny"
let_underscore_must_use = "deny"
undocumented_unsafe_blocks = "deny"
allow_attributes_without_reason = "deny"
allow_attributes = "warn"

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"
private_intra_doc_links = "deny"
```

clippy.toml

```toml
too-many-lines-threshold = 80
allow-unwrap-in-tests = false
allow-expect-in-tests = true
allow-panic-in-tests = true
check-incompatible-msrv-in-tests = true
```

deny.toml

```toml
[advisories]
ignore = []

[bans]
multiple-versions = "warn"
wildcards = "deny"
highlight = "all"
workspace-default-features = "allow"
external-default-features = "allow"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
]
confidence-threshold = 0.8
```

rustfmt.toml

```toml
edition = "2024"
style_edition = "2024"
newline_style = "Unix"
```

rust-toolchain.toml: verified patch baseline

```toml
[toolchain]
channel = "1.98.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

.alint.yml: rename-safe coverage

```yaml
# Adapted to the new brand and ALL workspace crates, including facade, CLI, and xtask.
# Validate against the selected alint release and run negative fixtures for every rule.
rules:
  - id: required-files
    kind: file_exists
    paths:
      - Cargo.toml
      - Cargo.lock
      - clippy.toml
      - deny.toml
      - rustfmt.toml
      - CODEOWNERS
      - .alint.yml
      - .config/nextest.toml
      - AGENTS.md
    level: error
    message: "Repo-shape file is missing"

  - id: crates-only
    kind: file_absent
    paths:
      include: ["**/*.rs"]
      exclude: ["crates/**"]
    level: error
    message: "First-party Rust sources MUST live under crates/"

  - id: rust-max-lines
    kind: file_max_lines
    paths:
      include: ["crates/**/*.rs"]
      exclude:
        - "**/fixtures/**"
        - "**/testdata/**"
    max_lines: 400
    level: error
    message: "Rust source exceeds 400 physical lines"

  - id: lib-main-max-lines
    kind: file_max_lines
    paths:
      include:
        - "crates/**/src/lib.rs"
        - "crates/**/src/main.rs"
    max_lines: 150
    level: error
    message: "src/lib.rs and src/main.rs MUST stay under 150 physical lines"

  - id: no-python-javascript-typescript
    kind: file_absent
    paths:
      include:
        - "**/*.py"
        - "**/*.pyi"
        - "**/*.pyc"
        - "**/*.ipynb"
        - "**/*.js"
        - "**/*.mjs"
        - "**/*.cjs"
        - "**/*.jsx"
        - "**/*.ts"
        - "**/*.tsx"
        - "**/package.json"
        - "**/package-lock.json"
        - "**/pnpm-lock.yaml"
        - "**/yarn.lock"
        - "**/bun.lock"
        - "**/bun.lockb"
        - "**/pyproject.toml"
        - "**/uv.lock"
    level: error
    message: "First-party executable code and clients are Rust-only"
```

Validate these policy keys with the selected tools and prove each enforcement rule with negative tests. A syntactically valid file is not proof that a repository follows its rules. Do not weaken a rule because it exposes implementation debt.
