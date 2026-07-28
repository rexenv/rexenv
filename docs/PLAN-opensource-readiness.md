# PLAN — open-source readiness: docs, agent-readiness, contributor path

Planned 28 Jul 2026 after a full doc audit (every doc read, claims spot-checked
against code at `55f1ec1`; two prior drift audits assumed staleness, this one verified
it). Scope: documentation, indexes, and repo hygiene for a public release. Out of
scope, deliberately (owner decisions or separate passes): the LICENSE choice, git
history rewrite vs fresh-repo, renaming personal-flavoured code/test fixtures, and
any code behaviour change.

## Ground truth from the audit

- The four **current-truth docs are healthy**: `ARCHITECTURE.md` (drifted in spots:
  migrations stop at v22, module map incomplete), `CLAIM-LEDGER.md` (current;
  mechanical tally 117 ✅ / 37 ◐ / 37 🔨 / 4 🚫 of 195), `PORTS.md` (current),
  `PLAN-testing-strategy.md` (current doctrine, no longer a "plan").
- `TODO.md` is 1,519 lines, ~90% completed history. Two parent checkboxes are
  unticked though their work shipped (testing strategy T1–T13; tunnel hardening 1–7).
  Two live items exist ONLY in `CODEBASE-REVIEW.md` (B29b, B33). A real honest-UI bug
  (the New Site Laravel card promises an installer that doesn't exist) was surfaced
  by two review docs and tracked by neither.
- Several completed one-shot reports (reviews, feature reports, handoffs, shipped
  plans) sit in `docs/` as if current; the archive already exists for that class.
- Machine-specific content (the maintainer's site inventory, local paths, local DB
  tooling) is concentrated in `PUBLISH-TESTING.md`, the four Valet/Herd plan docs,
  a few TODO evidence lines, and 11 archive lines. It is inventoried separately
  (not in this committed doc) and gets generalised where those docs are touched.

## Verdicts (every doc)

| Doc | Verdict |
|---|---|
| ARCHITECTURE.md | keep + refresh (module map, migrations v23–v25, tunnel lifecycle, content-dir rule) |
| CLAIM-LEDGER.md | keep as-is (the "what proves this invariant" answer) |
| PORTS.md | keep as-is |
| PLAN-testing-strategy.md | rename → `TESTING.md` (standing doctrine, not a plan); update referrers |
| TODO.md | rewrite: open work only, reconciled with evidence; shipped log → `archive/SHIPPED-2026-07.md` |
| PUBLISH-TESTING.md | keep + generalise (machine inventory → placeholders); outstanding checks cross-linked from TODO |
| SMOKE-TEST.md | keep as-is |
| INSTALL.md | keep + add uninstall-ordering section (lifted from homebrew README) |
| SIGNING.md | keep (drop one stale bullet) |
| xdebug-debug-build.md | keep (+ note: download host is an open decision, B33) |
| CLI-ROADMAP.md | keep (generalise one evidence line) |
| CODEBASE-REVIEW.md | archive (after migrating B29b + B33 + 2 nits into TODO) |
| UI-REVIEW.md | archive (after moving its 2 unowned items into TODO; drop one machine-specific name) |
| QA-ROUND-2-HANDOFF.md | delete (one-build, one-tester handoff; durable parts live in SMOKE/PUBLISH/GIT-FEATURE docs) |
| GIT-FEATURE-TEST.md | keep + de-personalise (neutral voice; note the nginx leg is now automated) |
| TLD-FEATURE-REPORT.md | archive (rationale already lifted into ARCHITECTURE; checklist tracked in TODO) |
| PLAN-tunnel-lifecycle.md | archive as decision record (lifecycle rule added to ARCHITECTURE first) |
| PLAN-content-dir-assets.md | archive (standing rule lifted into ARCHITECTURE first) |
| PLAN-site-provisioning-progress.md | archive (strip one local path) |
| PLAN-linked-sites.md | keep (best-argued design record; fix section order, drop one PID line) |
| PLAN-valet-herd-migration.md | keep + condense (drop shipped §9–§12, generalise the machine inventory; §2/§3/§6/§7 are the empirical record) |
| PLAN-valet-herd-import.md | keep + condense (§1/§3 duplicates out; status line names BOTH outstanding passes; resolver-takeover design is canonical here) |
| PLAN-valet-herd-db-import.md | keep + condense (fix two phantom example names; strip live captures; §2.4/§5/§6/§7/§9 canonical) |
| PLAN-valet-herd-rewrite.md | keep + light scrub (fixture domain; drop inventory count; fix illustrative diff) |
| PLAN-opensource-readiness.md | this doc; archive when done |
| archive/README.md | keep + fix (stale "never shipped" line; add `.test`→`.rex` note) |
| archive/PROJECT_SPEC.md, DESIGN_BRIEF.md, DESIGN-GAPS.md, FINDINGS.md, TASKS*.md | keep as archive; redact 11 machine-inventory lines; extract Design DNA + intentional-divergence guardrails → `docs/DESIGN.md`; TASKS-RELEASE gets a neutral historical header (its closed-source framing contradicts a public repo) |
| archive/BACKLOG.md | delete (every row superseded and recorded elsewhere) |
| homebrew-rexenv/README.md | rewrite (staging-copy framing, cask-hash caveat, uninstall ordering lifted to INSTALL) |
| scripts/wk-checks/README.md | keep as-is (model doc) |
| src-tauri/src/templates/README.md | delete with its empty directory (dead scaffold, zero references) |
| README.md | refresh (structure tree, verify.sh gate, docs list, prerequisites) |
| CLAUDE.md | router update to the final doc set (+ MAP row) |
| NEW: docs/MAP.md | subsystem → files → entry points → proof anchors |
| NEW: CONTRIBUTING.md | build/run, the gate, conventions, deliberate-decisions list |
| NEW: docs/DESIGN.md | Design DNA + intentional divergences (extracted, refreshed) |

## Tasks (one commit each)

- **T1** This plan. ✓ (this commit)
- **T2** TODO reconciliation: rewrite `TODO.md` as the verified open list (newly
  surfaced items folded in: B29b, B33, Laravel-card honesty, onboarding :443 probe,
  resolver-drift surfacing, sites_dir validation, small nits); shipped history moves
  verbatim-with-generalised-domains to `docs/archive/SHIPPED-2026-07.md`.
- **T3** ARCHITECTURE refresh (map, v23–v25, tunnels-die-with-app, content-dir rule,
  unpinned test counts).
- **T4** `docs/MAP.md` + rename testing plan → `docs/TESTING.md` + CLAUDE.md router.
- **T5** CONTRIBUTING.md + README refresh.
- **T6** `docs/DESIGN.md` extraction + archive corrections/redactions (incl.
  TASKS-RELEASE header, BACKLOG deletion, archive README).
- **T7** Doc moves/deletions: CODEBASE-REVIEW, UI-REVIEW, TLD-FEATURE-REPORT,
  PLAN-tunnel-lifecycle, PLAN-content-dir-assets, PLAN-site-provisioning-progress →
  archive; QA-ROUND-2-HANDOFF + templates/README deleted; referrers updated.
- **T8** PUBLISH-TESTING generalisation; GIT-FEATURE-TEST de-personalise; INSTALL
  uninstall section; homebrew README rewrite; SIGNING/xdebug/CLI-ROADMAP touch-ups.
- **T9** Valet/Herd plan-doc scrubs + condensations (4 docs) + PLAN-linked-sites fixes.
- **T10** Final `scripts/verify.sh` + wrap-up.

Doc-only commits rely on the session's green `verify.sh` baseline (539 lib tests);
the script runs again at T10 (and with any commit that touches code or scripts).

## Not done here (owner decisions)

1. **LICENSE** — analysis delivered separately; no file added until chosen.
2. **History** — current HEAD is cleaned, but git history keeps every earlier
   revision (plus one deleted screenshot); publish-fresh vs rewrite is a separate call.
3. **Code fixtures** — a handful of tests/dev-harness mocks use personal-flavoured
   strings; renaming touches code and is left for a code pass.
4. **homebrew-rexenv/** extraction to its own repo (its README says it's a staging copy).
