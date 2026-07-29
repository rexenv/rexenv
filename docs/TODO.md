# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here — the completed evidence
log this file used to carry is `docs/archive/SHIPPED-2026-07.md` (historical). When
you finish an item, tick it here with a one-line ✓ evidence note; when a section is
fully shipped, move it to the archive log. Reconciled against code 28 Jul 2026
(commit `b31ce3a`): every item below was re-verified as genuinely open, with the
evidence cited.

## Now — actionable code/test work

- [ ] **DNS agent answers ARBITRARY names when queried directly** (found in the
  28 Jul live tunnel diagnosis): `dig -p 15353 @127.0.0.1 <any-hostname>` returns
  `127.0.0.1` — the hickory handler is a catch-all, not per-TLD zones
  (`core/dns.rs` module doc; loopback bind keeps it safe, ledger #44). Harmless
  while only our `/etc/resolver/<tld>` files route to it, but "answers anything"
  is unintended. Scope answers to configured TLDs; NXDOMAIN the rest.
- [ ] **B29b — fpm pool reap is still one-miss** (`core/php.rs:534-535`): a single
  failed port probe with a dead-looking master reaps the pool, with no
  `ADOPTED_MISS_LIMIT`-style counter and no positive php-fpm title identification
  (the adopted-service reap got both in B29). Surfaced in the July codebase review;
  previously tracked only there.
- [ ] **New Site "Laravel" card promises an installer that doesn't exist**
  (`src/components/sites/NewSiteDialog.tsx:66` — "A fresh Laravel app via the
  installer…"): `SiteType::Laravel` is serve-only; `phase_defs` has no Laravel
  install phases and nothing invokes `laravel new`/`create-project`. Honest-UI
  bug: fix the copy (serve-an-existing-app) or build the flow. Surfaced by the
  UI review + migration plan; tracked by neither until now.
- [ ] **Onboarding does no :443 probe** (migration plan §3a, gap 2 of 2): a
  shadow-binding Herd/other proxy at onboarding time is only discovered later by
  the watchdog. `proxy::edge_answers_as_ours` exists and is called from Start-all,
  the watchdog, doctor, and import — but not from onboarding/`core/setup.rs`.
- [ ] **Resolver-drift surfacing is backend-only** (Stage 1 §4.10 shipped the cheap
  check, the user surface is unwired): startup only `log::warn!`s
  (`lib.rs:260-269`), the `resolverDrift()` IPC binding has zero frontend callers,
  and `rex doctor`'s JSON carries `resolverDrift` that `cli/src/main.rs` never
  renders. Decide: wire the three surfaces, or drop the binding. The
  "continuous watcher" deferral from Stage 1 D3 was never picked up by Stage 2 —
  re-decide or close it explicitly.
- [ ] **`teardown` never removes the Apache per-site config/log; neither does
  `change_site_domain`** (pre-existing, found in Stage 0 mapping): sweep covers
  FrankenPHP conf+log + tunnel log but not `apache::config_path`/`log_path`
  (`core/apache.rs:168,177`). App-data files only — additive fix + a test in the
  `teardown_removes_row_and_per_site_artifacts` shape.
- [ ] **`sites_dir` setting written unvalidated** (`commands/settings.rs` special-
  cases only `default_tld`): the value flows into provision docroots and generated
  configs (B26 charset concern). Validate at the setter.
- [ ] **Debug-log truth on Bedrock** (deferred with the wp-config-reader work):
  parse `config/application.php` env defines so WP_DEBUG/WP_DEBUG_LOG read
  truthfully on non-stock layouts; today's honest state is `indeterminate`
  ("can't determine", `core/logs.rs:208-252`).
- [ ] **Per-backend tunnel origins for override sites** (deferred, re-scoped 28 Jul —
  smaller than first estimated since the web-server-switch-while-shared refusal now
  exists): resolve the RECORDED override port at tunnel start as the `--url`
  origin; refuse when that backend isn't up; replace `ensure_tunnelable`'s
  "can't be shared yet" refusal (`core/tunnels.rs:117-123`). Probe/mu-plugin/row
  machinery are origin-agnostic.
- [ ] **WP Manager cron list: arguments display** — placeholder for QA's exact
  complaint (likely the event-args column in the SiteDetail cron tab). Get the
  repro or drop after the next QA round.
- [ ] Nits (batch into any nearby commit): `src/components/ui/dialog.tsx:101`
  interpolated Tailwind class (`mt-${…}` defeats JIT scanning);
  `core/wp_login.rs:185` comment says "256-bit" for a ~244-bit token; the
  `validate_linked_docroot` per-call `list(conn)` cost note
  (`core/sites.rs:875` — fine at current scale, hoist if imports grow).

## Ledger-driven proof backlog

The test metric is `docs/CLAIM-LEDGER.md` (mechanical tally 28 Jul 2026:
**117 ✅ · 37 ◐ · 37 🔨 · 4 🚫** of 195). The backlog = every 🔨 row + the noted
half of every ◐ row, worked by the ledger's blast-radius tiers, top first:

- [ ] Tier-1 cluster: tunnel second-Host negative (#10/#13), CF-header
  discriminator probes (#2/#33), Adminer-as-origin negative (#37), share-lifetime
  races (#25/#26/#29/#30/#31), second-brain drift guards (#54/#59), cancelled
  takeover rollback (#49), cancelled-dump server-side (#116), delete-kill ordering
  (#190).
- [ ] Then: Apache/FrankenPHP dotfile legs (#103), fpm candidate isolation
  (#104/#191), manifest HEAD+digest sweep, Bedrock live provision (#35),
  sandbox-adoption cohorts + `wp_fixture()` — incl. scoping
  `download_progress_check`'s bin-cache delete off the REAL shared cache
  (surface-coverage finding 29 Jul: the sandbox invariant is structural for only
  ~20 of 109 examples, and this one deletes a real content-addressed entry),
  `webview_dialogs` L2 (#166), import-graph lint (#163), rusqlite-outside-state
  guard (#167 — must scan SQL-STRING content, not just the `rusqlite` import, or
  it repeats the surface-coverage shape; see "Defect families" in the ledger).

## Release gates (human, scripted — see the docs named)

- [ ] **PUBLISH-TESTING §A** — Apple-Silicon ad-hoc launch test. Open, and the dmg
  it names is itself superseded: run it on the release-candidate dmg at publish time.
- [ ] **PUBLISH-TESTING §B** — uninstall removes the root :443 daemon (live launchd).
- [ ] **PUBLISH-TESTING §D** — full tap install dry-run (after Release + tap push;
  recompute the cask sha256 — the committed one is a marked stale placeholder).
- [ ] **PUBLISH-TESTING §K** — the whole migration as ONE journey (rebuild first).
- [ ] **PUBLISH-TESTING §F** — resolver takeover/hand-back/drift: clean-VM only.
- [ ] **PUBLISH-TESTING §G** — `/import` screen packaged GUI pass (only ever
  type-checked; Stage 1's status line now says so).
- [ ] **Release 5.4 — clean-Mac smoke test** (`docs/SMOKE-TEST.md`): first pass
  10 Jul 2026 green except multisite-convert (UI didn't exist yet — since built);
  re-verify converted-multisite + onboarding fixes + the TLD v1 Done-when list
  (`docs/archive/TLD-FEATURE-REPORT.md`) on the next cold run.
- [ ] **Tunnel probe session** (one sitting, real network,
  `scripts/tunnel-measure.sh`): kill -9 death-path timings, wifi-blip recovery,
  the banner→authoritative-DNS gap; plus Bedrock "Log in as" landing in wp-admin
  live, and the first real Radicle-layout link (flagged UNVERIFIED in code, #95).
- [ ] **Intel spot-run**: x86_64 bottle digests + MySQL 8.0.44 x86_64 were hashed
  from real downloads but never RUN (PORTS.md caveat) — run-verify on the next
  Intel machine.
- [ ] **In-app verifies owed** (CLI passthroughs whose service-touching half the
  example harness guard-blocks; fold into the next deep test): `php
  install/uninstall`, `php settings set`, `db versions --set`, `site
  server/domain/move`, `mail clear`, `tunnel start`, `wp core update/switch` —
  plus the packaged-GUI walks still noted inside their shipped entries: MariaDB
  site from the dialog, Apache site in-app, DB version switch from the Databases
  row, Settings CLI-install card, ref-picker on a real many-branch repo, wp.org
  chips + streamed installs, New Site streamed provisioning card.
- [ ] **PUBLISH-TESTING §E / §L** — 🟢 nice-to-haves (B22/B23 datadir recovery,
  B4 submodule clone, B24 wp-cli `--`, B20/B28/B29/B7 runtime wiring; §L Phase-A
  tcpdump one-off, re-run per reqwest bump).

## Decisions pending (owner)

- [x] **LICENSE** — DECIDED + LANDED 28 Jul 2026: **Apache-2.0** (explicit patent
  grant; §5 licenses inbound contributions without a CLA). ✓ `LICENSE` + `NOTICE`
  at root, `Apache-2.0` in `package.json` + both `Cargo.toml`s,
  `THIRD-PARTY-NOTICES.md` generated from the real graphs (389 crates + 112 npm
  packages + OFL fonts + bundled SQLite; regeneration commands in its header),
  DCO sign-off in `CONTRIBUTING.md`, README licence section. Regenerate the
  notices file per release.
- [ ] **B33 — php-debug download host**: `dl.rexenv.dev` is wired
  (`core/binaries.rs:40`, inert until the four SHA consts are filled) but the
  host choice vs the canonical domain was never settled. Note added to
  `docs/xdebug-debug-build.md`. When it activates, ship the PHP + Xdebug licence
  texts beside the artifacts (rexenv becomes a distributor of PHP at that moment).
- [ ] **`rex config get|set`** — parked on which settings keys to allow-list
  (never the whole KV table).
- [x] **Publish history or start fresh** — DECIDED 28 Jul 2026: **fresh start**.
  The public repo begins at the cleaned HEAD; the private repo keeps full
  history. Reason: the docs cite commit hashes as evidence throughout, and a
  filter-repo rewrite would break that proof chain. Execution happens at
  publish time (init public repo from HEAD, push, add remotes).
- [ ] Stage-3 leftovers awaiting a ruling only if they resurface: none — the
  collision-rename tell-only and pdo_mysql exclusions are SETTLED (pinned by
  tests; do not reopen).

## Parked (deliberate — needs explicit go; don't pick up silently)

- [ ] **Install WordPress into an empty LINKED folder** — out of Stage 0 by
  design (`docs/PLAN-linked-sites.md` decision 2): linking is adopt-only. If
  built: a deliberate site-page action, offered only when the linked folder is
  EMPTY, its own disclosure. Trap first: `configure` is only half idempotent
  (skips `wp config create` when wp-config.php exists but calls
  `create_database` unconditionally), and `phase_defs` blanket-skips WP phases
  on `docroot_managed == Some(false)` — needs an explicit opt-in flag.
- [ ] **Valet compatibility tails** (recorded in the migration research, §2):
  the `default` catch-all-site key and Laravel's `/storage/*` URI mapping are
  not handled; a Valet-named port-conflict attribution branch doesn't exist
  (Herd's does); serving one site under two domains needs multi-domain support
  (`sites.domain` is UNIQUE).
- [ ] **`rex` design-first set**: single-site restart (manager seam), web-tier
  single-service control (topology invariant), raw `wp` passthrough (security
  decision), `wp_user_delete` (no IPC exists), progress streaming over the CLI
  socket (design once, benefits every long op).
- [ ] **Rowless-orphan launch NOTICE**: the sweep runs before the webview mounts,
  so a plain event would be lost — needs a queued-notices channel; WARN log is
  the honest surface today.
- [ ] **Parent-death watcher helper** (kqueue `NOTE_EXIT`) to close the tunnel
  crash→relaunch gap entirely — revisit only if crash reports show the bounded
  gap mattering.

## Blocked on external work

- [ ] **Xdebug on PHP 8.0** — the Nov 2024 static-php 8.0.30 build exports zero
  Zend symbols (`nm -gU` = 0; dlopen fails `_OnUpdateBool`), upstream still
  serves that exact build (re-verified 16 Jul 2026). 8.1–8.5 solved via the
  bottle path. Fix needs an upstream rebuild or the self-build recipe
  (`docs/xdebug-debug-build.md`). PHP 8.0 is EOL — acceptable to leave excluded.
- [ ] **SMAppService privileged helper** (true single-prompt setup) — needs a
  signed + notarized bundle; packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** — needs a paid Apple account;
  runbook ready in `docs/SIGNING.md`.
- [ ] **OpenLiteSpeed override server** — no macOS artifact exists anywhere
  (upstream ships Linux tarballs only; no homebrew-core formula; the one
  community tap is frozen at EOL 1.4.51 and fails the trust model). A maintainer
  self-build is plausible but needs the same hosting infra as the Xdebug build.
  Code is ready and honest: `ensure_server_available` refuses OLS in CORE at
  create AND switch, and `OverrideKind` means enabling it later is one new arm.
  On Linux (Phase 4) this is cheap — official tarballs exist.

## Phase 4+ (next era)

- [ ] Windows platform impls — fill the `todo!()` stubs in
  `platform/windows/mod.rs` (trait-by-trait; no restructuring required).
- [ ] Linux platform impls — same, `platform/linux/mod.rs`.
- [ ] Packaging polish: Tauri updater (keypair, endpoint, `latest.json` —
  checklist in `docs/archive/TASKS-RELEASE.md` §6.1), public distribution.

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output — intentionally not filtered.
- On networks that negative-cache DNS, a fresh tunnel URL can be dead on THIS
  machine while live from a second device — the router race, not a bug
  (`docs/SMOKE-TEST.md` tunnels section).
