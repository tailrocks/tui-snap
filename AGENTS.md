# Rules

- No legacy code: finish every migration, remove old paths completely.
  No shims, aliases, or deprecation periods. Breaking changes preferred.
- Research project: unsafe, breaking changes expected, never
  production-ready. Break things when needed; ship fast.
- Judge by correctness, consistency, project fit. Never defer
  known-wrong for ROI/cost/effort.
- Stop only at a proven tool/model/project limit. Unsure? inspect,
  test, measure.
- Bugs: find why the architecture allowed the bug class first; prefer
  structural fixes. Name any deferred root cause.
- Delegate first: subagents for parallel research, implementation,
  review, verification. Resolve ambiguity from evidence.
- Commit verified changes frequently, push regularly. One working
  branch unless safety needs another. Merge small PRs promptly.
- Before PR merge: read ALL reviews/comments/threads; verify findings
  with independent subagents; fix+verify+commit+push+reply with the
  commit URL, or reply with evidence before resolving. Never delete
  feedback or resolve without a justified disposition.
- Merge only with zero unaddressed feedback, zero unresolved threads,
  all checks + approvals green at the final head SHA. Only explicit
  PR-specific human authorization waives feedback.
- Keep this file lean. Explanations, plans, progress go in docs/.
- Docs entry points: [README.md](README.md),
  [CONTRIBUTING.md](CONTRIBUTING.md), [docs/](docs/ARCHITECTURE.md).
