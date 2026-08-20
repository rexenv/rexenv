# SHIPPED — the August 2026 evidence log (ARCHIVED)

> **Historical.** These are the `[x]` rows that were finished between 1 and 21
> August 2026, moved out of `docs/TODO.md` on 21 Aug 2026 when that file was
> reconciled against the code again. Every entry is kept VERBATIM with its ✓
> evidence note; the headings are the TODO sections they were finished under.
>
> Open follow-ups that were recorded INSIDE these entries did not come here —
> they were promoted to open rows in `docs/TODO.md` first, because an entry that
> is 90% shipped is still the wrong place to keep the other 10%. Like everything
> in `docs/archive/`, this file may contradict current code: it records what was
> true when it was written.


## Finished under “Now — actionable code/test work”

- [x] **The edge had never started in ANY sandboxed example** ✓ 20 Aug 2026.
  Caddy's admin socket lives at `<sandbox root>/config/caddy-admin.sock`, macOS binds at
  most 103 bytes of socket path, and `std::env::temp_dir()` on macOS is a 49-character
  per-user TMPDIR — so every sandboxed socket path came to **110–115 bytes** and caddy
  died on `bind: invalid argument` before it ever listened.
  **`common::sandbox`'s own comment named the wrong cause**, which is why it survived: it
  blamed the TAG ("whether a sandboxed example can start the edge depends on how it was
  NAMED… shorten the tag"), and shortening `create_site_serve` to `createsrv` — 8
  characters off, exactly what the warning asked for — still failed. The base was always
  the larger half. Root moved to `/private/tmp` (12 chars): the same paths are now 75–79.
  **It was invisible because nothing asserted it.** These examples printed
  `READY https://…:8443` and slept for a human to curl; a reader who never curled saw a
  green run. The readiness gates added the same day are what turned it loud — the first
  thing they did was fail on a service that had been dead for weeks.
  Tradeoff stated in the code: `/private/tmp` is world-writable where TMPDIR is per-user
  0700. Still pid-scoped, still `SandboxGuard`-removed, still fixture scaffolding — but
  it is a weaker directory, and that is the price of an edge that starts.

- [x] **Adminer followed the OS theme, not rexenv's** ✓ 20 Aug 2026, ledger #375.
  A light-themed app framing a dark console. Fixed through Adminer's own `css()` hook
  (its return decides whether `dark.css` is media-gated at all), fed by a
  `.rexenv-theme` file the wrapper reads per request — a file rather than a URL
  parameter, because Adminer's own links would have dropped the parameter on the first
  click inside the console. L1 reads the head Adminer really emits for all three states
  plus junk; L0 ties the three filenames together, plant-proven.

- [x] **Re-uploading a zip of an installed plugin dead-ended** ✓ 19 Aug 2026, ledger
  #374, `wpinstallcard.js` (`install=blocked`, plant-proven). wp-cli refuses to unpack
  over an existing folder and says only `Error: No plugins installed.`; wp-admin offers
  "Replace current with uploaded" and rexenv offered nothing. The card now reads the
  folder out of wp-cli's own `Destination folder already exists` line, names it, and
  offers a replace that re-runs the same job with `--force`; `wp_install_stream_check`
  jobs 5/6 ran green — refused in the exact words the card parses, then replaced with
  force. **Follow-up the same day:** the fact moved onto the job STATE (`blockedBy`),
  because the TOAST never sees the log and was still announcing "Install … failed:
  Error: No plugins installed." beside a card offering the fix. Toast, glyph and colour
  now follow the fact; wp-cli's summary line stays verbatim. `force` is never defaulted
  on, and a git-tracked target gets a danger confirm first — an overwrite there takes the
  working tree, the branch and `.git` with it, which is a hazard wp-admin does not have.

- [x] **The install card stayed forever after a successful install** ✓ 19 Aug 2026,
  `wpinstallcard.js` (L2, plant-proven twice). Success now clears itself after 3s;
  failed/partial/cancelled/timed-out stay and gain an × instead, since that card is
  where the reason lives. Opening the log holds the timer — three seconds is exactly
  long enough to click "Show log" and lose it — and a running job offers Cancel, never
  dismiss. **Themes get the same behaviour and are checked separately** (same hook, same
  card — but a shared implementation still needs both call sites wired; dropping the
  props from the themes one alone fails `themes partial` and nothing else).

- [x] **Light-mode: the update badge and its target version were barely readable** ✓
  19 Aug 2026, ledger #373. Reported with a screenshot. Not a bad token — no token:
  `bg-amber-500/15 text-amber-400`, a raw Tailwind hue, which has one value and cannot
  follow the theme. 38 of them were in the tree (all in the WordPress manager), now
  tokens, with `-bright` wherever the colour is text. Two more routes into the same hole
  fell out of the fix and each carried real defects: `text-status-*` (an "Installed"
  label, a log chip, four destructive buttons at 4.14–4.45:1 in light) and inline
  `style={{ color: var(--rex-*) }}` (the site-type and Mail-avatar accents at
  4.27–4.41:1). All repaired; the AA scan now reads all three routes and
  `no_raw_tailwind_hue_reaches_the_ui` bans the palette outright, plant-proven.
  **Dark verified after** (a token swap moves both themes): full `wk-checks` suite green
  + the changed surfaces rendered and read. It surfaced a coverage hole, not a defect —
  the git chips on a plugin row had no fixture in any harness, so `wpgitchip.js` (new,
  both themes, computed styles) now renders them and catches the config-mapping failure
  L0 cannot see.

- [x] **Plugin search matched the slug, not the name on the row** ✓ 19 Aug 2026,
  ledger #372, `wpsearch.js` (L2, plant-proven both directions). Reported with a
  screenshot: "loopback" against a list reading `rexenv loopback DNS` answered "No
  plugins match" — the filter read `name` (the slug, `rexenv-dns`) while every row is
  labelled with `title`. Matches both now.
  **Themes checked in the same pass — not the same bug (that panel has no filter), but
  the same defect one step earlier:** its cards were labelled by SLUG because `WpTheme`
  carried no title, so a filter added there would have had nothing but the slug to
  match. ✓ Closed 19 Aug 2026 in the follow-up commit: `title` through `theme_list` →
  DTO → card, slug kept beside it, `themes-titles` (L2) plant-proven twice, and
  `wp_themes_check` ran green against real wp-cli ("Twenty Twenty" for the slug
  `twentytwenty` — non-empty AND different from the slug, so it cannot pass on a
  fallback). A theme
  FILTER was declined deliberately — 3–5 themes is not a list you search.

- [x] **Search on the Tunnels page** ✓ 19 Aug 2026, ledger #371, L2 plant-proven.
  Name, domain, or the public URL — the last one because this is the only screen that can
  answer "which site is this link?". The work was not the filter: a hidden row here is a
  site the internet can reach, so hidden SHARED sites are counted in amber with the
  consequence spelled out, on the list and on the no-match screen, while the header's
  count and Stop all sharing keep describing the machine rather than the view.
  Four `uireview.js` scenarios at both widths; the fixture runs two live tunnels so the
  plural copy is exercised.

- [x] **`wp_themes_check` was not re-runnable, and leaked a mysqld** ✓ 19 Aug 2026,
  found by running the network tier after #370. Two defects in one example, both of the
  fixture-ownership family: (1) the run rebuilt the DOCROOT but inherited the DATABASE,
  and since the example ends with `twentytwenty` active, the surviving `stylesheet`
  option made a freshly installed theme come back `active` where the first assertion
  demands `inactive` — it had been red since the previous run (14 Aug), and nobody knew;
  (2) mysqld was the raw child rather than a `common::OwnedService`, so that panic left
  it holding :13306 and the next run refused to start — the 14 Aug corpse-mysqld
  incident in the one example never converted. Now drops its own database first (name
  asserted before the DROP) and owns its child across the panic path. **Evidence: run
  twice back to back, green both times, port free after.**
  **Worth noting for whoever writes the next one:** `wp_plugins_check` survives only
  because it happens to delete what it installs. Idempotence there is luck, not design.

- [x] **Premium plugins showed no update badge** ✓ 18 Aug 2026, ledger #370,
  `core/wordpress.rs` (`update_context_arg`, `checked_list`). wp-admin listed BetterDocs
  Pro 3.9.0 → 4.1.0 and rexenv showed nothing: the vendors' updaters register their
  `pre_set_site_transient_update_plugins` filter behind
  `current_user_can( 'manage_options' )`, and a wp-cli run has no user. A second
  `--require` file grants three named capabilities + `WP_ADMIN` to the checked pass and
  to the update itself (the package URL comes from the same filter, so a badge without
  it is a dead button). Measured on the reporting site: **2 of 10 paid plugins reported
  an update before, 5 after** — the rest are ones wp-admin says nothing about either.
  Scoped by a source-scanning guard (plant-proven), and the checked pass falls back to
  the plain list on any non-clock failure.
  `examples/wp_premium_update_check.rs` (network tier) **ran green 19 Aug 2026** with the
  stack stopped, and is plant-proven: dropping the flag fails its second leg by name.
  What stays manual is the INSTALL of a premium update (a real licence) — `docs/SMOKE-TEST.md`.

- [x] **Premium plugins had no icon in the plugin list** ✓ 18 Aug 2026,
  `core/wporg.rs::plugin_icons` + `wporg_icons_check` (L1 network, run green:
  `betterdocs-pro` → `ps.w.org/betterdocs/…`, `wp-security-audit-log-premium` →
  `ps.w.org/wp-security-audit-log/…`, 5135-byte `image/png`). Every paid plugin on a
  real site was a letter tile while wp-admin's update screen showed the vendor's logo:
  rexenv asks wp.org per slug, and `betterdocs-pro`/`elementor-pro` are not in the
  directory. wp-admin reads those icons from the `update_plugins` transient — measured
  unreachable from wp-cli (the STORED transient had no premium row; a fully-loaded
  `wp eval` produced 1 of 9, the rest inject on `is_admin()` only). So the icon is
  derived from the free counterpart's slug, narrowly: `-pro`/`-premium` only, only
  where wp.org itself had nothing, and no counterpart the directory doesn't know.
  **Still open, deliberately:** a paid plugin whose free half is not a suffix away
  (`essential-addons-elementor` → `essential-addons-for-elementor-lite`) keeps its
  letter tile — the fix for that is a real dependency link (`Requires Plugins:`), not
  a fuzzier rule.

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
  - (promoted to `docs/TODO.md` 21 Aug 2026) the `validate_linked_docroot` per-call `list(conn)` cost note
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


## Finished under “Ledger-driven proof backlog”

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


## Finished under “Release gates”

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

- [x] **0.2.0 released** ✓ 16 Aug 2026. `rexenv_0.2.0_universal.dmg` `bd019d8d…`
  from `3755966` (tag `v0.2.0`, local — origin stays tagless while the repo is
  private). `verify-full: all green` → §A0 → §A → draft → publish → cask bumped by
  `update-cask.yml` (`449b576`). The published asset was re-checked ANONYMOUSLY and
  three-way-matched against the cask's pin and the bytes §A was run on. Two things
  this release cost that are written up rather than remembered: the licence texts
  now ship beside the PHP we build (#336), and §A's first run was VOID because the
  dev login already trusted the app — the script needed `/Applications/rexenv.app`
  removed and a browser download to be capable of failing at all.

- [x] **The update-claim rule now has a test** ✓ 13 Aug 2026 — `wk-checks/wpverdict.js`
  (ledger #250). Two fixture rows in `DevGitPanel`'s `plugins=list`: one claiming an
  update to its OWN version (must offer nothing) and one claiming `1.1.11` over
  `1.1.3.8` (must offer, since a string compare gets it backwards). Plant-proven both
  ways; a third ordinary row keeps the two from passing on a panel where nothing ever
  offers. **The plan's route was wrong and is corrected here**: `uireview.js` does NOT
  drive the plugin list — `DevGitPanel` + `wptoast.js` do.
  - (promoted to `docs/TODO.md` 21 Aug 2026) **the TIMING half** (cancel-then-settle beats an in-flight check)
    needs a real site — same run as the #249 wiring pass above.


## Finished under “Decisions pending (owner)”

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


## Finished under “Parked”

- [x] **In-app PHP patch updates — BUILT 17–18 Aug 2026, dark until the key
  ceremony.** Reverses the 16 Aug decision at the user's direction: the update
  button is 0.3.0's purpose. The custody objection that blocked it dissolved on a
  fact neither reading had noticed — the app holds only the PUBLIC key, so the
  app-side code is identical whether the private half is a CI secret or a hardware
  token, and custody can improve later for the price of a key rotation.
  Ledger #348 (verify + serial + four structural limits), #349 (`selected_patch`,
  pin as floor, the pool snapshot seam), #350 (the apply flow with revert, the
  button, the key ceremony), #353 (nothing that RUNS php asks for the pin),
  #354 (the apply reports what it changed; one apply per minor), #355 (never
  offer a half-published version). **Key ceremony DONE 18 Aug 2026** — key minted,
  `RELEASE_PUBKEY` pinned, manifest serial 1 published (8.2.32, 8.3.32), and the
  `manifest-signing` Environment secret + reviewer gate verified live (a dispatched
  run sat at `waiting` until approved; its CI-computed digests match the local
  publish byte for byte).

  **The eight-hour lesson, kept because the next feature will earn it again:** the
  button shipped working and every layer BEHIND it disagreed with it. `patch_to_run`
  existed in two places while eleven others asked for the pin; the toast asserted a
  running process from a bare `Ok(())`; the revert threw away both failure results
  while claiming recovery; the GC would have deleted the tree the user just
  installed. Each was found by a person using the app, not by a test — because the
  tests checked MECHANISM (does the signature verify, does the pool restart) and
  never *what the user sees after the action*. The guards added since fire on the
  call site, not on the outcome, for the same reason.

