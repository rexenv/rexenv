# docs/archive — historical records

⚠️ Everything in this folder is **historical**: phase plans, task logs with "Done when"
evidence, the founding spec, design briefs, completed reviews, and the shipped-work
log. It may contradict the current code. **Current truth lives in
`docs/ARCHITECTURE.md` (how the system works) and `docs/TODO.md` (open work).** Read
here only to trace *why* a past decision was made or *when/how* something was fixed.
Two global staleness notes: these files predate the `.test` → `.rex` default-TLD flip
(every `.test` here would be `.rex` today), and the once-deferred services
(MariaDB, Redis, Apache, per-engine version switch) DID ship later via the
bottle-bundle path — only OpenLiteSpeed remains blocked.

| File | What it records |
|------|-----------------|
| `SHIPPED-2026-07.md` | The June–July 2026 completed-work evidence log (moved out of `docs/TODO.md` when it became open-items-only). |
| `SHIPPED-2026-08.md` | The August 2026 completed-work evidence log — 57 finished blocks moved out of `docs/TODO.md` by the 21 Aug 2026 reconcile, plus the rows that reconcile found had shipped without ever being ticked. |
| `PROJECT_SPEC.md` | Founding spec: decisions, feature tiers, phase plan, per-OS divergence notes for the future ports, the naming appendix. |
| `DESIGN_BRIEF.md` | The prompt blocks that generated the comps in `/design` (repo root — kept there for its relative `support.js` paths). Its Design-DNA section now lives, refreshed, in `docs/DESIGN.md`. |
| `DESIGN-GAPS.md` | UI-fidelity burndown vs the comps — complete. Its intentional-divergences list now lives, refreshed, in `docs/DESIGN.md`. |
| `FINDINGS.md` | Read-only code audit (C1, H1–H5, M1–M7, L1–L6). **All 19 fixed** — resolution map with commits at the top. |
| `CODEBASE-REVIEW.md` | The July 2026 pre-publish review (B1–B37) — all dispositioned; its two live tails (B29b, B33) are tracked in `docs/TODO.md`. |
| `UI-REVIEW.md` | The July 2026 whole-app visual review — findings fixed; its two unowned items moved to `docs/TODO.md`. |
| `TLD-FEATURE-REPORT.md` | Configurable-TLD implementation report + the `.rex` backbone correction; the rationale lives in `docs/ARCHITECTURE.md`. |
| `PLAN-tunnel-lifecycle.md` | The tunnels-die-with-the-app decision record (ruled + shipped 28 Jul 2026). |
| `PLAN-content-dir-assets.md` | The recorded-content-dir threading plan (shipped; the rule lives in `docs/ARCHITECTURE.md` §8). |
| `PLAN-site-provisioning-progress.md` | The streamed-provisioning design (shipped). |
| `PLAN-opensource-readiness.md` | The 28 Jul 2026 doc-set overhaul: audit verdicts + the task list that produced the current docs. |
| `TASKS.md` | Phase 1 (MVP) task log. |
| `TASKS-PHASE2.md` | Phase 2 (multi-PHP, FrankenPHP, PostgreSQL) task log + the deferral diagnoses the bundle path later resolved. |
| `TASKS-PHASE3.md` | Phase 3 (WP Manager, Mailpit, Adminer, tunnels, multisite) task log. |
| `TASKS-FIXES.md` | Pre-release fixes from the audit — all done. |
| `TASKS-RELEASE.md` | The original limited-distribution .dmg packaging log (historical — predates open-sourcing); open tails tracked in `docs/TODO.md`. |
