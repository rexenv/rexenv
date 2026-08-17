# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here — the completed evidence
log this file used to carry is `docs/archive/SHIPPED-2026-07.md` (historical). When
you finish an item, tick it here with a one-line ✓ evidence note; when a section is
fully shipped, move it to the archive log. Reconciled against code 28 Jul 2026
(commit `b31ce3a`): every item below was re-verified as genuinely open, with the
evidence cited.

## Now — actionable code/test work

- [x] **Read-only "a newer PHP patch exists"** ✓ 16 Aug 2026, ledger #343,
  plant-proven. `core/php_upstream.rs` fetches php.net's `active.php?json` at launch,
  best-effort, and derives a version STRING per minor — nothing else. `source`/`sha256`
  are in that document and are never read; a source scan asserts exactly one field is.
  The Settings row says `8.3.33 exists`, with `checked N ago` beside the section and an
  explicit "couldn't reach php.net yet" when it never has. **The copy is the mechanism**:
  php.net leads static-php.dev (where 8.x installs come from) by weeks — measured, 8.4.24
  vs our pinned 8.4.23 — so "update available" would promise what no button delivers and
  "up to date" is unprovable; `core::copy_scan` bans both.

- [x] **Delete `php_versions.patch`; derive it** ✓ 16 Aug 2026, migration v36, two
  commits (`67f17c8` the live read, `afe5bb6` the drop). Ledger #339 RETIRED with its
  reason, #338/#340 amended, #342 added. `PhpVersionView::serving` carries what the pool
  is actually executing beside the pin, so the row says both — without it a derived row
  could only ever show the pin, which is the same silent lie somewhere harder to see.

- [ ] **Say WHY the "exists" line often names a patch rexenv can't install yet** —
  one sentence, somewhere a curious user lands, NOT in the row (which stays short).
  php.net publishes on release day; static-php.dev, where 8.x builds come from, trails —
  measured 17 days on 16 Aug 2026 (php.net 8.4.24/8.5.9 vs our pinned 8.4.23/8.5.8). So
  the row will spend most of its life truthfully naming a patch that has no portable
  build yet. That is honest but reads as a defect to someone who does not know the
  pipeline. Candidates: the Settings section's existing footer line, or `INSTALL.md`'s
  "how rexenv gets its components". Ledger #343 has the measurement and the reasoning;
  this is the user-facing half of it.

