# docs/archive — historical records

⚠️ Everything in this folder is **historical**: phase plans, task logs with "Done when"
evidence, the founding spec, design briefs, completed reviews, and the shipped-work
log. It may contradict the current code. **Current truth lives in
`docs/ARCHITECTURE.md` (how the system works), `docs/TODO.md` (open work) and the generated
`docs/STATUS.md` (`scripts/status.py`).** Read
here only to trace *why* a past decision was made or *when/how* something was fixed.
Two global staleness notes: these files predate the `.test` → `.rex` default-TLD flip
(every `.test` here would be `.rex` today), and the once-deferred services
(MariaDB, Redis, Apache, per-engine version switch) DID ship later via the
bottle-bundle path — only OpenLiteSpeed remains blocked.

| File | What it records |
|------|-----------------|
| `SHIPPED-2026-07.md` | The June–July 2026 completed-work evidence log (moved out of `docs/TODO.md` when it became open-items-only). |
| `SHIPPED-2026-09.md` | 21 Aug → 30 Sep 2026: 132 finished blocks moved by `scripts/todo-reconcile.py` on 5 Sep (menu-bar tray, per-site lifecycle, MCP M3 + parity P1, `rex` design-first set, Valet tails), the 11/12 Sep passes, the 23 Sep pass before 0.8.7 (the 18 Sep clean-VM fixes, the Windows rows, the multisite imports), the 29 Sep pass before 0.8.10 (16) and the 30 Sep pass before 0.8.11 (23: the week's VM-proven fixes), plus the original text of rows each reconcile rewrote or retired. |
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

## Design records — the shipped `PLAN-*.md` files (moved here 5 Sep 2026)

Each was written BEFORE its feature and kept as the record of why it is shaped the way
it is. The shipped behaviour is described in `docs/ARCHITECTURE.md`; the plan is where
the rejected options, the measurements and the rulings live. A plan sits in `docs/`
only while it is in flight (`scripts/status.py` lists those); it moves here when its
own Status line says shipped. Three headers in this set went stale AFTER shipping
(binary-updates, mcp-server, menubar-tray) — each says so in its first paragraph, which
is why a header is checked against the code before it is believed.

| File | What it records |
|------|-----------------|
| `PLAN-linked-sites.md` | Stage 0 of the Valet/Herd ladder, shipped as a first-class feature 26 Jul 2026: serve a folder from anywhere on disk (`docroot_managed`), never delete it. |
| `PLAN-valet-herd-migration.md` | The empirical research behind all four migration stages (Valet/Herd layouts, port conflicts, engine compat, dump flags), against a real messy two-tool install. |
| `PLAN-valet-herd-import.md` | Stage 1 (26 Jul 2026): read-only scan → review → import; the scan's mess taxonomy and the resolver ownership/takeover design. |
| `PLAN-valet-herd-db-import.md` | Stage 2 (27 Jul 2026): dump/restore into rexenv, provenance, mirrored credentials; source stays strictly read-only. |
| `PLAN-valet-herd-rewrite.md` | Stage 3 (28 Jul 2026): the opt-in connection rewrite — diff first, backup, the "connected" fact. |
| `PLAN-dist-archive.md` | `wp dist-archive` from the RepoPanel (5 Aug 2026): bundle-path ruling, git-only by design. |
| `PLAN-browser-preference.md` | Preferred browser + real app icons (11 Aug 2026): where the preference is enforced and why. |
| `PLAN-git-site-clone.md` | Create a site FROM a git repo, Laravel first (11 Aug 2026): clone, `.env`, composer, migrate; the Bedrock break found the day it shipped. |
| `PLAN-php-74-support.md` | PHP 7.4 (15 Aug 2026): rexenv's OWN build in `rexenv/runtimes`, self-build + hosting, the retired "has no build and never will" claim, EOL honesty. |
| `PLAN-webview-dialog-proofs.md` | Why the WebKit/wry dialog (#166) and custom-scheme redirect (#40) claims are NOT L2-provable, and where each leg lives instead. |
| `PLAN-binary-updates.md` | Signed-manifest in-app PHP patch updates (17–18 Aug 2026): the trust model, the key ceremony, the serial/replay gate. |
| `PLAN-adminer-updates.md` | Adminer as the SECOND manifest family (18 Aug 2026): `updates::Family`, the binding probe. |
| `PLAN-mcp-server.md` | MCP server M1/M2a/M2b/M3 (30 Jul – 25 Aug 2026): scratch sites, capability tiers, `db_query`; the honest "not a sandbox" guarantee (§6.0). |
| `PLAN-mcp-parity.md` | MCP parity P1–P7 (3 Sep 2026): every app function agent-drivable on real sites; scopes, the global Agent access dial (D15–D17). |
| `PLAN-menubar-tray.md` | The menu-bar app (31 Aug – 1 Sep 2026): why the CLI/MCP sockets died with the window, the no-dock-icon ruling, what Accessory costs, the second-instance guard. |
| `PLAN-self-update.md` | In-app self-update (6–7 Sep 2026): why `tauri-plugin-updater` was rejected, the second signed document sharing one key, the `renamex_np(RENAME_SWAP)` bundle exchange, the kqueue relauncher, the 30-row failure catalogue. **Archived with its last task still owed** — §T11, the first real 0.6.0 → 0.6.1 update on a Mac, is a human gate that needs a published release to exist; read §T11 before the first release that carries a descriptor. |
| `PLAN-install-scripts.md` | The one-command install (28–29 Sep 2026): why `curl \| bash` and `irm \| iex` meet no Gatekeeper or SmartScreen dialog (the DOWNLOADING program writes the mark those gates key on, measured), the four rulings, the per-OS design, and what only a signed-in desktop showed — interactive PowerShell 5.1 shadows `RuntimeInformation` (PSReadLine 2.0.0), which refused every Windows desktop for a day. The scripts themselves live in `rexenv/homebrew-tap`. |
| `PLAN-per-site-lifecycle.md` | Per-site start/stop + the Sites-page type filter (4 Sep 2026): stopping ONE site is a serving-surface change, not a process one. |
| `PLAN-postgres-sites.md` | PostgreSQL as a site database for Laravel and Blank PHP, never WordPress (9–11 Sep 2026): the engine-dispatched site-DB layer, why rexenv builds its own 8.1–8.5 (upstream's PDO advertises `pgsql` and hangs), the per-PATCH capability record, and the defects only real sites found (#550–#558). **Archived 11 Sep 2026.** |
| `PLAN-local-import.md` | Import from Local (WP Engine/Flywheel), the third migration source (11–12 Sep 2026): the registry instead of a symlink farm, `.local` re-homed, the per-site mysqld reached over its SOCKET (TCP answers ERR 1130 — the design guess the first real import disproved), the URL pass on rexenv's copy, and the owner's §9 rulings (Q1 reversed, Q3 built). **Archived 12 Sep 2026 with Q2 — multisite networks — parked** as its own TODO row. |
| `GIT-FEATURE-TEST.md` | The manual checklist for Add plugin/theme from Git (phases 1–5), human-verified 18 Jul 2026; superseded by `docs/SMOKE-TEST.md`'s sections. |
