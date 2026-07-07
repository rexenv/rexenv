# docs/archive — historical records

⚠️ Everything in this folder is **historical**: phase plans, task logs with "Done when"
evidence, the founding spec, design briefs, and the code-review audit. It may contradict
the current code. **Current truth lives in `docs/ARCHITECTURE.md` (how the system works)
and `docs/TODO.md` (open work).** Read here only to trace *why* a past decision was made
or *when/how* something was fixed.

| File | What it records |
|------|-----------------|
| `PROJECT_SPEC.md` | Founding spec: decisions, feature tiers, phase plan. Tier lists are aspirational (MariaDB/Redis/Apache/OLS never shipped). |
| `DESIGN_BRIEF.md` | Design DNA + the prompt blocks that generated the comps in `/design` (repo root — kept there for its relative `support.js` paths). |
| `DESIGN-GAPS.md` | UI-fidelity burndown vs the comps — complete. |
| `FINDINGS.md` | Read-only code audit (C1, H1–H5, M1–M7, L1–L6). **All 19 fixed** — resolution map with commits at the top. |
| `TASKS.md` | Phase 1 (MVP) task log. |
| `TASKS-PHASE2.md` | Phase 2 (multi-PHP, FrankenPHP, PostgreSQL) task log. |
| `TASKS-PHASE3.md` | Phase 3 (WP Manager, Mailpit, Adminer, tunnels, multisite) task log. |
| `TASKS-FIXES.md` | Pre-release fixes from the audit — all done. |
| `TASKS-RELEASE.md` | .dmg packaging tasks — done except the clean-Mac verification, now tracked in `docs/TODO.md`. |
| `BACKLOG.md` | Post-audit defer/decision rationale (M1/M5 etc.). Only live item, L7, moved to `docs/TODO.md`. |
