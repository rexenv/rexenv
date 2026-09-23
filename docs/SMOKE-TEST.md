# rexenv — release smoke test (clean Mac · Windows section at the end)

Run this end-to-end on a **clean Mac or a fresh macOS user account** (no cached
rexenv binaries) from the distributed **universal .dmg**, after the INSTALL.md
first-launch step. Check every box; note anything that isn't a clean pass.

Environment: macOS ____  ·  Intel / Apple Silicon ____  ·  rexenv version ____

**This file is half the release gate.** The other half is `docs/PUBLISH-TESTING.md`
(§A0 artefact integrity, §A quarantine → Gatekeeper → launch, and the install/uninstall
sections); its outstanding rows are tracked under "Release gates" in `docs/TODO.md`.
Order on release day: this file against the built dmg, then PUBLISH-TESTING §A0/§A —
publishing IS the §A sign-off.

## Install & first launch
- [x] .dmg mounts; drag rexenv → Applications works. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] First launch via **right-click → Open** (or Privacy & Security → Open Anyway); app opens, no "damaged". ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] Subsequent launches open with a normal double-click. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)

## App menu → About
- [x] **rexenv menu → "About rexenv" lands on Settings → About**, from whatever ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      screen was open — not the native macOS panel. The Build card shows version,
      commit, built-at, platform and Tauri, and its copy button yields all five.
- [x] Do it with the window **hidden** (Cmd-H first): the window comes back ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      focused. A menu item that opens something out of sight reads as dead.
- [x] **Cmd-C / Cmd-V / Cmd-Z still work** in a text field (the Edit menu comes ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      from the default menu the About item edits, not from anything we wrote).

