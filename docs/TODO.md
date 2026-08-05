# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here — the completed evidence
log this file used to carry is `docs/archive/SHIPPED-2026-07.md` (historical). When
you finish an item, tick it here with a one-line ✓ evidence note; when a section is
fully shipped, move it to the archive log. Reconciled against code 28 Jul 2026
(commit `b31ce3a`): every item below was re-verified as genuinely open, with the
evidence cited.

## Now — actionable code/test work

- [x] **MCP server M1 — read-only diagnosis + opt-in card** (branch `feat/mcp-m1`).
  ✓ Socket + `rex mcp` shim + registry (list_sites / site_status / tail_log) +
  ReadCtx read-only boundary (guard scans both surfaces) + secret-leak sweep +
  activity feed + the opt-in toggle that really binds/unbinds + Settings "AI
  agents" card + per-site SiteDetail section. Ledger #198–#203; wk-checks
  `agents-*`. Diverged from PLAN §7.3 honestly: no mail tools/sub-toggle in M1;
  `site_status` runs nothing (Option A, no HTTP GET); status line is feed-driven
  not a live-session list. **Next MCP stage = M2a (scratch sites)** — see
  `docs/PLAN-mcp-server.md §7.3`, reconciled against this shipped M1 on 1 Aug.
- [ ] **DNS agent answers ARBITRARY names when queried directly** (found in the
  28 Jul live tunnel diagnosis): `dig -p 15353 @127.0.0.1 <any-hostname>` returns
  `127.0.0.1` — the hickory handler is a catch-all, not per-TLD zones
  (`core/dns.rs` module doc; loopback bind keeps it safe, ledger #44). Harmless
  while only our `/etc/resolver/<tld>` files route to it, but "answers anything"
  is unintended. Scope answers to configured TLDs; NXDOMAIN the rest.
- [ ] **rexenv's WP-CLI inherits `~/.wp-cli/packages` — the pinned/offline posture
  does not cover the command set** (found 4 Aug 2026 while costing dist-archive;
  ledger #228). Every wp-cli spawn (`core/wordpress.rs:15/83/149`) uses the ambient
  environment and never sets `WP_CLI_PACKAGES_DIR`, so WP-CLI loads whatever a user
  composer-installed there, whenever, at whatever version. Proven live: `dist-archive`
  answered from a package installed on this laptop in **Dec 2021**; the same phar with
  an empty packages dir says `not a registered wp command`. Consequence that matters
  more than the posture: a bug in any wp-dependent feature can be **unreproducible
  with nobody able to guess why**, because the difference is a directory neither side
  mentions. **RECORDED, NOT FIXED — by owner instruction**: neutralising it globally
  could break someone's existing workflow, so it is a deliberate decision, below.
  When it is taken, the work is (a) the decision, (b) the spawn sites, (c) the L0
  scan that must cover ALL FOUR call sites or repeat the coverage/surface family.
  ✓ (d) **done 4 Aug** — `core/wordpress.rs`'s module doc now states it plainly
  ("What is pinned here is the BINARY, not the COMMAND SET"), so the code and
  ledger #228 no longer disagree while the decision waits.
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
- [x] **`wp dist-archive` in the RepoPanel — build a distributable zip to Downloads**
  ✓ **shipped 5 Aug 2026**, all 9 tasks (`docs/PLAN-dist-archive.md`, one commit each),
  ledger **#229–#236**, SMOKE §Git assets (5 steps, step 2 a HOLD — the zip is opened).
  ✓ **6 Aug** — three panel faults from the first real use, fixed in `d93afde` and
  written up in the plan's §8: the step frozen at pending (lost pre-attach events), the
  bogus offered row under Build zip, the zip re-announced on every re-expand.
  Researched + ruled 4 Aug 2026. Availability **ruled: bundle** the MIT package tree (~470 KB, one zero-dep
  transitive) and load it with `--require` — proven to register with an empty packages
  dir; `wp package install` rejected (network + composer at runtime + writes a dir we
  don't own), a Rust reimplementation rejected (a compatibility claim we'd defend
  forever). The three findings that shape it: **no `.gitignore` fallback exists** at
  v3.1.0/v3.2.0, so a missing `.distignore` ships `.git` + `node_modules` **as a
  `Success:`** ⇒ the feature REFUSES rather than warns; the tool **litters `TMPDIR` and
  never sweeps** (304 KB measured per run in the copy branch) ⇒ our own temp dir with
  `TMPDIR` pointed at it, swept on all three exits; and an occupied target makes the
  interactive prompt a **PHP fatal under a non-TTY** ⇒ build in temp so the path is
  never occupied. MCP tagged M-later (it can't ride `wp_run`: the zip lands somewhere
  an agent may not choose).
- [ ] **WP Manager cron list: arguments display** — placeholder for QA's exact
  complaint (likely the event-args column in the SiteDetail cron tab). Get the
  repro or drop after the next QA round.
- [ ] Nits (batch into any nearby commit): `src/components/ui/dialog.tsx:101`
  interpolated Tailwind class (`mt-${…}` defeats JIT scanning);
  `core/wp_login.rs:185` comment says "256-bit" for a ~244-bit token; the
  THIRD copy of the four-line `downloads_dir()` helper (`core/logs.rs:145`,
  `core/database.rs:145`, `core/dist_archive.rs` — one `UserDirs` call with
  nothing to drift, but three is where that stops being the argument); the
  `validate_linked_docroot` per-call `list(conn)` cost note
  (`core/sites.rs:875` — fine at current scale, hoist if imports grow).

## Ledger-driven proof backlog

The test metric is `docs/CLAIM-LEDGER.md` (mechanical tally 29 Jul 2026, after the
fix/docs/M1 merges: **123 ✅ · 38 ◐ · 37 🔨 · 5 🚫** of 203). The backlog = every 🔨
row + the noted half of every ◐ row, worked by the ledger's blast-radius tiers, top
first:

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

- [ ] **Live-check transients — a known-unknown, written down so the third one
  isn't a third undocumented data point.** Two unexplained failures on 3 Aug
  2026, both during full-suite runs, both passing standalone immediately after
  and on a clean re-run of the whole tier:
  - one lib test during `verify.sh` (name NOT captured — it passed before it
    could be identified; 4 clean runs after);
  - `apache_site_check` during `verify-full.sh` (passed standalone, then the
    whole sandbox tier passed at exit 0, then a captured full re-run was green).
  What is known: both under CPU contention (a vite dev server and/or WebKit in
  flight), both in checks that bind fixture ports or spawn services, neither
  reproducible in isolation. What is NOT known: whether it is port contention, a
  timing assumption, or something in the runner. **Mitigation already landed** —
  `live-checks.sh` now tees every example to a log and REPLAYS the failing one's
  last 40 lines next to the verdict, keeping the directory on failure, so the
  reason survives to the tail and a re-run no longer destroys the evidence
  (plant-proven). The remaining work is to diagnose the third occurrence when it
  is captured, not to guess now.

- [ ] **Verdict receipt — bind the COMMIT path, not just the verdict.** 3 Aug
  2026: `live-checks.sh ... | tail -3 && git commit` landed a commit on a RED
  tier, because a pipe replaces the script's exit code with `tail`'s. The
  documented rule ("verify.sh is the bar; never a piped check") did not hold —
  it was walked into by the person who wrote it, in the session where it was
  written about, which is as good an argument as exists that a rule relying on
  memory is not a control. **Half fixed:** all three verdict-bearing scripts now
  REFUSE to run with stdout piped (file redirect and TTY still fine;
  `REXENV_ALLOW_PIPE=1` to opt out), so no `&&` chain can follow a false green.
  **Still open:** the chain still reaches `git commit`, now after a refusal
  rather than a red verdict. The structural version is a receipt — `verify.sh`
  records `green <HEAD> <hash of git status --porcelain>` on success, and a
  `pre-commit` hook refuses when the receipt is missing or no longer matches the
  tree, with `--no-verify` as the explicit, traceable override. Scope it to
  commits that touch code (`src/`, `src-tauri/src/`, `examples/`) so doc-only
  work isn't gated. NOT done mid-release deliberately: a hook that misfires
  during the v0.1.0 gates would cost more than it saves.

## Release gates (human, scripted — see the docs named)

- [ ] **PUBLISH-TESTING §A** — Apple-Silicon ad-hoc launch test. Open, and the dmg
  it names is itself superseded: run it on the release-candidate dmg at publish time.
  ⚠ **Build it with `npm run release:mac`** (= `tauri build --target
  universal-apple-darwin`) — NOT a bare `tauri build`, which produces a thin
  arm64 `rexenv_<v>_aarch64.dmg` that an Intel user cannot run, while INSTALL.md,
  this document and `homebrew-rexenv/Casks/rexenv.rb` all promise a universal
  `rexenv_<v>_universal.dmg`. Built wrong once on 3 Aug by reaching for the
  generic command; naming the COMMAND here rather than the outcome is the fix.
  Verify before gating: `lipo -archs <app>/Contents/MacOS/rexenv` and the same
  for `MacOS/rex` must both report `x86_64 arm64`.
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
- [ ] **Neutralise the WP-CLI packages-dir inheritance, or accept it in writing?**
  (raised 4 Aug 2026; ledger #228, finding in "Now" above). Today rexenv's wp-cli
  loads a user's `~/.wp-cli/packages`. The three options and what each costs:
  **(a) neutralise** — set `WP_CLI_PACKAGES_DIR` to a rexenv-owned dir on every
  spawn; the posture becomes true, and a user whose workflow depends on a global
  package loses it **inside rexenv only**, silently unless we say so;
  **(b) neutralise with a tell** — same, plus naming it once where it can be read
  (the wp-cli terminal, the docs), which trades a surprise for a sentence;
  **(c) accept and document** — the posture claim is narrowed instead, and the
  unreproducibility stays. Not a code question: (a) can break someone's day, and
  the reason to decide it deliberately is that nobody would attribute the breakage
  to us. Blocked on nothing; wanted before anything else leans on wp-cli's command
  set (the dist-archive plan does, and bundles rather than installs partly for this).
- [x] **Publish history or start fresh** — DECIDED 28 Jul 2026: **fresh start**.
  The public repo begins at the cleaned HEAD; the private repo keeps full
  history. Reason: the docs cite commit hashes as evidence throughout, and a
  filter-repo rewrite would break that proof chain. Execution happens at
  publish time (init public repo from HEAD, push, add remotes).
- [ ] Stage-3 leftovers awaiting a ruling only if they resurface: none — the
  collision-rename tell-only and pdo_mysql exclusions are SETTLED (pinned by
  tests; do not reopen).
- [ ] **MCP server — M1 SHIPPED; M2 SCOPE RULED 1 Aug 2026, building M2a → M2b → M3**
  (`docs/PLAN-mcp-server.md`): expose an MCP server so a dev's AI agent can drive
  rexenv — disposable WordPress "scratch" sites (new `origin='agent'` column,
  TTL+cap+reaper), real-site DB SELECT-only via a native driver, read-only
  diagnostics. In-process beside `cli_server`, second `0600` socket, `rex mcp`
  pipe. Security is the bulk: the honest reckoning (§3.1) is that running the
  user's plugin code is user-level power — tiers deliver a safe paved road + no
  silent amplifier, NOT containment once scratch exec is in play. D1 (ship
  `wp_run` scratch-only), D3 (real-site mutation unpromised), D4 (mail opt-in
  sub-toggle), D6 (tunnels never a tool), D7 (accept the residual, toggle off +
  ledger 🚫) all SETTLED; **D2/D5 open but non-blocking; the M3 DB surface is
  ruled at M3, not now.** ✓ ahead of the feature: the mirrored-user GRANT
  wildcard-escaping fix landed on its own (ledger #196).
  - ✓ **M1 shipped** 30 Jul 2026 — ledger #198–#203.
  - ✓ **1 Aug 2026 — plan reconciled against shipped M1 + M2 scope ruled**, with
    the honest guarantee paragraph written FIRST (§6.0) so it constrained the
    design instead of describing it: it forced S1 and forbade a "no tool starts
    a service" claim (`scratch_create_site` starts MySQL). **S1** the dev-plugin
    add is a copy-on-write CLONE + explicit sync, not a symlink — `wp_run` makes
    the symlink's write-back guard unenforceable, and argv-screening a raw runner
    is the guard-covers-a-narrower-surface family (4th instance, refused).
    **S2** `wp_plugin`/`wp_theme` dropped (subsumed by `wp_run`). **S3** M2 splits
    into M2a (the scenario) / M2b (`set_php_version` + mail). Also corrected:
    v27/v28 (v26 is spent), mail's "scratch-addressed" filter is unsupportable so
    the tell is one rexenv creates (fail-closed), reaps get a typed
    `agent_actions.actor`, and two M1 leftovers that would have narrowed shipped
    claims by omission (the sweep walks ONE registry; `target_site` is unfillable
    for a create).
  - [ ] **M2a — the scratch-site scenario**, 14 tasks in `PLAN §7.3`, one commit
    each. ✓ task 2 — **v27** (`origin` / `agent_client` / `expires_at`), ledger
    #204: origin recorded not derived + read conservatively (anything ≠ "agent"
    is the user's), `expires_at` NULL = never so a user site and a Kept scratch
    site are indistinguishable to `Site::reap_due` (the ONE expression of the
    predicate), `agent_client` capped at the write and never branched on.
    Upgrade-path test proven to fail on a `DEFAULT 'agent'`.
    ✓ task 3 — **v28** `agent_actions.actor`, ledger #205 (+#202 amended): the
    feed keeps BOTH true claims ("an agent did this" / "everything consequential
    is visible") by labelling everywhere and filtering in exactly one place —
    `recent_head`, the agent status line, so a reaper sweep can never read as
    "Working — scratch_reap". Default records a known fact; the READ fails the
    other way (unknown actor = rexenv, never the agent). UI: "rexenv · automatic"
    + a reap's unresolvable target reads "(deleted site)", not a bare UUID —
    all three L2 probes proven to fail on the pre-v28 rendering.
    ✓ task 4 — **`ActedTarget`**, ledger #206 (+#202 amended): a create can name
    the site it made, WITHOUT the result becoming a channel for agent content —
    the only setter takes a `&Site` row, and it is an out-parameter so it
    survives `?` (a provisioning failure after the insert still names the
    half-built site). A row names a site iff a row exists. Both load-bearing
    tests proven to fail on a no-op `set`.
    ✓ task 5 — **TTL touch**, ledger #207 (+#199 scope sharpened): using a
    scratch site pushes its expiry out, from the SESSION layer beside the feed
    write so M1's read-only handler boundary is untouched. A touch can only MOVE
    an expiry — never start a clock, never reach a user's site (even one with a
    stale expiry), never resurrect a deleted one; each `WHERE` clause
    mutation-proven load-bearing. Audited what the shape narrows: the plan's
    §3.1c "no writes" and `mcp_socket_check`'s "writes nothing" were already
    loose (the feed writes) and are corrected — read-only scopes the HANDLER,
    not the call.
    ✓ task 6 — **`core::scratch::ScratchSite`**, ledger #208: the origin gate as
    a witness type in core (so CLI and UI inherit it), one door (`claim`, which
    reads the row itself), all three bypasses proven to be compile errors
    (E0423 / E0451 / E0277) with the captured messages in the row; refusals are
    policy statements naming site + rule + way forward, distinct for
    not-found vs not-yours, leaking no path or db name. Documented as NOT a lock
    — destructive writes still re-assert `origin` in their `WHERE`.
    ✓ task 7 — **the two-registry split**, ledger #209: executing tools get their
    own module + context (`ScratchCtx`, whose only door to a site is the witness),
    so #199 is untouched rather than widened; dispatch decides capability by
    WHICH registry a tool came from; `tools/list` and the secret sweep both walk
    the union. Disjointness guard plant-proven, and it teaches (names the
    offender, both module paths, the rule, and the shadowing consequence) —
    `panic!`ed rather than `assert_eq!`ed so the guidance isn't Debug-escaped onto
    one line. Wrote the SOCKET-level statement (different sentence from M1's) and
    guarded the card's "Today those are read-only" copy against the registry by
    `include_str!` — plant-proven to fail the day the first executing tool lands.
    ✓ task 8a — **the agent create PATH**, ledger #210: one `Ownership` value
    decides both what is recorded (`origin`/`agent_client`/`expires_at`, at the
    insert, so a create that fails later still expires) and whether a prompt may
    happen; `ResolverPrompt` is a REQUIRED parameter so a future call site can't
    be silent about its policy; ownership is unreachable from IPC (`create_site`
    has no such field). Never-prompt driven end to end through the real
    `start()` with a recording PrivilegeManager, **with a permanent control test**
    proving the same path DOES escalate for a user create. Cap + domain refusals
    name a way forward.
    ✓ task 8b — **`scratch_create_site`**, ledger #211: the tool runs through the
    SAME provision job as the app and the CLI (runtime generic erased by a
    `SiteCreator` trait object, since a static registry can't be generic);
    stack-stopped is a SUCCESS carrying M1's verdict/resolution vocabulary; a
    half-built create NAMES the site it made (id, owner-split actions, no local
    log path) and records it in the feed; enable-moment copy rewritten to
    describe CAPABILITY, approved 2 Aug, and the #209 guard re-scoped to drift
    (its first trip is spent) with BOTH surviving directions re-proven to fire.
    ⚠ Open for the owner: eyeball the longer paragraph in the PACKAGED webview at
    both widths — L2 covers render at 900/1440, L3 is the WKWebView bug class.
    
    ✓ task 9a — **`scratch_delete_site`**, ledger #212: gated by the WITNESS (not
    the name/suffix), refuses the user's sites with the ownership statement
    rather than a not-found, re-reads `origin` before anything destructive so a
    Keep mid-flight is reported honestly, and runs the app's own full delete
    path. Reap-failure policy decided: ONE row per distinct problem (dedupe on
    outcome+reason), never per launch; successes never deduped. `reap_due`'s
    three clauses each mutation-proven load-bearing.
    ✓ task 10a — **Keep**, ledger #213: ONE atomic UPDATE for one decision
    (origin + expiry), guarded by `AND origin='agent'` so it is idempotent and
    never touches the user's own; keeping at the ceiling frees a slot
    immediately, and the cap refusal now offers it as a third way forward.
    ✓ task 10b — **the promotion choke point**, ledger #214: the RULE ("the user
    deliberately changed this site") replaced §4.3's list — it added the PHP /
    web-server / Xdebug switches and removed share; six commands call one
    function; a source guard fails the build if a command stops calling it or a
    new one never does. ✓ 10b's Keep confirm DIALOG landed with task 13 —
    the approved copy is recorded verbatim in ledger #219, not referenced.
    ◐ task 9b — **the reaper's SWEEP**, ledger #215: skip-don't-stop for shared
    sites (calling the normal delete path would break #29 unattended), a failed
    stop is a skip, per-sweep ceiling of 5 with a loud log, outcomes deduped by
    `record_reap`. Wired to launch + hourly WITH the
    approved summary (names the domains, silent on a quiet launch). Remaining ◐:
    the skip-don't-stop and delete legs need a live tunnel + real site (L1,
    task 14). ◐ task 11 — **v29 `scratch_packages`** landed (new table, born empty;
    proven that a scratch site created before it reads as "no packages added" —
    a real answer, not an unknown). ✓ the MECHANISM (ledger #216):
    header-derived kind refusing both/neither, clone_tree proven byte-identical
    in BOTH directions, stat-only fingerprint with its limit recorded. ✓ the two TOOLS
    (ledger #217): claim → blast-radius → header → clone, sync re-reads only the
    RECORDED source, destination follows the recorded content dir, and the sync
    copy never claims "unchanged". **Task 11 done.** ✓ **The `cp -c` optimisation is RETIRED, on
    measurement** (4 Aug 2026, PLAN §4.4 + the `clone_tree` doc): the premise was
    backwards. `std::fs::copy` already uses `fclonefileat` on macOS, so the
    shipped code IS copy-on-write — a 400 MB file costs 0 bytes of disk — and it
    beats `/bin/cp -c -R` on wall clock (0.61–0.75 s vs 0.81–1.33 s on a
    4802-file tree) because it spawns no process. Doing it would have been
    slower AND added an OS-specific path plus a `todo!()` on Windows/Linux where
    the portable code works. No code written; the reasoning is recorded at the
    function so the next person re-measures instead of re-assuming.
    ◐ task 12 — **`wp_run`**, ledger #218: the target comes from the WITNESS
    (`--path` derived from the claimed site's recorded docroot) and an
    agent-supplied `--path`/`--url`/`--ssh`/`--http`/`@alias` is REFUSED in
    **core** — appending ours last only WINS a race, since a later `--path` wins
    in wp-cli. Recorded in the code and the row in those words: this screens
    TARGETS (closed, small, the tier boundary itself), unlike the VERB screen S1
    rejected — and there is therefore **no subcommand denylist, as a decision**,
    because plugin activation already grants arbitrary user PHP (#197). Witness
    re-asserted after binary resolution (a first-use download is a real
    multi-minute window). A non-zero exit is a RESULT, not a tool error. Output
    runs through `tail_log`'s scrubber — one function, plant-proven on four legs,
    and the planting found that a naive `#[cfg(test)]` stripper would have
    skipped most of this file (the test module sits mid-file), so the scan now
    carries its own coverage canary. Remaining ◐: a real `wp` command against a
    real scratch site is L1, task 14.
    ✓ task 12b — **the scrubber widened, not forked** (#201 amended): `wp cli
    info` and wp's errors print rexenv's own phar / PHP binary / config paths,
    all carrying the OS username — the same leak through a fourth door. The
    known-path set is now DERIVED from the `Paths` trait (`KnownPaths`), so a
    directory added there is scrubbed without a second list hearing about it,
    and `tail_log` + `wp_run` share it by calling one function. Recorded
    judgment: the claim is **"the paths rexenv knows are removed", not "no path
    escapes"** — a plugin can print `/opt/...` or a runtime-built path, and no
    prefix list reaches that. Asserted, not just written (an `/opt` line comes
    through unchanged), and stated in both tools' notes.
    ◐ task 13 — **the Sites UI**, ledger #219: the group reads `origin` and
    nothing else, so a KEPT site and a user's hand-named `mine.scratch.rex`
    both render as ordinary rows — plant-proven at L2 (the domain shortcut
    fails BY NAME). `expiresAt` null yields no label rather than a "never" one,
    so there is no third state to invent. Keep goes through the SAME single
    write as the implied promotion; its **approved copy is now recorded
    verbatim in #219** (it had been referenced-but-not-written, and was
    unavailable to the session that had to build it). A moved package source is
    its own state, never a stale timestamp. Scratch rows yield the metrics
    column — measured (167px overflow at 1440, the column reserves 168).
    Backend added: `keep_site` + `scratch_packages` commands, `store::
    all_scratch_packages` (joined to `sites`, because v29 has no FK).
    Two HARNESS defects found by the planting and fixed here: the new probe was
    never wired (`probeFor` is a name switch; the scenario tuple's third slot is
    ACTIONS), and `runActions` swallowed unknown actions silently — it now
    throws. Remaining ◐: the live page against real scratch rows (task 14 + a
    packaged look). ⚠ Also open: 10b's Keep dialog is now landed, so that
    caveat is spent.
    ⚠ Noticed, not fixed: `scratch_packages` rows are NOT deleted with their
    site (no FK, no delete hook) — harmless orphans, since ids are UUIDs and
    the read joins, but worth a cleanup when task 14 touches deletion.
    ◐ task 14 — **L1 `mcp_scratch_check` + the SMOKE M2a gate**, ledger #220.
    The executing registry over a real socket, sandbox tier. Fixture is
    adversarial where it counts: the USER'S site is named `mine.scratch.rex`,
    so all four tools refusing it proves the gate reads `origin`. The plugin
    source lives OUTSIDE the sandbox root (app-data is a blast-radius refusal
    being tested) behind its own `Drop` guard. **What moved:** #217 ✅ (the
    wired clone path, source byte-compared incl. a binary blob) and #218 ✅
    (four target forms refused + a REAL `wp cli info` child whose output is
    scrubbed — plant-proven both ways: the labels must be present, and removing
    the scrub fails on a raw app-data path). **What did not, and why:** #215's
    delete and skip-don't-stop stay 🔨 (need a provisioned DB + a live tunnel;
    faking either asserts against the fixture, not `delete_site_owned`); #219
    stays ◐ (a socket check cannot move a rendering claim — it moves at L3 in
    the PACKAGED webview, SMOKE 6/9/10); and no wp command that BOOTS
    WordPress is exercised, which leaves wp-cli's loaded behaviour unproven,
    not any rexenv guard. SMOKE §M2a = 6 steps; **8 (the tier boundary in front
    of a human) and 11 (no admin prompt) are HOLDs**, and 8 names the
    work-around a helpful model would reach for as a failure tell.
    **M2a's code is complete; the remaining M2a work is a release-candidate
    SMOKE run and the two 🔨 legs above.**
    ✓ **Orphan `scratch_packages` rows closed** (ledger #221): the delete lives
    in `core::sites::teardown` (not `delete_site_owned`) so the CLI and UI
    inherit it, and the read's join to `sites` STAYS as the second defence —
    they fail differently, and each leg is plant-proven on its own.
    ✓ #203's last leg — **L1 `mcp_control_check`** (sandbox tier): the OFF
    switch, built to prove what SMOKE step 4 is a HOLD for rather than the easy
    version. A session that was ALREADY CONNECTED is dropped (not just new
    connects denied), and the socket FILE is unlinked (not just closed); then
    re-enable rebinds and serves. Both plant-proven against the exact bug shape.
    Fixed while proving it: `serve` unlinked a RE-DERIVED path rather than the
    one its listener bound — latent in the app, but any other caller's shutdown
    would have deleted the APP'S socket.
    **M2a + M1 are code-complete. The release path is settled at nine steps:**
    (1) ✓ guarantee re-read, (2) ✓ #203, (3) packaged eyeball of the
    enable-moment paragraph, (4) `verify-full.sh`, (5) regenerate
    `THIRD-PARTY-NOTICES.md`, (6) fresh DMG, (7) PUBLISH-TESTING §A, (8) the MCP
    gate — SMOKE 1–5 then 6–11 (steps 4, 8, 11 are HOLDs), (9) §G.
    ⚠ Between the build and the tap push: recompute the cask sha256 (the
    committed one is a marked stale placeholder) — §D.
    ✓ **All three closing items done 4 Aug 2026**, none of them blocking v0.1.0:
    the orphan `scratch_packages` rows (#221 — deleted in `teardown`, the read's
    join KEPT as an independent second defence); `cp -c` **retired on
    measurement** (the premise was backwards — `std::fs::copy` already clones on
    APFS and beats shelling out; no code written); and **argv-in-feed landed as
    v30 `args_summary`** (#222), scoped by a property rather than a preference —
    `wp_run` is the only tool whose action leaves no other record, so it is the
    only one that needed it (+ `scratch_add_package`'s folder name). A VERB,
    never a value; per-tool `summarise` so the feed never parses agent JSON;
    clamped at the single writer to ≤2 `[a-z][a-z0-9-]` tokens, which is what
    stops agent text forging a row in the audit list. Rendered dimmed and
    unlabelled — captioning it "command" would overclaim what two tokens say.
    ⚠ **These stale the c7424fa DMG** (they touch `src-tauri/src/` and `src/`).
    Next = M2b.
  - [ ] **M2b** — ◐ nearly done. ✓ the mechanism extraction (#223 — *reusing a
    command reuses its POLICY*: routing an agent through the app's PHP switch
    would have inherited `promote_if_scratch`, adopting the scratch site and
    freeing a cap slot); ✓ `set_php_version` (#224 — unshipped versions refused
    BY NAME from a DERIVED list, never substituted); ✓ the scratch-mail stamp
    (#225 — one `stamp_for` shared by writer and reader; also closed a gap where
    one unswept mu-plugin blocked the v25 dir cleanup for EVERY site); ✓ the
    sub-toggle (#226 — backfill at the consent moment, which eliminates
    "predates the feature" as a category; both copy halves on the must-say
    list); ✓ `mail_list`/`mail_get` (#227 — ONE predicate filters and gates,
    because Mailpit ids are global and a trusted id would read the user's
    inbox). ✓ task 7 — the L1 legs ride `mcp_scratch_check`: the PHP
    refusal by name (row untouched) + a real 8.2→8.3 switch, with the site
    asserted still the AGENT'S afterwards — #223's cap bypass checked on the
    wired path, not only in a source guard; and the mail three-state
    discrimination, with "the user's site is never stamped" made non-vacuous
    (its fixture has a real docroot, so origin is the only thing skipping it)
    and plant-proven. **M2b is code-complete.**
    ⚠ Two legs stay open BY TIER, not by omission: the PHP DOWNLOAD window
    (every minor is warm on a dev machine — needs a cold cache, `network`), and
    the mail MATCH over real messages (needs Mailpit running, `stack`). Both
    are one small example each if wanted; neither blocks v0.1.0.
    **M3** — DB, its own session (T1 consent dialog).

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