- [x] **Audit every table for the user-fact-vs-derived-fact clobber** ✓ 17 Aug 2026,
  ledger #344 + #345. Swept every upsert, all 35 `UPDATE … SET` sites, every migration and
  every launch-path writer: **49 candidates, 48 rejected — no iceberg.** The three `sites`
  backfills are strict NULL-only set-once; the v6/v32 migrations weld their UPDATE to their
  own ADD COLUMN inside one batch; migration v9's `default_tld` flip is closed by release
  history (it shipped inside v0.1.1, so no released build ever sat at `user_version 8`).
  ONE live defect, in the statement already known to be dangerous: `seed_registry`'s INSERT
  arm gave a newly-pinned minor `is_default = 1` beside the user's, so every user got two
  rows badged Default the first time the pin moved to a new minor. Fixed (#344) and guarded
  (#345), the guard justified by that statement's history rather than by the count.

- [x] **`fpm_port` collided at an x.10 minor** ✓ 17 Aug 2026, ledger #346,
  plant-proven both paths. `base + major*10 + minor` gives each major ten slots, so
  `fpm_port("8.10") == fpm_port("9.0") == 9790`. The scheme keeps its ten slots and
  REFUSES the eleventh — every current port is byte-identical, because widening would
  move them all and adoption/managed-ports/the orphan sweep all enumerate by CALLING
  `fpm_port`, stranding running masters. `every_shipped_minor_has_a_unique_pool_port`
  turns the day PHP ships 8.10 into a `cargo test` failure that says what to change.
  **Deferred, not solved** — the scheme still needs a second base or an explicit table
  then; the guard is what makes that a build failure instead of a field incident.

- [x] **Create a site FROM a git repository — Stage 1, Laravel + Blank PHP** (planned +
  built 11 Aug 2026, `docs/PLAN-git-site-clone.md`). Laravel developers keep their
  projects in git; the Laravel card could only make a NEW app, so an existing repo meant
  cloning by hand, linking the folder, and wiring `.env` yourself. ✓ `NewSite.git_url`
  + one validator refusing clone-beside-link, agents, and WordPress (a checkout without
  its database is not a site); `clone_into_docroot` (staging sibling → `remove_dir` →
  `rename`, so the kernel — not a check of ours — is what makes a clone unable to delete
  a docroot's contents); `clone`/`deps`/`finalize` phases with `.env` written BEFORE
  composer (post-autoload-dump boots the app); the New Site dialog's third source with
  the ls-remote probe gating Create. v33 records the repo at INSERT because Retry is the
  recovery path. ARCHITECTURE §9; ledger #263–277; `git_site_clone_check` (L1, sandbox);
  SMOKE-TEST has the packaged-app half.
- [x] **Stage 2 — front-end assets for a cloned site** (built 11 Aug 2026). A Laravel
  app with Vite throws *"Unable to locate file in Vite manifest"* until `npm run build`
  has run, so Stage 1 was honest-but-incomplete for most real repos. ✓ v35
  `git_build_assets` + an `assets` phase that belongs to the CLONE (any repo can carry a
  `package.json`), running the repo's own package manager from the developer's
  login-shell Node. **The one NON-FATAL phase**: a failed build settles the job `ok`
  with an `assets_warning` banner, because failing it would park a created, wired,
  serving site behind a "setup incomplete" badge over the one step that was never
  rexenv's to guarantee. Ledger #280–283; `git_site_clone_check` §7 proves the phase's
  inputs; the non-fatal outcome is a SMOKE-TEST item (needs a machine without node).
  **Not done here, and deliberately:** the build is offered only at CREATE. Re-running
  it later belongs with Stage 3's site-level RepoPanel, which is where a per-site step
  runner already fits.
- [x] **Stage 3 — a Git panel on the site itself** (built 11 Aug 2026). ✓ A `site` job
  kind: `commands/repo.rs::job_target` resolves it to the project root, so nine existing
  commands work unchanged and the SAME `RepoPanel` renders a Repository tab — status,
  branch switcher, fetch/pull/push, the dependency steps a pull offers, and the repo's
  own package.json scripts (which is also how a Vite build gets re-run after create,
  closing Stage 2's known gap). A site target carries **no `dir_name`** — the one sent
  is the domain, display-only — making it the one kind with no user-supplied path
  segment. **Never an upward walk** from the docroot: `repo_site_info` is a single
  `<path>/.git` test, because the folder above a linked site can be a repo holding every
  project the user has. Ledger #284–288; `git_site_clone_check` §8; SMOKE-TEST covers
  the packaged half.
- [x] **Stage 4 — any PHP repo, and WordPress** (built 11 Aug 2026). ✓ A cloned
  Blank-PHP site gets `composer install` (Symfony/Craft/Statamic are `vendor/`-less by
  design), and WordPress runs the same four phases a created site does — each was
  already skip-aware, so what it needed was a dependency step first and one fork for
  Roots' layouts. What the user gets is their CODE and a fresh empty database, stated in
  the dialog rather than discovered. `core/dotenv.rs` came out of `core/laravel.rs` for
  the second caller and immediately caught a duplicate-key bug. Ledger #289–297;
  `git_site_clone_check` §9–10.
- [x] ⚠ **Bedrock from git — VERIFIED, and it was broken** (filed + fixed 11 Aug 2026,
  same day). Shipped marked unverified; running it against the real `roots/bedrock`
  found that `wp core install` was pinned to the docroot while Composer puts core in
  `web/wp`, so every wp-cli call on a Bedrock site answered "This does not seem to be a
  WordPress installation". ✓ `wordpress::core_root` follows the layout at the one place
  the invocation is built (so LINKED Bedrock checkouts get it too); ledger #294 + #299;
  `git_site_provision_check` case 4 now installs WordPress and lands 12 tables.
  **Radicle is still unverified** — same code path, no live project to hand.

- [x] **Every link opened in whatever browser the OS points at** (filed + fixed
  11 Aug 2026). rexenv could pick your code editor but not your browser, so a
  Chrome-default machine could not send its sites to the browser it develops in,
  and no button ever said WHERE a click would land. ✓ `preferred_browser` +
  `detect_browsers`/`open_in_browser`, applied at the ONE choke point
  (`open_external`, so all ~12 call sites obey), URL-only guard, per-open
  installed-ness re-check, real extracted app icons for browsers AND editors, a
  one-time chevron beside "Open in browser", and a Settings "Web browser" row.
  ARCHITECTURE §8.2; ledger #301–262; `browser_detect_check` (L1) +
  `openin.js` (L2, plant-proven); design record `docs/PLAN-browser-preference.md`.
- [x] **Open a site in a browser's PRIVATE window** (14 Aug 2026). Checking a site
  logged-out meant signing out of the session you were working in. ✓ Each row of
  the browser chevron carries a second target behind a divider (`MenuItem`'s
  `action`), backed by a per-browser private flag in the macOS `BROWSERS` table
  (`open -na <app> --args --incognito|-private-window <url>` — `-n` is
  load-bearing) and a `private` ARGUMENT on `open_in_browser`, so the URL-only
  guard stays one check for both modes. Offered only where the flag is known to
  work: Safari has none, and the backend refuses rather than opening a normal
  window. ARCHITECTURE §8.2; ledger #261 (widened) + #309; `browser_detect_check`
  (L1) + `openin.js` (L2, plant-proven ×2); the window really being private is a
  human eye — SMOKE "Which app opens a link".
- [x] ⚠ **A plugin's shutdown hook broke every WordPress screen on a PHP 8.4 site**
  (filed + fixed 14 Aug 2026). Activating Elementor 4.2.2 under PHP 8.4 made the
  Plugins/Themes/Users/Tools tabs die with `wp plugin: bad JSON: trailing characters
  at line 1 column 814` — the site was unmanageable from rexenv, and `wp option get
  home` was wrong the same way, so it was never a JSON problem. Cause, traced rather
  than guessed: Elementor registers its own WP-CLI logger and prints the notices it
  collected from a shutdown hook (`Manager::shutdown` → `Cli_Logger::save_log` →
  `WP_CLI::log` → `fwrite(STDOUT)`), i.e. AFTER the command's own output. ✓ Fixed by
  POSITION, not by mechanism: a rexenv `--require` file beside the phar registers the
  FIRST shutdown function, its marker separates the command's output from everything
  printed after it, and every captured spawn cuts there (`cut_post_run_tail`) and
  carries the tail over to stderr attributed rather than dropping it. The two
  plausible alternatives are recorded as MEASURED NON-FIXES —
  `-d display_errors=stderr` and a shutdown-opened output buffer both move nothing.
  ARCHITECTURE §9; ledger #316; 7 lib tests + the module-wide coverage guard +
  `wp_noise_check` (L1, sandbox, control leg plants the disease); verified end to end
  against the reporting site (7 plugins parsed, the notice on stderr).
- [x] ⚠ **…and the same report on PHP 8.5, from the OTHER end** (filed + fixed 14 Aug
  2026, ledger #317). Same dead WordPress tab, no plugin involved: PHP's CLI SAPI
  prints its own diagnostics to STDOUT, and the pinned 2.12.0 phar raises one under
  8.5 in its own vendored code (`Deprecated: Case statements followed by a semicolon
  (;) … react/promise/src/functions.php on line 369`) before wp-cli prints a byte — so
  the notice arrived in FRONT of every answer. ✓ `-d display_errors=stderr` in the
  SHARED argv prefix (so the streamed spawns get it too, and before the phar — a `-d`
  after the script name is an argument to the script). Moved, not silenced: streamed
  steps merge both streams into one live log, so nothing vanishes from an install.
  The pair is the point — this flag cannot fix #316's tail and the marker cannot fix
  this head; both ends verified end to end on the reporting site under 8.4 AND 8.5.
- [ ] **The pinned wp-cli phar (2.12.0) is not PHP 8.5-clean.** #317 moves its
  deprecation off stdout; it does not make it go away, and a user running `wp` in
  rexenv's terminal (deliberately unpinned, #228) still sees it on every command. Worth
  re-checking when wp-cli ships a release that fixes `react/promise` — the pin bump is
  the real fix, this is the containment. **Re-checked 15 Aug 2026:** 2.12.0 is still
  the latest release (upstream issue wp-cli/wp-cli#6271 tracks this exact deprecation);
  `react/promise` 3.3.0 carries the fix and wp-cli's source already depends on it
  transitively via composer ^2.9.5, so the NEXT wp-cli release should clear it —
  nothing to bump yet.
- [x] ✅ **Three text tokens failed WCAG AA on every surface, in both themes — and
  one carried a comment claiming it didn't** (ledger #337, reported 16 Aug 2026 by
  an outside reader building the docs site; **closed the same day**).
  `tokens.css` said `--rex-text-label: /* mono section labels (darker: small type
  needs AA) */` — the only contrast claim in the codebase, true on pure white and
  false on every other light surface.

  ✓ **The scan first, and it decided the fix.** `every_text_on_surface_pairing_meets_wcag_aa`
  + `every_rex_colour_class_names_a_token_that_exists` (`core::copy_scan`), both
  sets DERIVED from the frontend's own class usage. It sized the debt at 39 pairs
  and then answered the design question with data: **135 of 142 usages sat on
  `<div>/<span>/<li>`** — emails, versions, paths, a status pill, placeholders —
  and only 7 on icons. They were text tokens below AA, not ornament tokens.

  ✓ **The repair.** 135 consumers → `text-muted` (AA-clean everywhere); `text-faint`
  and `text-label` went unused and were **deleted**, which the sibling guard now
  polices for free since a deleted token names nothing; `text-dim` survives on its
  7 lucide glyphs; light `accent-blue` darkened one step (`#2e6f94` → `#2e6e93`)
  for a pair reading 4.49:1, one hundredth under. Every replacement's contrast was
  computed BEFORE it was written.

  ✓ **The scan found the worst instance in a place it could not originally see.**
  `globals.css` styled EVERY input's `::placeholder` with `--rex-text-faint` in raw
  CSS at 2.58–3.35:1 — the most widespread text in the app, invisible to a
  Tailwind-only scan. It now reads `color: var(--rex-*)` from `src/styles` too.

  ⚠ **And the exemptions had to be rebuilt, because a plant walked through them.**
  They were declared BY TOKEN NAME ("`text-dim` is icon-only"), so moving
  `text-rex-text-dim` onto a `<span>` kept the exemption and the guard stayed
  green — **the guard-covers-claimed-surface defect, committed inside the guard
  written to end that family**. Icons are now excluded STRUCTURALLY, by reading
  which element the class sits on. The same plant now fails with 13 pairs. The
  lesson is not "be careful with allow-lists" — it is that an exemption keyed on a
  NAME cannot notice when the thing changes underneath it, and the only version
  that holds reads the thing.

  ✓ **The ratchet's lifecycle, which is the reusable part.** 39 pairs recorded so
  the gate could go live while the fix was scoped (a red `verify.sh` blocks every
  commit through the pre-commit receipt), then a forced failure at "38 now PASS"
  that made the repayment be written down rather than absorbed, then deleted along
  with the debt. Both lists are gone; both assertions are unconditional.

  **Still owed, and stated:** no L2 render check — the app's look changed in ~40
  files and nothing but an eye has confirmed it. Worth a pass on the packaged app,
  or a wk-check that samples a dense screen.
- [ ] **Private-window flags for Arc, ChatGPT Atlas, Orion.** Left `None` in the
  `BROWSERS` table because no one has run the flag on a real install, and a fork
  that swallows the flag it inherited opens an ordinary window under a control
  that said private. One-line each once tested; the rows simply show no private
  icon until then.
- [ ] **Windows/Linux: `detect_browsers`/`open_in_browser` are the default empty
  stubs** (Phase 4, same shape as `detect_editors`). Until they are filled, those
  platforms open every link in the OS handler and show no chevron — honest, but
  the Settings row will read "No browser detected". The private-window arm is
  part of that stub: `supports_private` is false everywhere, so those platforms
  show no private target rather than a dead one.

- [x] **MCP server M1 — read-only diagnosis + opt-in card** (branch `feat/mcp-m1`).
  ✓ Socket + `rex mcp` shim + registry (list_sites / site_status / tail_log) +
  ReadCtx read-only boundary (guard scans both surfaces) + secret-leak sweep +
  activity feed + the opt-in toggle that really binds/unbinds + Settings "AI
  agents" card + per-site SiteDetail section. Ledger #198–#203; wk-checks
  `agents-*`. Diverged from PLAN §7.3 honestly: no mail tools/sub-toggle in M1;
  `site_status` runs nothing (Option A, no HTTP GET); status line is feed-driven
  not a live-session list. **Next MCP stage = M2a (scratch sites)** — see
  `docs/PLAN-mcp-server.md §7.3`, reconciled against this shipped M1 on 1 Aug.
- [x] **PHP could not resolve `.rex` — WP-Cron silently dead on every hosted site**
  (filed 10 Aug 2026). The bundled static-php builds link libcurl against **c-ares**,
  which reads `/etc/resolv.conf` alone and never `/etc/resolver/<tld>`; `gethostbyname`
  worked, every `curl` to a rexenv host returned errno 6, and WP-Cron never reports a
  failed spawn. ✓ Fixed by the `rexenv-dns.php` mu-plugin (`core/wp_dns.rs`) —
  `CURLOPT_RESOLVE` from the system resolver's own answer, restricted to
  loopback-served resolver zones; installed at provision, re-installed after a rename,
  swept for all sites at launch. Ledger #251–253; `wp_dns_check` reproduces the bug
  (errno 6) and proves the fix (200) under the real bundled PHP.
- [ ] **Eliminate the bug class: bundled PHP with curl's THREADED resolver** (the real
  fix for #251 — the mu-plugin covers the WordPress HTTP API, not raw `curl_init()` in
  a plugin, and not non-WordPress PHP apps rexenv hosts). Needs a self-built
  static-php (`--enable-threaded-resolver` instead of `--enable-ares`) for 7 minors ×
  cli/fpm × 2 arches. **That path is no longer blocked** — it was "the same
  self-hosted-artifact path the Xdebug debug build is blocked on", and as of
  14 Aug 2026 `rexenv/runtimes` builds, gates, signs and publishes exactly this
  shape of artifact (`docs/PLAN-php-74-support.md`). **RULED 15 Aug 2026: not
  now.** Rebuilding seven minors self-hosted is a maintenance burden carried
  forever, for a dependency nobody has complained about — a commitment, not a
  fix. The option stays recorded here with the runtimes-repo note so it is
  known when there is a reason. Until then `wp_dns_check` FAILS LOUDLY the
  day a build stops using c-ares — that is the signal this item is done.
- [x] **DNS agent answers ARBITRARY names when queried directly — ACCEPTED
  15 Aug 2026, not deferred.** `dig -p 15353 @127.0.0.1 <any-hostname>` returns
  `127.0.0.1`; the hickory handler is a catch-all, not per-TLD zones. RULED
  accepted with reason: scoping to configured TLDs would require the agent to
  KNOW the TLD set — reloadable state or per-query config reads — and #45's
  proven, load-bearing design is exactly "no in-process TLD state; adding a TLD
  never restarts DNS"; a KeepAlive LaunchAgent that outlives app updates is the
  worst place to introduce reloadable state. The actual containment is the
  loopback bind (#44, structural since T10) and it holds. **What would reopen
  this is the agent ever binding beyond loopback — never the arbitrary-names
  behaviour itself.**
- [x] **rexenv's WP-CLI no longer inherits `~/.wp-cli/packages`** — DONE, all four
  parts (found 4 Aug 2026
  while costing dist-archive; ledger #228). Every `wp` rexenv runs FOR A USER is
  pinned to the bundled command set; `core::terminal`'s `wp` wrapper stays ambient
  by decision — that is the user's own command line, and pinning it would break
  `wp package install` from inside rexenv in a way that looks like our bug.
  ✓ (a) **DECIDED 5 Aug 2026 — neutralise WITH A TELL**; ✓ **LANDED 13 Aug 2026**,
  first thing after v0.1.0 as queued.
  ✓ (b)+(c) **the spawn sites and the L0 scan — done by refusing to count them.**
  This is the part worth remembering: the ledger row named FOUR sites and there
  were SEVEN, two added after the row was written. A guard asserting the four
  would have shipped narrower than its own claim. So there is ONE argv builder
  (`wordpress::wp_argv_prefix`) and ONE `Command::new(php_bin)`
  (`wordpress::wp_command`), and the scan asserts the marker literal appears in
  exactly two files in `src/` + `examples/` — the builder, and the terminal
  wrapper with its reason. 6 lib guards, all five failure modes plant-proven;
  the scan itself first failed by reading its own prose (#235's defect, caught by
  its own canary) and now strips comments.
  ✓ (d) **done 4 Aug, rewritten 13 Aug** — `core/wordpress.rs`'s module doc stated
  the unfixed state; it now states the pin, its scope and why the terminal is out.
  ✓ **the tell — LANDED 13 Aug 2026** (ledger #301): the Settings card + the
    explanation APPENDED to `not a registered wp command` at the captured path and
    the MCP raw runner. Copy approved with one redline ("not loaded into the commands
    rexenv runs for you" — "into them" referred back across a sentence boundary).
    Both must-say lists plant-proven; the don't-guess rule proven at BOTH layers
    (L0 asserts the branch, L2 `wppackages-unnamed` asserts what it renders).
  ✓ **the L1 leg — LANDED 13 Aug 2026**: `wp_packages_check` (sandbox tier).
    Plants its own canary package and REQUIRES it to resolve unpinned first —
    a control that silently failed would make the whole check "nothing resolved
    either way", green and vacuous. Five legs, plant-proven five ways. Ledger
    #228 is ✅, **scoped to what the run showed**: a package registering via
    `WP_CLI::add_command`, on this machine, through both production spawns. Not
    a package that hooks WP-CLI another way, and by design not the terminal
    wrapper — leg D pins that the OPPOSITE way, because the tell's last sentence
    depends on it staying ambient.
- [x] **B29b — fpm pool reap is still one-miss** — ✓ DONE 15 Aug 2026, ledger #330.
  `Pool::misses` + `pool_fate` (the shared `adopted_reap_decision` arithmetic, one
  definition): probe evidence reaps only after `POOL_MISS_LIMIT` consecutive misses;
  a spawned child's `try_wait` exit still reaps on sight; an ADOPTED master is judged
  by `pid_command` containing `php-fpm`, never `kill -0` — a recycled pid or orphan
  workers on a green port accrue misses instead of living forever. Plant-proven both
  ways (limit=1 and identification-always-true each fail the named leg).
- [x] **New Site "Laravel" card promises an installer that doesn't exist** —
  the flow was BUILT rather than the copy softened (9 Aug 2026, `cfe4be3` +
  `fb4ae08` + `52c4783`). ✓ Evidence: `phase_defs` gains db → app_install →
  configure for `SiteType::Laravel`; `core::laravel` runs
  `composer create-project laravel/laravel` through the site's bundled PHP,
  wires `.env` (DB_* + APP_URL) and re-runs the migrations against the site's
  database — the skeleton's own post-create `migrate` runs while `.env` still
  says sqlite, so without that step the MySQL database stays empty; `.env` is
  kept OUT of the served tree by v32 `docroot_subdir` + `Site::served_root()`,
  with a backfill for pre-existing Laravel rows; `db_created` is recorded so
  delete drops the database instead of orphaning it. Live-verified against
  laravel/framework ^13.8. The nit parked here — every type deriving a `wp_`
  database name — was FIXED 13 Aug 2026: `wordpress::db_name_prefix` makes the
  prefix per type (`wp_`/`lv_`/`php_`) and `db_name_for` takes the type as a
  required parameter, so no call site can inherit `wp_` by omission. Creation-time
  only: pre-existing rows keep their stored name and the v6 backfill stays `wp_`,
  because renaming a live site's database is not cosmetic. ✓ Evidence: ledger #97,
  `db_name_prefix_is_per_site_type_and_never_wp_for_a_non_wp_site` +
  `create_stores_the_prefix_of_the_sites_own_type_not_wordpresss`.
- [x] **Onboarding's :443 probe — DONE 13 Aug 2026** (migration plan §3a, gap 2 of 2) — RULED
  13 Aug 2026: **warn and continue, never block** (onboarding needs nothing on :443,
  and Herd running while someone tries rexenv is a deliberate state; the follow-on
  surfaces already exist — the provision card's servingBlocked note, the watchdog's
  edge-blocked event, doctor's Edge line). **It was never a fifth caller**:
  onboarding runs BEFORE services, so `edge_answers_as_ours` — "is OURS what
  answers" — is false for every user on a clean first run, and adding it would have
  shipped a foreign-proxy warning to everyone.
  ✓ **Phase A landed** — `proxy::EdgeWire{Ours,Foreign,NoAnswer}` + `edge_wire`,
  with the boolean kept as `== Ours` so the four existing callers are untouched
  (ledger #304, `edge_wire_check`).
  ✓ **Phase B — all four callers landed 13 Aug 2026** (ledger #305). Each says
    something DIFFERENT about `NoAnswer`, because the variant means a different
    thing in each: import → the stack isn't running (and offers **Start all as a
    button in the toast**, since the fix is in this app); watchdog and doctor →
    our edge process is alive and not serving; `verify_edge_wire` → the start we
    just ran didn't take. `commands/site_provision.rs` needed no change — it reads
    `mgr.edge_blocked()`, which only the watchdog sets and only when our edge is
    alive.
  ✓ **Onboarding itself — landed 13 Aug 2026** (ledger #306). Warns, never blocks;
    `NoAnswer` renders NOTHING, because nothing on :443 at onboarding is the ordinary
    state and reporting it would be the import bug in a new place. Copy approved
    13 Aug, guarded, with `you can finish setting up` as the load-bearing clause.
    One voice across all five :443 messages: the holder is named from the supervisor,
    never guessed (the provision card's "most likely Herd" is gone), and "unreachable"
    became "won't load".
- [ ] **Resolver-drift surfacing — RULED 13 Aug 2026: wire the surfaces, keep the
  binding.** A user whose TLD was taken back has genuinely lost resolution; today
  they learn it from a log line nobody reads or by happening to visit `/import`.
  ✓ **`rex doctor` renders it** (13 Aug): a `Resolvers` line that COUNTS toward
  findings and the exit code, names the TLDs, and points at rexenv → Import (the
  only place a takeover can be redone — there is no `rex` command for it). An
  absent field reads ⚠ unknown, never ✓, so an older app cannot report a clean
  check it never ran. `doctor`'s one-line description gained resolvers.
  - [x] **remaining: the frontend binding needs a caller — RULED, copy approved
    with one redline ("You can take it back from Import."), BUILT 15 Aug 2026.**
    ✓ `ResolverDriftBanner` on Sites, first in the banner stack; ledger #334
    (L2 lifecycle probe, plant-proven ×2, and the probe's first run caught the
    self-heal firing on the query's loading state — it would have wiped every
    dismissal on every launch). Original ruling kept below.** Shape approved: a dismissible
    launch-time banner on the Sites screen when `resolverDrift()` reports lost
    TLDs, pointing at Import, doctor's voice. Two ruled conditions: (a) it must
    NOT render when nothing was taken back — `[]` renders NOTHING, the ordinary
    state, exactly the onboarding-notice rule (#306); (b) dismissal persists
    PER-TLD, and clears when that TLD reads as ours again — so a dismissed
    `.test` re-shows on the NEXT takeover-loss, and a newly lost `.dev` is never
    hidden by an old dismissal (self-healing: drop stored dismissals for TLDs no
    longer drifted). DRAFT COPY, awaiting redline before landing —
    title: "Your .test sites stopped resolving" (multi-TLD: "Your .test and
    .dev sites stopped resolving"); body: "Valet or Herd took .test's resolver
    file back, so those sites won't load until rexenv takes it over again.
    That's redone in Import."; actions: [Go to Import] [Dismiss].
  ✓ **The continuous watcher is CLOSED, not deferred again** (Stage 1 D3): a fact
  surfaced in doctor, Import and startup is enough without polling. If that is
  wrong it shows up as someone confused about why their sites stopped resolving,
  and that report is the evidence to reopen it — not a guess now.
- [x] **`teardown` and `change_site_domain` now remove the Apache per-site
  config/log** ✓ 13 Aug 2026, ledger #302. Additive fix in both sweeps + the test
  in the `teardown_removes_row_and_per_site_artifacts` shape — plus the guard that
  makes the fix hold: both sweeps are hand-maintained lists behind names promising
  ALL per-site artifacts, so detection moved to the WRITE side (a core module with
  `config_path`/`log_path` taking a `domain` owns a per-site file and must appear
  in both). Plant-proven; removing Apache from `change_site_domain` fails ONLY the
  guard, because no lib test reaches the rename path.
- [x] **`sites_dir` is validated at the setter** ✓ 13 Aug 2026, ledger #303.
  Refused, never sanitised — a stripped character hands back a folder the user did
  not pick. Relative paths refused; spaces, unicode and `'` accepted (the configs
  quote, nothing goes near a shell). **An existing value that would fail the rule is
  left alone**: validation is write-path only, because refusing at read time would
  relocate someone's sites folder to the default and make every site they own look
  missing. Plant-proven, including the read-path over-fix.
- [ ] **Debug-log truth on Bedrock** (deferred with the wp-config-reader work):
  parse `config/application.php` env defines so WP_DEBUG/WP_DEBUG_LOG read
  truthfully on non-stock layouts; today's honest state is `indeterminate`
  ("can't determine", `core/logs.rs:208-252`).
- [x] **Per-backend tunnel origins for override sites** ✓ DONE 15 Aug 2026, ledger
  #332. `tunnels::origin_port` resolves nginx-served → shared HTTP port, override →
  `sites::recorded_override_port` (the config generator's own accessor, so origin and
  reality cannot drift); a stopped override backend refuses at start from the
  ServiceManager's override map (ownership+liveness, never a bare port-listen);
  `ensure_tunnelable` and the Tunnels card's courtesy wall are retired. Mid-share
  drift was already covered (web-server switch/docroot move refuse while shared).
  Plant-proven at L0. **Still owed (the row's noted half): an override site serving
  through a REAL tunnel end to end** — SMOKE §Public sharing gained the step; a
  network-tier leg would need a FrankenPHP fixture on `tunnel_exposure_check`.
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
- [x] **Nits batch** ✓ 13 Aug 2026. The interpolated Tailwind class in
  `ui/dialog.tsx` (harmless by LUCK — `mt-3` existed because another file used
  it, `mt-0` never existed and its absence looks identical to a margin of zero)
  is now whole class names through `cn()`, **and the shape is linted**
  (`no_tailwind_class_name_is_built_by_interpolation`): every `className={…}`
  expression is brace-matched, and `${` must follow whitespace or a delimiter.
  The lint took three plants to become true — a line window both MISSED a `cn()`
  continuation line and FLAGGED an unrelated `example={…}` prop, and keying on
  `-${` alone missed `text-[${n}]` and `hover:${c}`. `wp_login.rs`'s "256-bit" is
  now ~244 (a v4 UUID carries 122 random bits, not 128; corrected because a
  security comment that rounds in the FLATTERING direction is one a later reader
  trusts instead of re-deriving). `downloads_dir()` is one definition
  (`core::downloads::user_downloads_dir`) — there turned out to be FOUR copies,
  not three: `core/wordpress.rs` had one the note never mentioned.
  - [ ] Still open: the `validate_linked_docroot` per-call `list(conn)` cost note
    (`core/sites.rs` — fine at current scale, hoist if imports grow).

- [x] ⚠ **The macOS floor we CLAIM and the one our binaries have are different
  numbers - RULED 'fix the claim, not the binaries' and DONE 15 Aug 2026, with the
  full measurement worse than this item knew.** The complete cache sweep
  (`docs/PORTS.md` now carries every number): PHP/caddy/mailpit/FrankenPHP 12.0,
  MySQL/MariaDB/Redis **14.0**, nginx/cloudflared **15.0**, PostgreSQL **26.0**.
  ✓ `minimumSystemVersion` 11.0→**15.0** and INSTALL.md says macOS 15 (Sequoia),
  because 15 is what the DEFAULT stack (edge+nginx+PHP+MySQL) actually requires -
  the ruling's 12.0 shape assumed nginx could be re-pinned ≤12, and it cannot:
  jirutka publishes nothing below minos 14 (checked 1.24.0→1.31.3), and the only
  14.0 builds are stale 1.24/1.26.1-2, a security downgrade to gain one macOS
  version. ✓ PORTS.md carries the per-binary `minos` beside the pins with the
  re-measure rule. Two follow-ups filed below. Original finding kept for the record: `tauri.conf.json` sets `minimumSystemVersion: "11.0"` and
  `docs/INSTALL.md:12` says "macOS 11 (Big Sur) or later" — but the pinned
  binaries, measured 14 Aug 2026 on the real cache, are: **php 8.1.34 / 8.3.31 /
  8.5.8 → `minos 12.0`** (static-php-cli's macOS default), **caddy → 12.0**,
  **mailpit → 12.0**, and **nginx 1.30.3 → `minos 15.0`**. So on macOS 11 or 12
  the app installs and then cannot run its own web server, and the install page
  promised it would. Found while setting the deployment target for the 7.4 build,
  which is why the number is measured rather than assumed.
  **Two ways out, and it is a product decision, not a bug fix:** raise the claim
  to what we actually ship (12.0, and re-pin nginx to something ≤ that), or keep
  11.0 and re-pin every binary to match. Either way `docs/PORTS.md` should carry
  the per-binary `minos` beside the version, because this drifted silently and a
  number nobody records drifts again. Do NOT fold this into the 7.4 work: 7.4
  matches the 12.0 the other PHP rows already have, so it neither causes nor
  worsens this.
- [ ] ⚠ **PostgreSQL's pinned builds carry `minos 26.0` — presumed dead below
  macOS 26, and the presumption cannot be tested from this machine** (found
  15 Aug 2026 during the floor sweep; MEASURED as far as this host allows the
  same day). What the measurement showed: dyld on macOS 26 enforces minos for
  NEITHER main executables NOR dylibs — postgres patched to `minos 99.0`
  (binary and libssl, re-signed) runs clean — while deterministic dyld crashes
  of minos-15 binaries on macOS 14 are documented in the wild. So enforcement
  is a property of the OLDER host's dyld, and only a macOS 14/15 VM can settle
  whether postgres actually fails there (PUBLISH-TESTING clean-VM shape).
  **The failure surface IS traced, labelled as prediction:** if dyld kills the
  child at spawn, the user gets "PostgreSQL did not start within Ns — see
  postgres-stdout.log" (`await_ready` names the log; `spawn_logged` captures
  BOTH streams, so dyld's real reason lands in that log) — a timeout pointing
  at a log that holds the truth, not a silent no-op, but the top-level line
  says nothing about macOS versions. The VM check has TWO questions, not one
  (ruled 15 Aug 2026): whether it fails, AND whether the message leads anywhere —
  a user reading "didn't start in Ns" looks at Postgres, not at their macOS
  version, so if the refusal is real the top-level line needs the version fact,
  not just the log pointer. If the VM confirms the refusal: re-pin to
  lower-target theseus-rs releases (or another source), and consider a
  version-aware tell on the Databases screen.
- [ ] **Option, not a commitment: a self-built nginx (deployment target 12)
  would drop the app floor from 15 to 14** (MySQL's floor). Same
  `rexenv/runtimes` path that built PHP 7.4; recorded like the c-ares ruling -
  known, waiting for a reason (e.g. macOS-14 users actually asking).
- [x] **A FrankenPHP site's `php_version` is a promise it cannot keep — RULED
  read-only-with-annotation and BUILT 15 Aug 2026** (refusal rejected: the pairing
  is not invalid, it is fixed by the backend, and refusing teaches nothing —
  the same reasoning that retired the override-site tunnel wall). ✓ The
  SiteDetail Environment card on a FrankenPHP site shows the SERVED version
  (the `frankenphp_embedded_php` command — one backend pin, no frontend copy
  to drift), a DISABLED select labelled "8.5 — FrankenPHP's embedded PHP", and
  the sentence "Fixed by FrankenPHP. Switch the web server to Nginx or Apache
  to choose a version." Ledger #326 (major-mismatch refusal) stands unchanged.
  Ledger #333; mock's `network.rex` is FrankenPHP now so the dev route renders
  the state. **L2 gap stated:** no wk-check asserts the picker is disabled.
- [ ] **PHP 7.4 support** (planned 14 Aug 2026, `docs/PLAN-php-74-support.md`). The
  standing claim that 7.4 "has no build and never will" was true about static-php.dev
  and **false about PHP**: static-php-cli has no version floor (7.4 download+extract
  run live), Herd already ships 7.4 built by spc 2.8.6, and a full WP extension set
  compiled clean against curl 8.21 / ICU 78.3 / OpenSSL 3.6.3 in 74s. Chosen source:
  self-build with spc in a public `rexenv/runtimes` repo, from
  `shivammathur/php-src-backports` (vanilla 7.4.33 fails on OpenSSL 3.6), hosted as
  immutable GitHub Release assets — one manifest arm + four checksums, versus ~76
  pinned digests for the ghcr-bottle alternative.
  - [x] **S0.1 — derive the "unshipped version" fixture.** ✓ `php::unshipped_minor()`
    /`unshipped_patch()` walk candidates and PANIC if they all ship; all 8 asserts,
    the example and both manual steps moved off the `7.4` literal. Ledger #318,
    plant-proven (7.2.34 into `PHP_VERSIONS` → fixture self-heals to 7.1). The two
    manual steps still carry a literal and the row says so.
  - [x] **S0.2 — `needs_tree_relink` waved through an ESCAPING `@loader_path`.**
    ✓ The prefix is now RESOLVED (lexically, component-wise) and required to land
    under the bundle root; `@executable_path` is always rewritten because it is
    unanswerable from the tree. Ledger #319, plant-proven at BOTH layers — L0
    `an_escaping_loader_path_is_not_mistaken_for_in_tree` and the new L1
    `relink_tree_check` (sandbox), whose leg B proves the tree is REPAIRED rather
    than merely refused and whose leg C is the control.
  - [x] **S0.3 — a bundle tree that dyld cannot load is cached FOREVER.** ✓ DONE
    15 Aug 2026, ledger #331 — the **relink-receipt** shape, not exec-the-member
    (a member can be a dylib — `xdebug.so` — and exec proves nothing about one).
    `.rexenv-prepared` records the prepare-logic revision, stamped after
    prepare+member-verify, before the atomic publish; `resolve_bundle` and
    `is_cached` both require a CURRENT receipt, absence is stale (every
    pre-receipt tree is from the era that includes #319's broken predicate, and
    no stat can tell a good one from a poisoned one — one refetch per cached
    bundle is the price of repairing the field). Fixing a future prepare bug =
    bump `PREPARE_REV`, which is what carries the fix to machines already
    holding the broken output. Plant-proven (absence-reads-cached fails by name).
  - [x] **S0.4 — per-minor Xdebug version.** ✓ `XdebugBottle` carries version +
    formula + both digests as ONE row; `bundle_manifest` gates on the row, not on
    `XDEBUG_VERSION` (now the DEFAULT the in-window minors reference). Ledger #320,
    plant-proven in both directions with a frozen `"7.4" => 3.1.6` row. The old gate
    failed SILENTLY — `None` reads exactly like "this minor has no Xdebug", which is
    a real state (8.0), so the bug wore a supported outcome's disguise.
  - [x] **S0.5 — Xdebug support in the DTO.** ✓ `PhpVersionView` (a SEPARATE type
    from the persistence `PhpVersion`, so a row whose derived fields were never
    filled is unrepresentable rather than merely unlikely) carries
    `xdebugSupported` + `xdebugVersion`, derived per read in `core::php::list_versions`.
    `SiteDetail`'s `minor === "8.0"` literal is gone. Ledger #321. **L2 gap stated:**
    no wk-check renders `XdebugCard`, so "the control is actually disabled" is
    unproven; `mock.ts` carries the 8.0 not-supported row so the dev route shows it.
  - [x] **S0.6 — the EOL tell.** ✓ Covers 8.0 (dead Nov 2023) and **8.1** (dead Dec
    2025), both of which rexenv had been offering silently — found while building
    this. `core::php::security_end` holds php.net's END DATES and `eol_since`
    compares against today, so the answer is computed, not remembered; a new minor
    without a date fails the build. Surfaced on the Settings row, the create-dialog
    note, and the site's own Environment card (most sites on a dead runtime got
    there by import or by outliving the version). For WordPress the note names WP's
    own outdated-PHP notice in advance. Ledger #322, DESIGN.md honest-UI rule.
    **L2 gap stated:** no wk-check renders any of the three surfaces.
  - [x] **S2.0 — branch `manifest()` on source BEFORE any checksum is pinned.** ✓
    `php_url` picks the publishing source; `php_self_hosted_tag` carries the FULL
    immutable release tag into the URL (a rebuild is a new tag, never a re-upload
    — the property neither static-php.dev nor FrankenPHP offers). An empty hash
    const reads as unpinned, so 7.4 is wired but unresolvable until its artifact
    exists. Ledger #323, plant-proven. `manifest_pins_every_pinned_php_version`
    strengthened from URL *shape* to HOST — shape was the hole: every 404 in this
    family has the right shape.
  - [x] **S1.1 — `rexenv/runtimes` + the build workflow.** ✓ Public repo created
    15 Aug 2026 with the workflow, four publish gates, licence collection (a source
    with no findable licence FAILS the build) and the immutability contract. 14
    build rounds; the three real blockers were all fixes upstream had already made
    and 7.4 never received — PLAN §10c.
  - [x] **S2.1 — pin + `PHP_VERSIONS`.** ✓ Ledger #325. Pinned from the bytes rexenv
    itself downloads, cross-checked against the release's SHA256SUMS, run-proven by
    hand AND through `php_versions_check`. The resolvability assertion flipped in
    the same commit as the pin.
  - [x] **S2.2 — doc sweep.** ✓ PORTS/ARCHITECTURE/README/valet-import/valet-migration
    corrected in place; the c-ares item's "blocked on the self-hosted path" is
    retired because that path now exists.
  - [x] **S3 — live proof beyond the download.** ✓ `php_versions_check` (NETWORK):
    7.4 downloads, verifies, relinks, signs and RUNS. ✓ `php_fpm_serve 7.4.33`
    (sandbox): 7.4's own php-fpm accepts the config rexenv generates for it
    (ledger #327). ✓ `php_pools_serve` (SERVICE, stack stopped): all seven pools up
    together, 7.4 on 9774, all stopped clean with no leaked workers.
    **Still owed, and not claimed:** a 7.4 SITE answering over HTTPS end to end
    (browser → Caddy → nginx → 9774 → WordPress). Needs a real site; it is a
    SMOKE-TEST item, not an example. Neither tier runs in `verify.sh`.
  - [x] **S1.2 — 7.4 is at parity with the 8.x rows except five.** ✓ 60 modules
    (was 36), release `php-7.4.33-6`: the phar fix brought back dba/pgsql/soap/
    xsl/gmp/bz2/ftp/calendar/posix/pcntl/readline/shmop/sysv* **and phar**, and
    the parity pass added **apcu, redis, imagick, imap, event** — all five
    attempted rather than assumed, because whether a PECL release still supports
    7.4 is a fact about that release. **redis mattered most**: rexenv ships Redis
    as a SERVICE, so without it a 7.4 site could not use the object cache the app
    itself offers. Absent, each for a reason and none an omission: `random` (a
    PHP 8.2 CORE extension), `opcache` (spc's static patch starts at 8.0),
    `opentelemetry`/`protobuf` (spc guards on < 8.0), `swoole` (dropped 7.4).
    Recorded in `docs/PORTS.md` beside the pin, which is where someone looks.
  - [x] **Ship the PHP 7.4 licence texts onto the user's machine.** ✓ 16 Aug 2026,
    ledger #336. rexenv BUILDS and hosts 7.4, so it is the distributor and PHP
    License 3.01 §2 attaches. Reproduced-in-the-docs was defensible; shipped-
    beside-the-bytes is not arguable, and a licence obligation is the last place
    to hold a position that needs defending. ✓ `php_licenses_spec` — a second
    manifest arm keyed on `is_self_distributed` (never on "7.4"), fetched INSIDE
    the same staging dir as the binary so it rides the atomic publish: a
    published 7.4 either carries `licenses/` or does not exist. A fetch failure
    FAILS the resolve, because "ship the interpreter anyway" is the outcome being
    prevented. Caches from before this are stale via `licenses_satisfied` and
    repair on next resolve (the `.rexenv-prepared` asymmetry, #331 — one refetch
    is the price of repairing the field). Both arches pinned from the release's
    own SHA256SUMS. L0 plant-proven both ways; L1 `php_versions_check` asserts 15
    files incl. `PHP-3.01.txt` beside BOTH `php` and `php-fpm`, and a
    licence-less cache was planted and observed self-repairing.
  - [x] **A guard for the class, not the instance** (the notices lesson). ✓
    `the_notices_cannot_disclaim_distribution_while_we_distribute`. The finding
    was not "check the notices": `docs/PLAN-php-74-support.md` §6.5 named that
    exact sentence, and called it the one item a later commit could not fix — and
    it shipped false anyway, in a public repo, for a day, through a docs sweep.
    **Flagging is not a mechanism**, which is `core::copy_scan`'s finding one
    layer down. Ban + must-say halves (a ban alone is satisfied by deleting the
    sentence and saying nothing), keyed on the live `is_self_distributed` fact so
    a second self-built runtime inherits it by existing and the guard stands down
    on its own if self-building ever stops.
  - [ ] **`rexenv/runtimes`' release notes for `php-7.4.33-6` describe `-4`.** Two
    lines are stale on the release page users and auditors read: it says
    `MACOSX_DEPLOYMENT_TARGET=11.0` was "asserted per artifact" (the artifacts
    measure `minos 12.0` — spc's macOS default; PLAN §10b corrected 11.0→12.0 and
    the workflow followed, the prose did not), and it says "the extension set is
    narrower than the 8.x builds … widening is in progress" when `-6` IS the
    parity build (60 modules, five documented absences). Fix in the runtimes repo;
    nothing in rexenv depends on it, but a release page is a claim surface.

  - [ ] **PCRE JIT is compiled OUT of 7.4** — PHP 7.4 bundles PCRE2 10.35 (May
    2020), too old for Apple Silicon JIT, so Composer died on `Allocation of JIT
    memory failed`. `--without-pcre-jit` removes the capability. Regex throughput
    on 7.4 is therefore lower than on the 8.x rows. Worth revisiting ONLY if
    someone builds 7.4 against a newer external PCRE2; not worth it for an EOL
    version nobody runs for speed.

## Ledger-driven proof backlog

The test metric is `docs/CLAIM-LEDGER.md`. **Do not copy the tally here** — this line
carried 29 Jul's numbers (123/38/37/5 of 203) until 13 Aug, when the ledger's own
generated line read 213/43/40/5 of 301: a second copy of a number that is generated
in one place drifts, and a stale one reads as progress that did not happen. Run
`scripts/ledger-tally.sh`. The backlog = every 🔨
row + the noted half of every ◐ row, worked by the ledger's blast-radius tiers, top
first:

- [x] **Tier-1 group B — the four tunnel-lifecycle L0s** ✓ 13 Aug 2026: #26 (claim
  revoked by the stop, against real SQLite), #29 (the reaper skips a shared site and
  can never reach `stop_for_domain` — guarded where the invariant became load-bearing,
  not where it was written), #30 (`should_signal_row`: never re-signal a reaped or
  sentinel pid), #31 (both readers settle dead children before reading). All
  plant-proven; live legs ride group A's one tunnel example.
  **Also fixed here: the stale cluster summary** that said #103 had never been proven
  live. It had — `dotfile_guard_check` covers nginx over the wire; Apache and
  FrankenPHP are what remain. A stale index over accurate rows reads as a decision
  rather than a gap, and it sent the next piece of work at something already built.
- [x] **Tier-1 group C — four independent L0s** ✓ 13 Aug 2026: #49 (the cancelled
  takeover DRIVEN, not asserted — plus a second test for the ordering the first one
  provably could not see), #54 (the cli crate's one dependency), #59 (what the DNS
  agent can reach, scope stated), #37 (a tunnel target can only be a site row).
  #54 and #59 fail with the DESIGN rather than a mismatch: both go false through one
  line added by someone reading a diff.
- [ ] **Tunnel-replay posture — RULED 14 Aug 2026; one leg still owed.** (ledger #307/#33/#308.)
  ✓ (1) What denies is identified: the **CF-header** gate fires first. The Host gate is
  reached only with that gate removed, and only while `rexenv-tunnel.php` is present —
  it is a second expression of the same Cloudflare fact, not an independent layer, and
  the include-time→`init` ordering that makes it work is WordPress's boot order, not
  filename sort (#308, guarded).
  ✓ (2) Posture ruled: two Cloudflare behaviours must BOTH change (CF sends its header
  set; CF appends the connecting IP to `X-Forwarded-For`) where one used to. Higher cost
  of failure, same shape. Not independence — the independent mark (cloudflared on its own
  loopback port, stamped by nginx) is recorded in #307 **with the objection attached**:
  a tunnel adopted from an older running version arrives unmarked, so it fails open
  across exactly one upgrade.
  ✓ The client-IP gate now reads the LAST hop, not the first (#307) — proven by
  `wp_login_client_ip_check` (sandbox tier, 13 shapes, self-defending matrix,
  plant-proved 3×). **Scope corrected after measuring:** the hole was reachable through
  a tunnel and ONLY through a tunnel — the edge replaces a caller-supplied
  `X-Forwarded-For` with its own peer, so the LAN exposure an earlier note claimed here
  never existed (leg E refuted it).
  ✓ `wp_login.rs`'s module doc rewritten to what is known, hedged where it is inference.
  ☐ (3) The leg. Two pieces: a **network-tier** leg in `wp_login_check` asserting our own
  edge replaces caller-supplied XFF — leg (E) ✓ RUN 14 Aug 2026 (with a control header, so a dropped request cannot read as a dropped header), and the
  end-to-end "replay denied through a real tunnel" observation on the next live
  `tunnel_exposure_check` run. Do NOT reinstate a leg that passes with the CF gate
  removed — that is what was cut, twice.
- [x] **Nine examples passed the MySQL basedir where `install_for_site` wants the client
  binary — FIXED 14 Aug 2026, all nine now use `database::mysql_client_bin`. Red since
  15 Jul 2026, invisible because of their tier.** ✓ 7 of 9 now pass end-to-end
  (`adminer_deeplink_check`, `blueprint_check`, `multisite_check`,
  `multisite_wildcard_check`, `network_check`, `wp_themes_check`, `wp_tools_check`); the
  two that still fail do so for their OWN reasons, listed as separate items below —
  **the second failure behind the first was real**. `b5861c4` renamed
  `mysql_basedir` → `db_client` and changed its MEANING (extracted tree → client binary).
  Both are `&Path`, so nothing failed to compile. That commit did touch these files, but
  only to add the unrelated `db_engine` field, so the wrong argument rode along.
  `create_database` execs the client unconditionally (`CREATE DATABASE IF NOT EXISTS` is
  SQL-level idempotence, not a skipped exec), and exec'ing a directory is EACCES before
  any DB contact — measured 14 Aug 2026 — so **no leftover database can make these pass
  on any machine**. They are simply red, and all are network/stack tier, which is not
  the tier that runs routinely.
  Affected: `adminer_deeplink_check`, `blueprint_check`, `multisite_check`,
  `multisite_wildcard_check`, `network_check`, `wp_create_serve`, `wp_plugins_check`,
  `wp_themes_check`, `wp_tools_check`. Already correct: `cli_wp_install_check`,
  `mariadb_site_check`, `wp_install_stream_check`, `wp_login_check` (fixed 14 Aug).
  ⚠ **The codebase already knew** — `wp_install_stream_check` carries "db_client = the
  CLIENT BINARY … not the base dir — wp_plugins_check passes the base and is latently
  stale". Someone hit it, fixed their own caller, named a second victim, and the note sat
  there. Fourth instance of the codebase-knew-already shape.
  Production is NOT reachable: every real caller derives the client through
  `DbEngine::sql_client_bins` (`site_provision.rs:1130/1386`, `dbrestore.rs:101`), the one
  place that knows the layout. Only examples hand-roll it.
  Work: (a) the nine one-line fixes ✓; (b) make `mysql_exec` refuse a directory with a
  message that NAMES the argument — **subsumed by (c), 15 Aug 2026**: a directory can no
  longer reach `mysql_exec`, because its argument can no longer be built from a path;
  (c) ✓ **DONE 15 Aug 2026, ledger #329** — `SqlClient` in `core/db.rs`, constructible
  only by `sql_client_bins`/`cached_sql_client` (plus a `#[cfg(test)]` door), private
  field, raw path helpers demoted to `pub(crate)`. Every client-taking signature in
  database/dbrestore/dbmirror/dbdump/confverify/wordpress takes `&SqlClient`; ~30
  example call sites converted to the constructors. **The migration found TWO more
  victims the 14 Aug sweep missed** (neither called `mysql_client_bin`, so the grep
  never saw them): `site_resources_check` passed a hand-built `bin_dir/mysql-<v>` TREE
  to `db_sizes`, and `db_drop_check` passed `&basedir` to `create_database`/
  `drop_database` — both latently red at the same tier that hid the first nine, both
  surfaced as compile errors the moment the type existed. That is the class argument in
  one sentence: the sweep fixed nine instances; the type found eleven.

- [ ] **`wp_plugins_check` failed its deactivate assertion once and has not reproduced —
  the product-bug flag raised 14 Aug 2026 is RETRACTED, mechanism refuted.** The suspicion
  was that `plugin_verb` returns WP-CLI's stdout as a String and so reports success from
  output rather than status. Measured instead of assumed, and it is wrong twice over:
  `wp_run`/`wp_run_timed` both test `status.success()` and turn a non-zero exit into an
  `Error`, and the instrumented run showed deactivate doing exactly what it claims —
  stdout `"Plugin 'hello-dolly' deactivated. Success: Deactivated 1 of 1 plugins."`, with
  the very next raw `wp plugin list` reporting `hello-dolly,inactive`.
  Nor is the shape elsewhere: `item_verb` is shared by plugin activate/deactivate/update
  and theme update/delete and every one goes through the checked path; the unchecked
  `wp_cli` is used only for boolean probes (`core is-installed`, `plugin is-active`,
  `config get MULTISITE`, `maintenance-mode is-active`, `language core is-installed`,
  `verify-checksums`) where a non-zero exit IS the answer. **There is no "we ignore exit
  status across the wp surface" problem to scope.**
  What remains is one unexplained failure. It happened in the isolated re-run immediately
  after the corpse-mysqld bulk run; it has passed 3× since the leftover docroots were
  removed. The tempting story — leftover plugin state — does NOT fit: in the bulk run this
  example died at `install_for_site` with errno 2, before any plugin work. **So the cause
  is unknown, and this is recorded as an unexplained assertion failure rather than a
  flake, because calling it a flake is a story too.** Worth catching if it recurs.
  One genuine measurement kept: `wp plugin deactivate` on an ALREADY-inactive plugin exits
  0 with `Success: Plugin already deactivated.` on stdout and `Warning: Plugin 'x' isn't
  active.` on stderr — so exit-zero-with-a-warning is real on this surface, it just isn't
  what bit here.
  **15 Aug 2026 — a recurrence now captures itself.** The original sighting produced no
  evidence because the assert printed only "not deactivated" and the panic then leaked
  mysqld into the next run. The example now dumps deactivate's own stdout, the parsed
  list, and a raw `wp plugin list` re-read (the raw read separates "parsed list stale"
  from "really still active") before panicking, and mysqld is Drop-owned
  (`common::OwnedService`) so the panic cannot manufacture the corpse-mysqld condition
  the first sighting was tangled with. Nothing new was ruled in or out — still filed
  as unexplained.
- [x] **`common::sandbox` roots are long enough to break Caddy's admin unix socket
  (macOS `sun_path` = 104 bytes). RULED 15 Aug 2026 — the current containment IS the
  fix.** Shape (1) (short roots like `/tmp/rx-<hex>`) is REJECTED: moving fixture
  roots out of the OS temp dir trades a real invariant every example depends on for
  a rarer failure. What stands: production refuses at the point of use
  (`core::proxy::admin_socket_path`, where whether an admin socket is even asked for
  is known) and `common::sandbox` WARNS with the byte arithmetic at the point the
  length is chosen (a refusal there was tried and blocked a working check — the
  sandboxed edge in `tunnel_exposure_check` runs with admin off). The class is
  contained where it can bite. Original diagnosis kept below. `wp_create_serve`'s edge never bound, and caddy's
  own first line said why once the example was made to print it before panicking:
  `starting caddy administration endpoint: listen unix //var/folders/51/…/T/
  rexenv-sandbox-wp_create_serve-4823/config/caddy-admin.sock: bind: invalid argument`.
  That path is **108 bytes against a 104-byte limit** — over by four. Nothing to do with
  readiness, ports, or certs.
  **This is not specific to one example.** It is a function of the sandbox tag's length:
  `/var/folders/<2>/<27>/T/rexenv-sandbox-<tag>-<pid>/config/caddy-admin.sock`. Shorter
  tags fit and longer ones don't, so any sandboxed example that starts the edge passes or
  fails on how it was NAMED. `wp_login_check` is unaffected only because it does not
  sandbox its paths.
  ❓ **NEEDS A RULING — three shapes, none obviously right.** (1) Shorten the sandbox root
  (e.g. `/tmp/rx-<8 hex>`), which fixes every example at once but moves fixtures out of
  the OS temp dir the invariant currently names. (2) Put the admin socket somewhere short
  regardless of app-data root, which touches production path logic for a test-only
  problem. (3) Cap the tag length in `common::sandbox` and refuse a tag that would
  overflow, which keeps the failure in the fixture layer and makes it loud — but leaves
  the underlying limit live for any real app-data path a user could choose.
  I lean (3) plus a refusal message naming the byte count, because it fails at the place
  the length is chosen; but (1) is the only one that makes the class go away.
  <details><summary>the symptom it was mistaken for</summary> Was
  diagnosed 14 Aug 2026 as a missing readiness wait; the wait is now IN (a 10s
  `ports::is_listening` gate, like every sibling) and it turned the symptom from a
  ConnectionRefused deep in reqwest into `edge never bound :8443 within 10s (caddy pid
  N)`. The wait was necessary but was not the bug: caddy spawns and never listens.
  Note the discriminator — the same edge on the same port comes up fine in
  `wp_login_check`, which does NOT sandbox its paths. So the suspicion is something the
  edge needs that `common::sandbox` relocates (CA/cert paths, config dir, admin socket).
  **Did not block the tunnel session**, whose fixture is `wp_login_check`.</details>
- [x] **`wp_create_serve` leaked its whole stack on the panic path — FIXED 14 Aug 2026,
  and proved by its own real failure rather than a plant.** Its four services are now
  owned by `common::OwnedService`, whose `Drop` runs while unwinding. Before: the panic
  left mysqld, nginx and php-fpm running. After, on the SAME panic path: zero marked
  processes and 18088/9783/13306/8443/8080 all free. `OwnedService` deliberately does NOT
  sweep its port the way `Reaped` does — that sweep decides ownership by program name,
  which is safe on a fixture port and would let a sweep of 9783 kill the user's own
  php-fpm. Ownership here is the `Child` handle. The residual is documented on the type:
  a master that ignores SIGTERM can still orphan workers, which is why it is paired with
  `require_ports_free`. Original finding kept below for the shape.
  <details><summary>what it looked like</summary> After the panic,
  `mysqld`, `nginx` and `php-fpm` were still running, all carrying its
  `rexenv-sandbox-wp_create_serve-<pid>` marker, and its sandbox datadir had been removed
  on drop — leaving a mysqld serving a datadir that no longer exists. Killing the masters
  left `nginx: worker process` and `php-fpm: pool www` holding 18088/9783 with `ppid=1`
  and no marker (the orphan-worker shape).</details>
- [x] **These examples didn't refuse a busy port, so they borrowed a broken server —
  FIXED 14 Aug 2026.** `common::require_ports_free` is now the first statement in
  `wp_create_serve`, `wp_plugins_check`, `wp_themes_check` and `wp_tools_check` (and in
  `wp_login_check`, earlier today).</details><details><summary>what it looked like</summary> The
  leaked mysqld above made `wp_plugins_check`/`wp_themes_check`/`wp_tools_check` fail with
  `ERROR 3680: Failed to create schema directory (errno 2)` — a message that names nothing
  useful.</details>
- [ ] **Running provisioning examples in bulk writes into the user's REAL Sites folder.**
  The nine added 8 directories to `~/rexenv/Sites` (`sites::provision` reads the
  `sites_dir` SETTING, which a sandboxed `Platform` cannot redirect). Known hazard, hit
  again by running nine in a row. Until the pin is structural, snapshot the folder before
  any bulk example run so the delta is attributable.

- [ ] ❓ **Does `tunnels::stop` routinely need SIGTERM?** Observed once, 14 Aug 2026, on
  the first run of `common::adopt_public_tunnel`: cloudflared was still alive 3s after
  `tunnels::stop`, and the guard's escalation stopped it. **One sighting is not a
  finding** — it may be a defect in the stop path or simply a graceful shutdown slower
  than a 3s window, and convicting `tunnels::stop` on a single observation would be the
  attribution-by-elimination move this file keeps refusing. What makes it worth queuing:
  if it IS routine, production's own stop path has the same gap and nothing there
  escalates. Not urgent — the app's exit hook and the crash sweep both cover a survivor —
  so the plan is to watch the guard's output over the next few tunnel runs and open it
  properly only if it recurs.
  **15 Aug 2026 — the watch now has a record to accumulate into.** Checked for
  accumulated evidence first: there was none and there COULD have been none — a
  successful `live-checks.sh` run deletes its log directory, so the guard's stderr only
  persisted if a human was watching (the one kept failure dir from 14 Aug has no
  escalation lines). Two changes in `common::reap_public_tunnel`: every reap now appends
  an outcome line (`stop` / `stop_slow_no_signal` / `stop_no_effect_10s` / `sigterm` /
  `sigkill` / `survived_all`, with elapsed ms) to
  `src-tauri/target/tunnel-stop-evidence.log`, fast path included so the base rate
  accumulates; and the anomalous path watches 3s→10s BEFORE signalling, because a death
  right after SIGTERM is indistinguishable from the earlier stop still finishing — the
  immediate escalation was destroying exactly the evidence this question needs.
  **And the recorder's first run impeached the sighting's own instrument**: it logged
  `tunnel_guard_check`'s sleep stand-in as SURVIVING SIGKILL, which is impossible — the
  guard's `pid_alive` was `kill -0`, which answers "alive" for a macOS ZOMBIE, and the
  tunnel pid is the example's own child that nothing `wait()`s, so every dead cloudflared
  is a zombie until the example exits. `pid_alive` now reads `ps -o state=` and counts
  `Z` as dead (production's `process_running` semantics; it already did this, so
  `tunnels::stop` itself was never fooled). The 14 Aug sighting sits on the broken
  measurement and is DOWNGRADED to suspect — not explained away: its "stopped after
  SIGTERM (verified dead)" step doesn't fit a pure-zombie story, since a zombie ignores
  SIGTERM too. Watch the evidence file; convict on clean measurements only.
  **First REAL-cloudflared datapoints, 15 Aug 2026** (`tunnel_delete_order_check`):
  production's `stop_for_domain` killed a live registered cloudflared INSIDE a 527ms
  delete, and the guard's own `tunnels::stop` killed a deliberately-leaked live one
  in **2ms** (evidence line `pid=39638 outcome=stop elapsed_ms=2`). Two clean
  measurements, both instant — the weight now leans hard toward the 14 Aug sighting
  having been the zombie artifact.

- [x] Tier-1 cluster — WORKED DRY 15 Aug 2026. Every automatable item is closed
  (the strikethrough history moved to the rows themselves; this line stops
  restating them — the 13 Aug and 15 Aug stale-cluster incidents both happened
  in exactly this list). What remains is not backlog:
  - **#2 is a WATCH** on Cloudflare's header set — not automatable, goes false
    silently, re-observed on every live `tunnel_exposure_check` run.
  - **#25/#26/#29/#30/#31's live legs ride the next `tunnel_exposure_check`
    run** (group A's one tunnel example), alongside the tunnel-replay leg (3)'s
    end-to-end denial observation.
  Everything else in the old list (#10/#13, #33, #37, #49, #54/#59, #103,
  #104/#191, #116, #190, #242, the manifest sweep) is ✅/◐-watch in the ledger —
  run `grep '^| <n> '` there, don't trust a list here.
- [x] Live re-point (#242 L1) ✓ 15 Aug 2026 — `linked_site_check`'s re-point leg:
  real rename, a 404 control proving the reload is load-bearing, the command's
  core sequence, and a post-move-only marker served through the vhost.
- [ ] Then: ~~Apache/FrankenPHP dotfile legs (#103)~~ (closed 15 Aug 2026 — all three backends live, plant-proven per template), ~~fpm candidate
  isolation (#104/#191)~~ (closed 15 Aug 2026, `fpm_candidate_check`, plant-proven), ~~manifest HEAD+digest sweep~~ (closed 15 Aug 2026, `manifest_sweep_check` #335 — 88 URLs answer, 78 re-hashed incl. every Intel digest), Bedrock live provision (#35),
  sandbox-adoption cohorts + `wp_fixture()` — incl. scoping
  `download_progress_check`'s bin-cache delete off the REAL shared cache
  (surface-coverage finding 29 Jul: the sandbox invariant is structural for only
  ~20 of 109 examples, and this one deletes a real content-addressed entry),
  ~~`webview_dialogs` L2 (#166)~~ (closed 15 Aug 2026 — the L2 shape was measured
  impossible before building, which was the queued instruction; legs landed at
  L0+L1, eye-half in SMOKE; `docs/PLAN-webview-dialog-proofs.md`),
  ~~import-graph lint (#163)~~ (closed 15 Aug 2026, plant-proven), ~~rusqlite-outside-state
  guard (#167)~~ (closed 15 Aug 2026 as the SQL-string scan the defect-families
  note demanded; its first run found the claim already false — feed.rs owns
  `agent_actions` SQL — and a live violation in `commands/mcp.rs`, both handled).

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
  **15 Aug 2026 — a third occurrence WAS captured, with a name and a mechanism.**
  `core::dns::tests::port_bound_true_when_held_false_when_free` failed a full
  `verify.sh` run ("a released UDP port should read as free") and passed standalone —
  and reading it explains itself: it dropped an ephemeral UDP socket and asserted the
  port reads free in a single shot, but nothing stops a parallel test or any process
  on the machine re-binding that exact port in the gap. Fixed by retrying across
  fresh sockets (the subject is `port_bound`'s answer for a known state, not this
  process's ability to reserve a port against the OS). Whether the un-named 3 Aug
  lib-test transient was this same test is NOT claimable — its name was never
  captured — but the shape fits, and this instance is closed. `apache_site_check`'s
  transient remains unexplained.

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
  ✓ **DONE 13 Aug 2026** — the release shipped, so the reason for waiting expired.
  `scripts/verify-receipt.sh` (the one definition, shared by verify.sh and the
  hook) + `scripts/git-hooks/pre-commit`, installed with
  `git config core.hooksPath scripts/git-hooks`. Verified on every path: fresh
  receipt passes, an edit after verify blocks, a docs-only commit passes with a
  deliberately stale receipt (control: adding one code file to the same commit
  blocks), and mid-merge / mid-rebase / mid-cherry-pick all skip. **Two design
  faults found by testing rather than by review**: the fingerprint first mixed in
  `git status --porcelain`, so `git add` invalidated it and EVERY commit was
  blocked; and hashing tracked and untracked files as two streams meant staging
  reordered the input. Both are why it hashes a SORTED SET of file contents now.
  `verify.sh` also refuses to write a receipt when the tree changed while it was
  running, which is what closes the verify→edit→commit window.

## Release gates (human, scripted — see the docs named)

- [x] **PUBLISH-TESTING §A** — Apple-Silicon ad-hoc launch test. ✓ **PASSED 12 Aug 2026**
  on `b29f21f7…`, the dmg actually published as v0.1.0 (quarantined → Gatekeeper
  blocked → `xattr -rd` → launched); §A0 passed on the same artefact. Re-run it on
  every future release candidate — the pass belongs to the artefact, not the app.
  ⚠ **Build it with `npm run release:mac`** (= `tauri build --target
  universal-apple-darwin`) — NOT a bare `tauri build`, which produces a thin
  arm64 `rexenv_<v>_aarch64.dmg` that an Intel user cannot run, while INSTALL.md,
  this document and the cask in `rexenv/homebrew-tap` all promise a universal
  `rexenv_<v>_universal.dmg`. Built wrong once on 3 Aug by reaching for the
  generic command; naming the COMMAND here rather than the outcome is the fix.
  Verify before gating: `lipo -archs <app>/Contents/MacOS/rexenv` and the same
  for `MacOS/rex` must both report `x86_64 arm64`.
- [ ] **PUBLISH-TESTING §B** — uninstall removes the root :443 daemon (live launchd).
- [x] **`rex` hangs forever on a half-alive app** — ✓ **FIXED 12 Aug 2026** (ledger
  #300): `soft_request` is bounded at 2s (it promised "works WITHOUT the app" and
  delivered it only for ENOENT/ECONNREFUSED), and `request` — which must stay
  unbounded, since a finished reply arrives in one write at the end — now prints one
  stall notice after 10s instead of leaving a dead terminal. Five tests, the deaf-peer
  one proven to fail on the pre-fix code, and `verify.sh` now runs the `cli` crate at
  all. **Still open, deliberately:** why that instance went deaf was never diagnosed —
  the evidence died with the pid. Reproduce before blaming App Translocation.
  **Shipped in v0.1.1** (13 Aug 2026, `14b64dae…`) — v0.1.0 was deliberately not
  re-cut (the trigger needs a half-alive app; a re-cut costs §A0 + §A again). A user
  still on v0.1.0 whose app goes deaf sees `rex` hang with no output; the answer is
  `brew upgrade --cask rexenv`.
  Original report: found running §D, 12 Aug 2026. An
  App-Translocated instance from §A owned `config/rexenv-cli.sock`, accepted the
  connection and never replied; `rex --version` and `rex status` sat in `recvfrom`
  with no output and no timeout, and only completed when that pid was killed. The
  no-read-timeout is deliberate in `request()` (a `site create` runs for minutes) but
  `soft_request()` inherits it while promising the opposite — `cli/src/main.rs:255-266`,
  "must work WITHOUT the app". Fix is a read timeout on `soft_request` at least;
  whether `request()` deserves a *connect-and-first-byte* deadline (distinct from the
  long-running body) is the real design question. Not yet diagnosed: WHY that instance
  stopped answering — the evidence died with the pid, so reproduce it before assuming
  translocation was the cause rather than a wedged app.
- [ ] **PUBLISH-TESTING §D** — full tap install dry-run. **Half done 12 Aug 2026**:
  v0.1.0 is published on `rexenv/homebrew-tap` (private repos 404 `brew`'s anonymous
  fetch, so the artefact ships from the tap — `docs/RELEASING.md`, interim section),
  the cask is bumped to the shipped `b29f21f7…`, the asset fetches anonymously (200),
  and `brew fetch --cask rexenv` verifies ✔︎. **Install half ✅ ran the same day**:
  installs, postflight de-quarantines (`No such xattr`), `rex` links to
  `/opt/homebrew/bin/rex`, app launches, `rex --version`/`status` answer, DNS agent
  plist repointed at `/Applications`. **Left: `--zap` only**, and NOT on this Mac —
  it trashes 17 GB of live app data behind real `.rex` sites. Clean Mac only
  (`docs/SMOKE-TEST.md`).
  Two teeth grown from the first real run: the cask hash bump now compares sha256
  as well as version (a placeholder hash under an unchanged version silently
  skipped), and `brew trust rexenv/tap` is a required user-facing install step.
- [x] **0.2.0 released** ✓ 16 Aug 2026. `rexenv_0.2.0_universal.dmg` `bd019d8d…`
  from `3755966` (tag `v0.2.0`, local — origin stays tagless while the repo is
  private). `verify-full: all green` → §A0 → §A → draft → publish → cask bumped by
  `update-cask.yml` (`449b576`). The published asset was re-checked ANONYMOUSLY and
  three-way-matched against the cask's pin and the bytes §A was run on. Two things
  this release cost that are written up rather than remembered: the licence texts
  now ship beside the PHP we build (#336), and §A's first run was VOID because the
  dev login already trusted the app — the script needed `/Applications/rexenv.app`
  removed and a browser download to be capable of failing at all.
- [ ] **Flip the release host back when `rexenv/rexenv` goes public** — three things
  in ONE commit, or the tap's guard fails the bump: the cask's `url`, its `verified:`,
  and `SOURCE_REPO` in `update-cask.yml` (all in `rexenv/homebrew-tap`). Then CI's
  `release.yml` resumes owning the build, and `docs/RELEASING.md`'s interim section
  is deleted rather than left as a second, wrong set of instructions.
- [ ] **PUBLISH-TESTING §K** — the whole migration as ONE journey (rebuild first).
- [ ] **PUBLISH-TESTING §F** — resolver takeover/hand-back/drift: clean-VM only.
- [ ] **PUBLISH-TESTING §G** — `/import` screen packaged GUI pass (only ever
  type-checked; Stage 1's status line now says so). **Grew step 9 on 8 Aug**: the
  batch progress card (`valet-import://progress`) — its arithmetic is L0-proven, but
  that `detail` really is the child job's own label and that the bar FREEZES rather
  than rolls back on a mid-batch failure are 🔨 L2 (ledger #243) and only this pass
  covers them today.
- [ ] **Plugin-update progress — the WIRING half** (ledger #249, 9 Aug 2026): the
  tracker is L0-proven against WP core's own strings and the pinned phar (plugins,
  themes and core), but that the emit reaches the WordPress tab and the bar really
  advances during a live WooCommerce/Elementor/core download is unproven at any
  layer. Needs one run against a real site (a plugin held one version back), or an
  L2 case rendering the panel with a scripted event stream.
- [x] **The update-claim rule now has a test** ✓ 13 Aug 2026 — `wk-checks/wpverdict.js`
  (ledger #250). Two fixture rows in `DevGitPanel`'s `plugins=list`: one claiming an
  update to its OWN version (must offer nothing) and one claiming `1.1.11` over
  `1.1.3.8` (must offer, since a string compare gets it backwards). Plant-proven both
  ways; a third ordinary row keeps the two from passing on a panel where nothing ever
  offers. **The plan's route was wrong and is corrected here**: `uireview.js` does NOT
  drive the plugin list — `DevGitPanel` + `wptoast.js` do.
  - [ ] **remaining: the TIMING half** (cancel-then-settle beats an in-flight check)
    needs a real site — same run as the #249 wiring pass above.
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
- [ ] ⚠ **The macOS floor is a claim about BOTH slices and half of it has never
  been measured.** PORTS.md's `minos` table is measured from this machine's
  binary cache, which only ever downloads the host arch — so every number in it
  is an **arm64** number, and `minimumSystemVersion: 15.0` is asserted for x86_64
  on the assumption that upstream builds both slices to the same deployment
  target. Nothing checks that. **This is the arm64-DMG mistake's shape**: a
  universal artifact whose two halves differ, working perfectly on the machine
  that made it and wrong for everyone on the other chip — except the failure
  here is worse than a thin binary, because it is invisible until an Intel user
  on macOS 15 finds their web server will not start. (The 15 Aug re-measure also
  showed the table can simply be WRONG where nothing depends on it: PHP 8.0.30
  is 14.0 and had been recorded as 12.0 since the table was written.)
  **What it would take, cheapest first:**
  1. **No Intel Mac needed for the measurement** — `minos` is metadata. Fetch the
     x86_64 artifact for every pin and read `otool -l`/`vtool -show` on it. The
     download URLs are already enumerated by `manifest_sweep_check`, which walks
     BOTH arches; a `minos` column is a few lines in an example that already
     fetches these bytes, and the sweep is the natural home because it is the one
     check that already refuses to be arch-blind.
  2. Assert the *derived* rule rather than the numbers: `max(minos)` over the
     default-stack binaries, per arch, must equal `tauri.conf.json`'s
     `minimumSystemVersion`. That fails on a pin bump that raises a floor, which
     is the event the current table can only be updated by hand for.
  3. Only then does an actual Intel machine matter, and for the OTHER half — the
     run-verify above, which metadata cannot stand in for.
- [ ] **In-app verifies owed** (CLI passthroughs whose service-touching half the
  example harness guard-blocks; fold into the next deep test): `php
  install/uninstall`, `php settings set`, `db versions --set`, `site
  server/domain/move`, `mail clear`, `tunnel start`, `wp core update/switch` —
  plus the packaged-GUI walks still noted inside their shipped entries: MariaDB
  site from the dialog, Apache site in-app, DB version switch from the Databases
  row, Settings CLI-install card, ref-picker on a real many-branch repo, wp.org
  chips + streamed installs, New Site streamed provisioning card, **Upload zip
  through the real native file dialog** (SMOKE §WordPress Manager — L0 proves
  the gate, L1 the install, L2 the card; nothing can drive the picker).
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
- [x] **B33 — php-debug download host — DECIDED + BUILT 14 Aug 2026: GitHub
  Releases in the public `rexenv/runtimes` repo; `dl.rexenv.dev` is not used.**
  ✓ Repo created with the build workflow, gates, licence collection and the
  immutability contract (`docs/PLAN-php-74-support.md` §6). What decided it: a
  release there is immutable and its tag is never reused, so a pinned URL can 404
  but can never resolve to different bytes — the property neither static-php.dev
  nor FrankenPHP offers, and the reason `core/binaries.rs` carries two comments
  about pins going stale. The distributor obligation is met in the repo (PHP
  licence shipped; dep licences collected from the sources the build actually
  downloaded, not a checked-in list that would describe last year's extension set).
  **The prize is smaller than this row implied**, and that correction matters more
  than the decision: `docs/PORTS.md` records Xdebug already shipping for PHP
  8.1–8.5 via ghcr bottles, so B33 unblocks Xdebug on **8.0 only** —
  `docs/xdebug-debug-build.md` said otherwise and has been corrected in place.
- [ ] **`rex config get|set`** — parked on which settings keys to allow-list
  (never the whole KV table).
- [x] **Neutralise the WP-CLI packages-dir inheritance, or accept it in writing?**
  — **DECIDED 5 Aug 2026: (b) NEUTRALISE WITH A TELL. LANDED 13 Aug 2026** (the pin
  + its L0 scan; the tell is ledger #301, the L1 leg still open). Queued as the
  FIRST thing after v0.1.0 shipped; deliberately not on the release artefact,
  because it is a behaviour change and wanted its own verification rather than
  riding a build that was gated before it existed. **What the work actually turned
  on**: the scope of (a) below was wrong — it named three spawn sites and there
  were seven, so the fix was to stop enumerating them (see the item above).
  **The reasoning, recorded so it is not re-derived**: neutralising alone would hand
  someone who genuinely relies on a global package a bare `not a registered wp
  command` with no explanation — the same unreproducibility pointed the other way.
  Accept-and-document would make the ledger honest and leave every future bug report
  just as unexplainable. The TELL is what makes it a fix rather than a trade: pin the
  command set, and **when a packages dir exists that WOULD have contributed, say so**,
  so a user learns what changed and why instead of discovering that a capability
  vanished.
  **Scope when picked up:** (a) set `WP_CLI_PACKAGES_DIR` at every wp-cli spawn
  (`core/wordpress.rs:15/83/149` + any later one); (b) the L0 scan that must cover ALL
  of them or repeat the coverage/surface family; (c) detect the would-have-contributed
  case and surface it once, not per call; (d) **the tell's wording comes for approval
  before it lands**, and joins a must-say list afterwards — the same route the MCP card
  copy and the Build-zip tooltip took.

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
    ✓ **Both "open by tier" legs resolved 13 Aug 2026 — and one of them was not
    a tier problem at all.**
    - The PHP DOWNLOAD window: **NOT APPLICABLE, record corrected** (ledger #224).
      Writing the example first would have proved the download happens and read
      as proving the guard. `switch_php_version` writes the row BEFORE it
      prefetches, so a Keep during the download lands on the far side of a check
      that already ran and a write that already committed. The comment and the
      row both claimed otherwise — #228's old module doc again: true of downloads
      generally, false of this path. Accepted rather than reordered: the write
      was authorised when it happened, and moving the prefetch on a shipped path
      shared by the UI and CLI buys a window with no real consequence.
    - The mail MATCH over real messages: **PROVEN** — `mcp_mail_check`,
      **`service` tier, not `stack`** as this note said. It brings its OWN
      Mailpit under sandboxed paths, because borrowing the user's running one
      means planting messages in their real store to prove we can tell their mail
      from a scratch site's — a test that contradicts the thing it tests. Own
      Mailpit means the fixed ports, which is what `service` means. Five plants,
      including the two that matter: `ends_with(domain)` returns the near-miss,
      and NOT sending the near-miss fails as FIXTURE BROKEN rather than as a
      passing filter. It also refuses to run beside a live Mailpit — found by
      running it with the stack up, where `mail::start` cannot bind, exits, and
      `mail::running()` sees the USER'S catcher on the fixed port.
    **M3** — DB, its own session (T1 consent dialog).

## Parked (deliberate — needs explicit go; don't pick up silently)

- [x] **In-app PHP patch updates — BUILT 17–18 Aug 2026, dark until the key
  ceremony.** Reverses the 16 Aug decision at the user's direction: the update
  button is 0.3.0's purpose. The custody objection that blocked it dissolved on a
  fact neither reading had noticed — the app holds only the PUBLIC key, so the
  app-side code is identical whether the private half is a CI secret or a hardware
  token, and custody can improve later for the price of a key rotation.
  Ledger #348 (verify + serial + four structural limits), #349 (`selected_patch`,
  pin as floor, the pool snapshot seam), #350 (the apply flow with revert, the
  button, the key ceremony). **Remaining, and it is an OPERATOR task, not a code
  one:** run `scripts/gen-release-key.sh`, put the private half in the CI secret
  FIRST, then pin `RELEASE_PUBKEY` and commit, then publish a signed manifest
  before that build reaches anyone. Until then `enabled()` is false, `updatable`
  is always None and the button does not render — which is the correct dark state,
  not a bug.

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


