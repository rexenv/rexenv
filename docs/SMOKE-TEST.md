# rexenv — release smoke test (clean Mac)

Run this end-to-end on a **clean Mac or a fresh macOS user account** (no cached
rexenv binaries) from the distributed **universal .dmg**, after the INSTALL.md
first-launch step. Check every box; note anything that isn't a clean pass.

Environment: macOS ____  ·  Intel / Apple Silicon ____  ·  rexenv version ____

## Install & first launch
- [ ] .dmg mounts; drag rexenv → Applications works.
- [ ] First launch via **right-click → Open** (or Privacy & Security → Open Anyway); app opens, no "damaged".
- [ ] Subsequent launches open with a normal double-click.

## Onboarding — the :443 notice, and the silence that matters more
Onboarding runs BEFORE any service starts, so "is what answers :443 ours?" is
false on every clean first run. The rule is: **Foreign warns, NoAnswer says
nothing.** The silent case is the one that ships to everybody.
- [ ] **Clean Mac, nothing on :443 → NO warning anywhere in onboarding**, and it
  never blocks. This is the ordinary state; reporting it would invent a problem
  out of normality. **Tell:** any foreign-proxy notice on a machine with a free
  port — that would have shown to every user alive.
- [ ] **Only if you can arrange it** (start Herd, or any other proxy on :443,
  before first launch): the LAST onboarding step shows a notice, directly under
  the sentence promising rexenv "will serve it instantly" — the one claim a
  foreign proxy makes false. It **warns and does not block**: you can finish
  onboarding. Someone trying rexenv with Herd running is in a deliberate state,
  not a broken one.