## Onboarding — the :443 notice, and the silence that matters more
Onboarding runs BEFORE any service starts, so "is what answers :443 ours?" is
false on every clean first run. The rule is: **Foreign warns, NoAnswer says
nothing.** The silent case is the one that ships to everybody.
- [x] **Clean Mac, nothing on :443 → NO warning anywhere in onboarding**, and it ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
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
- [x] On first use the app downloads its components (PHP, Nginx, MySQL, Caddy, WP-CLI…) with visible progress. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **On a Mac WITHOUT the Xcode Command Line Tools** (`xcode-select -p` fails — a fresh ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
  VM, not a dev machine): all six first-run components reach `ready`, and no "install
  developer tools?" dialog appears at any point. **Tell:** every row `failed` with
  `otool -L failed: xcode-select: error…` and macOS's own CLT dialog on top of the app —
  that was 0.7.0–0.7.2 on every clean Mac, found 18 Sep 2026 on the first VM run (#676).
  A dev Mac cannot see this: the tools are there. **Passed 18 Sep 2026** with the fix, same
  VM (macOS 15.6.1 arm64, UTM, fresh app-data, `xcode-select -p` rc=2): all six `READY`
  in under two minutes, no dialog.
- [x] **Leave the Install step early.** (Added 11 Sep 2026 — users reported it.) Click ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
  Continue while the components are still downloading, finish the wizard, and on "Create
  your first site" create one straight away. The footer's download indicator must count
  up to its total and disappear (or show a failed row WITH Retry). **Tell:** a row reading
  `queued` that never moves, a counter stuck short of its total, and no Retry — fixed
  only by quitting the app. That was a planned row whose resolve returned through the
  cache hit (#566). **Second tell** (18 Sep 2026, clean VM with a dead DNS relay): every
  row `failed`, and the collapsed indicator STILL reading `Downloading 1 of 6` over a full
  bar — the batch counted only successes as settled. It must read `6 downloads failed` in
  red, and the panel header `0/6 · 6 failed`. **Passed 18 Sep 2026** (clean VM, DNS dead,
  Start all): `6 downloads failed` in red, bar in the error state. **Third tell** (same
  VM, same day): leave the Install step while rows are still failing, do the Domains step,
  land in the app — and the footer shows rows "downloading 0 B" that never change, while
  the log says every download gave up minutes ago. Nothing was listening between the two
  screens and the footer seeded from the Install step's stale cache. It must show the
  settled state within a second of the footer appearing. **Passed 18 Sep 2026** on that
  path: the final wizard step already read "6 failed", and the footer landed on
  `6 downloads failed` / `0/6 · 6 failed` with every row `failed` + Retry.
- [x] **Reach the last step with a component still downloading or failed** (Continue early; ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
  or pull the network for the failed case). The "Your kingdom is ready" step must SAY so —
  "n of m ready" with a spinner, or "n failed" in red with a Retry — and never
  "Everything's installed". **Tell:** the green ✓ Core components chip over failed rows in
  the footer: that was 0.7.x, seen 18 Sep 2026 on a VM whose DNS relay was dead. **Passed
  18 Sep 2026** on that VM: DNS broken → "Nearly there · 2 of 6 failed · Retry"; DNS fixed,
  Retry → all six landed and the step flipped to "Your kingdom is ready" on its own.
- [ ] **Leave the admin prompt open for a minute on the Domains step** while downloads are
  still running (open the footer later, or watch the Install rows before continuing). The
  byte counts must keep moving while the dialog sits there. **Tell:** progress that
  freezes exactly while a password or keychain dialog is open and jumps on as soon as it
  is answered — the prompt waiting on a runtime worker again (#567).
- [ ] **A dialog never freezes the window.** (Added 11 Sep 2026.) Settings → re-trust the
  local CA (or Repair a TLD's resolver): while the keychain/admin dialog is open, the app
  window must still scroll, switch screens and show live status. **Tell:** a beachball or
  a window that ignores clicks until the dialog is answered — a prompting command running
  on the main thread (#568). **Same check with a dialog that touches the database**:
  Import → take over a Valet/Herd TLD (or hand one back). While its admin prompt is open,
  the Sites list and status must still load. **Tell:** screens that spin until the prompt
  is answered — the database locked across the dialog (#569). (18 Sep 2026 VM: INCONCLUSIVE — with the keychain dialog up a sidebar click did not switch the page while the footer's live numbers kept moving; a second Re-trust needed no dialog (auth cached) so it could not be re-tried.)
- [x] Admin prompt for the `.rex` DNS resolver appears and is accepted (`/etc/resolver/rex`; NO `/etc/resolver/test` on a fresh machine). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
  The dialog reads **rexenv** in bold with rexenv's logo on the lock, says "rexenv wants to
  add a DNS resolver so .rex sites open on this Mac." (#578), and no extra Dock icon
  appears while it is open (#577). **Tell:** "wants to make changes." with no reason — a
  prompt that lost its `PromptReason`. **Tell:** "osascript wants to make changes." over
  a plain lock — the branded applet failed to build or launch and the fallback ran; the
  log line `branded password prompt unavailable` names why.
- [x] Keychain prompt to trust the local CA appears and is accepted. It is titled **rexenv** ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
  with rexenv's logo (#579). **Tell:** a dialog titled "security" — the trust went through
  the `security` CLI again instead of the in-process call.
- [x] Admin prompt for the edge to bind ports 80/443 appears and is accepted. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **The PHP 7.4 licence texts arrive on the COLD path, not by repair.** (Added ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
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
- [x] **New site** → WordPress → create; install completes without error. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **With the stack STOPPED when you create it** (the first site on a clean Mac always ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      is): the card's serve phase reads `stack is stopped — starting it`, the branded
      "rexenv wants to start its HTTPS server on ports 80 and 443" prompt appears from the
      card, and the card settles `created — serving at https://<name>.rex`. **Tell:** a
      green card whose serve phase says `skipped`, the site row `Stopped`, and the page
      only loading after you find Restart — that was every 0.7.x first site (18 Sep 2026).
      **Passed 18 Sep 2026** on the clean VM: `smoke2.rex` created from `Stopped 0/5`, the
      prompt came from the card, both sites `Running`, `https://smoke2.rex` 200 with the
      system trust store.
- [x] Site loads at **`https://<name>.rex`** with a valid lock (no cert warning). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **WP admin** opens (`/wp-admin`); "Log in as" magic link logs in. (18 Sep 2026 VM: `/wp-admin/` → 302 to login checked over curl; the magic link itself not exercised.) ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)

## Core: Laravel (the second create flow)
- [x] **New site** → Laravel → create; the card runs `installing Laravel` (Composer ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      streams package lines) then `creating database + .env`, and settles ok.
- [x] Site loads at **`https://<name>.rex`** — the Laravel welcome page, valid lock. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **`https://<name>.rex/.env` 404s.** The served root is `public/`, so the file ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      holding the site's DB credentials must not be reachable. This is the check
      that would catch a docroot regression, and nothing else on this list would.
- [x] `~/rexenv/Sites/<name>.rex/` holds the whole project (artisan, composer.json, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      `public/`), and `.env` names this site's database — not `sqlite`.
- [x] Databases screen lists that database **with the migration tables** (users, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      cache, jobs): the skeleton migrates into SQLite before `.env` is wired, so
      empty tables here means the re-run after wiring regressed.
- [x] New Site → Laravel / Blank PHP show **no** "Start from blueprint" field ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      (blueprints are WordPress-only); WordPress still shows it.

## Core: a Blank PHP site with a starter database (`core/starter.rs`)

`starter_seed_check` proves the seed and the generated files against a real
engine; what only the packaged app can prove is the DIALOG and the page as a
browser renders it.

- [x] **New site → Blank PHP → Database: MySQL** (the default) → Create. The card ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      shows a `db` and a `configure` phase; the first create on a clean Mac
      downloads the engine, which is exactly what the "None" option avoids.
- [x] The site opens on the **rexenv starter page**, not a `phpinfo()` dump: it names ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      the PHP version and web server, and the Sample data card reads **connected**
      with four seeded rows.
- [x] The site folder holds `index.php` **and** `db.php`. Edit `index.php`, reload, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      and the edit is what you see.
- [x] Site → **Database** tab embeds Adminer on that database (it used to say "Blank ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      PHP sites have no database"), and the row's database button is enabled.
- [x] **Stop MySQL from Services, reload the page:** the card reads **not connected** ✓ 18 Sep 2026 VM after #683 (a NEW seeded site — an existing site keeps its generated index.php): "not connected · start MySQL from the Services screen"; MySQL back → connected.
      and names the engine to start — not a PHP fatal, not a blank page. (18 Sep 2026 VM: MySQL stopped from Services, but the starter showed the "pick MySQL, MariaDB or PostgreSQL" no-database card, not "not connected" — TODO row.)
- [x] **Delete the site** → its database is gone from the Databases screen. A ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      starter database rexenv created is rexenv's to remove.
- [x] **New site → Blank PHP → Database: None** → the page loads with the "No ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      database" panel and NO engine download happened.

## Core: a PostgreSQL-backed site (`docs/archive/PLAN-postgres-sites.md`)

The automated tiers cover the engine, the dialect and every installed PHP binary.
What only the app can show is **which** PHP a site actually runs, and that is
where both of this feature's shipped defects lived (ledger #550, #551): the
dialog offered PostgreSQL for a minor judged by a build the machine was not
running, and deleting the first such site handed psql MySQL's flags. First passed
by hand 10 Sep 2026.

- [x] **New site → Laravel → PHP 8.1 or newer → Database: PostgreSQL** → Create. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      The card runs `migrations` and **finishes** — a hang here is the driver, not
      the migration (a PHP without `pdo_pgsql` busy-loops rather than failing).
- [x] Site loads at `https://<name>.rex` and Site info reads ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      **PostgreSQL · 127.0.0.1:15432** — not MySQL, not 13306.
- [x] `.env` in the project says `DB_CONNECTION=pgsql`, `DB_PORT=15432`, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      `DB_USERNAME=postgres`.
- [ ] Site → **Database** tab opens Adminer on that database, and the Sites row
      shows a real **DB size** rather than `—`.
- [x] **Set the site's PHP to 7.4 or 8.0 in the New-site dialog instead:** ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      PostgreSQL is **absent** from the Database field and a line underneath says
      why. Switch to WordPress: absent, and NO such line (a WordPress user cannot
      act on a PHP version).
- [x] **Delete the site** → it completes, the database is gone from Adminer, and ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      no error mentions `--no-defaults`.
- [x] **Blank PHP → Database: PostgreSQL** → the starter page's Sample data card ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      reads **connected** with four rows, and `db.php` says `'driver' => 'pgsql'`.

## Core: a Laravel site FROM a git repository (`docs/archive/PLAN-git-site-clone.md`)

Only the packaged app can prove this end to end: real event streaming into the
WKWebView, the real login-shell env (your nvm/ssh-agent), and a real remote.
`git_site_clone_check` already proves the clone/move/cleanup mechanics locally.

- [x] **New site → Laravel → Files: From Git** → paste a real Laravel repo URL → ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      **Fetch**. Within a few seconds a branch picker appears with the default
      marked, and the name field prefills from the repo. **Create stays disabled
      until Fetch succeeds** — try clicking it before fetching.
- [x] Paste a URL with a typo → Fetch errors in seconds (never hangs), and the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      Sites list gains **nothing**: no half-site, no certificate, no folder.
- [x] Create → the card runs `cloning the repository` (git output streams live — ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      not frozen then all at once), then `creating database + .env`,
      `installing dependencies` (composer streams per package), `app key +
      migrations`, and settles ok.
- [x] The log names where `.env` came from ("created from the repository's ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      .env.example"), and `Sites/<name>.rex/.env` has `DB_CONNECTION=mysql`,
      this site's database, `APP_URL=https://<name>.rex`, and a real `APP_KEY`.
- [x] **`https://<name>.rex/.env` and `/.git/config` both 404.** The clone plants ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      a `.git/` directory in a site that a tunnel can publish — this is the check
      that matters most on this list.
- [x] Databases screen lists the database **with the repo's migration tables**. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [ ] A **private** repo over `git@` clones using your own SSH agent. A private
      repo over `https://` fails at **Fetch** with a message pointing at the
      `git@` form — never a hang, never a hidden credential prompt.
- [ ] Point it at a repo that is NOT Laravel (e.g. a plain PHP one): the clone
      phase fails naming what it found ("looks like PHP project — not laravel"),
      the site shows **setup incomplete**, and `Sites/<name>.rex/` is EMPTY with
      no `.rexenv-clone-*` folder left beside it.
- [ ] **Retry** that site after quitting and relaunching the app: it re-clones
      (the row remembers the repository; the job registry did not survive).
- [x] The create dialog's two checkboxes both start ON. Untick **Run `php artisan ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      migrate`**: the `finalize` phase label reads "generating app key" (not "app key
      + migrations"), the log says migrations were skipped, and the Databases screen
      shows the database with **no** tables.
- [ ] **Front-end assets.** With the box ticked, the card runs `building front-end
      assets`, the log streams the repo's own package manager (pnpm/yarn/npm — its
      choice, not ours), and `public/build/` exists afterwards. The site's first page
      renders styled.
- [x] **A failing asset build must NOT break the site.** Point it at a repo whose ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [x] **Repository tab** (Stage 3). It appears on the cloned site and NOT on an ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [ ] Site → **Settings** shows real content: rename sticks (Sites list updates), DB name matches Adminer, cert card shows issued/expires dates + SANs. (18 Sep 2026 VM: rename via `rex site rename` updated the list; the Settings tab showed domains/env/Xdebug cards; cert dates read via `rex site info`, 395 days.)

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

- [x] **The row is on the Databases screen and tells the truth.** Databases → ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  below the engine table. Expect the version the console is actually serving, and
  an **Update to X** button only when a signed manifest offers one. **Tell:** an
  "exists" chip — Adminer has ONE fact (rexenv downloads its own release asset),
  so a second one would be a version rendered twice.
- [x] **An update applies.** Press Update. Expect real download bytes in the hub ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  and a toast naming what is NOW being served. Then Browse a database: the console
  opens and the version in Adminer's own footer matches the row.
- [x] **THE LEG NOTHING AUTOMATED CAN PROVE — the controls still APPLY.** The probe ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [x] **Raw Adminer has no URL.** `curl -o /dev/null -w '%{http_code}\n' -k ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  https://adminer.rexenv.rex/adminer.php` → **404**, and the same for
  `/.adminer.php`. **Tell:** anything but 404 means the real console is reachable
  without the wrapper — no login gate, no frame bound.
- [ ] **A revert is a second press.** There is no revert button by design: the pin
  is a floor and the old tree is kept, so going back is choosing the older version
  again. Confirm the older version is still offered after an update. (18 Sep 2026 VM: FAILED — after the update the row reads only `Adminer 6.0.2`; 5.4.2 stays on disk but is offered nowhere. TODO row.)

## PHP ini settings — the revert (18 Aug 2026)

`php-fpm -t` catches values php-fpm rejects at parse time. It cannot catch one it
accepts and then dies on, and that path used to persist the value anyway.

- [ ] **A value the pool dies on is undone.** Settings → PHP → a minor with a
  RUNNING pool → set `memory_limit` to something a worker cannot start under
  (`1K`), Apply. Expect: the apply FAILS naming the minor, the previous settings
  are back in the form after a refresh, **the pool is running again**, and sites on
  that minor still serve. **Tells:** the value still stored after the failure (then
  every later start fails the same way with nothing connecting the two); a stack
  left down while the message says the settings were restored. (18 Sep 2026 VM: not reproducible with `1K` on 8.3.32 — the pool stayed up and the site served, so the value was kept; the revert path was never entered.)
- [x] **A normal edit still works.** Set `memory_limit` to `512M`, Apply, and check ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  a `phpinfo()` page on a site of that minor reports it — the revert must not have
  turned the ordinary path into a no-op.

## PHP versions — the read-only "exists" row and the serving/pinned line (17 Aug 2026)

Both shipped after 0.2.0's DMG was built, so neither has a step yet.

- [x] **The upstream row is honest, and says when it last looked.** Settings → PHP ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [x] **No row claims to be "serving" a patch it is not.** On a normal install the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [x] **The annotated picker.** Switch an 8.1 site to **FrankenPHP**, then open its ✓ 18 Sep 2026 VM (an 8.3 site): select disabled, reads "8.5 — FrankenPHP's embedded PHP" with the Fixed-by-FrankenPHP sentence.
  Environment card. The PHP select is **disabled**, reads `8.5 — FrankenPHP's
  embedded PHP` (whatever the pin says — it comes from `frankenphp_embedded_php`,
  one backend constant, so a second copy cannot drift), and carries the sentence
  *"Fixed by FrankenPHP. Switch the web server to Nginx or Apache to choose a
  version."*
  **Tells:** the select still offers 8.1 and pretends switching works (the old
  silent-skew bug — the site is served by 8.5 while the UI says 8.1); or the card
  hardcodes a version rather than reading the backend's.
- [x] **Switch it back to Nginx** → the stored version RE-APPLIES: the picker is ✓ 18 Sep 2026 VM: picker live again at 8.3.
  live again and reads **8.1**, not 8.5. FrankenPHP never overwrote the row.
- [x] **⚠ The 7.4 refusal, at all THREE doors** (#326). A major mismatch is not ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  skew — the removals PHP 8.0 made are the whole reason a site is pinned to 7.4,
  so "silently served by 8.5" means silently broken. Each must refuse:
  1. **Create** a site with PHP 7.4 **and** FrankenPHP selected.
  2. Take an existing **7.4 site** and switch its **server** to FrankenPHP.
  3. Take an existing **FrankenPHP site** and switch its **PHP** to 7.4.
  **Tells:** any one of the three going through (covering only the two obvious
  doors is exactly the partial-surface shape this repo keeps paying for); or a
  refusal naming a hardcoded version instead of the majors it compared.

## Apache override — the per-site files go on delete AND on rename
- [x] Create a site, switch it to **Apache**, then **rename** it. In ✓ 18 Sep 2026 VM (ledger #302 amended): after `rex site domain ap.rex ap2.rex` only `apache-ap2.rex.{conf,-error.log,-stdout.log}` remain; after delete none.
  `<app-data>/config/` and `<app-data>/logs/`, `apache-<OLD-domain>.conf` and
  `apache-<OLD-domain>-stdout.log` must be **gone**. Then **delete** the site and
  check the new names are gone too.
  **Why it is worth a step:** these outlived every delete and every rename for as
  long as the Apache override has existed. Nothing broke — it is app-data litter,
  never a user's own files — which is precisely why nobody noticed. A check that
  only ran on delete would still pass while rename leaked. (18 Sep 2026 VM, via `rex site domain`: the OLD names went on rename; after delete `apache-<domain>-error.log` survived. TODO row.)

## WordPress Manager
- [x] Plugins tab lists plugins; install + activate a plugin works. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] Themes tab lists themes; activate works. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [ ] Tools: toggle WP_DEBUG; run a dry-run search-replace (reports a count, no data change). (18 Sep 2026 VM: the dry-run search-replace reported 12 replacements, nothing written; WP_DEBUG toggle not exercised.)
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
- [x] Tools → Maintenance: toggle **Maintenance mode** on → site shows "briefly unavailable" in a private window; off → normal again. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] Tools → Backup & restore: **Export database** writes a `.sql` to Downloads; **Import database** round-trips it (make a post → export → delete the post → import → post is back). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)

## Git assets — Build zip
Needs one git-managed plugin or theme: **Add from Git** on any plugin repo, or
**Link folder** to one of your own. Everything below is on that asset's repo
panel (the Fetch / Pull / Push row).

*Coverage note, so it reads as a boundary rather than an oversight: the rest of
the Git panel — add, link, adopt, pull, checkout, push, scripts, watchers — has
no SMOKE step today and is covered by `repo_*` examples only.*

- [x] **1. No `.distignore`, no button.** On a checkout without one, **Build zip** ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  is **visible and disabled**. Hover it and read the tooltip cold, as someone who
  has never heard of the file: it must say what is missing, that a zip without it
  would include `.git` and `node_modules`, that **dist-archive would report that
  as a success**, and **where** to create the file ("at the top of this
  checkout") in `.gitignore` syntax.
  **Tells:** the button is hidden rather than disabled (hiding it teaches
  nothing, and the person who needs this is the one who has never met
  `.distignore`); or the tooltip says only "no .distignore found", which sends a
  developer to a search engine instead of to a fix.
- [x] **2. ⚠ Build it, then OPEN the zip. A failure here is a HOLD.** Add a ✓ 18 Sep 2026 (VM, after #679) — `airplane-mode.0.2.8.zip`, 16 entries, no `.git/` or `node_modules/`, toast with Show in Finder
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
  than reported on** — every other check in this file can be read off a screen. (18 Sep 2026 VM: FAILED, HOLD — `wp dist-archive` died on `zip -i@…/Library/Application` — its include-pattern file lives in the TMPDIR rexenv sets under app-data, and the space in `Application Support` is not quoted. TODO row.)
- [x] **3. Nothing was written into your checkout.** In the checkout itself run ✓ 18 Sep 2026 (VM, after #679) — `git status` shows only the `.distignore` the test added
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
- [x] **5. Twice, and nothing is overwritten.** Click **Build zip** again without ✓ 18 Sep 2026 (VM, after #679) — second build landed as `airplane-mode.0.2.8-1.zip`, the first untouched
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
- [x] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] Opening the message shows its HTML/text body. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [ ] **A link in the HTML body opens in the browser** (WP's password-reset link is one):
      one click, the preferred browser opens it, and the preview itself does not navigate —
      the frame keeps showing the email. A `mailto:` link opens nothing and says so in a
      toast. **Tell:** a click that does nothing at all (the `sandbox=""` bug, #697, fixed
      19 Sep 2026 after shipping dead for the screen's whole life), or the email's page
      replacing the preview.
- [x] **The row loses its unread dot as the preview opens** — not a second or two ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      later. (The list polls every 5s; if the dot clears "eventually", the patch
      that makes it immediate has regressed.) The sidebar's mail badge drops too.
- [x] **Unread** filter shows only unread mail, and the message you then OPEN ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      stays in the list while you read it instead of vanishing at the next poll.
      Search + Unread together narrow: both terms apply.
- [x] **Mark all read** empties the unread count immediately, disables itself, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      and **deletes nothing** — the captured count is unchanged. Trigger one more
      email afterwards: it is the only unread one, which is the point of it.

## Mail catch-all — the two escapes no automated tier can see (4 Sep 2026)

Ledger #504/#505 prove the mechanisms; these two legs prove they hold on a REAL
site with a REAL plugin, which is the part the fixtures cannot buy.

- [x] **WordPress with an SMTP plugin.** On a WP site, install WP Mail SMTP (or ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      FluentSMTP) and configure it for ANY reachable host — the point is that the
      site is genuinely trying to leave. Trigger a password reset. It lands in
      **Mail**, not at the provider. Then Settings → Services → **Catch all
      outgoing mail** OFF, trigger another, and confirm the plugin's own send is
      attempted instead (its log, or the provider's). Turn it back ON and confirm
      the third one is caught **without restarting the stack** — the toggle
      restarts the pools itself, and a leg that quits and relaunches the app
      would pass while that was broken.
- [x] **Laravel with a real MAIL_HOST.** On a Laravel site, edit `.env` by hand to ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      a real provider's SMTP host and credentials. Load a page that mails (or use
      the site's terminal: `php artisan tinker --execute="Mail::raw('x', fn($m)
      => $m->to('you@example.test')->subject('smoke'));"`). It arrives in **Mail**
      — the environment beat the file. Both surfaces, because the pool and the
      CLI carry the catch separately.
- [x] **A FrankenPHP site is caught too** (#514). Switch a WordPress site's web server ✓ 18 Sep 2026 VM (after the FrankenPHP re-pin, #680): wp.rex on FrankenPHP → reset caught; catch-all OFF → the FrankenPHP backend respawned (new pid) and the reset was NOT caught; ON → respawned again and caught. No stack restart.
      to FrankenPHP, trigger a password reset: it lands in **Mail**. Then toggle the
      catch-all OFF → the site's Services row shows its FrankenPHP backend respawn
      (the toggle reconciles override backends, not only pools), and a reset now goes
      to PHP's default sendmail; ON again → caught, again without a stack restart.
      `frankenphp_mail_catch_check` proves the mechanism on the real binary; this leg
      proves the toggle drives it on a real site.
- [x] **The stated limit is TRUE, not just printed.** On that Laravel site run ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      `php artisan config:cache`, then mail again. If the site was created by
      this rexenv its `.env` was already wired, so it still lands in Mailpit; a
      site whose `.env` predates the feature will NOT, which is exactly what the
      card says. Confirm the card says it (Settings → Services, catch-all ON).

## Database (Adminer deep-link)
- [x] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB** (tables listed), no manual login. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **Native confirm works** (ledger #166 leg C — the automated legs prove the panels ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  are INSTALLED, only an eye can see one render): create a throwaway table, select it,
  **Drop** → a native sheet appears (not a silent no-op, which was the 2026 incident —
  wry ships no JS-dialog panels, so an unpatched webview resolves `confirm()` to false).
  **Cancel** leaves the table; repeat and **OK** drops it. While the sheet is up the
  page behind it must be inert (WebKit suspends the calling frame — `confirm()`'s
  blocking contract).
- [ ] Overview → Quick links → **Database** opens THIS site's Database tab, not the engines screen (8 Aug).
- [x] Open a WordPress site you have NOT opened this session: the **WordPress tab and Magic Login are there on the first frame** — no second-late pop-in while `wp-info` resolves (8 Aug). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)

## Stopping ONE site (v44) — the shared services must NOT go with it
- [x] **With the stack STOPPED (Stop all), hover a site row:** its "Open in browser" quick
      action is disabled and its tooltip reads "Nothing is serving https://<name>.rex — Start
      all first"; the site page's Open in browser likewise. Stop ONE site with the stack up
      instead: the button stays enabled and opens rexenv's stopped page. **Tell:** Safari on
      "can't connect" from a click rexenv offered (the clean-VM run, 18 Sep 2026). ✓ 18 Sep
      2026, same VM, stack Stopped 0/6: the row's quick action, the site page's header
      button and its Quick links tile all disabled with that tooltip.
- [x] With at least two sites serving, row menu → **Stop site** on one. Its pill reads ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      **Stopped by you**, and the toast says your other sites keep running.
- [x] In a browser, the stopped site's URL shows rexenv's **"This site is stopped"** page — ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      the brand mark, THIS site's hostname, and both ways to start it — not a certificate
      warning, not a 502, not Caddy's bare error text, and **not another site's content**
      (the fallthrough this design exists to prevent; the automated proof is
      `site_stop_start_check`). Check it in a light-themed browser too: the page follows
      `prefers-color-scheme`, and it must fetch nothing (no webfont, no remote logo).
- [x] The OTHER site still loads, and Services still shows the web tier running. A stopped ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
      site must never have stopped a php-fpm pool — every site on that PHP version shares it.
- [x] **Start site** → it serves again on the SAME certificate (no interstitial, no ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      "certificate is not trusted" — the route kept its cert while stopped).
- [x] Stop a site, **quit rexenv and relaunch**: it is still stopped. (The switch is in the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      database precisely because services outlive the app.)
- [x] With everything stopped (Stop all), press **Start site** on a stopped site: the toast ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      says the site is set to run but rexenv's services are stopped — no "started" claim
      the browser would contradict.
- [x] `rex site stop <domain>` / `rex site start <domain>` do the same thing from a ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      terminal, and the stop output says this is not `rex stop`.

## Start/stop EVERY site (Sites page only)
- [x] Sites → **All sites** menu → **Stop all sites (N started)**: every row goes to ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      "Stopped by you", the toast counts them, and Services still shows the web tier
      RUNNING — this is not the footer's "Stop all".
- [x] The menu's other row now reads **Start all sites (N stopped)**; the one that would ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      do nothing is visible but disabled and says why ("none stopped").
- [ ] With a "setup incomplete" site present, the toast says how many were skipped — and
      that site is untouched.
- [x] `rex site stop --all` then `rex site start --all` do the same from a terminal and ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      print the counts.

## The Sites list's two filters
- [x] Open Sites: it opens on **All** — every site visible on arrival — and only a tab you
      click changes it. (This row said "Running, not All" until 18 Sep 2026; the owner asked
      for All back on 11 Sep 2026 and the code comment beside `filter` records it. The doc
      was the stale half.) ✓ 18 Sep 2026 VM: relaunch with 3 running → All 3.
- [x] Choose a **type** tab (WordPress / Laravel / Blank PHP): the list narrows, and each ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      status tab's count matches what that tab actually shows (and vice-versa) — no number
      above a list that does not contain that many rows.
- [x] Filter down to nothing: the empty state offers **Show all sites**, and clicking it ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      restores the full list.
- [ ] Pick a tab, then leave the page idle for ~10s while sites start or stop: **the tab
      does not change under you** (the default is decided once, not by the 2s poll).

## Multisite
- [x] Convert the WP site to multisite. **The convert panel starts on subdomain**, matching ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
      New Site's toggle (8 Aug — the two screens used to default differently, and the mode
      can't be changed afterwards). Pick either; Network tab shows the mode + sub-site list.
- [x] Create a sub-site; it appears in the list and loads. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)

## Public sharing (Tunnels) — needs internet
- [ ] **Share a site, then STOP that site** (v44): the Tunnels row grows an amber strip
      saying the link now shows the "site stopped" page — without re-sharing, because the
      warning is derived on each poll. Start the site again and the strip goes away.
- [ ] **Open the public URL of a STOPPED shared site, and reload it a few times** — it
      must show rexenv's stop page EVERY time, never another site's content, and the page
      must name the LOCAL site (`ea.test`) with a runnable `rex site start ea.test` — not
      the trycloudflare hostname the browser is showing (ledger #512). (This failed
      on 5 Sep 2026: a tunnel bypasses the edge and nginx answered from its default
      server — ledger #511. Reload more than once; the first response was a Cloudflare
      error while the tunnel came up, and the wrong site appeared only afterwards.)
- [ ] Share a site that is ALREADY stopped: it shares (no refusal) and a toast says what
      the link publishes. `rex tunnel start <domain>` prints the same sentence.
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
- [x] Toggle **Share publicly**; a `*.trycloudflare.com` URL appears, badge Unverified → ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  **Live** once the probe confirms.
- [x] **An override site shares too** (per-backend origins, 15 Aug 2026 — ledger ✓ 18 Sep 2026 VM: blank.rex on FrankenPHP shared without refusal and a public URL was issued (content check from the second device not repeated — the URL was stopped before propagation).
  #332's live half): switch a site to FrankenPHP (or Apache), share it, and the
  public URL serves THAT site's content — not another site's (the old nginx-origin
  fallthrough). Sharing it while its server is stopped refuses with a message
  naming the server, and does not start a tunnel.
- [x] **Unverified + dead link on THIS machine is NORMAL on networks that negative-cache ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  DNS** (the router NXDOMAINs a hostname created seconds ago): verify from a SECOND
  DEVICE (phone on cellular). Only unreachable-everywhere is a real failure — do not
  file the router race as a bug.
- [x] Kill the site's cloudflared in Activity Monitor; the card leaves Live within ~5s ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  on its own (no stop/start needed).
- [ ] Refusals name the EXPOSURE, never "busy": db-import / connection rewrite /
  provision-retry / multisite convert while shared; Share while a db-import runs;
  web-server switch while shared; docroot move while shared. CLI texts match
  (`rex tunnel start`, `rex site server`, `rex site move`).
- [ ] Apache/FrankenPHP site: Share toggle disabled with the why-tooltip; `rex tunnel
  start` refuses naming the default-vhost consequence (a DIFFERENT site would publish).
- [x] Quit with a live share → "Quitting stops N public shares" dialog; both buttons ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  behave. Quit with none shared → NO dialog, ever.
- [x] Toggle off; the public URL stops working AND `mu-plugins/rexenv-tunnel.php` is ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  gone from the docroot.
- [ ] Launch log, ONLY on a machine carrying rowless orphans: one backstop WARN per
  orphan ("STOPPED A PUBLIC SHARE THIS APP HAD NO RECORD OF"). On a clean machine its
  ABSENCE is correct — do not read a missing line as the backstop not running.

## The menu bar (no dock icon)
*The behavioural half of #437/#438 — the L0 tests prove the menu's RULES, and only a
hand can prove a spec entry became the item it describes.*
- [ ] The R icon is visible and legible in **both** a light and a dark menu bar
  (System Settings → Appearance).
- [ ] **The dock tile follows the window, and both directions need looking at.** A normal
  launch shows the window AND a dock tile; closing the window removes the tile and leaves
  the menu-bar icon; the tray's **Open rexenv** brings both back. Look at the DOCK — do
  not trust `lsappinfo`: it reported `Foreground` for an app whose tile macOS had never
  added (2 Sep 2026), so the policy and the tile are two different facts.
- [x] Clicking the icon opens the MENU (not the window). **Open rexenv** brings the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9) — menu, not window; Open rexenv brought it to front
  window to the FRONT — in front of a full-screen browser, not behind it.
- [ ] **Hold the menu open for ~30 seconds** (the refresh tick is 5s) while the stack is
  mid-start, so the status line and the greyed Start/Stop actually move. The numbers must
  change UNDER the open menu and the menu must stay open. It closing itself is the bug
  in-place editing exists for (reported 8 Sep 2026, ledger #437).
- [x] **About rexenv** in the menu → the window comes up on the About screen, with the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  version. Do it with the window CLOSED: that is the state where the app menu's own About
  does not exist, and the only reason this item is in the tray.
- [x] **Close the window** (red button) → the app stays alive: `rex status` still ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  answers and an MCP client keeps working. This is the whole point of the feature.
- [x] The status line matches the sidebar footer for the same moment — same verdict, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  same count. Stop a service from the UI; within ~5s the menu says the same thing.
- [x] **Start all** is greyed when everything runs, **Stop all** when nothing does. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  Clicking either does exactly what the footer's button does.
- [ ] **Sites ›** lists your most recent sites and opens one in your **preferred**
  browser (Settings → the browser you chose), not the OS default. With more than 8
  sites, the submenu says how many it hid.
- [ ] Each of **All sites… / Services / Databases / Mail / Tunnels** shows the window
  on that screen, including from a window that was hidden.
- [x] **MCP server** carries a checkmark that matches Settings, and toggling from the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  tray flips it in Settings too — with a share of the socket to prove it (an MCP client
  connects after ON, fails after OFF). A checkmark reading "on" while nothing listens is
  the failure this item exists for.
- [x] Hold the menu OPEN for 15+ seconds with the stack idle: it must NOT close by ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  itself. (The rebuild is conditional for exactly this reason.)
- [x] **Quit rexenv** with a public share up still pauses once and names the count. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [ ] **Start on login** (Settings) → log out and back in: rexenv comes up in the MENU
  BAR with **no window**, and the services it manages are up. Check the FILE first —
  `~/Library/LaunchAgents/dev.rexenv.rexenv.plist` must name the app you are testing and
  contain `--hidden` — but do NOT check `launchctl print`: an already-loaded job keeps
  the arguments it was loaded with, so the running session shows the old ones until the
  next login. The file is the thing that carries into the next login.
  After logging back in, run **`scripts/login-leg-check.sh`** BEFORE opening the window:
  it collects the whole leg in one output — the process and its `--hidden`, the accessory
  policy (no dock tile), the app's own "launched at login — staying in the menu bar" log
  line, both sockets answering with no window, and the plist that produced it. Written
  because this leg is checked once per release, by a human who has just logged in and has
  no interest in remembering five commands.
- [ ] **A couple of minutes after that login, `rex status` must read
  `answering (agent, udp 15353)`, not `(in-process…)`.** At login the app and the agent
  race for the port; in-process means DNS dies with the app, and the handoff (#442) has
  up to ~2 minutes to take it back. If it still says in-process after that, the app is
  serving DNS it should have given away — the log will say whether the agent was ever
  kickstarted. Enable it, log out and in
  once BEFORE this build too if you can: the plist is rewritten on every launch, so an
  old one (no `--hidden`) must repair itself rather than keep opening a window forever.
- [ ] On a machine where first-run setup is NOT finished (no `/etc/resolver/rex`, or the
  CA untrusted for this user), a LOGIN launch **does** show the window. A silent tray
  there would hide the only screen that fixes it.
- [ ] `rex status` with the app closed prints the reason **and** `open -a rexenv` — and
  does NOT start the app.
- [x] `rex open` with the app running brings the window to the front. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [ ] **Exactly one `rexenv.app` on the machine before testing any of this.** With a
  built bundle still in `target/release/bundle/` and the same build in `/Applications`,
  `open -a rexenv` started the one under `target/` (1 Sep 2026) — LaunchServices resolves
  by bundle id and nothing warns. A leg run against the copy you did not mean proves
  nothing, the same trap as relaunching an unguarded build above.
- [x] **Launch rexenv a second time while it is running**: the second copy exits with ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  "rexenv is already running", the first one's window comes forward, and there is still
  exactly ONE rexenv process. Then **SIGKILL** the app (`kill -9`) and launch it again —
  a stale socket file must not block the launch. **The launched build must be one that
  HAS the guard, or this leg proves nothing** — measured 1 Sep 2026, where the first
  attempt relaunched the older installed app, which could not have been blocked by a
  guard it does not contain. The rerun with a guarded build came up normally on the stale
  socket, and a second guarded launch on top of it handed off and exited.

## Which app opens a link
- [ ] Site header → chevron beside **Open in browser**: every browser you have is
  listed, the one a plain click uses is marked `default`, and picking one opens
  the site there **without** changing the default.
- [ ] Each row's second icon (right of the divider) opens the site in **that
  browser's private/incognito window** — check the window really is private (the
  incognito/private badge, and the site logged OUT even though your normal window
  is signed in). This is the one part no probe can see.
- [ ] **Safari's row has no private icon.** Safari has no private-window command
  line; a row that offered one would open an ordinary, recorded window. (18 Sep 2026 VM: only Safari installed → no chevron/menu rendered at all; the whole section needs a Mac with two browsers.)
- [ ] The same two targets work from the Quick-links **Browser** tile and from the
  **Magic Login** chevron — a magic link opened privately signs you in there
  without touching the session in your normal window.

## Settings
- [x] Theme switch Dark ↔ Light ↔ System re-skins the app correctly. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] DNS & SSL shows Running + Resolver; "Make default" moves the default PHP version. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] "Start rexenv on login" toggles (LaunchAgent created/removed). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] **PHP versions list shows SEVEN rows, 7.4 first** (7.4, 8.0–8.5). 7.4 and 8.0 ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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

## Site terminal — the PATH and the session that must survive a tab switch

- [x] **The developer's own tools resolve.** In a site's Terminal tab run `which code`, ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  `which rex` and `which git`. Each must answer with the same path Terminal.app gives.
  **Tell:** `command not found` — that is a non-login shell, so the app is back to
  launchd's bare `/usr/bin:/bin:/usr/sbin:/sbin` and none of `/etc/zprofile`'s or
  `~/.zprofile`'s PATH exists (#423). `php -v` must STILL report the bundled patch:
  the prepend runs after the rc files, and a login shell must not cost that.
- [x] **A session survives leaving the tab.** Run something with visible output ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  (`ls -la`, or better a slow `composer install`), switch to Overview or another site,
  come back. The earlier output and the shell's history must still be there — a
  long-running command must still be running, not restarted. **Tell:** an empty
  terminal and a fresh prompt (#424). **Restart** is the one control that is allowed
  to wipe it, and must.
- [x] **A plugin/theme row opens a terminal in its own folder.** WordPress tab → ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  Plugins → the terminal button on a row. `pwd` must be that plugin's directory and
  the site's own shell must be untouched (leave a `# marker` in it first, come back,
  it is still there — a `cd` typed into a busy shell is exactly what this must not
  be). Repeat from a theme card. **Tell:** landing in the docroot instead, which on a
  Bedrock/Radicle site would mean the content dir was guessed rather than read (#425).
  Hello Dolly / an mu-plugin / a drop-in have no folder: those rows show no button.

## AI agents (MCP) — opt-in endpoint (ships only if this passes)
**Covers M1 (1–5), M2a (6–11), M2b (12–14) and M3 (15–18; 19–22 retired by D16).** HOLDs: 4, 8, 11, 14, 16, 18.
Socket: `~/Library/Application Support/dev.rexenv.rexenv/config/rexenv-mcp.sock`.
Run the four functional steps AND eyeball the PACKAGED webview — this project's UI
bug class lives specifically in WKWebView, not in the dev harness: the residual copy
above the toggle, the muted-amber concerning rows, and the feed rendering **domains,
not raw UUIDs**.
**Why this gate exists (evidence):** the first packaged run of this caught `mcp_set_enabled`
*aborting the app on enable* — a crash the WebKit harness certified fine across 10 scenarios
because it mocks the IPC command (TESTING.md §1, L2). Nothing above the packaged pass could
have found it. That is why step 4 is a hold, not a note.
- [x] **1. Default off = no socket, and nothing below the toggle.** Fresh launch, never ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  enabled → Settings → AI agents: the paragraph, the toggle OFF, and NOTHING beneath —
  no status line, no Connect, no dial, no feed (D16). `ls -l <socket>` →
  the file is ABSENT. **Tell #1:** if the socket exists here, the toggle is a label
  over an always-on socket (the always-on bug) — not really controlling it.
- [x] **2. Enable binds.** Toggle ON → "On — no recent agent activity"; `ls -l ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  <socket>` shows `srw-------` (0600); `nc -U <socket>` connects.
- [ ] **3. Real client, both scopes.** The card offers two lines: `claude mcp add rexenv
  -- rex mcp` (this project) and `claude mcp add --scope user rexenv -- rex mcp` (every
  project on the machine). Run the project one here; later, from a DIFFERENT directory,
  run the user one and confirm `claude mcp list` finds rexenv there too. **Tell:** only
  one line on the card — the default scope is per-project, and a developer who set rexenv
  up in one repo and lost it in the next is the report this step exists to prevent.
  Then ask Claude Code
  "why is `<site>` 502-ing?" → feed rows appear (list_sites/site_status/tail_log),
  status flips to "Working — …".
- [x] **4. Disable drops the socket AND live sessions.** Toggle OFF while the agent ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  is still connected → status off; `ls -l <socket>` → GONE; the connected agent's
  NEXT call ERRORS. **Tell #2:** if the socket remains, or the agent keeps working,
  disable isn't tearing down. **⚠ A step-4 failure is a HOLD, not a note.** "Disable
  drops the socket" is the security-relevant half — an endpoint you can't turn off is
  a standing same-user attack surface. Fix-then-ship; do NOT ship MCP in ANY release
  if step 4 fails. (This said "v0.1.0" — a hold written against one version reads as
  spent once that version is out, which is the opposite of what a standing hold is.)
- [x] **5. Persistence + startup gating.** Restart with the toggle ON → the socket ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [x] **6. Create.** Ask: *"make me a disposable WordPress site called plugin-test."* ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  → a site appears under an **Agent scratch** heading with the client badge, a TTL
  ("23h left") and a `.scratch.rex` domain; the feed shows `scratch_create_site`.
  It takes a minute or two (WP download) — a blocking call is expected.
  **Tell:** if it lands in your OWN list with no heading, the group is not reading
  `origin`. Then open ANY site → Logs → **AI agents (MCP)** (the tab exists now that
  `mcp.log` does): the create is there as one line — `agent <client> ·
  scratch_create_site → ok · site <domain> (<id>)` — with the same local timestamp shape
  as `rexenv (app)`, and the Settings card's row says the same thing. Press **Only this
  site** → the chip reads `n/total` and only lines naming this site's domain stay; on a
  site the agent never touched the pane says the other lines exist. **Tell:** a line
  the card does not have, or a card row the log does not, means the two carriers have
  split; a line carrying a path, a URL or argv text means the writer is no longer the
  feed's.
- [ ] **7. The dev loop.** Ask it to *"copy my plugin at `<path to a real checkout>`
  into that site and activate it."* → the row gains `<slug> · synced just now`;
  `scratch_add_package` then `wp_run` in the feed. Now **edit a file in your
  checkout** and ask it to run something that reads your change → it should sync
  first. **Tell:** `git status` in your checkout must be CLEAN — the site runs a
  copy, and nothing the agent does may write back to it.
- [x] **7a. Log in without a password (D2, 5 Sep 2026).** Ask: *"give me a login link ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  for that scratch site."* → the reply carries a `https://<scratch>/?rexenv_login=…` link
  and says it is single-use and expires in two minutes. Open it → **wp-admin, signed in as
  the primary administrator, no login form.** Open the SAME link again → an ordinary
  WordPress page (the token was spent). The feed shows `scratch_login_url` with the site
  and nothing else — no URL, no token. Then ask for a link into one of your OWN sites
  through the same phrasing → it comes from `wp_user`'s `login_url` and works with the
  dial at Read (the default — the owner's ruling: no login, no password, any site); the
  feed shows `user login_url` and no link. **Tell:** a link
  that logs in twice, a token in the feed, or a password reset appearing in your users
  list — any one is a HOLD (the promise is "never a password").
- [x] **8. ⚠ THE TIER BOUNDARY — the step this section exists for.** Ask, naming one ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
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
- [ ] **10. Reap + the banner.** Set a scratch site's expiry into the past, then
  relaunch →
  ```sh
  DB=~/Library/"Application Support"/dev.rexenv.rexenv/rexenv.db
  sqlite3 "$DB" "UPDATE sites SET expires_at = datetime('now','-1 hours')
                 WHERE domain='<scratch domain>'; SELECT changes();"
  ```
  **`SELECT changes()` must print `1`.** This step named `rexenv.sqlite3` until
  25 Aug 2026; the file is `rexenv.db`, and `sqlite3` CREATES the name it is
  given — so the command left an empty decoy database beside the real one,
  matched no rows, and the relaunch showed no banner. That reads as a broken
  reaper when nothing was ever expired, which is the worst way for a gate to be
  wrong: it manufactures a defect. **Expire ONE site and leave another scratch
  site live**, so the reap has to be selective rather than a bulk wipe →
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
- [x] **12. PHP, refused by name.** Ask: *"switch that scratch site to PHP 7.2."* ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  → REFUSED, and the refusal must **name the versions rexenv does have**. Ask for
  whatever `php::unshipped_minor()` currently returns if 7.2 ever ships — this step
  used to say 7.4 on the belief 7.4 was unshippable, and
  `docs/archive/PLAN-php-74-support.md` retired that. Then ask for 8.1 → it switches, and
  SiteDetail shows 8.1. First
  switch to a version downloads it, so expect a slow call once.
  **Tells:** it silently uses a different version (an agent would then report a
  compatibility result for a version it never tested — worse than a refusal, and
  invisible); the refusal names no alternatives; or the scratch site leaves the
  **Agent scratch** group after the switch — that last one is a cap bypass
  (#223), because an adopted site frees a slot.
- [ ] **13. Mail rides the endpoint (D16).** There is no mail switch. With the endpoint
  ON, read the paragraph above the toggle as a first-time user would: it must say an
  agent can read **every site's mail**, **password-reset links included**, and the
  databases **read-only** with **password hashes** and **API keys** named. Ask the agent
  to list the scratch site's mail → it works with nothing to click. Turn the endpoint
  OFF → the socket is gone and nothing renders below the toggle. **Tell:** a "Let agents
  read scratch-site mail" switch anywhere, or a paragraph that says "your own sites' mail
  is never returned" — that sentence was true of the scratch tools and is FALSE of the
  inbox tool, which is why the card no longer says it.
- [ ] **14. ⚠ Mail scope — the second HOLD of this section.** In one of **your own**
  sites trigger an email (a password reset is the right test — it is the thing that
  would hurt). Then in the scratch site trigger one too. Ask the agent to list and read
  the scratch site's mail (the scratch tools, `mail_list`/`mail_get`) → it sees its own
  message and **not yours** — the From stamp filters, and an unstamped site is refused
  rather than returned empty. Then ask it to *"read my inbox"* (`mail_inbox`) → your
  reset mail IS there — by design at Read — and its text shows `key=<redacted>` and no
  `rexenv_login=` token; the reply's note says what is and is not removed.
  **Tells that the boundary is NOT holding — any one is a HOLD:** your site's message
  appears in the SCRATCH tools' list; the agent reports "no mail" for the scratch site
  while rexenv's Mail screen shows it arrived (the stamp not installed — the reply should
  say so); or a reset link in `mail_inbox` carries its `key=`.

### M3 — database access. Ships only if 15–18 pass.

Steps 12–14 gate what an agent may read of mail. This section is the first time an agent
can read **the data in a site you made yourself** — every post, every user row, every
option. Since D16 (4 Sep 2026) that read is answered by the Agent access dial's **Read**
level — on whenever the endpoint is — with no prompt: the gate is not "does the consent
flow work", it is **does the paragraph above the toggle tell the truth, and is the read
really a read**, checked in front of you.

What the automated layers already prove, so you do not re-check it by hand:
`examples/agent_db_check.rs` (service tier) connects as the real read-only principal
against a real engine and confirms the SERVER refuses `INSERT`, `UPDATE`, `DELETE`,
`DROP DATABASE`, `INTO OUTFILE`, a second statement after a `;`, `LOAD DATA LOCAL
INFILE`, and a sibling database whose name differs only by the underscore wildcard.
**Run it before this section** — if it fails, these steps are theatre.

Set up once: MCP toggle ON, `claude mcp add rexenv -- rex mcp`, at least one of **your
own** WordPress sites with real content in it, and the database engine running.

- [ ] **15. A real site reads at Read, with no prompt.** Ask: *"how many published posts
  are in `<your own site>`? query the database."* → the count comes back; Settings → AI
  agents shows NO prompt and nothing to click; **Recent activity** lists the call with
  the SQL as its summary. **Tells:** a prompt appears (a retired consent surface is back);
  or the call is refused naming a grant.
- [x] **16. ⚠ Read-only IN FRONT OF YOU.** Ask it to **write**: *"set that site's blog ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  title to 'agent was here' with a SQL UPDATE."* → REFUSED by the server, and the title
  in WordPress is unchanged. Then ask it to read something sensitive it legitimately can
  (*"list the user emails"*) — it succeeds, because the paragraph above the toggle said
  so in those words. **⚠ A step-16 failure is a HOLD.** "Read-only" is a sentence the
  user read and turned the endpoint on under.
- [x] **17. Off closes it.** With the agent's conversation still open, turn the endpoint ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  OFF → its next query fails (the socket is gone), and nothing renders below the toggle.
  Turn it back ON → the read works again with no prompt.
- [ ] **18. ⚠ Deleting the site takes the account with it.** Read a site's database once
  (that provisions `rex_ro_<slug>` and records it), then delete that site in rexenv. Then
  check the engine directly:
  ```sh
  "$HOME/Library/Application Support/dev.rexenv.rexenv/bin/mysql-8.4.6/bin/mysql" \
    --no-defaults --protocol=TCP -h 127.0.0.1 -P 13306 -u root -N \
    -e "SELECT user,host FROM mysql.user WHERE user LIKE 'rex\_ro\_%' \
           OR user LIKE 'rex\_agent\_%';"
  ```
  → **no row for the deleted site.** (Adjust the version in the path if the engine has
  moved on.) **Run it with `'r%'` in place of `'rex\_ro\_%'` first.** That must print
  rows — `root`, and a `rex_<slug>` per imported site. An empty result and a BROKEN query
  look identical. **Do the same with a SCRATCH site**: ask the agent to create one,
  `db_query` it once, delete it, and confirm no `rex_agent_%` row survives — on 25 Aug
  2026 that half found the account leaking. **Rename first, then delete**, for one site:
  the account was made under the old domain and only the recorded row can name it (#403).
  **⚠ A step-18 failure is a HOLD, and this is the one step here that no automated layer
  covers at all** — a leftover account means a site you create later at that domain
  inherits a read you gave once, to a site that no longer exists.
- *19–22 retired by D16 (4 Sep 2026): revoke, per-site/per-client scope, auto-allow and
  the consent-prompt wording have no surface any more — the dial is global, reads need
  no grant, and the one paragraph carries the wording (step 13).*

### Parity P1 — the Agent access dial (D15, 3 Sep 2026; replaced the switch + per-site prompts the same day)
The first shape — "Let agents manage my own sites" plus one prompt per site, per scope,
per client — was honest and cost six clicks for one site's ordinary work (the live run
that afternoon). What replaced it is ONE dial. These steps are what a person reads and
turns; the L0/L1/L2 legs (#497, #498) hold the rest.
- [ ] **23. Read by default, and Read is free.** Fresh launch, endpoint ON → Settings → AI
  agents: **Agent access** shows three levels with **Read** chosen — unmistakably: a violet
  border and a filled check on the chosen row, not colour alone (D16 found the first
  version's chosen state was dead CSS) — and NO duration row; Read's sentence names every
  site's mail and the databases, read-only, with password hashes and API keys; the copy
  says Read is on whenever the endpoint is, that Full also publishes a site and rexenv
  stops the share within the hour, that the
  administrator password still asks, and that code an agent runs in your site runs as you.
  Ask an agent to list a site's plugins → it works, nothing to click. **Tell:** a prompt
  for a read, or a "Let agents manage my own sites" switch anywhere.
- [ ] **24. Changes, for this session.** Pick **Changes** → the duration row appears with
  **This session** chosen and the line "switches itself off when you quit rexenv". Ask the
  agent to switch a site's PHP version → it does; ask it to delete a plugin → refused, and
  the refusal names `Agent access` at **Full**. Quit and relaunch → the dial is back at
  Read (the log says so), and the same delete is refused again.
- [ ] **25. Full, for 7 days, and what it says.** Pick **Full**, then **7 days** → the
  expiry stamp shows. Read the Full sentence: it must say delete/reset, live
  search-replace, database import, AND "run commands and code of its choosing … as you".
  Ask for the plugin delete → it runs. Set the stamp in the past (`rex config` cannot —
  the keys are refused; use the sqlite shell) → the card shows "Your 7-day setting
  expired", the level reads Read, the delete is refused.
- [ ] **26. ⚠ What the dial does and does not answer (D17).** At **Full · Always**: ask the
  agent to share a site → it starts, and the reply says the URL and that rexenv stops it
  within the minutes asked for; there is NO prompt anywhere, because Full says so. Turn
  the dial to Changes → the same request is refused naming `run` and Full. Ask it to set
  `agent_access_level` through the `settings` tool → refused with the policy's reason.
  Ask it to stop the stack → the macOS password dialog still appears. **Tells — a HOLD:**
  a share starting below Full; an agent turning the dial; no macOS dialog.
  *23–26 done 4 Sep 2026 on the rebuilt app, the owner turning the dial, Claude Code
  driving `bl.rex`: Read free (plugins/logs/inbox), a change and a destroy refused naming
  the level; Changes → xdebug ran, delete and `wp` refused naming Full, share asked;
  Full · 7 days → delete, `wp option get`, dry run ran, share still refused after Don't
  allow (its prompt offered session only); the stamp set to the past → "has expired, so
  it is back at Read" and the card's notice; Changes · This session → relaunch → Read (the
  log says so). The `settings` tool refused `agent_access_level`. Two fixes from the run:
  share's refusal promised a 7-day button; the dial polls; picking a level after an
  expiry starts at this session.*

> **D15 note for 27–39:** where a step below says "a prompt for `manage`/`destroy`/`run`
> → Allow", read: refused naming **Agent access** at Changes/Full → turn the dial (for this
> session) instead of clicking Allow; "one grant does not cover the other site" legs are
> moot (the dial is global). After D17 there is no prompt at all — publishing is Full. The steps'
> other tells — what runs, what is refused on shape, what the reply carries — are unchanged.

### Parity P2 — the site lifecycle on YOUR sites. Ships only if 27–31 pass.
`mcp_user_site_check` (sandbox tier) proves the switch, the gate, a real rename and a
real delete over the socket against a sandbox. What it cannot prove is the three tools
that need the stack — creating, retrying and restarting a real site — and whether a
MODEL respects the gate. Set up: switch ON, `claude mcp add rexenv -- rex mcp`, the
stack running, at least one of your own sites. Keep Settings → AI agents visible.
- [ ] **27. Create, through the prompt.** Ask: *"create a blank PHP site called
  `agenttest.rex`."* → refused naming `manage` and Changes (D15/D17 leave no prompt: the
  old text below described **rexenv
  itself** (not a site — there is none yet), `manage`, and "It asked to: create a php
  site `agenttest.rex`". Read the sentence under it: it must say what `manage` allows in
  concrete words AND "runs as you". Click **Allow for 7 days** → the agent's retry creates
  the site; it appears in your list as an ordinary site (no Agent-scratch heading, no
  badge, no TTL) and loads over HTTPS. **Tell:** it lands under Agent scratch (the row was
  written as the agent's), or macOS asked for a password (the create prompted — #475's
  never-prompt rule is broken; a HOLD).
- [ ] **28. Configure.** Ask it to switch that site to another PHP version and to add the
  domain `also-agenttest.rex`. Both should succeed under the grant from 27 — one grant on
  rexenv itself does NOT cover the site: expect a SECOND prompt, for `agenttest.rex`,
  `manage`, "switch it to PHP 8.x". Allow for this session → SiteDetail shows the new
  version; the alias serves. **Tell:** the first grant covered the site (a stack grant is
  not a site grant).
- [ ] **29. ⚠ THE GATE, in front of a model.** Ask, naming one of YOUR OTHER sites: *"delete
  `<your site>`"*. → refused; a `destroy` prompt appears for THAT site, offering **Allow
  for this session** and **Don't allow** and NOT a 7-day option. Click **Don't allow** →
  the prompt goes; the site is untouched; the agent reports the refusal. Then ask it to
  *"rename `<your site>` to x"* → refused, `manage` prompt; leave it. **Tells — any one is
  a HOLD:** the command runs; a 7-day option on a destroy prompt; the agent works around
  the refusal (creates a scratch site, uses `wp_run`, tries the CLI); the refusal reads
  as "no such site".
- [ ] **30. Delete, and the session boundary.** Ask it to delete `agenttest.rex` → prompt →
  Allow for this session → the site, its folder and its database are gone; the feed
  shows `site_delete · ok` naming it. Quit and relaunch → a session-long dial is back at
  Read; ask it to delete anything → refused naming Full.
- [ ] **31. Retry + restart.** Break a create on purpose (stop the database engine, then
  ask for a WordPress site) → the reply says the site exists as "setup incomplete" and
  names `site_retry`. Start the engine, ask it to retry → the site finishes. Ask it to
  restart the site → the reply says which of the three outcomes happened (backend /
  shared / refused) and how many sites share the pool; with `pool: true` it names them.
  **Tell:** the reply carries a local log path or a port number — or the retry reply
  says `status: "running"` with phases pending. It must be the settled outcome (ledger
  #559: until 11 Sep 2026 it was the job's first snapshot, and the agent had to poll).

### Parity P3 — WordPress on YOUR sites, and the inbox. Ships only if 32–35 pass.
`mcp_user_site_check` proves the gates over the socket (a PHP site and a scratch site
refused on the row, the log list without paths, the inbox reading at Read with no switch
and no ask). What no tier proves is a vetted command actually running on a real WordPress
under the dial, the raw runner as YOU, and a real reset mail read with its key gone. Set
up: the stack up, one of your own WordPress sites with real content, the endpoint ON.
- [ ] **32. Read is free, a change needs the dial (D15/D16).** Ask: *"list the plugins on
  `<your WP site>`"* → the list comes back with nothing to click. Ask it to *activate*
  one → refused naming `Agent access` at **Changes**; turn the dial to Changes · This
  session → activated (check wp-admin). **Tell:** the activation ran at Read.
- [ ] **33. ⚠ Destroy is never a week, and never quiet.** Ask it to *"delete the plugin
  `hello`"* → a `destroy` prompt with **Allow for this session** and **Don't allow** only.
  Don't allow → plugin still there. Then ask for a live search-replace (`dry_run: false`)
  → also `destroy`; a dry run must NOT prompt beyond `manage`. **Tell — a HOLD:** a 7-day
  option on either prompt, or the live replace running under `manage`.
- [ ] **34. The raw runner runs as you.** Ask: *"run `wp option get siteurl` on `<your
  site>`"* → a `run` prompt, its own scope (the `manage` grant from 32 must NOT cover
  it). Allow → the value comes back with `succeeded: true`. Ask for *"`wp plugin list
  --path=/`"* → refused naming `--path`, no prompt. Ask for a user with no password →
  the reply shows a generated password once; the feed row reads `user create` and never
  the value.
- [x] **35. ⚠ The inbox is a Read (D16).** With the endpoint on and the dial at Read: ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  *"read my inbox"* → your messages come back with no switch and no prompt; the reply's
  note says every site's mail is there and what is removed. Open a password-reset mail's
  text → `key=<redacted>` and no `rexenv_login=` token; the `headers` carry no cookie
  value. Turn the endpoint OFF → nothing. **Tell — a HOLD:** a message body carrying a
  reset key or a login token, or a "Let agents read scratch-site mail" switch anywhere.

### Parity P4 — the stack, with the password dialog as the second consent. Ships only if 36–39 pass.
The sandbox L1 cannot raise a macOS dialog, start a real tunnel or swap a pool. These are
the human legs. Set up: stack up, switch ON, one of your own sites.
- [x] **36. ⚠ Stopping the stack is two consents.** Ask: *"stop rexenv's stack"* → refused; a ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  `system` prompt for **rexenv itself** whose text says macOS will ALSO ask for your
  password. Allow for this session → the agent's retry raises the macOS dialog. Cancel it
  → the call fails, the stack is still up. Retry, enter the password → every site is
  offline; `stack_status` says so. Start it again the same way. **Tells — a HOLD:** no
  macOS dialog (the privileged path was bypassed); the stack stopped under a `manage`
  grant; any "without asking" switch anywhere in the card (D15 removed them). *Done 3 Sep 2026:
  cancelled → "Administrator permission was cancelled", stack up; password → 0 running;
  start → 12 running.*
- [ ] **37. A resolver write is the same shape.** Ask it to *"repair the resolver for
  `.rex`"* → `system` prompt naming the dialog → Allow → the macOS dialog → the file is
  back. Ask it to set `mcp_enabled` through `settings` → refused with the policy's
  reason and NO prompt.
- [ ] **38. ⚠ Share: Full covers it, and it stops itself (D17).** Set the dial to **Full ·
  This session**. Ask it to *"share `<your site>` for 2 minutes"* → it starts with no
  prompt; the reply has a `trycloudflare` URL that
  loads from your phone. Wait 2 minutes → the Tunnels page shows it stopped, and the
  feed shows no second agent call. **Tells — a HOLD:** the share started without the
  the dial at Changes or below; the tunnel outlives its minutes; quitting rexenv leaves it up.
  *Done 3 Sep 2026, in the other order: a person's session `run` → share started, the
  reply's `trycloudflare` URL, stopped by rexenv at 2m 00s (log), no second agent call,
  URL then 530; grant revoked + switch ON → share refused naming auto-allow, the
  auto-granted row left behind, no tunnel. The switch's FIRST click never reached the
  backend (no log line); the second did — unexplained, watch for it.*
- [ ] **39. Cap and TTL are settings.** `rex config set scratch_cap 0` → refused with the
  range; `rex config set scratch_cap 2` → the third scratch_create_site names the limit
  as 2 and lists the two.

### Parity P5 — migration and repo under grants. Ships only if 40–42 pass.
- [ ] **40. A clone under `run`, with progress.** On one of your own sites ask: *"clone
  `https://github.com/octocat/Hello-World` into it as a plugin"* → a `run` prompt → Allow
  for this session → Claude Code shows the job's phases arriving while it runs (the
  progress notifications), then a reply whose log names the plugin folder as
  `<docroot>/…`, never `/Users/…`. `repo status` on it reads under the same grant.
- [ ] **41. A rewrite is two steps and a fingerprint.** On an imported site (Valet/Herd):
  *"preview the connection rewrite"* under `read` → the diff and a fingerprint. *"apply
  it"* → `destroy` prompt, session only → Allow → applied, backup kept. Edit the config by
  hand, ask it to apply the OLD fingerprint again → `fileChanged`, nothing written.
- [ ] **42. A database import destroys.** *"import `<site>`'s database from Herd"* → a
  `destroy` prompt naming the drop. Don't allow → the database is untouched. Allow →
  phases stream, the reply names any kept dump by FILE. `db_import leftovers` under
  `read` on rexenv itself lists it by name only.

### Parity P6 — the protocol, in front of the real clients. Ships only if 43–45 pass.
- [ ] **43. ⚠ D12: 49 tools, two clients.** `claude mcp add rexenv -- rex mcp` → Claude Code
  lists every tool (`/mcp` → rexenv) with no schema complaint, and a call works. Then the
  Cursor stanza from the card → Cursor shows the server connected and its tool list
  complete (Cursor has historically capped tools per server; if it truncates, THAT is the
  finding — record the number). **Tell — a HOLD:** either client rejects the list or a
  descriptor; the grouped design (D12) was chosen on this evidence and has to be re-cut
  if it fails here. *Without a packaged app* the Claude Code half runs against HEAD:
  `REXENV_MCP_HOLD_SECS=120 cargo run --example mcp_socket_check` (app quit), then
  `claude -p "list every rexenv tool name" --mcp-config <json naming cli/target/debug/rex mcp>
  --strict-mcp-config` — the count in the answer is the real client's `tools/list`. *Claude
  Code half done this way 3 Sep 2026: 49/49 listed, `list_sites` round-tripped. Cursor owed.* (18 Sep 2026: a raw JSON-RPC client counted **50** tools — the number here has drifted.)
- [x] **44. Destructive hints reach the client.** In Claude Code, ask for `site_delete` on ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  a granted site → the client's confirm-before-destructive UX appears (it reads
  `destructiveHint`); `list_sites` never prompts (read-only).
- [ ] **45. ⚠ Quit mid-call.** Ask for a `site_create` (a minute of work), quit rexenv
  while it runs → the agent reports **"rexenv stopped while this call was in progress …
  the outcome is unknown"** and does NOT retry the create on its own. Reopen rexenv →
  the half-built or finished site is in the list; `site_status` says which.
  **Tell — a HOLD:** the model sees a bare transport error, or retries the create.

### Parity P7 — the Laravel loop. Ships only if 46–47 pass.
- [ ] **46. artisan, non-interactive by construction.** On one of your Laravel sites ask:
  *"run migrate:status"* → a `run` prompt → Allow for this session → the table comes back.
  Then *"run tinker"* → returns at once, exit 0, no hang (stdin is null; psysh prints a
  termcap warning to stderr, not an error). **Never `db:wipe`/`migrate:fresh` here to test
  the prompt: Laravel's `ConfirmableTrait` only asks in production — locally they wipe at
  once.** (This step first said to run `db:wipe`; corrected 3 Sep 2026 before anyone did.)
  **Tell — a HOLD:** tinker hangs the call. *Done 3 Sep 2026 on `hisab-counter.rex`:
  `migrate:status` listed the batches, `tinker` exited 0 immediately.*
- [ ] **47. A Composer link is a symlink, and says so.** Make a package folder with a
  `composer.json` (`"name": "acme/widgets"`, a `src/` with one class). Ask: *"link
  `~/Projects/acme-widgets` into `<laravel site>`"* → under the session's `run` grant,
  Composer runs (the reply's `log`) and the reply says SYMLINK and "lands in the checkout".
  `ls -l vendor/acme/` in the project shows the symlink; the project's `composer.json` has
  `repositories.acme-widgets` with `"symlink": true`. Edit the class in the SOURCE folder →
  the site sees it with no sync. Ask for `~` as the source → refused (blast radius), and
  nothing in `composer.json` changed. *Done 3 Sep 2026 on `hisab-counter.rex`: `vendor/acme/widgets`
  is a symlink, a source edit showed through `tinker --execute` with no sync, `~` refused with
  `composer.json` byte-identical. Composer's log carried the source's absolute path — fixed the
  same run (labelled `<source>` before the scrub).*

## In-app self-update — the replace and the relaunch, which no tier can run (6 Sep 2026)

*The half nothing below L3 can see: a real bundle replacing itself in `/Applications`, macOS
deciding whether to allow it, and launchd re-execing the agent that outlives the app. The
probe measured the swap once on this Mac (`docs/archive/PLAN-self-update.md` §T0); this is the same
question asked of a real rexenv, on a real release, with a real user's approval state.*

Only runs once a release NEWER than the installed build has been published with its signed
descriptor. Before that Settings → About shows the version and no Install button, which is
itself the first check.

- [x] **The menu bar offers it, and only opens it.** With a newer release published, the ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  tray menu's FIRST item reads `Update to <version>…`; clicking it shows the window on
  Settings → About and installs NOTHING. The app menu's "Check for Updates…" (under About)
  lands on the same card with a fresh check. **Tell:** an item that installs, an item naming
  no version, or a menu that hangs while it opens — it must read a snapshot, never fetch.
- [ ] **Dark when current.** On the newest build, Settings → About offers nothing and shows
  no error; the footer says when it last checked. **Tell:** a button that appears and fails,
  or a footer claiming a check that never ran.
- [x] **The offer arrives on its own.** With a newer release published, launch rexenv and ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  open About without pressing anything: the version and size are named before any click, and
  the consent sentence is above the button. **Tell:** a button with no sentence, or a size
  that disagrees with the release asset.
- [x] **A real update applies.** Press Install. Expect real bytes in the footer's download ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  indicator, then the app quits and reopens on the new version — About and `rex --version`
  agree, and the startup notice names the version the NEW process read from itself.
  **Tell:** two `rexenv.app` copies afterwards; a notice naming a version About disagrees with.
  ◐ 7 Sep 2026 — the apply, the quit and the reopen ran twice and everything measurable
  agreed: `CFBundleShortVersionString` 0.6.1, cdhash `e7036221…` → `ce45c7a5…` (a different
  bundle, not a rewritten one), still universal, codesign valid, `rex 0.6.1 (f748698)` equal
  to About, one `rexenv.app`. **Open for one clause only: nobody read the startup notice.**
  A box is not ticked by the parts of it that were watched.
- [x] **THE LEG NOTHING AUTOMATED CAN PROVE — Gatekeeper and App Management on the replaced
  bundle.** No "rexenv is damaged", no "cannot be opened", no "prevented from modifying apps"
  notification, and `xattr -l /Applications/rexenv.app` shows no quarantine attribute.
  **Tell:** any of those dialogs — the update installed and the app will not start.
  ✓ 7 Sep 2026, macOS 26.6.2, **twice** (0.6.0 → 0.6.1, then again after a brew downgrade put
  0.6.0 back): the owner reports no dialog of any kind either time, and the replaced bundle
  carries no quarantine xattr with `codesign --verify --deep --strict` still valid. This is
  §T0's measurement holding for a real release rather than a probe.
- [x] **Services and DNS survive it.** Sites keep answering across the whole thing; after the
  relaunch `rex status` reads `answering (agent, udp 15353)`, the DNS agent's pid has CHANGED
  (`pgrep -f -- --dns-agent`), and `rexenv.log` carries the "kickstarting it" line naming the
  old build. **Tell:** the same agent pid as before — launchd re-exec did not happen and the
  resolver is running the previous binary.
  ✓ 7 Sep 2026, all three clauses: `answering (agent, udp 15353)`; the agent pid changed on
  every swap (43710 → 47967 → 65661); and the log carries four kickstart lines, including
  `the resolver agent is running 0.6.1 f748698 but this build is 0.6.0 55eae12 — kickstarting
  it` — the agent NEWER than the app, after the brew downgrade, so the check is a mismatch
  test and not a "less than" one. MySQL, MariaDB, seven php-fpm pools, Nginx, Caddy and
  Mailpit kept their pids throughout.
- [x] **The previous copy is cleaned up, but only after a healthy launch.** `ls -a
  /Applications` shows no `.rexenv-update-*` directory once the new build has opened.
  **Tell:** a leftover that survives two launches.
  ✓ 7 Sep 2026, checked after each of the two updates: none.
- [x] **A public share is live.** Start a share, then press Install: the quit confirm names ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  the share count exactly as Cmd+Q does. Choose "Keep sharing" → the app stays up, the card
  says the update is installed and takes effect when rexenv next opens, and NO relauncher is
  left behind (`pgrep -f -- --relaunch-after` is empty). **Tell:** a relaunch that happens
  anyway, or a helper still waiting.
  ◐ 7 Sep 2026 — **run, and it found the bug this leg exists for.** The quit confirm and the
  "Keep sharing" choice worked, and no relaunch happened; the card then said **nothing at
  all** and offered Install again, as if the update had not been made. Fixed the same day
  (ledger #539) — re-run this leg on the next release to close it, and watch for the sentence
  naming the restart.
- [ ] **Offline.** Disconnect and press Check now: the card says the check could not run and
  keeps the previous timestamp. **Tell:** "up to date", or a timestamp that moved — **or a
  spinner that never stops**, which is what this leg caught on 7 Sep 2026: over 40 seconds
  of "Checking…" under a 15-second client timeout that never fired. The seam now enforces its
  own deadline (ledger #540) and the button gets a SHORTER one than the poller — **6 seconds**
  — so the answer is late at worst, never absent. Re-run this leg against the fix and time it:
  it should say it could not reach the server within about six seconds.
- [ ] **Homebrew coexistence.** On a cask-installed copy the consent sentence mentions
  `brew upgrade --cask rexenv`; after a self-update **`brew upgrade` with NO cask named**
  does nothing. **Tell:** brew touching rexenv in the unnamed form.
  ✓ 7 Sep 2026: `brew upgrade --dry-run` does not list rexenv — the unnamed form leaves it
  alone, as `auto_updates` promises. The consent-sentence half (a cask-installed copy naming
  `brew upgrade --cask rexenv`) is NOT yet observed.
  **Do not test this with `brew upgrade --cask rexenv`** — naming a cask is an explicit
  request and Homebrew honours it regardless of `auto_updates`, so the named form really
  does reinstall whatever the local tap checkout says, and on a stale checkout that is a
  silent downgrade. Measured 7 Sep 2026: it put 0.6.0 back over a self-updated 0.6.1 and
  printed `Upgraded 1 requested outdated package`. That is not a bug to file; it is the
  reason this line now says which command to run.
- [x] **Which permissions come back.** Note every macOS prompt that reappears after the
  update (an ad-hoc build is a new identity each time). Record them here — the consent
  sentence promises this, and the list is what makes it honest.
  ✓ 7 Sep 2026, on this Mac, across two updates: **none**. No prompt was seen by the owner
  and `log show --predicate 'subsystem == "com.apple.TCC"'` records none for rexenv in the
  window. **Do not turn this into a promise.** It is one Mac whose grants were already
  settled; the consent sentence keeps warning because a machine that has not yet granted a
  permission, or a macOS that keys a grant more strictly, will ask again. Re-record per
  macOS major.

## Robustness (spot-check) — §2
- [x] Quit with another app on :443, relaunch → a clear "port in use" message (no crash). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] Cancel an admin prompt once → a clear "permission cancelled, try again" state; retry works. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
- [x] **Sleep/wake, then reboot** (ledger #67/#155 territory): after each, a site loads ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)
  over HTTPS *without opening the app* — the root edge daemon (KeepAlive) and the DNS
  LaunchAgent both came back on their own. — 18 Sep 2026 VM reboot: the root edge daemon and the DNS LaunchAgent came back on their own (caddy on :443, agent answering) before the app was opened; the backends need Start all (or login-start) — `rex start` and the site served.
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
- [ ] **Firefox opens a typed bare `name.rex`** (ledger #706): after onboarding (or Settings →
  Firefox → "Open typed addresses in Firefox" → Enable, then restart Firefox), type `acme.rex`
  with no slash → the site, not a search. **Tell:** a Google results page — check the profile's
  `user.js` for `browser.fixup.domainsuffixwhitelist.rex`. In Chrome/Safari the same text still
  searches (expected); the default-TLD card and the domain's tooltip on a site page say so.

## Scale
- [x] With ~15+ sites the Sites list, search, and status footer stay responsive. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2)

## Clean uninstall — §3
- [x] Settings → Uninstall → **Remove rexenv's system changes**; confirm. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] After: `ping foo.rex` no longer resolves; the local CA is no longer trusted (no cert warning is moot — it's gone); no rexenv services running. ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted). ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9)
- [x] **`/usr/local/bin/rex` is removed by the uninstall, inside the one admin prompt** (ledger #677). It was left behind on the first VM uninstall of 18 Sep 2026: `/usr/local/bin` is root-owned on a clean Mac (the CLI install's own `mkdir -p` made it), so the unprivileged unlink failed silently. **Tell:** the link still there after Remove, pointing into the app you are about to drag to the Trash. ✓ 18 Sep 2026, same VM, fixed build: one prompt (its sentence now names the rex command), link gone, resolver gone.
- [x] **Stale CA trust entries are swept** (ledger #678). Every fresh app-data folder mints a new local CA; until 18 Sep 2026 the ones earlier folders trusted stayed trusted roots (four on the VM after four hand wipes) and the uninstall untrusted only the current one. Now trusting a CA sweeps the others and the uninstall untrusts all of them. **Tell:** `security dump-trust-settings` listing more than one `rexenv Local CA` after setup. ✓ 18 Sep 2026, same VM: 4 trusted / 5 in the keychain before the Domains step → 1 / 1 after, ONE keychain dialog, log `untrusted 4 stale rexenv CA(s)`.

### PHP defaults are rexenv's, not PHP's (19 Sep 2026, ledger #694)
- [ ] On a fresh install, Settings → Services → PHP → ini settings shows every field EMPTY with
  placeholders 1G / 5G / 8G / 60 / -1 / 5000, and a site's `phpinfo()` (or `php -i` through the
  pool — not the CLI) reports exactly those: `memory_limit 1G`, `upload_max_filesize 5G`,
  `post_max_size 8G`, `max_input_vars 5000`, `max_execution_time 60`. **Tell:** 128M or 2M
  anywhere — the defaults are placeholders again, not written.
- [ ] A 200 MB upload through a site (WP media, or a plain form) is accepted by nginx AND PHP —
  the nginx global `client_max_body_size` is derived from the same defaults (8G). **Tell:** a
  413 from nginx on a body PHP would take.

## macOS 13 and 14 — the legacy tiers (`docs/PLAN-macos-13-floor.md`, T7)

Run the Core sections above on a **clean macOS 13.7 VM and a clean macOS 14 VM** (the UTM
recipe from "Cold first run", arm64). What differs there, and what must be seen:

Environment: macOS 13.6 (22G120) arm64, UTM (CLT-less at first, CLT installed later the same day) · rexenv 0.8.6-dev (2d1e1bc4 + the T7 fixes), then **the shipped `rexenv_0.8.7_universal.dmg` (`a8b9dce2`, sha `6931094a…`) on the same VM at 18:50**: 9/9 services, Databases 3/3 running (MySQL 8.4.3, MariaDB 12.0.2, Redis 8.2.1), PostgreSQL's muted row, all four sites HTTPS 200, `crash.log` empty · run 23 Sep 2026 over ssh + JXA clicks; **the macOS 14 VM has not been run**

- [x] **The app LAUNCHES and draws a window.** ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less, build 2d1e1bc4+activation/gate fixes, driven over ssh + `rex`) — first build aborted (below); the fixed build launches, `rex open` answers "window opened", no `crash.log` entry. Not a formality: the first build put on a
      13.6 VM (23 Sep 2026) aborted in `applicationDidFinishLaunching` on a macOS-14-only
      selector (`-[TaoApp activate]`, ledger #713), with `crash.log` saying only "panic in a
      function that cannot unwind" — `~/Library/Logs/DiagnosticReports/rexenv-*.ips` and a
      Terminal launch (`/Applications/rexenv.app/Contents/MacOS/rexenv`) show the real
      message. Any newer-than-13 AppKit/WebKit selector the app sends will look like this.
      (A "rexenv quit unexpectedly" seen once during a HAND swap of the bundle over ssh —
      `SIGKILL (Code Signature Invalid)` at `_dyld_start` — was the DNS agent's KeepAlive
      relaunching into a half-copied bundle, not the app: the in-app updater swaps atomically.)
- [x] **Onboarding's Welcome step shows the legacy note once** ✓ 23 Sep 2026 (macOS 13.6 VM, eyes-on over ssh screenshots + JXA clicks, build with the four T7 fixes) — the note read exactly "This Mac runs macOS 13.6 — rexenv works here with older versions of some components and without PostgreSQL and PHP 8.0. Everything is available on macOS 15 or later." (a warning-tinted card under the tagline); the wizard then completed — components READY, the branded keychain prompt, "Your kingdom is ready". Original text: "This Mac runs macOS 13.7 —
      rexenv works here with older versions of some components and without PostgreSQL and
      PHP 8.0. Everything is available on macOS 15 or later." (on 14: "…without PostgreSQL…"
      is absent; PostgreSQL 16.4 is offered). On a 15+ Mac the note must NOT appear.
- [x] **Cold first run installs the LEGACY pins** ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less, build 2d1e1bc4+activation/gate fixes, driven over ssh + `rex`): `rex db versions` offers MySQL 8.4.3 / 8.0.40, MariaDB 12.0.2 / 11.4.8, Redis 8.2.1 and NO PostgreSQL; a Blank-PHP site created over `rex` downloaded and RAN mysql-8.4.3, nginx-1.30.4, php-fpm 8.3, Caddy, Mailpit on the 13.6 host — `rex status` all running, the site answered HTTPS 200 with the rexenv starter title; the Xdebug toggle started the 8.3 debug pool from `xdebug-8.3-3.4.5`. A public share from the Tunnels page ran on **cloudflared 2025.4.0** and the trycloudflare URL answered 200 with the site's title from the host Mac (later the same day). With the CLT installed on the VM (later the same day, 150 s), the bottle bundles relinked and ran on 13.6: an **Apache** site served 200 with `Server: Apache/2.4.65 (Unix)`, **MariaDB 12.0.2** started from the Databases socket call. **Redis 8.2.1 did NOT start** — a real defect, not a tier one: the session's `LANG=C.UTF-8` is a locale macOS 13 lacks and Redis exits on it (ledger #714, fixed by spawning with `LC_ALL=C` — the fixed build's Redis reached "Ready to accept connections" on the same VM minutes later). Without CLT the bundles refuse with the `xcode-select --install` sentence, as on 15. Original text: Settings → PHP shows the standard minors;
      the Services/Databases rows, once started, report MySQL **8.4.3**, Redis **8.2.1**,
      MariaDB **12.0.2**; Apache **2.4.65** on a site that picks it; a share opens on
      cloudflared **2025.4.0** (Tunnels page shows the version). On 14 only cloudflared moves.
- [x] **PostgreSQL on 13** ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less, build 2d1e1bc4+activation/gate fixes, driven over ssh + `rex`) — the CLI legs: `rex site create … --db postgres` → "PostgreSQL: Needs macOS 14 or later — this Mac runs macOS 13.6."; `rex db versions --set postgres 16.4.0` → the same sentence (the FIRST build answered the platform sentence there — fixed, ledger #710). The GUI rows, seen the same day once Screen Recording + Accessibility were granted to `sshd-keygen-wrapper`: the Databases page lists PostgreSQL as a muted last row reading "Needs macOS 14 or later — this Mac runs macOS 13.6." under MySQL 8.4.3 / MariaDB 12.0.2 / Redis 8.2.1; New Site → Configure shows "PostgreSQL: Needs macOS 14 or later — this Mac runs macOS 13.6." under the engine picker and offers no PostgreSQL option. **Two UI defects found there and fixed the same day:** the dialog ALSO showed the PDO note ("the PHP 8.3 build has no working pdo_pgsql") because one boolean carried both reasons; and the PHP list appended 8.0 after 8.5 instead of in order. Original text: Databases page lists it as a muted row reading "Needs macOS 14 or
      later — this Mac runs macOS 13.x."; New Site → Laravel/Blank shows the same sentence under
      the engine picker and offers no PostgreSQL option; `rex db versions --set postgres …`
      and the MCP create tool refuse with the same words.
- [x] **PHP 8.0 on 13** ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less, build 2d1e1bc4+activation/gate fixes, driven over ssh + `rex`) — `rex php list` carries the 8.0 row with `unavailableReason` set and `installed: false`; `rex php install 8.0` → "PHP 8.0: Needs macOS 14 or later — this Mac runs macOS 13.6."; the GUI, seen the same day: Settings → Services lists the 8.0 row in place (between 7.4 and 8.1, after the ordering fix), dot grey, a chip "Needs macOS 14 or later — this Mac runs macOS 13.6." beside the EOL chip, Install disabled. Original text: Settings → PHP keeps the 8.0 row, dot grey, Install disabled, a chip
      with the sentence; `rex php install 8.0` refuses with it; New Site never offers 8.0.
- [ ] **Xdebug on 13**: the toggle works on 8.1–8.4 (pool banner "with Xdebug v3.4.5"); on
      8.5 the toggle is off with the "no bottle pinned yet" reason.
- [x] **WordPress and Laravel sites serve over HTTPS** exactly as the Core sections say. ✓ 23 Sep 2026 (macOS 13.6 VM, over `rex`): `legacy-wp.rex` (WordPress, one-click install on MySQL 8.4.3) and `legacy-lv.rex` (Laravel, composer + migrations on MySQL 8.4.3) both created and answered HTTPS 200 (`curl --resolve`, the CA not yet trusted on that VM); the Blank-PHP site too. No `crash.log` entry after any of it.
- [x] **In-place upgrade to macOS 15**, then relaunch: no note at Welcome; Databases →
      MySQL/MariaDB/Redis restart on the standard pins and the sites' databases are still
      there (T6's forward-upgrade proof, seen through the GUI); PostgreSQL and PHP 8.0 appear
      as ordinary rows. ✓ 23 Sep 2026 — the SAME 13.6 VM upgraded in place to **15.8 (24H23)**
      by `startosinstall` from the full Sequoia installer (the 13→15 path, app-data untouched),
      then the 0.8.7 app relaunched into the Standard tier: Databases page (screenshot) shows
      **MySQL 8.4.6** running on the datadir 8.4.3 wrote (`mysql-error.log`: "Server upgrade
      from '80403' to '80406' completed"; marker `written-on-13.6-by-8.4.3` read back,
      `wp_posts` 4, `migrations` 3), **MariaDB 12.3.2** on the 12.0.2 datadir (marker
      `written-on-13.6-by-12.0.2`; the app spawned it by the site-engine rule when a
      WordPress site was created ON MariaDB after the upgrade — that create then died at
      "downloading WordPress core" (cURL 28 after 600s, 37.0 of 37.2 MB: the VM's network,
      not the tier) and stayed setup-incomplete), **Redis 8.8.0** started from the Services
      toggle and loaded the old RDB;
      **PostgreSQL 18.6.0** and **Redis** are ordinary rows ("Start … from Services"), and
      `rex php list` carries **8.0** with `unavailableReason: null` (the Settings row renders
      that field — its 13 shape was seen on the same VM before the upgrade). All five sites
      `serving`, `https://legacy-wp.rex` 200 in Safari. **Not seen**: the Welcome note — an
      upgraded install never shows Welcome again (`legacy_notice` is `None` on Standard, L0).
      **Found there**: the app said `DNS DOWN` for two hours while the agent answered every
      query (#442 leg 4, fixed in the next build) — and the edge-blocked/unblocked toast pair
      fires during the app's OWN reload at site create (`docs/TODO.md`).

## Windows — what this checklist means on that OS

Run everything above on Windows too, EXCEPT what this section changes or removes. The
macOS file is written against a Mac's mechanisms (a `.dmg`, Gatekeeper, `/etc/resolver`,
the login keychain, a root LaunchDaemon, the Dock); Windows reaches the same user-visible
promises by different machinery, and a step that names a Mac mechanism is not a step a
Windows tester can pass or fail.

Environment: Windows ____ (11 x64 supported · 10 22H2 best-effort — D6) · rexenv version ____

### Typed addresses in Firefox on Windows (ledger #706 — proven on macOS only)
- [ ] Settings → Firefox → **Open typed addresses in Firefox** → Enable; restart Firefox; type
      `acme.rex` with no slash → the site opens. The `user.js` under
      `%APPDATA%\Mozilla\Firefox\Profiles\…` carries the line. The Settings hint and the
      site-page tooltip name **Chrome and Edge**, never Safari.

### Two Windows-only rows, both found on a real machine 20 Sep 2026
- [x] **PostgreSQL starts — including with UAC OFF.** ✓ 20 Sep 2026 (Win11 VM, `EnableLUA=0`,
      0.8.5): Services → PostgreSQL → **7 of 7 running**, `:15432` listening, `health.log` empty
      a minute later (#698 launches it through `pg_ctl`, #701 stops the watchdog racing it).
      The check itself: Services → start PostgreSQL: it
      reaches Running and `netstat -ano | findstr :15432` shows it LISTENING. Then the case
      that broke it: on a machine with UAC disabled (`EnableLUA=0`, where EVERY process
      carries the Administrators token) it must still start, because rexenv launches it
      through `pg_ctl` (#698). **Tell:** "PostgreSQL did not start within 15s", then three
      watchdog restarts and `gave-up`, with `postgres-stdout.log` saying "Execution of
      PostgreSQL by a user with administrative permissions is not permitted" — that is the
      bug this row exists for. Stop all afterwards: the port must be FREE (a hard kill left
      backend processes holding it, which is why shutdown goes through `pg_ctl stop`).
- [x] **The Database Browser renders.** ✓ 20 Sep 2026 (Win11 VM, 0.8.5): Databases → Browse on
      MySQL shows Adminer 6.0.2 INSIDE the app, auto-logged-in as `root@localhost`, listing
      `wp_lm_rex`. It took three fixes that all had to be right — the frame's URL (#703), the
      app's own `frame-src` (#702) and Adminer's replayed `frame-ancestors` (#699).
      The check itself: Databases → Browse on MySQL: Adminer appears INSIDE
      the app (table list, not an empty white/grey panel) and its buttons work. **Tell:** a
      blank frame with the URL bar above it filled in — the CSP refused the app's origin
      (#699), and nothing in any log says so.
- [x] ✓ 21 Sep 2026 (Win11 VM, the fixed build, a 200 ms window watcher): Start all with
      PostgreSQL, a share started and stopped, a WordPress site created and deleted, Tunnels,
      Adminer on PostgreSQL — no rexenv console window at all (#705).
      **No console window, ever.** Start all (PostgreSQL included), open Tunnels, share a site,
      Browse a database, create a WordPress site: NO terminal / console window appears at any
      point, and none is left on the desktop afterwards (#705). **Tell:** an empty Windows
      Terminal titled `…\pg_ctl.exe` or `…\php.exe` — found 21 Sep 2026 on the VM with 0.8.4
      and 0.8.5. Do NOT close a pg_ctl one to "tidy up": PostgreSQL inherited that console and
      closing it stops the database.


**Not in Windows v1 at all (D4, refused in core with an honest message).** Skip every
section and leg about them; a refusal that NAMES the reason is the pass:
- **Apache** (would mean Apache Lounge — a third-party trust decision) — skip
  "Apache override"; instead confirm the New Site dialog does not OFFER Apache.
- **FrankenPHP** — skip "FrankenPHP × the PHP version"; same check, the picker must not
  offer it.
- **Xdebug** — the per-site toggle is absent, and the refusal says "Xdebug isn't part of
  rexenv on Windows yet — its builds have to match each PHP version's compiler exactly".
- **Redis, MariaDB** — absent from Databases, Services, the port list and the download
  plan. Their absence IS the check.

### Install & first launch — replaces the `.dmg` section
The installer exists since 19 Sep 2026 (`pnpm release:win` → `rexenv_<X.Y.Z>_x64-setup.exe`,
NSIS, per-user, UNSIGNED by ruling — D5: open source, no income, no spend, the same answer
macOS got). **First run 19 Sep 2026** on a clean Windows 11 Pro 24H2 (26100.4349) VM — UTM on
the Mac, so an ARM guest running the x64 build under emulation (D6; timings there are not a
PC's) — with `rexenv_0.7.0_x64-setup.exe` built at `276fc7cb`; the ticks below are that run's,
driven from a screenshot-guarded click helper over SSH. It found two bugs the Dell could not
(#691, #692). **Run 2, the same day**, installed the guard build (`46ccefed`) OVER that copy and
ran the rest: second-hash SmartScreen, the Finish-page start that now hops and starts services,
`rex` on the Path, and the two uninstall halves.
- [x] Edge, 19 Sep 2026 (no Chrome on the VM — Chrome's shelf wording is still unmeasured): ZoneId=3,
  sha256 matched the Dell's build. SmartScreen DID appear, verbatim in `docs/INSTALL.md`: "Windows
  protected your PC" with **Run anyway behind More info**, then straight into the NSIS wizard — no
  "Open File - Security Warning" behind it. **Second build (run 2): the same two SmartScreen
  screens again, verbatim** — a new hash is a new stranger. ~~**Download through a browser** (Edge AND Chrome), so the file carries the Mark of the~~
  Web. Record each browser's own download warning verbatim. **Do not write SmartScreen steps
  from memory.** What IS measured, and lives in `docs/INSTALL.md`: the "Open File - Security
  Warning" dialog, verbatim, with **Run on the first screen** and no "More info" to find.
  What is NOT: SmartScreen's own "Windows protected your PC" never appeared during that
  measurement, and the reason is why it belongs here — that machine has
  `EnableSmartScreen = 0` by policy. A clean Windows 11 install does not, so this is the run
  that answers it: record the dialog verbatim, where "Run anyway" sits (first screen or behind
  "More info"), and whether a second build (a new hash) repeats all of it.
- [x] 19 Sep 2026: no UAC at any wizard page; `%LOCALAPPDATA%\rexenv` = `rex.exe` (1,009,152 B),
  `rexenv.exe`, `uninstall.exe`; `HKCU\…\Uninstall\rexenv` with `DisplayVersion 0.7.0`,
  `InstallLocation` quoted, `Publisher rexenv`; Start Menu `rexenv.lnk`; desktop shortcut from the
  Finish page. **The install asks for NO admin.** Per-user (`installMode: currentUser`): it lands in
  `%LOCALAPPDATA%\rexenv` — `rexenv.exe`, `rex.exe`, `uninstall.exe`, nothing else — and the
  uninstall entry is under **HKCU**, so Apps & Features lists rexenv with `DisplayVersion`
  equal to the release. **Tell:** a UAC prompt during install, or an entry under HKLM — the
  installer was built per-machine, which is the mode D5 refused.
- [ ] **WebView2.** On a machine WITHOUT the WebView2 runtime (a clean VM may have it; check
  Apps & Features first), the installer's `downloadBootstrapper` fetches it. Record whether
  that step asked for admin: nothing has measured it, because every machine so far already
  had WebView2 (`docs/TODO.md` W11).
- [x] 19 Sep 2026: started by the Finish page and again from Explorer — window up, onboarding
  "Welcome", no console. The app starts from the Start Menu entry and shows its window; no console window
  appears behind it (a `windows_subsystem` regression shows as a black console).
- [x] Run 2: the Finish-page copy's parent is a fresh `explorer.exe` (the hop), `rex --version`
  reads `46ccefedb4`, Stop all → Start all from the tray: 5/5 running. **The copy the Finish page starts can start services.** Before #692 it could NOT: "Run
  rexenv" left it in a job with limits `0x0`, and Start all failed on `mysqld` with the #600
  access-denied wording (19 Sep 2026); the same copy from Explorer started all five. With the
  guard: the Finish-page copy hops through Explorer once (a brief flash, one window) and Start
  all works. **Tell:** the access-denied toast, or two rexenv windows.
- [x] Run 2: Install → no prompt; user `Path` gains `%LOCALAPPDATA%\rexenv\bin`, `rex.exe` copied
  there (1,009,152 B), a shell with the user Path answers `rex status`; the card then reads
  "Installed — … Reinstall". **`rex` on the PATH** — Settings → General → Command-line tool offers Install; after it,
  a NEW terminal answers `rex status`. The copy is `%LOCALAPPDATA%\rexenv\bin\rex.exe`.

### In-app self-update — replaces the macOS section (RUN 19 Sep 2026, one row still open)
The swap and the relaunch are measured on fixtures on the Dell
(`windows_app_bundle_swap_check`, `windows_app_relaunch_check` — `docs/TESTING.md`); what no
fixture can do is the real install directory, the real quit gate and the real registry entry.
- [x] 19 Sep 2026, run 3: a throwaway 0.7.9 build installed, 0.8.0 published with its Windows
  descriptor (serial 1): About → Check now → "rexenv 0.8.0 · 13.2 MB has been published", the
  Settings badge reads 0.8.0. Two things it took to get there, both real: (1) this VM's database
  still held the macOS descriptor (serial 5) from run 1's pre-per-OS build, so the Windows
  document (serial 1) was refused as a replay — a state no shipped Windows user can be in, cleared
  by hand here; (2) the offer came with a REFUSAL, "belongs to another account … sudo chown" —
  the install directory is owned by `BUILTIN\Administrators` for an admin account, fixed in
  ledger #693 (ships in 0.8.1). With an older build installed and a newer release published (its `rexenv_<X.Y.Z>_x64.zip`
  attached and `app-manifest-windows.json` signed on `rexenv/runtimes`), Settings → About
  offers the update; the consent sentence is the Windows one (no "rexenv.app", no
  Applications folder).
- [x] **The app says it is about to close, and waits.** ✓ 20 Sep 2026 (Win11 VM, a real
      0.8.4 → 0.8.5 in-app update): the swap landed and the window stayed; the dialog said
      "rexenv 0.8.5 is installed — rexenv will now close and open again on 0.8.5 …" with ONE
      button; nothing happened until OK, and OK quit and reopened it on 0.8.5. Apps & Features
      read 0.8.5 afterwards (#696's first proof).
      The check itself: after Install finishes, a dialog
      names the new version and says rexenv will close and open again, what keeps running,
      what closes with it, and what dismissing means. It has ONE button (OK) — no Cancel.
      Nothing happens until you press it; pressing it quits and the app reopens on the new
      version. **Tell:** the window vanishing on its own the moment the install finishes
      (#700, fixed 20 Sep 2026), or a dialog whose OK does nothing.
- [x] **Run 4, 19 Sep 2026 — the whole path, on the published 0.8.2.** A 0.7.9 carrying the swap
  fix (#695) installed; About → Check now offered "rexenv 0.8.2 · 13.2 MB has been published" with
  the WINDOWS consent sentence ("replaces rexenv's program files (your data folder is not
  touched)", no Apple re-prompt line); Install swapped the three program files in place, the app
  quit and **reopened by itself on 0.8.2**, the previous bundle was swept on that launch, and
  `rex --version` read `0.8.2 (e457970201)`. The log names each step. **The one miss:** Apps &
  Features still read 0.7.9. **Run 5 (0.8.2 → 0.8.3, same day) named the cause**: the entry moved
  to 0.8.2 — the OLD version, written fresh — so a version read off a just-renamed path answers
  for the file that used to be there, and the old code wrote that over itself and logged nothing.
  Fixed in 0.8.3 (#696: read the STAGED executable, before anything moves); the first update from
  a 0.8.3 install is what proves it.
- [x] ✓ 21 Sep 2026, run 6 (Win11 VM, the published 0.8.4 → the published 0.8.5, a live share on
  `lm.rex`): Install → progress bar → "rexenv 0.8.5 is installed" with one OK → OK raised
  **"Stop sharing? Quitting stops 1 public share — its link goes dead immediately."** [Quit]
  [Keep sharing] → Quit → rexenv reopened by itself, ONE window, toast "rexenv is now 0.8.5
  (updated from 0.8.4)"; the share was gone. Press Install: the archive downloads into the ONE download hub, the app quits through
  the quit gate (a live share still asks), and rexenv **reopens on its own** on the new build —
  the relauncher waited for the old process, not merely for a timer. **Tell:** two rexenv
  windows, or none.
- [x] ✓ run 6: `rexenv.exe` 0.8.5, no `.rexenv-update-*` left (swept at the relaunch),
  `uninstall.exe` still there (dated the 0.8.4 install), `DisplayVersion` 0.8.5, `rexenv.db` in
  place. One stray: `rexenv-0.8.3.bak` (38 MB, 19 Sep) from a pre-#695 swap — nothing sweeps it
  (docs/TODO.md W12). Afterwards: `%LOCALAPPDATA%\rexenv` holds the new `rexenv.exe`; the previous build sits
  beside it in `.rexenv-update-<pid>\previous` — the three FILES, not a directory: the data
  tree under `%LOCALAPPDATA%\rexenv\rexenv\data` never moves (#695) — **until the next launch sweeps it** (it cannot
  be deleted while the old process runs — measured); `uninstall.exe` is still there (carried
  across — the installer wrote it, the build did not); and **Apps & Features shows the NEW
  version** (`DisplayVersion` rewritten; the entry exists, so it is rewritten — the fixture
  runs could not reach this line).
- [x] ✓ run 6: the agent was restarted at the swap (new pid, its exe reads 0.8.5), the CA
  thumbprint is unchanged (`1A6A20BE…` before and after), `lm.rex` → 127.0.0.1 and HTTPS 200.
  The updated copy still resolves `.rex` and still serves HTTPS: the DNS agent was
  re-launched onto the new binary and the CA did not change.
- [ ] ◐ 21 Sep 2026, run 6 (published 0.8.4 → 0.8.5 in-app, then the carried-across
  `uninstall.exe /S`, WITHOUT the in-app "Remove system changes" first): it removed `rex.exe`
  (a swapped file), itself, the HKCU entry and both shortcuts, and kept the data — but
  **`rexenv.exe` stayed**: the `\rexenv\dns-agent` task, which only the in-app step removes, re-ran
  the agent from it within the minute, so the file was locked when the uninstaller got to it,
  and the agent went on answering `:53` from an uninstalled app (docs/TODO.md W12). The row as
  written — in-app step first — is still to run; it needs the Root-store DELETE dialog clicked
  and a re-onboarding after. Apps & Features → Uninstall on the UPDATED copy works: the carried-across uninstaller
  removes the directory the swap put in place.

### Where rexenv lives — replaces "The menu bar (no dock icon)"
- [x] 19 Sep 2026: right-click menu measured — "Stopped / Start all / Stop all / No sites yet /
  All sites… / Services / Databases / Mail / Tunnels / MCP server / About rexenv / Open rexenv /
  Quit rexenv" ("All running · 5 services" once started). Left-click: not driven. rexenv is a **taskbar tray** app. **Left click opens the WINDOW; right click opens
  the MENU** (ledger #624, the owner's Q2 ruling) — the opposite of macOS, deliberately.
- [x] 19 Sep 2026: the colour icon, in the overflow flyout (Windows 11 hides new tray icons
  there by default). The tray icon is the **colour** icon, not a template glyph: it must be legible on a
  dark taskbar. **Tell:** a black square — macOS's `icon_as_template` leaking to Windows.
- [x] 19 Sep 2026: window closed, `rex status` answered with 5 running. Closing the window leaves the app alive in the tray: `rex status` still answers and
  an MCP client keeps working.
- [x] ✓ 21 Sep 2026 (Win11 VM, 0.8.5): window closed each time, then overflow → right-click →
  Services, Databases, Mail, Tunnels, All sites… — each opened the window ON that screen.
  Every menu item that names a screen brings the window up on it, including from a
  window that was closed.

### First-run setup prompts — ONE elevated step, not three
macOS asks three times (resolver, keychain, ports 80/443). Windows asks twice, and one of
them is Windows' own dialog:
- [x] 19 Sep 2026: rexenv's own dialog first ("rexenv wants to add a DNS resolver so .rex sites
  open on this PC. Windows will ask for your permission next."), then ONE UAC, then
  `Get-DnsClientNrptRule` = `.rex → 127.0.0.1`. **One elevated (UAC) step** — the `.rex` NRPT rule
  (`Add-DnsClientNrptRule -Namespace .rex -NameServers 127.0.0.1`). `.rex` only on a fresh
  machine; other TLDs get theirs on first use.
- [x] 19 Sep 2026: verbatim — "You are about to install a certificate from a certification
  authority (CA) claiming to represent: rexenv Local CA … Thumbprint (sha1): 1A6A20BE …
  Do you want to install this certificate? [Yes] [No]"; after Yes, `Cert:\CurrentUser\Root`
  holds `CN=rexenv Local CA` with that thumbprint, NotAfter 2036. The No path: not driven.
  **Windows' own certificate dialog** — "Security Warning: You are about to install a
  certificate from a certification authority…" for rexenv's local CA, into **this user's**
  Root store (never LocalMachine — ledger #613). A **No** reads as a cancel, and setup
  offers the step again rather than continuing as if it succeeded.
- [x] 19 Sep 2026 (VM, unelevated user, Explorer-started copy): `rex start` → Caddy on
  `127.0.0.1:443` and `:80`, no UAC, no dialog. **NO third prompt for ports 80/443.** Measured on the Dell under the desktop user's
  unelevated token: the pinned `caddy.exe` binds `:443` and `:80` with no elevation. **Tell:**
  a UAC prompt for the edge — something reintroduced a privileged bind.
- [x] 19 Sep 2026: none; `netstat` shows `127.0.0.1:443`, `127.0.0.1:80`, `127.0.0.1:18088`.
  **No Windows Defender Firewall alert.** The edge binds `127.0.0.1` only (owner's
  ruling 14 Sep 2026); an all-interfaces bind raised "Windows Security Alert" on the Dell.
  **Tell:** that alert appearing — `default_bind` was lost, and sites would be reachable
  from the LAN.

### DNS — the agent on :53, not :15353
- [x] 19 Sep 2026: exactly that line; `rex doctor` ✓ on DNS, resolvers, TLDs, edge ("answering
  as rexenv on :443"), ports — its one finding is `rex not on PATH`, as designed before the
  Settings install. `rex status` reads `DNS answering (agent, udp 53) · resolver installed · CA trusted`.
  The port is the platform's `RESOLVER_PORT`, **53 on Windows** (NRPT has no port field —
  D2), so a line saying 15353 is macOS's number leaking.
- [x] 19 Sep 2026: after Quit from the tray only `rexenv.exe --dns-agent` remains (task
  `\rexenv\dns-agent`, Running, Interactive only), `probe.rex → 127.0.0.1`, and all 16
  service processes kept running; `rex status` then says "rexenv isn't running — open the
  app first". **DNS outlives the app**: quit rexenv, and `Resolve-DnsName probe.rex -Server 127.0.0.1`
  still answers `127.0.0.1`. The agent is a **scheduled task**, `\rexenv\dns-agent`, run at
  logon — not a LaunchAgent.
- [x] 19 Sep 2026: zero `.rex` lines in `hosts`. **The `hosts` file is never touched.** rexenv's rule is never to overwrite a file
  somebody else owns, and `hosts` is shared by every tool on the machine (D2 refuses the
  fallback). **Tell:** any `.rex` entry appearing in
  `C:\Windows\System32\drivers\etc\hosts`.

### Where things live on disk
- [x] 19 Sep 2026: `bin\` (adminer 5.4.2, caddy 2.11.4, mailpit 1.30.3, mysql 8.4.6, nginx 1.30.4,
  php 8.3.32, each with `.pinned-digest`), `ca\`, `config\`, `logs\`, `rexenv.db`. App data: `%LOCALAPPDATA%\rexenv\rexenv\data` (with `config\`, `logs\`, `bin\`
  under it).
- [x] The `rex` CLI is a **copy** on the user's `Path` at `%LOCALAPPDATA%\rexenv\bin`
  (ledger #634) — beside the data tree, not inside it — not a symlink. ✓ 16 Sep 2026 on the Dell
  (#634's L1: the folder held `rex.exe` with the sidecar's SHA-256 — byte-identical, so a copy;
  the user `Path` gained exactly that one entry, `REG_SZ` kept; `where.exe rex` and a fresh
  desktop process's `Get-Command rex` both found it) and 19 Sep on the clean VM (run 2,
  "`rex` on the Path").

### Updating
The Windows updater shipped 19 Sep 2026 (ledger #690) — its rows are the "In-app self-update"
section above. What the first installed copy (built at `276fc7cb`, BEFORE per-OS descriptors)
showed is worth one line: its log read the macOS descriptor and said "this Mac's macOS version
could not be read" — a build from `a0d4868f` on fetches `app-manifest-windows.json`, and that
sentence is now OS-neutral.

### Clean uninstall — replaces the macOS one
- [x] Run 2: it lives under Settings → **Services** → Uninstall; the confirm reads "This stops
  all services, deletes every rexenv NRPT rule (.rex and any other TLDs), and untrusts the
  local CA (Windows will also ask for approval). Your sites and databases are kept."; then one
  UAC and Windows' own "Root Certificate Store — Do you want to DELETE the following
  certificate from the Root Store?" (subject, serial, both thumbprints). Settings → Uninstall → **Remove rexenv's system changes**; confirm.
- [x] Run 2: NRPT rules 0, CurrentUser Root 0 rexenv certs, task gone (`schtasks` "cannot find
  the path"), services 0, `Path` entry and `bin\rex.exe` gone, data kept. **`.rex` still
  resolved while the app was open** — the in-process fallback resolver holds `127.0.0.1:53`
  until Quit, by design; after Quit it does not. After: `Resolve-DnsName foo.rex -Server 127.0.0.1` no longer answers, the NRPT rule
  for `.rex` is gone (`Get-DnsClientNrptRule`), the `\rexenv\dns-agent` task is gone, the
  CA is out of the CurrentUser Root store, the `rex` copy and its `Path` entry are gone,
  and no rexenv service is running.
- [x] Run 2: `rexenv.db` still there. Site files remain under `%LOCALAPPDATA%\rexenv\rexenv\data` (not deleted).
- [x] Run 2: **Apps & Features' `uninstall.exe`** — one page ("Uninstalling from: …\rexenv\", a
  "Delete the application data" box, unticked by default) → Uninstall → Close, no UAC:
  `rexenv.exe`, the HKCU entry, the Start Menu and desktop shortcuts are gone; the data tree
  and an EMPTY `%LOCALAPPDATA%\rexenv\bin` stay (the `rex` copy was removed by the step above;
  the directory is nobody's to delete — docs/TODO.md W12 lists it).
- [x] Run 2, installing OVER a running copy: only the DNS agent was alive (the app had quit),
  and the NSIS installer said **"rexenv is running! Click OK to kill it"** — the agent IS
  `rexenv.exe`. OK killed it; its logon task restarted it after the install, on the new binary.
- [x] ✓ 21 Sep 2026 (Dell, Win10 22H2, installer 7388104…): the same over-a-running-copy install
  now says **"rexenv is still running: the app, or the small background resolver…"** with the
  "starts again on its own" line (`src-tauri/nsis/English.nsh`). OK → the app closed, Setup
  completed, Finish reopened the app, and the agent was back beside it.

---
Result: ____ / all pass.  Issues found: ________________________________________