- [x] **Exercise the PHP-row states in the WebKit harness (L2).** ✓ 18 Aug 2026 —
  `uireview.js`'s `php-versions` probe asserts the row's TRUTH as well as its
  layout (button iff offered AND installed, chip suppressed once a button names
  the same version, a settled row renders quiet, the note only where it applies),
  over five fixture states with a coverage assert so no rule is a branch nothing
  exercises. Green at both widths; three plants fail by name. Ledger #356. Two of
  the rules were wrong as first written and planting is what said so — one of them
  convicted a legitimate state. **Still not askable here:** whether the patch shown
  is the post-update one; nothing in the DOM carries the pin, so that stays L0.

- [x] **In-app ADMINER updates — the SECOND manifest family.** ✓ 18 Aug 2026.
  Upstream was six releases and a MAJOR ahead of the pin (5.4.2 → 6.0.1) the day
  it shipped. `updates::Family` replaces the flat name allowlist; the ceiling and
  the binding probe answer the one axis on which Adminer is worse than PHP
  (rexenv's login gate and frame protections live inside Adminer's own plugin
  API). Ledger #361–#369, design in `docs/PLAN-adminer-updates.md`.

  **Three pre-existing bugs fell out of building it, all live:** a publish could
  reset the serial to 1 from one flaky `gh` call and lock every install out of
  updates forever; naming an explicit version DELETED every other version from the
  document; and `/adminer.php` served the real console with no wrapper — no login
  gate, no frame bound — because every `.php` in that docroot is executable.

  **And the delivery mechanism broke in production while shipping it:** GitHub
  burns a tag name once an immutable release on it is deleted, so the moved
  `manifest` tag is gone for good. The manifest is two files on a branch now,
  which is also atomic where delete-then-create never was (#368).

## Closed by the 21 Aug 2026 reconcile — the work had landed, the tick had not

- [x] **Say WHY the "exists" line often names a patch rexenv can't install yet** ✓
  17 Aug 2026, `be6a14d` + `387e788` — closed four days after the row was written, in
  BOTH places the row named as candidates, and the tick was missed until the 21 Aug
  reconcile. `docs/INSTALL.md` §"'8.4.24 exists' but rexenv is still on 8.4.23" carries
  the pipeline and the 17-day measurement; `src/routes/Settings.tsx` renders
  `data-probe="php-upstream-note"` ("'exists' is not a button. rexenv installs
  checksum-verified builds, which are published some weeks after php.net announces a
  release") gated on `anyUnbuildableUpstream`, so it appears only when such a chip is
  actually on screen. It sits ABOVE the rows rather than in a footer under seven of
  them — as a footer it never reached the first person to read the chip, who took the
  missing button for a broken feature. Original row below.
  <details><summary>the row as it stood</summary>

  - (open when written) **Say WHY the "exists" line often names a patch rexenv can't install yet** —
  one sentence, somewhere a curious user lands, NOT in the row (which stays short).
  php.net publishes on release day; static-php.dev, where 8.x builds come from, trails —
  measured 17 days on 16 Aug 2026 (php.net 8.4.24/8.5.9 vs our pinned 8.4.23/8.5.8). So
  the row will spend most of its life truthfully naming a patch that has no portable
  build yet. That is honest but reads as a defect to someone who does not know the
  pipeline. Candidates: the Settings section's existing footer line, or `INSTALL.md`'s
  "how rexenv gets its components". Ledger #343 has the measurement and the reasoning;
  this is the user-facing half of it.

  </details>

- [x] **Resolver-drift surfacing — RULED 13 Aug 2026, DONE 15 Aug 2026.** All three
  surfaces landed and the parent checkbox was simply never flipped (found by the 21 Aug
  reconcile). `rex doctor` renders a `Resolvers` line that counts toward findings and the
  exit code (`cli/src/main.rs::resolver_verdict`, fed by `dns::drifted_takeovers` through
  `cli_server.rs`); an absent field reads ⚠ unknown, never ✓, so an older app cannot
  report a clean check it never ran. `ResolverDriftBanner` is first in the Sites banner
  stack (ledger #334, L2 lifecycle probe, plant-proven ×2 — and the probe's first run
  caught the self-heal firing on the query's LOADING state, which would have wiped every
  dismissal on every launch). The continuous watcher is CLOSED, not deferred again.
  Original row below.
  <details><summary>the row as it stood</summary>

  - (open when written) **Resolver-drift surfacing — RULED 13 Aug 2026: wire the surfaces, keep the
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

  </details>

- [x] **Tunnel-replay posture — RULED 14 Aug 2026, and the "one leg still owed" was
  owed for about four hours.** Both legs ran the same day in `77ba587`: the control leg
  in `wp_login_check` (a request with the CF header removed) and the end-to-end denial in
  `tunnel_exposure_check` leg 8. That commit updated the example and
  `docs/CLAIM-LEDGER.md` and never touched this file, so the row kept saying a leg was
  outstanding for a week — and said it in TWO places, which is why the reconcile counted
  it as one defect with two instances rather than two staleness bugs. Original row below.
  <details><summary>the row as it stood</summary>

  - (open when written) **Tunnel-replay posture — RULED 14 Aug 2026; one leg still owed.** (ledger #307/#33/#308.)
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

  </details>

- [x] **Verdict receipt — bind the COMMIT path, not just the verdict** ✓ DONE
  13 Aug 2026; **the checkbox was flipped 21 Aug**, eight days after the row's own body
  started with "✓ DONE". A row whose text says DONE while its box says open is the
  worst of both: readers who skim the box think it is work, readers who read the body
  think it is finished. `scripts/verify-receipt.sh` is the one definition, shared by
  `verify.sh` and `scripts/git-hooks/pre-commit` (installed with `git config
  core.hooksPath scripts/git-hooks`). Original row, with the two design faults testing
  found, below.
  <details><summary>the row as it stood</summary>

  - (open when written) **Verdict receipt — bind the COMMIT path, not just the verdict.** 3 Aug
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

  </details>

- [x] **PHP 7.4 support — SHIPPED 15–16 Aug 2026** (planned 14 Aug,
  `docs/PLAN-php-74-support.md`). Pinned to our own immutable release `php-7.4.33-6`
  from `rexenv/runtimes`, licences riding the same publish; every stage below is `[x]`
  and the parent box was simply never flipped (21 Aug reconcile). **Two residuals moved
  to open rows in `docs/TODO.md` rather than travelling here inside a ticked block** —
  the runtimes-repo release notes, and PCRE JIT. Full stage log below.

  <details><summary>the stage log</summary>

  - [x] **PHP 7.4 support** (planned 14 Aug 2026, `docs/PLAN-php-74-support.md`). The
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

  </details>

- [x] **Spawn-then-use without a readiness gate — the sweep is COMPLETE** ✓ 20 Aug 2026,
  `be3c88d` (the release-gate tier) then `2f564bb` (the remaining 21 files, 27 sites).
  `common::await_listening` for TCP subjects and `common::await_ready` for the edge's
  admin UNIX SOCKET (a TCP admin on a root Caddy is arbitrary file r/w as root, so that
  one could not poll a port at all); both PANIC rather than `process::exit`, because
  `exit` skips the `OwnedService`/`Reaped` destructors — on this helper's first day an
  `exit` left a php-fpm on :9783 and took out EIGHT later examples in the same tier run.
  Two candidates were REFUTED by the adversarial pass and left alone on purpose:
  `tunnel_check` (its first use is cloudflared's own log poll, not the spawned service)
  and `mail_adopt_settings_check` (already conditioned on a LISTEN probe).
  **The tick landed 21 Aug, a day late, and the row it replaced still listed 23 files as
  open work that was already in the tree** — `2f564bb` touched 21 example files and never
  touched `docs/TODO.md`. **An adversarial audit of the sweep on 21 Aug then confirmed 22
  defects inside it** (gates placed after an early exit, gates in front of raw `Child`s,
  examples exiting 0 on a failed `start_all`); those are an OPEN row in `docs/TODO.md`,
  not part of this entry. The row as it stood, with the reasoning worth keeping:

  <details><summary>the original row</summary>

  - (open when written) **Spawn-then-use without a readiness gate — 33 confirmed sites, 5 fixed** (20 Aug
    2026). Found by fixing three flakes in one gate run and then sweeping every example for
    the shape: an example spawns a service (php-fpm, nginx, caddy, httpd) and depends on it
    with either NOTHING in between, a flat `thread::sleep`, or a poll on a DIFFERENT port.
    All three spawn helpers bottom out in `spawn_logged` → `Command::spawn()`, which returns
    at fork, not at bind.
    **Why it is worth a row rather than a shrug:** the failure never says "not ready". It
    says `php-via-fpm=false`, `502`, `503`, `ConnectionRefused` — the SERVER's symptoms —
    so it reads as a product bug and gets investigated as one. `apache_site_check`'s
    instance sat in this file as an unexplained transient for 17 days for exactly that
    reason.
    **And the project had already learned it once:** `wp_create_serve.rs` carries the
    recorded incident ("`proxy::start` returns when the process is SPAWNED, not when it is
    listening… gate on the socket like every sibling does") and polls properly — while
    every sibling kept the pattern that incident was about. One example fixed, the class
    left open: the guard-covers-claimed-surface shape again.
    ✓ **Closed for the RELEASE GATE**: `common::await_listening` added (waits, then fails
    naming the service and spilling its log), and every **sandbox-tier** site converted —
    `apache_site_check`, `dotfile_guard_check`, `linked_site_check`, `nginx_php_serve`,
    `php_fpm_serve`, `retry_recovery_check`, `valet_import_check`. All run green.
    **Still open — tiers the gate does not run**, each verified by an adversarial pass, so
    this list is findings and not suspicions:
    - **service**: `adminer_serve_check`(1), `create_site_serve`(1), `delete_site_serve`(1), `frankenphp_edge_serve`(1), `frankenphp_serve`(1), `log_tail_check`(1), `mail_route_check`(1), `monitor_coverage_demo`(1), `override_fallthrough_check`(1), `php_per_site_serve`(3), `php_pools_serve`(1), `php_switch_serve`(4), `server_switch_serve`(1)
    - **network**: `adminer_deeplink_check`(1), `multisite_check`(1), `multisite_wildcard_check`(1), `tunnel_check`(1), `wp_create_serve`(1), `wp_install_serve`(1)
    - **demo**: `wp_real443_setup`(1)
    Mechanical: replace the sleep with `common::await_listening(PORT, "name", log)`. Left
    undone deliberately rather than swept into a release commit — they cannot flake
    `verify-full.sh`, and 23 files of untested edits on the way out the door is the trade
    this project's own rules warn about.

  </details>