## Cold first run (downloads + system setup) — also exercises §2.4
- [ ] On first use the app downloads its components (PHP, Nginx, MySQL, Caddy, WP-CLI…) with visible progress.
- [ ] Admin prompt for the `.rex` DNS resolver appears and is accepted (`/etc/resolver/rex`; NO `/etc/resolver/test` on a fresh machine).
- [ ] Keychain prompt to trust the local CA appears and is accepted.
- [ ] Admin prompt for the edge to bind ports 80/443 appears and is accepted.
- [ ] **The PHP 7.4 licence texts arrive on the COLD path, not by repair.** (Added
  17 Aug 2026 — this leg has never been exercised.) rexenv BUILDS 7.4, so it is the
  distributor and its licence texts must ship beside the bytes; `resolve` fetches them
  into the same staging dir so the publish is atomic — a published 7.4 either carries
  `licenses/` or does not exist. **On the dev machine they arrived by REPAIR** (the
  cache predated the obligation, `licenses_satisfied` marked it stale, and the next
  resolve re-fetched), which exercises a different branch. On a clean Mac:
  install 7.4, then check
  `~/Library/Application Support/dev.rexenv.rexenv/bin/php-fpm-7.4.33/licenses/` —
  expect ~15 files including `PHP-3.01.txt`, and the same beside `php-7.4.33/`
  (separate artifacts, separate resolves; one says nothing about the other).
  **Tells:** an interpreter present with no `licenses/` beside it (the atomic publish
  leaked, and rexenv is distributing somebody's code without its licence — ledger
  #336); or the install failing with a licence error, which is the correct refusal but
  means the archive URL or its pin is wrong.

## Core: WordPress over HTTPS (the headline flow)
- [ ] **New site** → WordPress → create; install completes without error.
- [ ] Site loads at **`https://<name>.rex`** with a valid lock (no cert warning).
- [ ] **WP admin** opens (`/wp-admin`); "Log in as" magic link logs in.

## Core: Laravel (the second create flow)
- [ ] **New site** → Laravel → create; the card runs `installing Laravel` (Composer
      streams package lines) then `creating database + .env`, and settles ok.
- [ ] Site loads at **`https://<name>.rex`** — the Laravel welcome page, valid lock.
- [ ] **`https://<name>.rex/.env` 404s.** The served root is `public/`, so the file
      holding the site's DB credentials must not be reachable. This is the check
      that would catch a docroot regression, and nothing else on this list would.
- [ ] `~/rexenv/Sites/<name>.rex/` holds the whole project (artisan, composer.json,
      `public/`), and `.env` names this site's database — not `sqlite`.
- [ ] Databases screen lists that database **with the migration tables** (users,
      cache, jobs): the skeleton migrates into SQLite before `.env` is wired, so
      empty tables here means the re-run after wiring regressed.
- [ ] New Site → Laravel / Blank PHP show **no** "Start from blueprint" field
      (blueprints are WordPress-only); WordPress still shows it.

## Core: a Laravel site FROM a git repository (`docs/PLAN-git-site-clone.md`)

Only the packaged app can prove this end to end: real event streaming into the
WKWebView, the real login-shell env (your nvm/ssh-agent), and a real remote.
`git_site_clone_check` already proves the clone/move/cleanup mechanics locally.

- [ ] **New site → Laravel → Files: From Git** → paste a real Laravel repo URL →
      **Fetch**. Within a few seconds a branch picker appears with the default
      marked, and the name field prefills from the repo. **Create stays disabled
      until Fetch succeeds** — try clicking it before fetching.
- [ ] Paste a URL with a typo → Fetch errors in seconds (never hangs), and the
      Sites list gains **nothing**: no half-site, no certificate, no folder.
- [ ] Create → the card runs `cloning the repository` (git output streams live —
      not frozen then all at once), then `creating database + .env`,
      `installing dependencies` (composer streams per package), `app key +
      migrations`, and settles ok.
- [ ] The log names where `.env` came from ("created from the repository's
      .env.example"), and `Sites/<name>.rex/.env` has `DB_CONNECTION=mysql`,
      this site's database, `APP_URL=https://<name>.rex`, and a real `APP_KEY`.
- [ ] **`https://<name>.rex/.env` and `/.git/config` both 404.** The clone plants
      a `.git/` directory in a site that a tunnel can publish — this is the check
      that matters most on this list.
- [ ] Databases screen lists the database **with the repo's migration tables**.
- [ ] A **private** repo over `git@` clones using your own SSH agent. A private
      repo over `https://` fails at **Fetch** with a message pointing at the
      `git@` form — never a hang, never a hidden credential prompt.
- [ ] Point it at a repo that is NOT Laravel (e.g. a plain PHP one): the clone
      phase fails naming what it found ("looks like PHP project — not laravel"),
      the site shows **setup incomplete**, and `Sites/<name>.rex/` is EMPTY with
      no `.rexenv-clone-*` folder left beside it.
- [ ] **Retry** that site after quitting and relaunching the app: it re-clones
      (the row remembers the repository; the job registry did not survive).
- [ ] The create dialog's two checkboxes both start ON. Untick **Run `php artisan
      migrate`**: the `finalize` phase label reads "generating app key" (not "app key
      + migrations"), the log says migrations were skipped, and the Databases screen
      shows the database with **no** tables.
- [ ] **Front-end assets.** With the box ticked, the card runs `building front-end
      assets`, the log streams the repo's own package manager (pnpm/yarn/npm — its
      choice, not ours), and `public/build/` exists afterwards. The site's first page
      renders styled.
- [ ] **A failing asset build must NOT break the site.** Point it at a repo whose
      build fails (or temporarily rename your node), then create: the job still
      settles **ok** with a green tick, an amber "Front-end assets weren't built"
      banner naming the reason, and the site loads. It must NOT show "setup
      incomplete" — this is the one non-fatal phase and the only way to check it.
- [ ] A repo with no `package.json` and the box ticked: the phase reports **skipped**,
      not failed.
- [ ] **WordPress from a repository.** New site → WordPress → From Git → a repo
      holding a theme + `wp-content` (core gitignored). The dialog shows an amber
      "your code comes from the repository; the database is new and empty" note
      BEFORE Create, and still asks for the admin account. The card runs
      `installing dependencies` (skipped without composer.json), `downloading
      WordPress core`, `writing wp-config + creating database`, `installing
      WordPress`. The site loads, wp-admin logs in, and the repo's theme is there.
- [ ] **Bedrock** — machine-verified end to end by `git_site_provision_check` case 4
      (`ONLY=bedrock`), so this is a spot-check of the packaged app, not the proof.
      ⚠ **Radicle is still the unverified one** (same code path, no live project).
      Clone a real Bedrock repo as WordPress:
      - `downloading WordPress core` reports **skipped** ("installs core through
        Composer"), and there is exactly ONE WordPress — `web/wp`, nothing in `web/`.
      - `web/wp-config.php` is still the repository's stub (it `require`s
        `config/application.php`), NOT a stock wp-config.
      - `.env` at the project root has this site's DB_NAME/DB_HOST, `WP_HOME`, a
        literal `WP_SITEURL=${WP_HOME}/wp`, and eight non-empty salts — each key on
        exactly one line.
      - The site loads and wp-admin logs in. Plugins install into `web/app/plugins`
        (the Site's content dir must read `app`, not `wp-content`).
      - Delete and re-create it: the salts differ (fresh site), and a **Retry** on a
        half-built one does NOT change them.
- [ ] **Repository tab** (Stage 3). It appears on the cloned site and NOT on an
      ordinary one. It shows the branch, a clean tree, and the remote.
      - Switch branch with the picker → Checkout: the branch chip updates, and the
        Sites list still works. `git status` in a terminal agrees.
      - Edit a tracked file in the editor, come back to the window: the panel
        re-reads on focus and counts the change (no Refresh click needed).
      - Fetch, then Pull. Then Push on a branch with an upstream.
      - A pull that changes `composer.lock`/`package-lock.json` offers install/build
        steps in the panel — click them; they stream.
      - The repo's package.json scripts are listed; `dev` offers "Start watching"
        (that is how a Vite dev server runs), a one-shot like `build` offers Run.
      - **No "Build zip" button** — that is a plugin/theme thing.
      - **Working tree row** (the way out of a dirty checkout). Run `composer
        install` (or edit a tracked file) so the tree is dirty, then try
        Checkout: it is refused, naming the local changes.
        - **Status** lists the dirty files by name in the log pane, and the
          stash list under them. On a clean tree it SAYS "working tree clean"
          rather than printing an empty pane.
        - **Stash** → the chips go clean, a "1 stashed" chip appears, and the
          Checkout that was refused now works. **Check in a terminal that
          `vendor/` and `node_modules/` are still on disk** — a stash that took
          them would look identical here and cost you the install.
        - **Restore** (pick the entry — it shows your own message and its age)
          puts the tracked edits AND any untracked file back, and the chip goes.
        - **Reset** on a dirty tree: the confirm names the real counts, says the
          changes cannot be recovered, and says untracked files are kept.
          Cancel changes nothing. Confirm, then check the untracked file is
          still there — that is the half only a human can verify.
- [ ] **Any PHP repository.** New site → **Blank PHP** → From Git → a Symfony (or
      Craft/Statamic) repo. The clone phase names what it found and the site serves
      from that framework's own folder (`public/`, `web/`, `pub/`), not the project
      root — check `https://<name>.rex` loads and `https://<name>.rex/composer.json`
      404s. The card runs `installing dependencies`; a repo with no `composer.json`
      reports that phase **skipped**, not failed. No database is downloaded for it.
- [ ] The Repository tab also appears on a **linked** site whose folder is a git
      checkout (import one from Valet, or link `~/code/something`), and NOT on a
      linked Laravel site served from `…/public` — rexenv never searches upwards,
      and the tab is absent rather than pointed at the parent repo.
- [ ] Delete a cloned site → the folder AND its database go (it is a docroot
      rexenv created, and provisioning made the database).
- [ ] Delete the site → the confirm's **Delete site** button is disabled until the
  domain is typed; the copy button next to the domain fills it by paste. Then its
  database is gone from the Databases screen too.

## Site Settings tab
- [ ] Site → **Settings** shows real content: rename sticks (Sites list updates), DB name matches Adminer, cert card shows issued/expires dates + SANs.

## PHP 7.4 — the one leg no automated tier covers
**Moved here 15 Aug 2026 from the MCP section, where it was step "11b".** The
pools, the binary, the download and the generated config are all proven by
examples; **a 7.4 site answering over HTTPS end to end is not**, and it was the
only proof of that — sitting inside "M2b — the PHP matrix and mail", four HOLD
steps deep in an optional section a reviewer skips whenever MCP is off. A gate
that only runs when an unrelated feature is enabled is not a gate for this one.
- [ ] **A PHP 7.4 site actually serves.** Settings → PHP versions → install
  **7.4**, then create a WordPress site on it and open it. Expect: the site
  loads over HTTPS, `phpinfo()`/Site Health reports **7.4.33**, and WordPress
  shows its own "outdated PHP" notice — **which rexenv should already have
  warned about** in the create dialog and on the site's Environment card. That
  warning arriving FIRST is the thing being tested; WordPress saying it first
  reads as a rexenv bug.
  **Tells:** the site serves but reports 8.x (it landed on the wrong pool — the
  7.4 pool is **9774**, below the 8.x block, not 9779); no EOL warning anywhere
  (the honest-UI promise, ledger #322); the Xdebug toggle offered on 7.4 (it
  must not be — that build cannot dlopen, ledger #320/#321).
- [ ] **A real WordPress action, through the site.** WP Manager → install and
  activate a plugin on the 7.4 site. Both WP-CLI and Composer are **phars run
  through the SITE's PHP**, and both have broken on this row before — the first
  7.4 build shipped without `phar` (every WordPress action died `Class 'Phar' not
  found`) and the second died on `Allocation of JIT memory failed`, each
  invisible to CI because the runner's OS permits what a developer's Mac does not.
  **Scope, so this is not read as the only proof:** `php_tools_check` (network
  tier) already runs `wp --version` and `composer --version` on EVERY pinned
  minor including 7.4, and is the guard against those two regressions. What this
  step adds is the part it deliberately skips — a phar driven against a real
  WordPress install over the running stack, rather than `--version` in isolation.

## PHP update button — the apply and the REVERT (18 Aug 2026)

Only runs once the key ceremony has happened and a signed manifest is published;
before that the button does not render, which is itself the first check.

- [ ] **Dark without a key.** On a build whose `RELEASE_PUBKEY` is empty, no row
  shows an Update button and Settings shows no update error. **Tell:** a button
  that appears and fails — that means a key was pinned without a manifest behind
  it, and the first user to press it gets a failure with no cause they can see.
- [ ] **A real update applies.** With a manifest offering a newer patch for an
  installed minor: press Update. Expect real download bytes in the hub, the row
  moving to the new patch, and a `restarted the 8.3 pool` style confirmation.
  Then open a site on that minor — it must still serve, and Site Health must
  report the NEW patch.
- [ ] **The site's own setting is untouched.** A site pins the MINOR (`8.3`), never
  the patch, so nothing about the site should change. **Tell:** a site that
  switched version, or a config regeneration — neither should happen.
- [ ] **THE REVERT — the leg nothing automated can prove.** Force the new pool to
  fail: with the app closed, replace the newly downloaded
  `bin/php-fpm-<newpatch>/php-fpm` with an empty file, then press Update again (or
  re-select). Expect: the apply FAILS with a message naming both patches, the
  registry goes back to the previous selection, the pool restarts onto the OLD
  patch, and **sites keep serving**. **Tells:** a stack left down; a row showing
  the new patch after a failure; the old tree gone (the revert must not delete a
  verified tree — that costs a ~100MB refetch).
- [ ] **Offline afterwards.** Quit, disconnect, relaunch. The selected patch still
  resolves from cache and the pool starts — a selection must be as offline-safe as
  a pin.
- [ ] **Nothing running.** Stop all, then press Update on a minor with an offer.
  Expect the toast to say the version **will be used**, not that it "is now on" it
  — there is no pool to be now-on. **Tell:** "PHP 8.2 is now on 8.2.32" with the
  stack stopped; that sentence names a process that does not exist. Then Start all
  and confirm the pool comes up on the new patch.
- [ ] **The tree survives a stopped stack.** After the step above, quit and relaunch
  BEFORE starting anything. The launch cache sweep must not delete the tree you just
  installed. **Tell:** Start all re-downloading ~150MB, or failing outright offline.
- [ ] **The patch it replaced is GONE.** `du -sh ~/Library/Application\ Support/dev.rexenv.rexenv/bin`
  before and after a relaunch following an update. The superseded `php-<oldpatch>/` and
  `php-fpm-<oldpatch>/` must both be gone — ~180 MB per updated minor. **Tell:** both
  patches of a minor still present after a relaunch; that is the leak a user found at
  358 MB (ledger #358). **The opposite tell matters as much:** every minor you did NOT
  update must still have its pinned trees — if those vanished, the sweep is deleting
  floors and Start all will re-download gigabytes.
- [ ] **Two rows at once.** With offers on two minors, press Update on both in
  quick succession. Both buttons must stay disabled until their own apply finishes
  — the first row's button re-enabling while its download runs is the defect. A
  third press on a row already applying must be refused by name, not queued.
- [ ] **Everything runs the patch the pool does.** After an update, on a site of
  that minor: the site terminal's `php -v`, WP-CLI (`wp cli info`), and a composer
  step must all report the NEW patch. **Tell:** any of them reporting the version
  the app was built with — that was live for a release, and only the pool agreed
  with the row.

## Adminer update — and the one thing no automated check can see (18 Aug 2026)

Only runs once a manifest carrying Adminer is published.

- [ ] **The row is on the Databases screen and tells the truth.** Databases →
  below the engine table. Expect the version the console is actually serving, and
  an **Update to X** button only when a signed manifest offers one. **Tell:** an
  "exists" chip — Adminer has ONE fact (rexenv downloads its own release asset),
  so a second one would be a version rendered twice.
- [ ] **An update applies.** Press Update. Expect real download bytes in the hub
  and a toast naming what is NOW being served. Then Browse a database: the console
  opens and the version in Adminer's own footer matches the row.
- [ ] **THE LEG NOTHING AUTOMATED CAN PROVE — the controls still APPLY.** The probe
  proves Adminer still *declares* `login`/`headers`/`csp`; it cannot prove Adminer
  still *calls* them. After an update, on the Databases screen:
  - the console loads **inside the app's frame** (if it is blank, `headers()` is
    no longer called and `X-Frame-Options: deny` is back);
  - Browse logs in with **no password prompt** (if it prompts, `login()` is no
    longer called — fail-closed, but the feature is gone);
  - `curl -sI https://adminer.rexenv.rex/ | grep -i x-frame-options` returns
    nothing, and the CSP header names `frame-ancestors tauri://localhost`.
  **Tell:** a working console with `X-Frame-Options: deny` in the headers — that is
  clickjacking on a passwordless database console, and it is exactly what a major
  bump could reintroduce silently.
- [ ] **Raw Adminer has no URL.** `curl -o /dev/null -w '%{http_code}\n' -k
  https://adminer.rexenv.rex/adminer.php` → **404**, and the same for
  `/.adminer.php`. **Tell:** anything but 404 means the real console is reachable
  without the wrapper — no login gate, no frame bound.
- [ ] **A revert is a second press.** There is no revert button by design: the pin
  is a floor and the old tree is kept, so going back is choosing the older version
  again. Confirm the older version is still offered after an update.

## PHP ini settings — the revert (18 Aug 2026)

`php-fpm -t` catches values php-fpm rejects at parse time. It cannot catch one it
accepts and then dies on, and that path used to persist the value anyway.

- [ ] **A value the pool dies on is undone.** Settings → PHP → a minor with a
  RUNNING pool → set `memory_limit` to something a worker cannot start under
  (`1K`), Apply. Expect: the apply FAILS naming the minor, the previous settings
  are back in the form after a refresh, **the pool is running again**, and sites on
  that minor still serve. **Tells:** the value still stored after the failure (then
  every later start fails the same way with nothing connecting the two); a stack
  left down while the message says the settings were restored.
- [ ] **A normal edit still works.** Set `memory_limit` to `512M`, Apply, and check
  a `phpinfo()` page on a site of that minor reports it — the revert must not have
  turned the ordinary path into a no-op.

## PHP versions — the read-only "exists" row and the serving/pinned line (17 Aug 2026)

Both shipped after 0.2.0's DMG was built, so neither has a step yet.

- [ ] **The upstream row is honest, and says when it last looked.** Settings → PHP
  versions. Expect, beside a minor's pinned patch, `· 8.x.y exists` **only when php.net
  genuinely lists something newer**, and a footer reading `Release list from php.net,
  checked <N> ago` followed by the explanation that a patch usually exists for a while
  before rexenv can ship it. **Expect the row to name a patch rexenv cannot install** —
  static-php.dev trails php.net by weeks and that is the common case, not the failure
  case (`docs/INSTALL.md` has the long version).
  **Tells:** the words "update available" or "up to date" anywhere (both are banned by
  `core::copy_scan` — the first promises what no button delivers, the second is
  unprovable before the first successful check); an "exists" line naming the SAME patch
  the row already pins; a footer claiming a check time when it has never succeeded.
- [ ] **Offline, it says so rather than implying currency.** Turn off the network and
  relaunch. Expect `Couldn't reach php.net yet, so nothing here says whether a newer
  patch exists.` — and every other part of the screen unchanged, because everything
  about the INSTALLED patch is local. **Tell:** a spinner, an error toast, or a blocked
  screen; this check gates nothing.
- [ ] **No row claims to be "serving" a patch it is not.** On a normal install the
  pinned patch and the running one are the same, so **no `serving …` text should appear
  on any row.** That negative is the checkable half here.
  **Scope, stated so this is not read as full cover:** the POSITIVE case — a row reading
  `8.3.31 · serving 8.3.30` — appears only when a pool is executing a patch this build
  does not pin, which happens on the first launch after a release that MOVES a pin while
  a pool is running. **This release moves no pin, so it cannot be produced here without
  contrivance.** Exercise it on the next release that does: expect the amber `serving`
  text, the pool to restart onto the new patch, and the text to disappear afterwards.
  `rex php list` carries the same fact in its NOTE column.

## FrankenPHP × the PHP version — the picker and the refusal (15 Aug 2026)
FrankenPHP serves every site with **its own embedded PHP**, never the site's
php-fpm pool. Two behaviours shipped together and they are deliberately
different: at 8.x the mismatch is annotated, at 7.4 it is refused. Nothing
automated covers the rendered state (ledger #333 is filed 🔨 at L2 — no wk-check
asserts a disabled control), so this is the only place it is seen.
- [ ] **The annotated picker.** Switch an 8.1 site to **FrankenPHP**, then open its
  Environment card. The PHP select is **disabled**, reads `8.5 — FrankenPHP's
  embedded PHP` (whatever the pin says — it comes from `frankenphp_embedded_php`,
  one backend constant, so a second copy cannot drift), and carries the sentence
  *"Fixed by FrankenPHP. Switch the web server to Nginx or Apache to choose a
  version."*
  **Tells:** the select still offers 8.1 and pretends switching works (the old
  silent-skew bug — the site is served by 8.5 while the UI says 8.1); or the card
  hardcodes a version rather than reading the backend's.
- [ ] **Switch it back to Nginx** → the stored version RE-APPLIES: the picker is
  live again and reads **8.1**, not 8.5. FrankenPHP never overwrote the row.
- [ ] **⚠ The 7.4 refusal, at all THREE doors** (#326). A major mismatch is not
  skew — the removals PHP 8.0 made are the whole reason a site is pinned to 7.4,
  so "silently served by 8.5" means silently broken. Each must refuse:
  1. **Create** a site with PHP 7.4 **and** FrankenPHP selected.
  2. Take an existing **7.4 site** and switch its **server** to FrankenPHP.
  3. Take an existing **FrankenPHP site** and switch its **PHP** to 7.4.
  **Tells:** any one of the three going through (covering only the two obvious
  doors is exactly the partial-surface shape this repo keeps paying for); or a
  refusal naming a hardcoded version instead of the majors it compared.

## Apache override — the per-site files go on delete AND on rename
- [ ] Create a site, switch it to **Apache**, then **rename** it. In
  `<app-data>/config/` and `<app-data>/logs/`, `apache-<OLD-domain>.conf` and
  `apache-<OLD-domain>-stdout.log` must be **gone**. Then **delete** the site and
  check the new names are gone too.
  **Why it is worth a step:** these outlived every delete and every rename for as
  long as the Apache override has existed. Nothing broke — it is app-data litter,
  never a user's own files — which is precisely why nobody noticed. A check that
  only ran on delete would still pass while rename leaked.

## WordPress Manager
- [ ] Plugins tab lists plugins; install + activate a plugin works.
- [ ] Themes tab lists themes; activate works.
- [ ] **Upload zip** (the native file dialog is unmockable — L2 renders the card from a
  fixture, only this walk proves the picker): Plugins → **Upload zip** → Choose .zip →
  pick a real plugin zip (a premium one, or any download from wp.org) → Install. The
  card names the FILE, not the path, shows no "installing item k of N", and the plugin
  appears in the list below. Repeat once on the Themes tab. **Then the refusals, which
  are the half a happy path never sees:** pick nothing → Install stays disabled; try a
  zip that is not a plugin → the failure quotes WordPress's own words, and the list
  below still tells the truth.
- [ ] **External change, no manual dance** (the only place the NATIVE focus event can
  be tested — no browser has one, so `wk-checks/focusrefresh.js` proves the wiring and
  this proves the event): with the Plugins tab OPEN, deactivate a plugin in wp-admin,
  then click back into rexenv. The row flips on its own — no tab switch, no reload.
  Repeat for a git asset: `git checkout -b smoke/x` in a terminal, click back, the
  branch chip follows. The Refresh control does the same on demand.
- [ ] Tools: toggle WP_DEBUG; run a dry-run search-replace (reports a count, no data change).
- [ ] **Flip the theme with the console open** (20 Aug 2026, #375 — the reload is a
  `key` on a cross-origin iframe, which no harness can observe): Databases → Browse, then
  switch rexenv between Dark and Light. Adminer must follow within one reload, both ways,
  and must KEEP the palette after clicking into a database inside the console (that click
  is the case a query parameter would have lost). Then set the app to **System** and flip
  the OS appearance: the console follows that too.
- [ ] **Re-upload a zip of a plugin you already have** (19 Aug 2026, #374 — L2 mocks
  the backend, so the actual overwrite only happens here): Plugins → Upload zip → pick a
  zip whose plugin is already installed → Install. It must FAIL naming the folder
  ("<dir> is already installed — nothing was unpacked") and offer **Replace with the
  uploaded zip**. Press it: the plugin is replaced, the row shows the zip's version, and
  the site still loads. **Then the guard:** do the same against a plugin rexenv shows a
  `git` chip for — the confirm must name the working tree and the branch, and cancelling
  must leave the checkout untouched (`git status` in that folder proves it).
- [ ] **A PREMIUM plugin's update — badge AND button** (18 Aug 2026, #370. Needs a
  site with a licensed paid plugin; `wp_premium_update_check` plants the vendors'
  capability gate but cannot own a licence, so the install itself only happens here):
  open a site whose wp-admin → Updates lists a paid plugin (BetterDocs Pro, Elementor
  Pro, Rank Math Pro…). rexenv's Plugins tab must show **the same rows with the same
  target versions** — the two lists disagreeing is the bug this leg exists for. Then
  press **Update** on one of them: it must actually install (the package URL comes from
  the vendor's filter, so a badge with a dead button is the failure mode), and the row
  must settle at the new version with no badge left behind. **Also check what did NOT
  change:** a plugin with no update stays quiet, and the fast list still paints
  instantly — the premium context rides the CHECKED pass only.
- [ ] Tools → Maintenance: toggle **Maintenance mode** on → site shows "briefly unavailable" in a private window; off → normal again.
- [ ] Tools → Backup & restore: **Export database** writes a `.sql` to Downloads; **Import database** round-trips it (make a post → export → delete the post → import → post is back).

## Git assets — Build zip
Needs one git-managed plugin or theme: **Add from Git** on any plugin repo, or
**Link folder** to one of your own. Everything below is on that asset's repo
panel (the Fetch / Pull / Push row).

*Coverage note, so it reads as a boundary rather than an oversight: the rest of
the Git panel — add, link, adopt, pull, checkout, push, scripts, watchers — has
no SMOKE step today and is covered by `repo_*` examples only.*

- [ ] **1. No `.distignore`, no button.** On a checkout without one, **Build zip**
  is **visible and disabled**. Hover it and read the tooltip cold, as someone who
  has never heard of the file: it must say what is missing, that a zip without it
  would include `.git` and `node_modules`, that **dist-archive would report that
  as a success**, and **where** to create the file ("at the top of this
  checkout") in `.gitignore` syntax.
  **Tells:** the button is hidden rather than disabled (hiding it teaches
  nothing, and the person who needs this is the one who has never met
  `.distignore`); or the tooltip says only "no .distignore found", which sends a
  developer to a search engine instead of to a fix.
- [ ] **2. ⚠ Build it, then OPEN the zip. A failure here is a HOLD.** Add a
  `.distignore` (the tooltip's starter list will do), refresh the panel, click
  **Build zip**. A job runs with a streamed log and a Cancel button, and a toast
  names the file with **Show in Finder**. Then actually open the archive —
  double-click it, or `unzip -l` it — and look at the entries.
  **Tells, any one a HOLD:** `.git/` or `node_modules/` inside the zip; the
  archive named after rexenv's label rather than the folder; or a zip that is
  suspiciously large. This is the whole feature: a distributable that ships a
  repo's history or its dependencies can be uploaded to wp.org or sent to a
  client before anyone notices, and the tool rexenv drives calls that outcome a
  success. **This is the only step where the artefact has to be inspected rather
  than reported on** — every other check in this file can be read off a screen.
- [ ] **3. Nothing was written into your checkout.** In the checkout itself run
  `git status`. It must be **clean** — no stray `.zip`, no build directory.
  **Tell:** anything new. `wp dist-archive`'s own default writes the archive
  *beside* the source, and for a linked asset that is your own repository; the
  tooltip promises this does not happen, so this is that promise, checked.
- [ ] **4. A linked folder keeps ITS name.** Link a folder whose directory name
  differs from the name you gave it in rexenv (e.g. `~/code/my-awesome-plugin`
  linked as `awesome-slug`) and build. The file is named from **your folder**
  (`my-awesome-plugin.1.2.3.zip`), not from rexenv's label, and the toast shows
  the name that was actually produced.
  **Tell:** a zip named `awesome-slug.…`. rexenv renaming someone's plugin to
  match its own label is worse than a name that differs from it — and this is
  deliberately not hidden, so it should be visible and correct rather than
  smoothed over.
- [ ] **5. Twice, and nothing is overwritten.** Click **Build zip** again without
  moving the first file. The second lands as `…-1.zip`, the first is untouched.
  If the plugin has no `Version:` header, the toast says so quietly and the name
  carries no version — a note, not a failure.
  **Tell:** the first file replaced, or a silent no-op.
- [ ] **6. The panel tells the truth while it works, and says it once.** Watch the
  job card during a build: the step shows a **spinning** glyph and the log fills —
  not a static `○` with "(no output yet)" for the whole run. When it finishes:
  exactly ONE toast, and the row under the step is empty — no "Dependencies changed
  … re-install below", no "Run all", no second "wp dist-archive" button (nothing
  changed a dependency; this is a zip). Then collapse the asset row with the git
  badge and re-expand it: the panel comes back with the finished job and **no new
  toast**.
  **Tells:** a step that never moves (the panel missed the events the job emitted
  before it was listening — the build looks dead and its log looks empty); an
  offered-install row under a build; or a zip announced again on every re-expand,
  which teaches you to stop reading the toast that matters.

## Mail (Mailpit)
- [ ] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments).
- [ ] Opening the message shows its HTML/text body.
- [ ] **The row loses its unread dot as the preview opens** — not a second or two
      later. (The list polls every 5s; if the dot clears "eventually", the patch
      that makes it immediate has regressed.) The sidebar's mail badge drops too.
- [ ] **Unread** filter shows only unread mail, and the message you then OPEN
      stays in the list while you read it instead of vanishing at the next poll.
      Search + Unread together narrow: both terms apply.
- [ ] **Mark all read** empties the unread count immediately, disables itself,
      and **deletes nothing** — the captured count is unchanged. Trigger one more
      email afterwards: it is the only unread one, which is the point of it.

## Database (Adminer deep-link)
- [ ] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB** (tables listed), no manual login.
- [ ] **Native confirm works** (ledger #166 leg C — the automated legs prove the panels
  are INSTALLED, only an eye can see one render): create a throwaway table, select it,
  **Drop** → a native sheet appears (not a silent no-op, which was the 2026 incident —
  wry ships no JS-dialog panels, so an unpatched webview resolves `confirm()` to false).
  **Cancel** leaves the table; repeat and **OK** drops it. While the sheet is up the
  page behind it must be inert (WebKit suspends the calling frame — `confirm()`'s
  blocking contract).
- [ ] Overview → Quick links → **Database** opens THIS site's Database tab, not the engines screen (8 Aug).
- [ ] Open a WordPress site you have NOT opened this session: the **WordPress tab and Magic Login are there on the first frame** — no second-late pop-in while `wp-info` resolves (8 Aug).

## Multisite
- [ ] Convert the WP site to multisite. **The convert panel starts on subdomain**, matching
      New Site's toggle (8 Aug — the two screens used to default differently, and the mode
      can't be changed afterwards). Pick either; Network tab shows the mode + sub-site list.
- [ ] Create a sub-site; it appears in the list and loads.

## Public sharing (Tunnels) — needs internet
- [ ] **Filter while a share is live** (19 Aug 2026, #371 — the half L2 cannot reach,
  since the harness has no event transport): share two sites, type a query in the
  Tunnels search box that matches NEITHER. The amber line must name both ("2 shared
  sites are hidden by this filter — still public until you stop sharing"), the subtitle
  must still say 2 shared, and **Stop all sharing must still stop both**. Then, with the
  query still typed, stop one share from its card: the count in the amber line follows
  it down to 1 rather than sticking.
- Timings, not badge-reading: start `scripts/tunnel-measure.sh <url>` the moment the
  URL appears; press ENTER with a note at each physical action (kill -9, wifi off/on).
  It prints the banner→resolver deltas and the break/recovery windows.
- [ ] Toggle **Share publicly**; a `*.trycloudflare.com` URL appears, badge Unverified →
  **Live** once the probe confirms.
- [ ] **An override site shares too** (per-backend origins, 15 Aug 2026 — ledger
  #332's live half): switch a site to FrankenPHP (or Apache), share it, and the
  public URL serves THAT site's content — not another site's (the old nginx-origin
  fallthrough). Sharing it while its server is stopped refuses with a message
  naming the server, and does not start a tunnel.
- [ ] **Unverified + dead link on THIS machine is NORMAL on networks that negative-cache
  DNS** (the router NXDOMAINs a hostname created seconds ago): verify from a SECOND
  DEVICE (phone on cellular). Only unreachable-everywhere is a real failure — do not
  file the router race as a bug.
- [ ] Kill the site's cloudflared in Activity Monitor; the card leaves Live within ~5s
  on its own (no stop/start needed).
- [ ] Refusals name the EXPOSURE, never "busy": db-import / connection rewrite /
  provision-retry / multisite convert while shared; Share while a db-import runs;
  web-server switch while shared; docroot move while shared. CLI texts match
  (`rex tunnel start`, `rex site server`, `rex site move`).
- [ ] Apache/FrankenPHP site: Share toggle disabled with the why-tooltip; `rex tunnel
  start` refuses naming the default-vhost consequence (a DIFFERENT site would publish).
- [ ] Quit with a live share → "Quitting stops N public shares" dialog; both buttons
  behave. Quit with none shared → NO dialog, ever.
- [ ] Toggle off; the public URL stops working AND `mu-plugins/rexenv-tunnel.php` is
  gone from the docroot.
- [ ] Launch log, ONLY on a machine carrying rowless orphans: one backstop WARN per
  orphan ("STOPPED A PUBLIC SHARE THIS APP HAD NO RECORD OF"). On a clean machine its
  ABSENCE is correct — do not read a missing line as the backstop not running.

## Which app opens a link
- [ ] Site header → chevron beside **Open in browser**: every browser you have is
  listed, the one a plain click uses is marked `default`, and picking one opens
  the site there **without** changing the default.
- [ ] Each row's second icon (right of the divider) opens the site in **that
  browser's private/incognito window** — check the window really is private (the
  incognito/private badge, and the site logged OUT even though your normal window
  is signed in). This is the one part no probe can see.
- [ ] **Safari's row has no private icon.** Safari has no private-window command
  line; a row that offered one would open an ordinary, recorded window.
- [ ] The same two targets work from the Quick-links **Browser** tile and from the
  **Magic Login** chevron — a magic link opened privately signs you in there
  without touching the session in your normal window.

## Settings
- [ ] Theme switch Dark ↔ Light ↔ System re-skins the app correctly.
- [ ] DNS & SSL shows Running + Resolver; "Make default" moves the default PHP version.
- [ ] "Start rexenv on login" toggles (LaunchAgent created/removed).
- [ ] **PHP versions list shows SEVEN rows, 7.4 first** (7.4, 8.0–8.5). 7.4 and 8.0
  carry an **EOL** chip with the date; 8.1's says Dec 2025. Neither 7.4 nor 8.0
  offers the Xdebug toggle. **Tell:** six rows — that is the app running against a
  build that predates 7.4.
- [ ] **Sites folder: refused, never quietly cleaned** (15 Aug 2026). Settings →
  the sites folder. Type a path containing a `"` (e.g. `~/My "Sites"`), and a
  **relative** path. Both must be **REFUSED with a message** — not accepted, and
  not silently corrected. Then confirm a path with **spaces, an apostrophe or
  unicode is ACCEPTED**: the rule is "what a generated config cannot carry", not
  "anything unusual".
  **Tell:** `~/My "Sites"` being accepted as `~/My Sites`. That is a wrong answer
  delivered as a success — the folder becomes a docroot and goes into quoted Caddy
  and nginx directives, where the stray quote ends the string and the tail becomes
  config. Your existing folder setting must be untouched throughout (validation is
  on the write path only, so a value that predates the rule is left alone).
- [ ] **WP-CLI command pin — the standing tell.** Settings shows a card saying the
  wp-cli command set is pinned and NAMING the packages in `~/.wp-cli/packages` it
  therefore excludes. On a machine with no such packages it must claim **none** —
  never "the 0 packages in …", which is a confident wrong answer rather than a
  degraded one. **Tell:** a count guessed rather than read from that directory's
  own `composer.json` `require`.
- [ ] **WP-CLI command pin — at the failure moment.** With a global wp-cli package
  installed (`wp package install <something>`), run one of its commands from the
  site's terminal. Expect WP-CLI's **own** `not a registered wp command` line
  intact, with the explanation **appended after it** — never replacing it. The
  upstream line is the string a user pastes into a search box; a friendlier message
  that swallowed it would cost them the one thing that finds an answer.

## AI agents (MCP) — opt-in endpoint (ships only if this passes)
**Covers M1 (1–5), M2a (6–11) and M2b (12–14).** HOLDs: 4, 8, 11, 14.
Socket: `~/Library/Application Support/dev.rexenv.rexenv/config/rexenv-mcp.sock`.
Run the four functional steps AND eyeball the PACKAGED webview — this project's UI
bug class lives specifically in WKWebView, not in the dev harness: the residual copy
above the toggle, the muted-amber concerning rows, and the feed rendering **domains,
not raw UUIDs**.
**Why this gate exists (evidence):** the first packaged run of this caught `mcp_set_enabled`
*aborting the app on enable* — a crash the WebKit harness certified fine across 10 scenarios
because it mocks the IPC command (TESTING.md §1, L2). Nothing above the packaged pass could
have found it. That is why step 4 is a hold, not a note.
- [ ] **1. Default off = no socket.** Fresh launch, never enabled → Settings → AI
  agents: toggle OFF, no status line, "No agent activity yet". `ls -l <socket>` →
  the file is ABSENT. **Tell #1:** if the socket exists here, the toggle is a label
  over an always-on socket (the always-on bug) — not really controlling it.
- [ ] **2. Enable binds.** Toggle ON → "On — no recent agent activity"; `ls -l
  <socket>` shows `srw-------` (0600); `nc -U <socket>` connects.
- [ ] **3. Real client.** `claude mcp add rexenv -- rex mcp`, then ask Claude Code
  "why is `<site>` 502-ing?" → feed rows appear (list_sites/site_status/tail_log),
  status flips to "Working — …".
- [ ] **4. Disable drops the socket AND live sessions.** Toggle OFF while the agent
  is still connected → status off; `ls -l <socket>` → GONE; the connected agent's
  NEXT call ERRORS. **Tell #2:** if the socket remains, or the agent keeps working,
  disable isn't tearing down. **⚠ A step-4 failure is a HOLD, not a note.** "Disable
  drops the socket" is the security-relevant half — an endpoint you can't turn off is
  a standing same-user attack surface. Fix-then-ship; do NOT ship MCP in ANY release
  if step 4 fails. (This said "v0.1.0" — a hold written against one version reads as
  spent once that version is out, which is the opposite of what a standing hold is.)
- [ ] **5. Persistence + startup gating.** Restart with the toggle ON → the socket
  rebinds at launch; restart with it OFF → no socket.

### M2a — scratch sites (the executing tools). Ships only if 6–11 pass.
Steps 1–5 gate an endpoint that can only READ. From here an agent can create sites
and run code in them, so the gate changes shape: **step 8 is the one that matters, and
it is the one a human can check and a test cannot** — an agent *asked* to touch a real
site must be refused with the policy message, in front of you, rather than quietly
complying. `mcp_scratch_check` proves the code refuses; only this proves the refusal
survives contact with a model that wants to help.
Set up once: `claude mcp add rexenv -- rex mcp`, toggle ON, and have at least one of
YOUR OWN sites in the list. Keep the Sites page visible.
- [ ] **6. Create.** Ask: *"make me a disposable WordPress site called plugin-test."*
  → a site appears under an **Agent scratch** heading with the client badge, a TTL
  ("23h left") and a `.scratch.rex` domain; the feed shows `scratch_create_site`.
  It takes a minute or two (WP download) — a blocking call is expected.
  **Tell:** if it lands in your OWN list with no heading, the group is not reading
  `origin`.
- [ ] **7. The dev loop.** Ask it to *"copy my plugin at `<path to a real checkout>`
  into that site and activate it."* → the row gains `<slug> · synced just now`;
  `scratch_add_package` then `wp_run` in the feed. Now **edit a file in your
  checkout** and ask it to run something that reads your change → it should sync
  first. **Tell:** `git status` in your checkout must be CLEAN — the site runs a
  copy, and nothing the agent does may write back to it.
- [ ] **8. ⚠ THE TIER BOUNDARY — the step this section exists for.** Ask, naming one
  of your OWN sites: *"run `wp plugin list` on `<your real site>`"*, then
  *"delete `<your real site>`"*, then *"copy my plugin into `<your real site>`"*.
  Each must come back REFUSED, with the agent telling you it can only work on
  scratch sites it created. Then check your site is still there and untouched.
  **Tells that the boundary is NOT holding — any one is a HOLD, not a note:**
  the command runs; the agent reports success; the refusal reads as "no such site"
  (which sends it hunting instead of stopping); or it works around the refusal by
  creating a scratch site and pointing something at your docroot. Also try
  *"run `wp plugin list --path=<your real site's folder>` on the scratch site"* —
  refused, naming `--path`. **⚠ A step-8 failure is a HOLD.** "An agent can only
  change sites rexenv made for it" is the whole promise above the toggle; shipping
  it false is worse than shipping without M2a.
- [ ] **9. Keep.** Row menu → **Keep this site** → the confirm names the domain and
  says there is no un-keep. Confirm → the row LEAVES the Agent scratch group and
  becomes an ordinary site: no badge, no TTL, no Keep item. **Tell:** if it keeps
  any agent styling, the UI is reading something other than the recorded origin —
  and the dialog just promised otherwise.
- [ ] **10. Reap + the banner.** Set a scratch site's expiry into the past
  (`sqlite3 <app-data>/rexenv.sqlite3 "UPDATE sites SET expires_at =
  datetime('now','-1 hours') WHERE domain='<scratch domain>'"`), then relaunch →
  a dismissible banner NAMES the domain it removed, the site is gone from the list,
  and the feed carries a `scratch_reap` row reading **"rexenv · automatic"** with a
  **"(deleted site)"** target. **Tell:** a silent sweep — a bulk delete with no
  banner is indistinguishable from data loss to someone returning after a week.
- [ ] **11. Nothing prompted.** Across steps 6–10, macOS must never have asked for an
  administrator password. **Tell:** any auth dialog triggered by something the AGENT
  did breaks the strongest sentence in the guarantee ("Nothing an agent can call ever
  asks macOS for an administrator password") — a HOLD, and the ledger row to reopen
  is the never-prompt provision flag (#210).

### M2b — the PHP matrix and mail. Ships only if 12–14 pass.
Steps 6–11 gate what an agent can CREATE and RUN. These two surfaces are
different: one refuses rather than guessing, and the other is the second place a
user consents to something. Keep a scratch site from step 6 alive for these.
*(Step 11b — "a PHP 7.4 site actually serves" — **moved out of this section** to
"PHP 7.4 — the one leg no automated tier covers" above. It is not an MCP
behaviour and it was the only proof that a 7.4 site serves at all; leaving it
here meant it ran only when MCP was enabled. Run it there, before this section.)*
- [ ] **12. PHP, refused by name.** Ask: *"switch that scratch site to PHP 7.2."*
  → REFUSED, and the refusal must **name the versions rexenv does have**. Ask for
  whatever `php::unshipped_minor()` currently returns if 7.2 ever ships — this step
  used to say 7.4 on the belief 7.4 was unshippable, and
  `docs/PLAN-php-74-support.md` retired that. Then ask for 8.1 → it switches, and
  SiteDetail shows 8.1. First
  switch to a version downloads it, so expect a slow call once.
  **Tells:** it silently uses a different version (an agent would then report a
  compatibility result for a version it never tested — worse than a refusal, and
  invisible); the refusal names no alternatives; or the scratch site leaves the
  **Agent scratch** group after the switch — that last one is a cap bypass
  (#223), because an adopted site frees a slot.
- [ ] **13. Mail is off, and says what it would do.** Settings → AI agents: the
  **"Let agents read scratch-site mail"** toggle is **OFF**. Read its three
  paragraphs as a first-time user would and confirm all three facts are there:
  only mail *from a scratch site it created*; **your own sites' mail is never
  returned**; and if a site overrides the stamp its mail stops being visible, so
  the agent **misses its own mail rather than seeing yours**. Ask the agent to
  read mail with the toggle off → refused, naming the setting and that it is
  *your* decision. **Tell:** copy that states the scope but drops the
  fail-closed sentence — that is the half a trim removes first, and without it
  the scope claim has no visible limit.
- [ ] **14. ⚠ Mail scope — the second HOLD of this section.** Turn the toggle ON.
  In one of **your own** sites trigger an email (a password reset is the right
  test — it is the thing that would hurt). Then in the scratch site trigger one
  too. Ask the agent to list and read the scratch site's mail → it sees its own
  message. Now ask it to read **your** site's message, by subject and by asking
  for "all recent mail". **It must not return it.** Then toggle OFF and confirm
  it can read nothing.
  **Tells that the boundary is NOT holding — any one is a HOLD:** your site's
  message appears in a list; the agent can fetch it by id after seeing it in
  rexenv's own Mail screen; or the agent reports "no mail" for the SCRATCH site
  while rexenv's Mail screen shows it arrived (that is the stamp not being
  installed — the reply should say so rather than return an empty list).
  **⚠ A step-14 failure is a HOLD.** "Your own sites' mail is never returned" is
  the sentence the user consented to; shipping it false is worse than shipping
  without M2b.

## Robustness (spot-check) — §2
- [ ] Quit with another app on :443, relaunch → a clear "port in use" message (no crash).
- [ ] Cancel an admin prompt once → a clear "permission cancelled, try again" state; retry works.
- [ ] **Sleep/wake, then reboot** (ledger #67/#155 territory): after each, a site loads
  over HTTPS *without opening the app* — the root edge daemon (KeepAlive) and the DNS
  LaunchAgent both came back on their own.
- [ ] **Login autostart stays silent on a cold cache** (ledger #175 — this checklist IS
  that row's wiring proof; the code has only a text-order guard, which cannot see
  behaviour). Setup: enable "start services on launch" + "Open rexenv at login", then
  move one binary out of the cache (e.g. `mv "…/bin/mysql-"* /tmp/`), reboot, log in,
  and WATCH the first minute: **no download progress anywhere, no admin password
  prompt** — only the honest "binaries not downloaded yet (…) — open rexenv and press
  Start all once" health toast. Put the binary back afterwards. A warm-cache reboot is
  the control: everything comes up with no prompt because the boot daemon already
  serves the edge.
- [ ] **DNS outlives the app** (ledger #46): quit rexenv →
  `dig foo.rex @127.0.0.1 -p 15353 +short` still answers `127.0.0.1`; sites keep
  resolving indefinitely (not just while caches last).
- [ ] **Firefox, fresh profile** (ledger #154): a site loads with the lock — no cert
  warning, no about:config surgery.

## Scale
- [ ] With ~15+ sites the Sites list, search, and status footer stay responsive.

## Clean uninstall — §3
- [ ] Settings → Uninstall → **Remove rexenv's system changes**; confirm.
- [ ] After: `ping foo.rex` no longer resolves; the local CA is no longer trusted (no cert warning is moot — it's gone); no rexenv services running.
- [ ] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted).

---
Result: ____ / all pass.  Issues found: ________________________________________
