# rexenv — release smoke test, all three OSes (the main body is the clean Mac · the Windows and Linux sections at the end say what changes on each)

Run this end-to-end on a **clean Mac or a fresh macOS user account** (no cached
rexenv binaries) from the distributed **universal .dmg**, after the INSTALL.md
first-launch step. Check every box; note anything that isn't a clean pass.

Environment: macOS ____  ·  Intel / Apple Silicon ____  ·  rexenv version ____

**Where the main body has run** (reconciled 28 Sep 2026 — each ✓ names its host and build):
- the **dev Mac** (macOS 26, the owner's machine — not clean): the MCP gates (25 Aug, 3–4 Sep),
  the menu-bar walk (31 Aug – 2 Sep), the first real in-app update 0.6.0 → 0.6.1 (7 Sep);
- the **clean UTM VM, macOS 15.6.1 arm64**, 18 Sep 2026 — build `39610cc7`, then the in-app
  update to 0.7.2, then `8437afd9` without the Command Line Tools; the widest pass, MCP driven as
  a raw JSON-RPC client (so the rows that exist for a MODEL's half stay open);
- the **13.6 VM** (T7, the legacy tier) and the same VM upgraded in place to **15.8**;
- the **clean 15.8 VM**, 23 Sep 2026 — `rexenv_0.8.7_universal.dmg` `cb17756a…` from `5aab0be0`,
  every rexenv artefact wiped first (root daemon, `/etc/resolver`, CA trust, DNS agent, app-data,
  Sites, app), driven over ssh + JXA clicks with screenshots. Passed as the rows say: Gatekeeper's
  "Not Opened" (no "damaged"), Welcome with no :443 notice and no legacy note, six components
  landed while the wizard was left early, branded resolver + keychain + edge prompts, WordPress
  from `Stopped 0/5` → 200, Laravel, Blank PHP + starter, Laravel on PostgreSQL, reset mail
  captured, stop ONE site and start again on the same cert, window close → `rex status` alive →
  tray Open, Settings, the in-app uninstall. It found #715 (the watchdog resurrecting a removed
  resolver); the fixed dmg `8e3f78db…` re-ran the uninstall row green on 24 Sep;
- the **15.8 VM as an upgraded install**, 27 Sep 2026 — the public 0.8.7 → the published 0.8.8
  (`docs/PUBLISH-TESTING.md` §A 0.8.8).

Until 28 Sep many ticked rows here carried a run's stamp glued onto the step text, several
ticks hid a leg the same line said was "not driven", and some rows described states a later
fix had closed. Reconciled like the Windows and Linux sections: one row per step, open legs
split into their own rows, every open row ending with **Open:** and what it waits for ("no run
recorded" where nothing was found).

**Still open on macOS, by section** (the rows say why):
- *Onboarding / first run:* the :443 notice with another proxy on :443 · downloads moving while
  a prompt is open (#567) · a dialog never freezing the window (#568, ◐) · a database dialog
  after a Valet/Herd takeover (#569).
- *Create flows:* WordPress "Log in as" · a PostgreSQL site's Database tab and size · the git
  flows: private `git@` repo and the `https://` refusal, non-Laravel refusal, Retry after
  relaunch, front-end assets built / skipped, WordPress and Bedrock from a repo, any PHP repo
  (◐), the Repository tab on linked checkouts, delete of a cloned site · the typed-domain delete
  confirm · Site Settings through the tab itself (◐).
- *PHP:* 7.4 serving on macOS · the PHP update button's apply and REVERT (#351) and its eight
  follow-on rows · the ini revert (◐, `1K` did not kill the pool) · the offline and positive
  `serving` rows of PHP versions.
- *Adminer:* the documented revert (FAILED 18 Sep — its TODO row).
- *WordPress Manager / Git assets / Mail / Database / Sites:* zip upload, focus refresh,
  WP_DEBUG, #375, #374, a premium update (#370) · Build zip 4 and 6 · an HTML mail's link (#697)
  · Quick links → Database · the setup-incomplete count and the idle filter tab.
- *Tunnels:* the stopped-share strip and stop page, sharing a stopped site, the live filter
  (#371), an override site's content through the tunnel (◐), the refusals (stopped server, no
  recorded backend port, "never busy"), the orphan WARN.
- *Menu bar / links / Settings / terminal:* the dock tile going away (◐), the 30 s hold (#437),
  routes from a closed window, services up after a real login, the agent after a real login
  (◐), a login launch with setup
  unfinished, `rex status` with the app closed · all four "which app opens a link" rows (needs
  two browsers) · the Sites-folder refusals, the WP-CLI pin card and its failure line.
- *AI agents (MCP):* 3, 7, 8 (◐), 9, 11, 13, 14 (◐), 15 (◐), 18 (◐), 23 (◐), 26 (◐), 27–31,
  32 (◐), 33 (◐), 37 (◐), 38–42, 43 (◐, Cursor), 44 (◐), 45.
- *Self-update:* the card when current, offline (against #540), the Homebrew consent sentence,
  permissions per macOS major.
- *Robustness / PHP defaults:* sleep/wake, a silent cold-cache login (#175), DNS after Quit
  (#46, ◐), Firefox fresh profile (#154) and typed `name.rex` (#706, ◐) · the ini placeholders
  and a 200 MB upload (#694).
- *Legacy tiers:* on 13 the MCP create refusing PostgreSQL, New Site never offering 8.0, Xdebug
  on 8.1/8.2/8.4/8.5 (◐) · the whole list on a clean macOS 14 VM.

**This file is half the release gate.** The other half is `docs/PUBLISH-TESTING.md`
(§A0 artefact integrity, §A quarantine → Gatekeeper → launch, and the install/uninstall
sections); its outstanding rows are tracked under "Release gates" in `docs/TODO.md`.
Order on release day: this file against the built dmg (and the Windows and Linux sections
against their installers), then PUBLISH-TESTING §A0/§A — publishing IS the §A sign-off.

## Install & first launch
- [x] .dmg mounts; drag rexenv → Applications works. ✓ 18 Sep 2026 (clean 15.6.1 arm64 VM,
      CLT-less, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7 dmg `cb17756a…`).
- [x] First launch via **right-click → Open** (or Privacy & Security → Open Anyway); app opens,
      no "damaged". ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) — the Open Anyway click itself.
      23 Sep (clean 15.8 VM, 0.8.7) and 27 Sep (15.8 VM upgraded install, 0.8.8,
      PUBLISH-TESTING §A): Gatekeeper's **"rexenv" Not Opened**, no "damaged"; both launched by
      clearing quarantine, so the click was proven only on 18 Sep.
- [x] Subsequent launches open with a normal double-click. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9).
- [x] **The one command** (`curl -fsSL https://rexenv.rex.bd/install.sh | bash`,
      `docs/archive/PLAN-install-scripts.md`) installs `/Applications/rexenv.app` from the `.app.tar.gz`
      after the checksum, and it OPENS with no Gatekeeper dialog: no `com.apple.quarantine`,
      `codesign --verify --deep --strict` passes, `spctl -a` still says `rejected` (ad-hoc —
      expected, and irrelevant without quarantine). A second run says "already installed" and
      changes nothing. ✓ 28 Sep 2026 (15.8 VM, 0.8.9; the script piped from a local copy because
      `rexenv.rex.bd/install.sh` was not redirecting yet; the VM's own bundle moved aside first and
      restored after): the window came up, no dialog in the screenshot — only macOS's two
      "Background Items Added" notifications for rexenv's login item and agent. **Tell:** "rexenv
      Not Opened", or a quarantine attribute on the installed bundle.
- [x] **The same, from `rexenv.rex.bd` on a Mac that never had rexenv** — the literal command
      through the redirect, then onboarding. ✓ 29 Sep 2026 on two fresh GitHub runners with a GUI
      session and Gatekeeper `assessments enabled` — macOS 26.6.2 arm64 and 15.7.9 Intel, neither
      with `/Applications/rexenv.app` nor its app data: installed, opened on onboarding's
      **Welcome** screen (screenshot), a rexenv window on screen and NO `CoreServicesUIAgent`
      window (the process that draws Gatekeeper's dialog), no quarantine attribute (one temporary
      run, 36469732464). The redirect is byte-identical to the tap's `install.sh`, and the literal
      command on the dev Mac stopped at the installed copy.

## App menu → About
- [x] **rexenv menu → "About rexenv" lands on Settings → About**, from whatever screen was
      open — not the native macOS panel. The Build card shows version, commit, built-at,
      platform and Tauri, and its copy button yields all five. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9).
- [x] Do it with the window **hidden** (Cmd-H first): the window comes back focused. A menu
      item that opens something out of sight reads as dead. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9).
- [x] **Cmd-C / Cmd-V / Cmd-Z still work** in a text field (the Edit menu comes from the
      default menu the About item edits, not from anything we wrote). ✓ 18 Sep 2026 (clean
      15.6.1 VM, 39610cc7 + self-update to 0.7.2).

## Onboarding — the :443 notice, and the silence that matters more
Onboarding runs BEFORE any service starts, so "is what answers :443 ours?" is
false on every clean first run. The rule is: **Foreign warns, NoAnswer says
nothing.** The silent case is the one that ships to everybody.
- [x] **Clean Mac, nothing on :443 → NO warning anywhere in onboarding**, and it never
  blocks. This is the ordinary state; reporting it would invent a problem out of normality.
  **Tell:** any foreign-proxy notice on a machine with a free port — that would have shown to
  every user alive. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM,
  0.8.7): Welcome with no :443 notice.
- [ ] **Only if you can arrange it** (start Herd, or any other proxy on :443,
  before first launch): the LAST onboarding step shows a notice, directly under
  the sentence promising rexenv "will serve it instantly" — the one claim a
  foreign proxy makes false. It **warns and does not block**: you can finish
  onboarding. Someone trying rexenv with Herd running is in a deliberate state,
  not a broken one. **Open: no run recorded** (both clean VMs had a free :443).

## Cold first run (downloads + system setup) — also exercises §2.4
- [x] On first use the app downloads its components (PHP, Nginx, MySQL, Caddy, WP-CLI…) with
      visible progress. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8
      VM, 0.8.7): six components landed.
- [x] **On a Mac WITHOUT the Xcode Command Line Tools** (`xcode-select -p` fails — a fresh VM,
  not a dev machine): all six first-run components reach `ready`, and no "install developer
  tools?" dialog appears at any point. **Tell:** every row `failed` with `otool -L failed:
  xcode-select: error…` and macOS's own CLT dialog on top of the app — that was 0.7.0–0.7.2 on
  every clean Mac, found 18 Sep 2026 on the first VM run (#676). A dev Mac cannot see this: the
  tools are there. ✓ 18 Sep 2026 with the fix (clean 15.6.1 VM, UTM, fresh app-data,
  `xcode-select -p` rc=2, 8437afd9): all six `READY` in under two minutes, no dialog.
- [x] **Leave the Install step early.** (Added 11 Sep 2026 — users reported it.) Click Continue
  while the components are still downloading, finish the wizard, and on "Create your first
  site" create one straight away. The footer's download indicator must count up to its total
  and disappear (or show a failed row WITH Retry). **Tell:** a row reading `queued` that never
  moves, a counter stuck short of its total, and no Retry — fixed only by quitting the app.
  That was a planned row whose resolve returned through the cache hit (#566). **Second tell**
  (18 Sep 2026, clean VM with a dead DNS relay): every row `failed`, and the collapsed
  indicator STILL reading `Downloading 1 of 6` over a full bar — the batch counted only
  successes as settled. It must read `6 downloads failed` in red, and the panel header `0/6 ·
  6 failed`. **Third tell** (same VM, same day): leave the Install step while rows are still
  failing, do the Domains step, land in the app — and the footer shows rows "downloading 0 B"
  that never change, while the log says every download gave up minutes ago. Nothing was
  listening between the two screens and the footer seeded from the Install step's stale
  cache. It must show the settled state within a second of the footer appearing.
  ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9): the second tell's path (DNS dead, Start all) →
  `6 downloads failed` in red, bar in the error state; the third tell's path → the final
  wizard step already read "6 failed", and the footer landed on `6 downloads failed` / `0/6 ·
  6 failed` with every row `failed` + Retry. ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7): the happy
  path — six components landed while the wizard was left early.
- [x] **Reach the last step with a component still downloading or failed** (Continue early; or
  pull the network for the failed case). The "Your kingdom is ready" step must SAY so — "n of m
  ready" with a spinner, or "n failed" in red with a Retry — and never "Everything's
  installed". **Tell:** the green ✓ Core components chip over failed rows in the footer: that
  was 0.7.x, seen 18 Sep 2026 on a VM whose DNS relay was dead. ✓ 18 Sep 2026 (clean 15.6.1
  VM, 8437afd9): DNS broken → "Nearly there · 2 of 6 failed · Retry"; DNS fixed, Retry → all six
  landed and the step flipped to "Your kingdom is ready" on its own.
- [ ] **Leave the admin prompt open for a minute on the Domains step** while downloads are
  still running (open the footer later, or watch the Install rows before continuing). The
  byte counts must keep moving while the dialog sits there. **Tell:** progress that
  freezes exactly while a password or keychain dialog is open and jumps on as soon as it
  is answered — the prompt waiting on a runtime worker again (#567). **Open: no run
  recorded** — #567's L0 guard proves the call sits off the runtime; the behaviour is this
  hand check.
- [x] **A dialog never freezes the window.** (Added 11 Sep 2026.) Settings → re-trust the
  local CA (or Repair a TLD's resolver): while the keychain/admin dialog is open, the app
  window must still scroll, switch screens and show live status. **Tell:** a beachball or
  a window that ignores clicks until the dialog is answered — a prompting command running
  on the main thread (#568). **◐ 18 Sep 2026 (clean 15.6.1 VM): INCONCLUSIVE** — with the
  keychain dialog up a sidebar click did not switch the page while the footer's live numbers
  kept moving; a second Re-trust needed no dialog (auth cached), so it could not be re-tried.
  **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9, first Re-trust after a reboot so the dialog was
  real):** the branded keychain sheet up ("rexenv — You are making changes to your
  Certificate Trust Settings", Re-trust's spinner running) → one click on the Sites sidebar
  entry → the page switched to the Sites list behind the still-open dialog, the footer's
  CPU/RAM kept updating, rexenv stayed frontmost; the dialog was then cancelled.
- [ ] **Same check with a dialog that touches the database**: Import → take over a Valet/Herd
  TLD (or hand one back). While its admin prompt is open, the Sites list and status must still
  load. **Tell:** screens that spin until the prompt is answered — the database locked across
  the dialog (#569). **Open: no run recorded** (#569's L0 proves no lock is held at the
  prompt; a Valet/Herd TLD to take over is a PUBLISH-TESTING §F clean-VM setup).
- [x] Admin prompt for the `.rex` DNS resolver appears and is accepted (`/etc/resolver/rex`;
  NO `/etc/resolver/test` on a fresh machine). The dialog reads **rexenv** in bold with
  rexenv's logo on the lock, says "rexenv wants to add a DNS resolver so .rex sites open on
  this Mac." (#578), and no extra Dock icon appears while it is open (#577). **Tell:** "wants
  to make changes." with no reason — a prompt that lost its `PromptReason`. **Tell:**
  "osascript wants to make changes." over a plain lock — the branded applet failed to build
  or launch and the fallback ran; the log line `branded password prompt unavailable` names
  why. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7):
  branded, from the right place.
- [x] Keychain prompt to trust the local CA appears and is accepted. It is titled **rexenv**
  with rexenv's logo (#579). **Tell:** a dialog titled "security" — the trust went through the
  `security` CLI again instead of the in-process call. ✓ 18 Sep 2026 (clean 15.6.1 VM,
  8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7): branded.
- [x] Admin prompt for the edge to bind ports 80/443 appears and is accepted. ✓ 18 Sep 2026
  (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7): branded, raised from the
  first-site card.
- [x] **The PHP 7.4 licence texts arrive on the COLD path, not by repair.** (Added 17 Aug 2026.)
  rexenv BUILDS 7.4, so it is the distributor and its licence texts must ship beside the
  bytes; `resolve` fetches them into the same staging dir so the publish is atomic — a
  published 7.4 either carries `licenses/` or does not exist. **On the dev machine they
  arrived by REPAIR** (the cache predated the obligation, `licenses_satisfied` marked it
  stale, and the next resolve re-fetched), which exercises a different branch. On a clean Mac:
  install 7.4, then check
  `~/Library/Application Support/dev.rexenv.rexenv/bin/php-fpm-7.4.33/licenses/` —
  expect ~15 files including `PHP-3.01.txt`, and the same beside `php-7.4.33/`
  (separate artifacts, separate resolves; one says nothing about the other).
  **Tells:** an interpreter present with no `licenses/` beside it (the atomic publish
  leaked, and rexenv is distributing somebody's code without its licence — ledger
  #336); or the install failing with a licence error, which is the correct refusal but
  means the archive URL or its pin is wrong. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) — the
  first time this cold leg was exercised.

## Core: WordPress over HTTPS (the headline flow)
- [x] **New site** → WordPress → create; install completes without error. ✓ 18 Sep 2026
      (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] **With the stack STOPPED when you create it** (the first site on a clean Mac always is):
      the card's serve phase reads `stack is stopped — starting it`, the branded "rexenv wants
      to start its HTTPS server on ports 80 and 443" prompt appears from the card, and the card
      settles `created — serving at https://<name>.rex`. **Tell:** a green card whose serve
      phase says `skipped`, the site row `Stopped`, and the page only loading after you find
      Restart — that was every 0.7.x first site (18 Sep 2026). ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9): `smoke2.rex` created from `Stopped 0/5`, the prompt came from the card, both
      sites `Running`, `https://smoke2.rex` 200 with the system trust store. ✓ 23 Sep 2026
      (clean 15.8 VM, 0.8.7): from `Stopped 0/5` → 200 with the system trust store.
- [x] Site loads at **`https://<name>.rex`** with a valid lock (no cert warning). ✓ 18 Sep 2026
      (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] **WP admin** opens (`/wp-admin`). ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update
      to 0.7.2): `/wp-admin/` → 302 to login, checked over curl.
- [x] The **"Log in as"** magic link logs in. **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** `rex
      site login smoke1.rex --print` minted `https://smoke1.rex/?rexenv_login=<token>`; fetched
      with a cookie jar it answered 302 → `/wp-admin/`, and `/wp-admin/` with that session was
      200 "Dashboard ‹ smoke1 — WordPress"; the same URL with no cookie redirected to
      `wp-login.php?reauth=1`. (The button's door on the site page: the Site Settings row's
      automation note.)

## Core: Laravel (the second create flow)
- [x] **New site** → Laravel → create; the card runs `installing Laravel` (Composer streams
      package lines) then `creating database + .env`, and settles ok. ✓ 18 Sep 2026 (clean
      15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] Site loads at **`https://<name>.rex`** — the Laravel welcome page, valid lock. ✓ 18 Sep
      2026 (clean 15.6.1 VM, 8437afd9).
- [x] **`https://<name>.rex/.env` 404s.** The served root is `public/`, so the file holding the
      site's DB credentials must not be reachable. This is the check that would catch a
      docroot regression, and nothing else on this list would. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] `~/rexenv/Sites/<name>.rex/` holds the whole project (artisan, composer.json, `public/`),
      and `.env` names this site's database — not `sqlite`. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      8437afd9).
- [x] Databases screen lists that database **with the migration tables** (users, cache, jobs):
      the skeleton migrates into SQLite before `.env` is wired, so empty tables here means the
      re-run after wiring regressed. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026
      (clean 15.8 VM, 0.8.7): migrations in MySQL.
- [x] New Site → Laravel / Blank PHP show **no** "Start from blueprint" field (blueprints are
      WordPress-only); WordPress still shows it. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9).

## Core: a Blank PHP site with a starter database (`core/starter.rs`)

`starter_seed_check` proves the seed and the generated files against a real
engine; what only the packaged app can prove is the DIALOG and the page as a
browser renders it.

- [x] **New site → Blank PHP → Database: MySQL** (the default) → Create. The card shows a `db`
      and a `configure` phase; the first create on a clean Mac downloads the engine, which is
      exactly what the "None" option avoids. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23
      Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] The site opens on the **rexenv starter page**, not a `phpinfo()` dump: it names the PHP
      version and web server, and the Sample data card reads **connected** with four seeded
      rows. ✓ 18 Sep 2026 (clean 15.6.1 VM, 8437afd9) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7):
      connected.
- [x] The site folder holds `index.php` **and** `db.php`. Edit `index.php`, reload, and the
      edit is what you see. ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2).
- [x] Site → **Database** tab embeds Adminer on that database (it used to say "Blank PHP sites
      have no database"), and the row's database button is enabled. ✓ 18 Sep 2026 (clean
      15.6.1 VM, 8437afd9).
- [x] **Stop MySQL from Services, reload the page:** the card reads **not connected** and names
      the engine to start — not a PHP fatal, not a blank page. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      after #683, on a NEW seeded site — an existing site keeps its generated `index.php`): "not
      connected · start MySQL from the Services screen"; MySQL back → connected. History: the
      first pass that day showed the "pick MySQL, MariaDB or PostgreSQL" no-database card
      instead — the defect #683 fixed.
- [x] **Delete the site** → its database is gone from the Databases screen. A starter database
      rexenv created is rexenv's to remove. ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 +
      self-update to 0.7.2) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7): delete drops the database.
- [x] **New site → Blank PHP → Database: None** → the page loads with the "No database" panel
      and NO engine download happened. ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update
      to 0.7.2).

## Core: a PostgreSQL-backed site (`docs/archive/PLAN-postgres-sites.md`)

The automated tiers cover the engine, the dialect and every installed PHP binary.
What only the app can show is **which** PHP a site actually runs, and that is
where both of this feature's shipped defects lived (ledger #550, #551): the
dialog offered PostgreSQL for a minor judged by a build the machine was not
running, and deleting the first such site handed psql MySQL's flags. First passed
by hand 10 Sep 2026.

- [x] **New site → Laravel → PHP 8.1 or newer → Database: PostgreSQL** → Create. The card runs
      `migrations` and **finishes** — a hang here is the driver, not the migration (a PHP
      without `pdo_pgsql` busy-loops rather than failing). ✓ 18 Sep 2026 (clean 15.6.1 VM,
      39610cc7 + self-update to 0.7.2) · ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] Site loads at `https://<name>.rex` and Site info reads **PostgreSQL · 127.0.0.1:15432** —
      not MySQL, not 13306. ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2).
      Also 27 Sep (15.8 VM upgraded install, 0.8.8, PUBLISH-TESTING §A): `smoke-pg.rex` 200 over
      HTTPS.
- [x] `.env` in the project says `DB_CONNECTION=pgsql`, `DB_PORT=15432`, `DB_USERNAME=postgres`.
      ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2) · ✓ 23 Sep 2026 (clean
      15.8 VM, 0.8.7): `pgsql`/15432/postgres.
- [ ] Site → **Database** tab opens Adminer on that database, and the Sites row shows a real
      **DB size** rather than `—`. **Open: no run recorded.**
- [x] **Set the site's PHP to 7.4 or 8.0 in the New-site dialog instead:** PostgreSQL is
      **absent** from the Database field and a line underneath says why. Switch to WordPress:
      absent, and NO such line (a WordPress user cannot act on a PHP version). ✓ 18 Sep 2026
      (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2).
- [x] **Delete the site** → it completes, the database is gone from Adminer, and no error
      mentions `--no-defaults`. ✓ 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update to
      0.7.2).
- [x] **Blank PHP → Database: PostgreSQL** → the starter page's Sample data card reads
      **connected** with four rows, and `db.php` says `'driver' => 'pgsql'`. ✓ 18 Sep 2026
      (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2).

## Core: a Laravel site FROM a git repository (`docs/archive/PLAN-git-site-clone.md`)

Only the packaged app can prove this end to end: real event streaming into the
WKWebView, the real login-shell env (your nvm/ssh-agent), and a real remote.
`git_site_clone_check` already proves the clone/move/cleanup mechanics locally;
`git_site_provision_check` (network tier) drives the real provisioning job against real
remotes — Laravel, Blank PHP with no `composer.json`, a failing clone, Bedrock.

Every ✓ below: 18 Sep 2026 (clean 15.6.1 VM, 39610cc7 + self-update to 0.7.2). The
23 Sep clean-15.8 pass did not run this section.

- [x] **New site → Laravel → Files: From Git** → paste a real Laravel repo URL → **Fetch**.
      Within a few seconds a branch picker appears with the default marked, and the name field
      prefills from the repo. **Create stays disabled until Fetch succeeds** — try clicking it
      before fetching. ✓
- [x] Paste a URL with a typo → Fetch errors in seconds (never hangs), and the Sites list gains
      **nothing**: no half-site, no certificate, no folder. ✓
- [x] Create → the card runs `cloning the repository` (git output streams live — not frozen
      then all at once), then `creating database + .env`, `installing dependencies` (composer
      streams per package), `app key + migrations`, and settles ok. ✓
- [x] The log names where `.env` came from ("created from the repository's .env.example"), and
      `Sites/<name>.rex/.env` has `DB_CONNECTION=mysql`, this site's database,
      `APP_URL=https://<name>.rex`, and a real `APP_KEY`. ✓
- [x] **`https://<name>.rex/.env` and `/.git/config` both 404.** The clone plants a `.git/`
      directory in a site that a tunnel can publish — this is the check that matters most on
      this list. ✓
- [x] Databases screen lists the database **with the repo's migration tables**. ✓
- [ ] A **private** repo over `git@` clones using your own SSH agent. A private repo over
      `https://` fails at **Fetch** with a message pointing at the `git@` form — never a hang,
      never a hidden credential prompt. **◐ the `https://` half, 28 Sep 2026 (15.8 arm64 VM,
      0.8.9):** Fetch on `https://github.com/rexenv/rexenv.git` (private) came back in seconds
      with the toast "github.com asked for credentials — this is a PRIVATE repo (or the URL is
      wrong; github.com answers both the same way). rexenv never prompts for credentials: check
      the URL, use the SSH form (git@github.com:owner/repo.git), or log the git CLI in once:
      `gh auth login`" (copy button), nothing created, no prompt. **Open: the `git@` clone —
      the VM has no SSH key for a private repository; a machine whose agent holds one.**
- [x] Point it at a repo that is NOT Laravel (e.g. a plain PHP one): the clone phase fails
      naming what it found ("looks like PHP project — not laravel"), the site shows **setup
      incomplete**, and `Sites/<name>.rex/` is EMPTY with no `.rexenv-clone-*` folder left
      beside it. **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** New site → Laravel → From Git →
      `symfony/demo` → Fetch (branch `main`, both checkboxes on, the "runs the repository's own
      code" note) → Create site → the dialog's job card: "✕ creating demo.rex — failed at:
      cloning the repository · this repository looks like Symfony — not laravel. Create the
      site as php instead (the code is cloned, nothing else has been set up)."; the Sites row
      shows `setup incomplete` with the retry icon; `Sites/demo.rex/` is empty and no
      `.rexenv-clone-*` folder remains.
- [x] **Retry** that site after quitting and relaunching the app: it re-clones (the row
      remembers the repository; the job registry did not survive). **✓ 28 Sep 2026 (15.8 arm64
      VM, 0.8.9, through `rex site retry demo.rex` after the quit + relaunch):** a fresh
      `.rexenv-clone-demo.rex-<id>` staging folder appeared within 5 s, a second
      `site-provision-demo.rex-*.log` was written, the clone ran to the same refusal, and the
      staging folder was gone afterwards with the docroot still empty. (The row's retry icon
      could not be pressed by automation — the site-page note on the Site Settings row.)
- [x] The create dialog's two checkboxes both start ON. Untick **Run `php artisan migrate`**:
      the `finalize` phase label reads "generating app key" (not "app key + migrations"), the
      log says migrations were skipped, and the Databases screen shows the database with
      **no** tables. ✓
- [ ] **Front-end assets.** With the box ticked, the card runs `building front-end assets`, the
      log streams the repo's own package manager (pnpm/yarn/npm — its choice, not ours), and
      `public/build/` exists afterwards. The site's first page renders styled. **◐ the
      honest-failure half, 28 Sep 2026 (15.8 arm64 VM, 0.8.9, no Node installed):**
      `laravel/laravel` → Laravel → From Git, box ticked → after `app key + migrations` the
      phase "── building front-end assets" printed "✕ npm not found in your shell environment —
      npm ships with Node.js — install Node, then hit Re-detect: $ brew install node" and the
      job went on to "── starting to serve": the site is `serving`, `https://laravel.rex/` 200,
      no `public/build/` — the "site is still created and says so" promise. **Open: the
      build itself on a Mac with Node** (`public/build/` and a styled first page).
- [x] **A failing asset build must NOT break the site.** Point it at a repo whose build fails
      (or temporarily rename your node), then create: the job still settles **ok** with a green
      tick, an amber "Front-end assets weren't built" banner naming the reason, and the site
      loads. It must NOT show "setup incomplete" — this is the one non-fatal phase and the only
      way to check it. ✓
- [x] A repo with no `package.json` and the box ticked: the phase reports **skipped**, not
      failed. **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9, the Symfony run below — a Blank PHP site,
      same phase code):** "── building front-end assets / no package.json in this repository —
      nothing to build", the job went on to serve.
- [ ] **WordPress from a repository.** New site → WordPress → From Git → a repo holding a theme
      + `wp-content` (core gitignored). The dialog shows an amber "your code comes from the
      repository; the database is new and empty" note BEFORE Create, and still asks for the
      admin account. The card runs `installing dependencies` (skipped without composer.json),
      `downloading WordPress core`, `writing wp-config + creating database`, `installing
      WordPress`. The site loads, wp-admin logs in, and the repo's theme is there. **Open: no
      run recorded.**
- [ ] **Bedrock** — machine-verified end to end by `git_site_provision_check` case 4
      (`ONLY=bedrock`, ledger #294), so this is a spot-check of the packaged app, not the
      proof. ⚠ **Radicle is still the unverified one** (same code path, no live project —
      `docs/TODO.md` row). Clone a real Bedrock repo as WordPress:
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
      **✗ then fixed, 28 Sep 2026 (15.8 arm64 VM, 0.8.9, `roots/bedrock` through New site →
      WordPress → From Git):** Fetch → branch `master`, the amber "your code comes from the
      repository; the database is new and empty" note, MySQL, the admin fields → Install
      WordPress → `cloning`, `installing dependencies` (composer, WordPress core through
      Composer), **`downloading WordPress core` → "this layout installs WordPress core through
      Composer"** (skipped), `writing wp-config + creating database` → ".env created from the
      repository's .env.example / .env wired to this site's database and URL", `installing
      WordPress` → "Success: WordPress installed successfully." — **and then the job failed:**
      "wp core install: setting the admin password failed: wp eval-file failed … This does not
      seem to be a WordPress installation. The used path is …/bedrock.rex/web/". Exactly ONE
      WordPress (`web/wp`; `web/` holds `app index.php wp wp-config.php`), `web/wp-config.php`
      is the repository's stub (`config/application.php`), `.env` has `DB_NAME=wp_bedrock_rex`,
      `DB_HOST=127.0.0.1:13306`, `WP_HOME=https://bedrock.rex`, the literal
      `WP_SITEURL=${WP_HOME}/wp` — but the eight salts still read `generateme` (the step that
      writes them never ran) and the site did not serve. Cause and fix: ledger #737 (one
      `--path` builder resolving `web/wp`; the manager on the served root — `rex wp
      bedrock.rex` had failed the same way). **The rest of this row — salts, wp-admin, plugins
      into `web/app/plugins`, delete/re-create salts differ, Retry keeps them — waits for the
      release carrying #737.**
- [x] **Repository tab** (Stage 3). It appears on the cloned site and NOT on an ordinary one.
      It shows the branch, a clean tree, and the remote. ✓ (the tab's presence and header;
      the sub-steps below were not recorded individually on 18 Sep).
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
      Craft/Statamic) repo. The clone phase names what it found and the site serves from that
      framework's own folder (`public/`, `web/`, `pub/`), not the project root — check
      `https://<name>.rex` loads and `https://<name>.rex/composer.json` 404s. The card runs
      `installing dependencies`; a repo with no `composer.json` reports that phase **skipped**,
      not failed. No database is downloaded for it. **◐** `git_site_provision_check` case 2
      proves the no-`composer.json` skip and no database. **✓ 28 Sep 2026 (15.8 arm64 VM,
      0.8.9, `symfony/demo` as Blank PHP → From Git):** Fetch → branch `main`, the assets box,
      the "runs the repository's own code: composer install … then the repository's package
      manager" note, Database `None` → Create → the clone phase printed **"✓ Symfony — serving
      public/"**; `installing dependencies` ran and the job card then said "✕ failed at:
      installing dependencies · composer install failed…" — the repository's lock requires
      PHP ≥ 8.4.1 and the site was on the default 8.3 (composer's `platform_check.php`, in Show
      log), the site left as `setup incomplete`. **Since 0.8.11 (ledger #751) the card must say
      it before composer runs:** "this repository's composer.json requires PHP … and this site
      runs PHP 8.3 — … switch the site's PHP version to 8.4 or 8.5 (Site → Settings) and Retry"
      — ◐ owed on this row's next run. `rex php install 8.4` + `rex site php
      demosymdemo.rex 8.4` + `rex site retry` → dependencies installed, **`building front-end
      assets` → "no package.json in this repository — nothing to build"** (skipped, not
      failed), `starting to serve` → `https://demosymdemo.rex/` **200 "Symfony Demo
      application"**, `/composer.json` **404** (served from `public/`), no `public/build`, and
      `SHOW DATABASES LIKE 'php_demosymdemo%'` empty — no database made (the Sites row's
      `mysql` is the engine default, not a database). **TODO row:** the create dialog could read
      the repository's `require.php` at Fetch and say the picked PHP is too old before
      cloning.
- [x] The Repository tab also appears on a **linked** site whose folder is a git checkout
      (import one from Valet, or link `~/code/something`), and NOT on a linked Laravel site
      served from `…/public` — rexenv never searches upwards, and the tab is absent rather than
      pointed at the parent repo. **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9, the rule the tab
      renders from — `repo_site_info`: `present = <site path>/.git exists`, no walk up):**
      `rex site create linked.rex --path ~/code/symdemo` (a real `git clone`) → path
      `/Users/linkonvm/code/symdemo`, `.git` there → present; `rex site create lvcheck.rex
      --path ~/code/lvcheck/public` (a checkout served from its `public/`) → `.git` at the
      path **no**, at the parent **yes** → absent. Both adopted as-is: `lvcheck.rex` served its
      `index.php`, `linked.rex` answered 403 (a Symfony project root has no index — the folder
      was not written into). The tab itself on the site page: the Site Settings row's
      automation note.
- [x] Delete a cloned site → the folder AND its database go (it is a docroot rexenv created,
      and provisioning made the database). **✓ folder half, 28 Sep 2026 (15.8 arm64 VM,
      0.8.9):** `rex site delete demosymdemo.rex --yes` (a Blank PHP clone, so it had no
      database) → "deleted … (files removed — it had no database)", `Sites/demosymdemo.rex/`
      gone, `SHOW DATABASES LIKE 'php_demosymdemo%'` empty. The earlier `demo.rex` (a Laravel
      clone that failed before provisioning) deleted with "database + files removed". The
      database half of a clone that finished provisioning is the same delete the starter and
      PostgreSQL rows above ran.
- [ ] Delete the site → the confirm's **Delete site** button is disabled until the domain is
      typed; the copy button next to the domain fills it by paste. Then its database is gone
      from the Databases screen too. **Open: no run recorded** (the starter and PostgreSQL
      delete rows above ran the delete, not this confirm).

## Site Settings tab
- [ ] Site → **Settings** shows real content: rename sticks (Sites list updates), DB name
      matches Adminer, cert card shows issued/expires dates + SANs. **◐ 18 Sep 2026 (clean
      15.6.1 VM):** rename via `rex site rename` updated the list; the Settings tab showed
      domains/env/Xdebug cards; cert dates read via `rex site info` (395 days). **Open: the
      rename, the DB-name match and the cert card through the Settings tab itself.** **28 Sep
      2026 (15.8 VM, 0.8.9): the site page could not be reached by automation** — a System
      Events click anywhere on a Sites row, and an AXPress on the row's own "Open <site>"
      accessibility button, opened the site in Safari every time while the app stayed on the
      list; sidebar, Settings and dialog clicks work. A hand run (or a tool that sends real
      HID events) is what this row and the Database-tab / delete-confirm rows wait for; the
      23 Sep hand run reached the page.

## PHP 7.4 — the one leg no automated tier covers
**Moved here 15 Aug 2026 from the MCP section, where it was step "11b".** The
pools, the binary, the download and the generated config are all proven by
examples; **a 7.4 site answering over HTTPS end to end is not**, and it was the
only proof of that — sitting inside "M2b — the PHP matrix and mail", four HOLD
steps deep in an optional section a reviewer skips whenever MCP is off. A gate
that only runs when an unrelated feature is enabled is not a gate for this one.
- [x] **A PHP 7.4 site actually serves.** Settings → PHP versions → install
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
  **◐ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** Settings → PHP versions → 7.4 → Install (the
  row read `yes` seconds later — the build was already cached from the 23 Sep run), `rex
  site create php74.rex --type wordpress --php 7.4` → `serving`, `https://php74.rex/` 200
  and a `PHP_VERSION` probe in its docroot answered **7.4.33** over HTTPS; `rex status` shows
  `PHP-FPM 7.4` on **9774** beside 8.3 on 9783. The 7.4 row in Settings carries make-default,
  ini settings and remove — no Xdebug control (8.3's row has ini settings only). **The create
  dialog warns FIRST:** New site → WordPress → Configure → PHP version `7.4 — end of life` →
  the amber note "PHP 7.4 stopped receiving upstream security fixes in November 2022. It still
  runs — use it to work on a legacy project, not to build a new one. WordPress will show its
  own 'outdated PHP' notice and a Site Health critical on sites using it." **And WordPress
  then says it second:** the magic-login session's Site Health → Status lists the critical
  "Your site is running on an outdated version of PHP (7.4.33)" under Security. **Open:** the
  Environment card on the site page — not reachable by automation on this VM (the Site
  Settings row).
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
  **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9, the CLI door of the same manager):** `rex wp
  php74.rex plugin install hello-dolly --activate` → "installed hello-dolly (activated)";
  `rex wp php74.rex plugin list` → hello-dolly active beside the two must-use plugins; the
  site still answered 7.4.33 afterwards. No Phar or JIT failure. (WP Manager's GUI door on
  the site page: not driven — see the Site Settings row.)

## PHP update button — the apply and the REVERT (18 Aug 2026)

The key is pinned (`RELEASE_PUBKEY`, 17 Aug 2026) and a signed manifest is published
(`php_update_check` runs against the live one, ledger #351), so on a release dmg the
button renders whenever that manifest offers a newer patch for an installed minor.
Until 28 Sep this intro said the section waited on the key ceremony — it had long since
happened. `php_update_check` proves the chain up to a pool on the new patch answering on
a FIXTURE port; everything below is the production-port half (TODO "The live pool swap
is still L3").

- [ ] **Dark without a key.** On a build whose `RELEASE_PUBKEY` is empty, no row
  shows an Update button and Settings shows no update error. **Tell:** a button
  that appears and fails — that means a key was pinned without a manifest behind
  it, and the first user to press it gets a failure with no cause they can see.
  **Open: no run recorded — only a dev build with the key emptied can show it; no
  release dmg can.**
- [ ] **A real update applies.** With a manifest offering a newer patch for an
  installed minor: press Update. **Blocked 28 Sep 2026 (VM):** the 8.3 row reads "8.3.32 ·
  8.3.35 exists" with NO Update button — php.net announced 8.3.35 but `rexenv/runtimes` has
  published no 8.3.35 build, and "exists" is not a button by design. This row and the seven
  below it wait for the next published patch. Expect real download bytes in the hub, the row
  moving to the new patch, and a `restarted the 8.3 pool` style confirmation.
  Then open a site on that minor — it must still serve, and Site Health must
  report the NEW patch. **Open: no run recorded.**
- [ ] **The site's own setting is untouched.** A site pins the MINOR (`8.3`), never
  the patch, so nothing about the site should change. **Tell:** a site that
  switched version, or a config regeneration — neither should happen.
  **Open: no run recorded.**
- [ ] **THE REVERT — the leg nothing automated can prove.** Force the new pool to
  fail: with the app closed, replace the newly downloaded
  `bin/php-fpm-<newpatch>/php-fpm` with an empty file, then press Update again (or
  re-select). Expect: the apply FAILS with a message naming both patches, the
  registry goes back to the previous selection, the pool restarts onto the OLD
  patch, and **sites keep serving**. **Tells:** a stack left down; a row showing
  the new patch after a failure; the old tree gone (the revert must not delete a
  verified tree — that costs a ~100MB refetch). **Open: no run recorded** (ledger
  #351 names this as SMOKE's step).
- [ ] **Offline afterwards.** Quit, disconnect, relaunch. The selected patch still
  resolves from cache and the pool starts — a selection must be as offline-safe as
  a pin. **Open: no run recorded.**
- [ ] **Nothing running.** Stop all, then press Update on a minor with an offer.
  Expect the toast to say the version **will be used**, not that it "is now on" it
  — there is no pool to be now-on. **Tell:** "PHP 8.2 is now on 8.2.32" with the
  stack stopped; that sentence names a process that does not exist. Then Start all
  and confirm the pool comes up on the new patch. **Open: no run recorded.**
- [ ] **The tree survives a stopped stack.** After the step above, quit and relaunch
  BEFORE starting anything. The launch cache sweep must not delete the tree you just
  installed. **Tell:** Start all re-downloading ~150MB, or failing outright offline.
  **Open: no run recorded.**
- [ ] **The patch it replaced is GONE.** `du -sh ~/Library/Application\ Support/dev.rexenv.rexenv/bin`
  before and after a relaunch following an update. The superseded `php-<oldpatch>/` and
  `php-fpm-<oldpatch>/` must both be gone — ~180 MB per updated minor. **Tell:** both
  patches of a minor still present after a relaunch; that is the leak a user found at
  358 MB (ledger #358). **The opposite tell matters as much:** every minor you did NOT
  update must still have its pinned trees — if those vanished, the sweep is deleting
  floors and Start all will re-download gigabytes. **Open: no run recorded** (ledger #358's
  sweep is ✅ at L0; this is the real cache).
- [ ] **Two rows at once.** With offers on two minors, press Update on both in
  quick succession. Both buttons must stay disabled until their own apply finishes
  — the first row's button re-enabling while its download runs is the defect. A
  third press on a row already applying must be refused by name, not queued.
  **Open: no run recorded.**
- [ ] **Everything runs the patch the pool does.** After an update, on a site of
  that minor: the site terminal's `php -v`, WP-CLI (`wp cli info`), and a composer
  step must all report the NEW patch. **Tell:** any of them reporting the version
  the app was built with — that was live for a release, and only the pool agreed
  with the row. **Open: no run recorded.**

## Adminer update — and the one thing no automated check can see (18 Aug 2026)

Only runs once a manifest carrying Adminer is published — it is (the 18 Sep 2026 VM run
updated 5.4.2 → 6.0.2 through it; the pin itself has since moved to 6.1.0, 21 Sep, and
6.1.1, 27 Sep).

- [x] **The row is on the Databases screen and tells the truth.** Databases →
  below the engine table. Expect the version the console is actually serving, and
  an **Update to X** button only when a signed manifest offers one. **Tell:** an
  "exists" chip — Adminer has ONE fact (rexenv downloads its own release asset),
  so a second one would be a version rendered twice.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] **An update applies.** Press Update. Expect real download bytes in the hub
  and a toast naming what is NOW being served. Then Browse a database: the console
  opens and the version in Adminer's own footer matches the row.
  ✓ 18 Sep 2026 (same VM and build): 5.4.2 → 6.0.2.
- [x] **THE LEG NOTHING AUTOMATED CAN PROVE — the controls still APPLY.** The probe
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
  ✓ 18 Sep 2026 (same VM and build).
- [x] **Raw Adminer has no URL.** `curl -o /dev/null -w '%{http_code}\n' -k
  https://adminer.rexenv.rex/adminer.php` → **404**, and the same for
  `/.adminer.php`. **Tell:** anything but 404 means the real console is reachable
  without the wrapper — no login gate, no frame bound.
  ✓ 18 Sep 2026 (same VM and build).
- [ ] **A revert is a second press.** There is no revert button by design: the pin
  is a floor and the old tree is kept, so going back is choosing the older version
  again. Confirm the older version is still offered after an update.
  **Open: FAILED 18 Sep 2026 (VM)** — after the update the row reads only `Adminer 6.0.2`;
  5.4.2 stays on disk but is offered nowhere. Waits on the TODO row "Adminer: the
  documented revert does not exist" (offer the kept versions, or drop this row and the
  design note).

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
  **Open: the revert path has never been entered (ledger #357 ◐).** 18 Sep 2026 (VM):
  `1K` on 8.3.32 did NOT kill the pool — it stayed up, the site served, and the value
  was kept — so this row needs a value that passes `php-fpm -t` and then kills a worker;
  `1K` is not one on that build.
- [x] **A normal edit still works.** Set `memory_limit` to `512M`, Apply, and check
  a `phpinfo()` page on a site of that minor reports it — the revert must not have
  turned the ordinary path into a no-op.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).

## PHP versions — the read-only "exists" row and the serving/pinned line (17 Aug 2026)

Both shipped after 0.2.0's DMG was built; the first recorded run is the 18 Sep 2026 VM.

- [x] **The upstream row is honest, and says when it last looked.** Settings → PHP
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
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [ ] **Offline, it says so rather than implying currency.** Turn off the network and
  relaunch. Expect `Couldn't reach php.net yet, so nothing here says whether a newer
  patch exists.` — and every other part of the screen unchanged, because everything
  about the INSTALLED patch is local. **Tell:** a spinner, an error toast, or a blocked
  screen; this check gates nothing. **Open: no run recorded.**
- [x] **No row claims to be "serving" a patch it is not.** On a normal install the
  pinned patch and the running one are the same, so **no `serving …` text should appear
  on any row.** That negative is the checkable half here.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [ ] **The POSITIVE case — a row reading `8.3.31 · serving 8.3.30`.** It appears only
  when a pool is executing a patch this build does not pin, which happens on the first
  launch after a release that MOVES a pin while a pool is running — it cannot be produced
  without contrivance on a release that moves none. On the next release that does: expect
  the amber `serving` text, the pool to restart onto the new patch, and the text to
  disappear afterwards. `rex php list` carries the same fact in its NOTE column.
  **Open: no run recorded — waits on a release that moves a macOS PHP pin while a pool
  runs.**

## FrankenPHP × the PHP version — the picker and the refusal (15 Aug 2026)
FrankenPHP serves every site with **its own embedded PHP**, never the site's
php-fpm pool. Two behaviours shipped together and they are deliberately
different: at 8.x the mismatch is annotated, at 7.4 it is refused. The rendered
picker is proven in WebKit (ledger #333, `wk-checks/phppicker.js`, since 2 Sep 2026);
the rows below are the real app.
- [x] **The annotated picker.** Switch an 8.1 site to **FrankenPHP**, then open its
  Environment card. The PHP select is **disabled**, reads `8.5 — FrankenPHP's
  embedded PHP` (whatever the pin says — it comes from `frankenphp_embedded_php`,
  one backend constant, so a second copy cannot drift), and carries the sentence
  *"Fixed by FrankenPHP. Switch the web server to Nginx or Apache to choose a
  version."*
  **Tells:** the select still offers 8.1 and pretends switching works (the old
  silent-skew bug — the site is served by 8.5 while the UI says 8.1); or the card
  hardcodes a version rather than reading the backend's.
  ✓ 18 Sep 2026 (clean UTM VM, an 8.3 site): select disabled, reads "8.5 — FrankenPHP's
  embedded PHP" with the Fixed-by-FrankenPHP sentence.
- [x] **Switch it back to Nginx** → the stored version RE-APPLIES: the picker is
  live again and reads **8.1**, not 8.5. FrankenPHP never overwrote the row.
  ✓ 18 Sep 2026 (VM): picker live again at 8.3 (the site's stored version).
- [x] **⚠ The 7.4 refusal, at all THREE doors** (#326). A major mismatch is not
  skew — the removals PHP 8.0 made are the whole reason a site is pinned to 7.4,
  so "silently served by 8.5" means silently broken. Each must refuse:
  1. **Create** a site with PHP 7.4 **and** FrankenPHP selected.
  2. Take an existing **7.4 site** and switch its **server** to FrankenPHP.
  3. Take an existing **FrankenPHP site** and switch its **PHP** to 7.4.
  **Tells:** any one of the three going through (covering only the two obvious
  doors is exactly the partial-surface shape this repo keeps paying for); or a
  refusal naming a hardcoded version instead of the majors it compared.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).

## Apache override — the per-site files go on delete AND on rename
- [x] Create a site, switch it to **Apache**, then **rename** it. In
  `<app-data>/config/` and `<app-data>/logs/`, `apache-<OLD-domain>.conf` and
  `apache-<OLD-domain>-stdout.log` must be **gone**. Then **delete** the site and
  check the new names are gone too.
  **Why it is worth a step:** these outlived every delete and every rename for as
  long as the Apache override has existed. Nothing broke — it is app-data litter,
  never a user's own files — which is precisely why nobody noticed. A check that
  only ran on delete would still pass while rename leaked.
  ✓ 18 Sep 2026 (VM, after the fix — ledger #302 amended): after `rex site domain ap.rex
  ap2.rex` only `apache-ap2.rex.{conf,-error.log,-stdout.log}` remain; after delete none.
  History: the first pass that day (via `rex site domain`) found the OLD names gone on
  rename but `apache-<domain>-error.log` surviving delete — `apache::error_log_path` is now
  swept on both (72fc47df).

## WordPress Manager
- [x] Plugins tab lists plugins; install + activate a plugin works.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Themes tab lists themes; activate works.
  ✓ 18 Sep 2026 (same VM and build).
- [ ] **Upload zip** (the native file dialog is unmockable — L2 renders the card from a
  fixture, only this walk proves the picker): Plugins → **Upload zip** → Choose .zip →
  pick a real plugin zip (a premium one, or any download from wp.org) → Install. The
  card names the FILE, not the path, shows no "installing item k of N", and the plugin
  appears in the list below. Repeat once on the Themes tab. **Then the refusals, which
  are the half a happy path never sees:** pick nothing → Install stays disabled; try a
  zip that is not a plugin → the failure quotes WordPress's own words, and the list
  below still tells the truth. **Open: no run recorded.**
- [ ] **External change, no manual dance** (the only place the NATIVE focus event can
  be tested — no browser has one, so `wk-checks/focusrefresh.js` proves the wiring and
  this proves the event): with the Plugins tab OPEN, deactivate a plugin in wp-admin,
  then click back into rexenv. The row flips on its own — no tab switch, no reload.
  Repeat for a git asset: `git checkout -b smoke/x` in a terminal, click back, the
  branch chip follows. The Refresh control does the same on demand.
  **Open: no run recorded.**
- [x] Tools: run a dry-run search-replace — it reports a count, no data change.
  ✓ 18 Sep 2026 (VM): 12 replacements reported, nothing written.
- [ ] Tools: toggle WP_DEBUG. **Open: not exercised on the 18 Sep 2026 VM run; no run
  recorded.**
- [ ] **Flip the theme with the console open** (20 Aug 2026, #375 — the reload is a
  `key` on a cross-origin iframe, which no harness can observe): Databases → Browse, then
  switch rexenv between Dark and Light. Adminer must follow within one reload, both ways,
  and must KEEP the palette after clicking into a database inside the console (that click
  is the case a query parameter would have lost). Then set the app to **System** and flip
  the OS appearance: the console follows that too. **Open: no run recorded** (`adminer_check`
  proves the head Adminer emits per theme, ledger #375; not the reload).
- [ ] **Re-upload a zip of a plugin you already have** (19 Aug 2026, #374 — L2 mocks
  the backend, so the actual overwrite only happens here): Plugins → Upload zip → pick a
  zip whose plugin is already installed → Install. It must FAIL naming the folder
  ("<dir> is already installed — nothing was unpacked") and offer **Replace with the
  uploaded zip**. Press it: the plugin is replaced, the row shows the zip's version, and
  the site still loads. **Then the guard:** do the same against a plugin rexenv shows a
  `git` chip for — the confirm must name the working tree and the branch, and cancelling
  must leave the checkout untouched (`git status` in that folder proves it).
  **Open: no run recorded.**
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
  **Open: no run recorded — needs a licensed paid plugin (ledger #370 ◐).**
- [x] Tools → Maintenance: toggle **Maintenance mode** on → site shows "briefly unavailable"
  in a private window; off → normal again.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Tools → Backup & restore: **Export database** writes a `.sql` to Downloads; **Import
  database** round-trips it (make a post → export → delete the post → import → post is back).
  ✓ 18 Sep 2026 (same VM and build).

## Git assets — Build zip
Needs one git-managed plugin or theme: **Add from Git** on any plugin repo, or
**Link folder** to one of your own. Everything below is on that asset's repo
panel (the Fetch / Pull / Push row).

*Coverage note, so it reads as a boundary rather than an oversight: the rest of
the Git panel — add, link, adopt, pull, checkout, push, scripts, watchers — has
no SMOKE step today and is covered by `repo_*` examples only.*

- [x] **1. No `.distignore`, no button.** On a checkout without one, **Build zip**
  is **visible and disabled**. Hover it and read the tooltip cold, as someone who
  has never heard of the file: it must say what is missing, that a zip without it
  would include `.git` and `node_modules`, that **dist-archive would report that
  as a success**, and **where** to create the file ("at the top of this
  checkout") in `.gitignore` syntax.
  **Tells:** the button is hidden rather than disabled (hiding it teaches
  nothing, and the person who needs this is the one who has never met
  `.distignore`); or the tooltip says only "no .distignore found", which sends a
  developer to a search engine instead of to a fix.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] **2. ⚠ Build it, then OPEN the zip. A failure here is a HOLD.** Add a
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
  ✓ 18 Sep 2026 (VM, after #679): `airplane-mode.0.2.8.zip`, 16 entries, no `.git/` or
  `node_modules/`, toast with Show in Finder. History: the first pass that day FAILED as a
  HOLD — `wp dist-archive` died on `zip -i@…/Library/Application`: its include-pattern file
  lived in a TMPDIR under app-data, and the space in `Application Support` was not quoted,
  so Build zip failed on every Mac while the dev-Mac examples (space-free fixture paths)
  stayed green. The scratch dir now comes from the OS temp dir (ledger #679, 720465ae).
- [x] **3. Nothing was written into your checkout.** In the checkout itself run
  `git status`. It must be **clean** — no stray `.zip`, no build directory.
  **Tell:** anything new. `wp dist-archive`'s own default writes the archive
  *beside* the source, and for a linked asset that is your own repository; the
  tooltip promises this does not happen, so this is that promise, checked.
  ✓ 18 Sep 2026 (VM, after #679): `git status` shows only the `.distignore` the test added.
- [ ] **4. A linked folder keeps ITS name.** Link a folder whose directory name
  differs from the name you gave it in rexenv (e.g. `~/code/my-awesome-plugin`
  linked as `awesome-slug`) and build. The file is named from **your folder**
  (`my-awesome-plugin.1.2.3.zip`), not from rexenv's label, and the toast shows
  the name that was actually produced.
  **Tell:** a zip named `awesome-slug.…`. rexenv renaming someone's plugin to
  match its own label is worse than a name that differs from it — and this is
  deliberately not hidden, so it should be visible and correct rather than
  smoothed over. **Open: no run recorded.**
- [x] **5. Twice, and nothing is overwritten.** Click **Build zip** again without
  moving the first file. The second lands as `…-1.zip`, the first is untouched.
  If the plugin has no `Version:` header, the toast says so quietly and the name
  carries no version — a note, not a failure.
  **Tell:** the first file replaced, or a silent no-op.
  ✓ 18 Sep 2026 (VM, after #679): the second build landed as `airplane-mode.0.2.8-1.zip`,
  the first untouched.
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
  which teaches you to stop reading the toast that matters. **Open: no run recorded.**

## Mail (Mailpit)
- [x] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments).
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Opening the message shows its HTML/text body. ✓ 18 Sep 2026 (same VM and build).
- [ ] **A link in the HTML body opens in the browser** (WP's password-reset link is one):
      one click, the preferred browser opens it, and the preview itself does not navigate —
      the frame keeps showing the email. A `mailto:` link opens nothing and says so in a
      toast. **Tell:** a click that does nothing at all (the `sandbox=""` bug, #697, fixed
      19 Sep 2026 after shipping dead for the screen's whole life), or the email's page
      replacing the preview. **Open: no run in the real app recorded** — `maillink.js`
      clicks a real anchor in WebKit (ledger #697 ✅); the browser actually opening is this row.
- [x] **The row loses its unread dot as the preview opens** — not a second or two
      later. (The list polls every 5s; if the dot clears "eventually", the patch
      that makes it immediate has regressed.) The sidebar's mail badge drops too.
      ✓ 18 Sep 2026 (same VM and build).
- [x] **Unread** filter shows only unread mail, and the message you then OPEN
      stays in the list while you read it instead of vanishing at the next poll.
      Search + Unread together narrow: both terms apply. ✓ 18 Sep 2026 (same VM and build).
- [x] **Mark all read** empties the unread count immediately, disables itself,
      and **deletes nothing** — the captured count is unchanged. Trigger one more
      email afterwards: it is the only unread one, which is the point of it.
      ✓ 18 Sep 2026 (same VM and build).

## Mail catch-all — the two escapes no automated tier can see (4 Sep 2026)

Ledger #504/#505 prove the mechanisms; these two legs prove they hold on a REAL
site with a REAL plugin, which is the part the fixtures cannot buy.

- [x] **WordPress with an SMTP plugin.** On a WP site, install WP Mail SMTP (or
      FluentSMTP) and configure it for ANY reachable host — the point is that the
      site is genuinely trying to leave. Trigger a password reset. It lands in
      **Mail**, not at the provider. Then Settings → Services → **Catch all
      outgoing mail** OFF, trigger another, and confirm the plugin's own send is
      attempted instead (its log, or the provider's). Turn it back ON and confirm
      the third one is caught **without restarting the stack** — the toggle
      restarts the pools itself, and a leg that quits and relaunches the app
      would pass while that was broken.
      ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] **Laravel with a real MAIL_HOST.** On a Laravel site, edit `.env` by hand to
      a real provider's SMTP host and credentials. Load a page that mails (or use
      the site's terminal: `php artisan tinker --execute="Mail::raw('x', fn($m)
      => $m->to('you@example.test')->subject('smoke'));"`). It arrives in **Mail**
      — the environment beat the file. Both surfaces, because the pool and the
      CLI carry the catch separately. ✓ 18 Sep 2026 (same VM and build).
- [x] **A FrankenPHP site is caught too** (#514). Switch a WordPress site's web server
      to FrankenPHP, trigger a password reset: it lands in **Mail**. Then toggle the
      catch-all OFF → the site's Services row shows its FrankenPHP backend respawn
      (the toggle reconciles override backends, not only pools), and a reset now goes
      to PHP's default sendmail; ON again → caught, again without a stack restart.
      `frankenphp_mail_catch_check` proves the mechanism on the real binary; this leg
      proves the toggle drives it on a real site.
      ✓ 18 Sep 2026 (VM, after the FrankenPHP re-pin, #680): wp.rex on FrankenPHP → reset
      caught; catch-all OFF → the FrankenPHP backend respawned (new pid) and the reset was
      NOT caught; ON → respawned again and caught. No stack restart.
- [x] **The stated limit is TRUE, not just printed.** On that Laravel site run
      `php artisan config:cache`, then mail again. If the site was created by
      this rexenv its `.env` was already wired, so it still lands in Mailpit; a
      site whose `.env` predates the feature will NOT, which is exactly what the
      card says. Confirm the card says it (Settings → Services, catch-all ON).
      ✓ 18 Sep 2026 (same VM and build).

## Database (Adminer deep-link)
- [x] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB**
  (tables listed), no manual login.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9).
- [x] **Native confirm works** (ledger #166 leg C — the automated legs prove the panels
  are INSTALLED, only an eye can see one render): create a throwaway table, select it,
  **Drop** → a native sheet appears (not a silent no-op, which was the 2026 incident —
  wry ships no JS-dialog panels, so an unpatched webview resolves `confirm()` to false).
  **Cancel** leaves the table; repeat and **OK** drops it. While the sheet is up the
  page behind it must be inert (WebKit suspends the calling frame — `confirm()`'s
  blocking contract).
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [ ] Overview → Quick links → **Database** opens THIS site's Database tab, not the engines
  screen (8 Aug). **Open: no run recorded.**
- [x] Open a WordPress site you have NOT opened this session: the **WordPress tab and Magic
  Login are there on the first frame** — no second-late pop-in while `wp-info` resolves (8 Aug).
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).

## Stopping ONE site (v44) — the shared services must NOT go with it
- [x] **With the stack STOPPED (Stop all), hover a site row:** its "Open in browser" quick
      action is disabled and its tooltip reads "Nothing is serving https://<name>.rex — Start
      all first"; the site page's Open in browser likewise. Stop ONE site with the stack up
      instead: the button stays enabled and opens rexenv's stopped page. **Tell:** Safari on
      "can't connect" from a click rexenv offered (found by the clean-VM run, 18 Sep 2026).
      ✓ 18 Sep 2026 (same VM; stack Stopped 0/6): the row's quick action, the
      site page's header button and its Quick links tile all disabled with that tooltip.
- [x] With at least two sites serving, row menu → **Stop site** on one. Its pill reads
      **Stopped by you**, and the toast says your other sites keep running.
      ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9).
- [x] In a browser, the stopped site's URL shows rexenv's **"This site is stopped"** page —
      the brand mark, THIS site's hostname, and both ways to start it — not a certificate
      warning, not a 502, not Caddy's bare error text, and **not another site's content**
      (the fallthrough this design exists to prevent; the automated proof is
      `site_stop_start_check`). Check it in a light-themed browser too: the page follows
      `prefers-color-scheme`, and it must fetch nothing (no webfont, no remote logo).
      ✓ 18 Sep 2026 (same VM, build 8437afd9).
- [x] The OTHER site still loads, and Services still shows the web tier running. A stopped
      site must never have stopped a php-fpm pool — every site on that PHP version shares it.
      ✓ 18 Sep 2026 (same VM, build 8437afd9).
- [x] **Start site** → it serves again on the SAME certificate (no interstitial, no
      "certificate is not trusted" — the route kept its cert while stopped).
      ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Stop a site, **quit rexenv and relaunch**: it is still stopped. (The switch is in the
      database precisely because services outlive the app.) ✓ 18 Sep 2026 (same VM and build).
- [x] With everything stopped (Stop all), press **Start site** on a stopped site: the toast
      says the site is set to run but rexenv's services are stopped — no "started" claim
      the browser would contradict. ✓ 18 Sep 2026 (same VM and build).
- [x] `rex site stop <domain>` / `rex site start <domain>` do the same thing from a
      terminal, and the stop output says this is not `rex stop`. ✓ 18 Sep 2026 (same VM and build).

## Start/stop EVERY site (Sites page only)
- [x] Sites → **All sites** menu → **Stop all sites (N started)**: every row goes to
      "Stopped by you", the toast counts them, and Services still shows the web tier
      RUNNING — this is not the footer's "Stop all".
      ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] The menu's other row now reads **Start all sites (N stopped)**; the one that would
      do nothing is visible but disabled and says why ("none stopped").
      ✓ 18 Sep 2026 (same VM and build).
- [ ] With a "setup incomplete" site present, the toast says how many were skipped — and
      that site is untouched. **Open: no run recorded.**
- [x] `rex site stop --all` then `rex site start --all` do the same from a terminal and
      print the counts. ✓ 18 Sep 2026 (same VM and build).

## The Sites list's two filters
- [x] Open Sites: it opens on **All** — every site visible on arrival — and only a tab you
      click changes it. (This row said "Running, not All" until 18 Sep 2026; the owner asked
      for All back on 11 Sep 2026 and the code comment beside `filter` records it. The doc
      was the stale half.) ✓ 18 Sep 2026 (VM): relaunch with 3 running → All 3.
- [x] Choose a **type** tab (WordPress / Laravel / Blank PHP): the list narrows, and each
      status tab's count matches what that tab actually shows (and vice-versa) — no number
      above a list that does not contain that many rows.
      ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Filter down to nothing: the empty state offers **Show all sites**, and clicking it
      restores the full list. ✓ 18 Sep 2026 (same VM and build).
- [ ] Pick a tab, then leave the page idle for ~10s while sites start or stop: **the tab
      does not change under you** (the default is decided once, not by the 2s poll).
      **Open: no run recorded.**

## Multisite
- [x] Convert the WP site to multisite. **The convert panel starts on subdomain**, matching
      New Site's toggle (8 Aug — the two screens used to default differently, and the mode
      can't be changed afterwards). Pick either; Network tab shows the mode + sub-site list.
      ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] Create a sub-site; it appears in the list and loads. ✓ 18 Sep 2026 (clean 15.6.1 VM,
      `39610cc7` + self-update to 0.7.2).

## Public sharing (Tunnels) — needs internet
- [x] **Share a site, then STOP that site** (v44): the Tunnels row grows an amber strip
      saying the link now shows the "site stopped" page — without re-sharing, because the
      warning is derived on each poll. Start the site again and the strip goes away.
      **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** `smoke1.rex` shared live, then `rex site stop
      smoke1.rex` → the Tunnels page (tray → Tunnels) showed under its card "⚠ smoke1.rex is
      stopped in rexenv, so this link shows the "site stopped" page to anyone who opens it.
      Start the site to serve it." with the toggle still Live and "Public link confirmed
      reachable — checked every 30 s"; `rex site start smoke1.rex` → that strip gone while the
      other stopped share's strip stayed. The subtitle followed the share count: "2 sites
      shared publicly" → "1 site shared publicly" after one `rex tunnel stop`.
- [x] **Open the public URL of a STOPPED shared site, and reload it a few times** — it
      must show rexenv's stop page EVERY time, never another site's content, and the page
      must name the LOCAL site (`ea.test`) with a runnable `rex site start ea.test` — not
      the trycloudflare hostname the browser is showing (ledger #512). (This failed
      on 5 Sep 2026: a tunnel bypasses the edge and nginx answered from its default
      server — ledger #511. Reload more than once; the first response was a Cloudflare
      error while the tunnel came up, and the wrong site appeared only afterwards.)
      **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9, four reloads from the dev Mac after `rex site
      stop smoke1.rex`):** every reload 503 with `<title>This site is stopped · rexenv`, the
      page naming `smoke1.rex` and `rex site start smoke1.rex`, never the trycloudflare
      hostname, never another site. **Before the fix:** — `site_stop_start_check`
      (L1) proves the direct-to-nginx path a tunnel takes, not Cloudflare in front of it.
- [x] Share a site that is ALREADY stopped: it shares (no refusal) and a toast says what
      the link publishes. `rex tunnel start <domain>` prints the same sentence.
      **✓ the CLI door, 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** `rex site stop smoke-lv.rex`,
      then `rex tunnel start smoke-lv.rex` → consent → "✓ public URL: https://…trycloudflare.com"
      and "warning: smoke-lv.rex is stopped in rexenv, so this link shows the "site stopped"
      page to anyone who opens it. Start the site to serve it." — `rex tunnel list` repeats
      that warning under each stopped share (derived per poll, not stored). The toast in the
      app: not seen (the share was started from the CLI).
- [ ] **Filter while a share is live** (19 Aug 2026, #371 — the half L2 cannot reach,
  since the harness has no event transport): share two sites, type a query in the
  Tunnels search box that matches NEITHER. The amber line must name both ("2 shared
  sites are hidden by this filter — still public until you stop sharing"), the subtitle
  must still say 2 shared, and **Stop all sharing must still stop both**. Then, with the
  query still typed, stop one share from its card: the count in the amber line follows
  it down to 1 rather than sticking. **Open: no run recorded.**
- Timings, not badge-reading: start `scripts/tunnel-measure.sh <url>` the moment the
  URL appears; press ENTER with a note at each physical action (kill -9, wifi off/on).
  It prints the banner→resolver deltas and the break/recovery windows. (The full timing
  session is its own Release-gates row in `docs/TODO.md`, "Tunnel probe session".)
  **◐ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** with two live shares the subtitle read "2 sites
  shared publicly", one `rex tunnel stop` took it to "1 site shared publicly", and **Stop all
  sharing** (pressed with a share live) left `rex tunnel list` at "no public tunnels running"
  with no confirm step. **Open: the filter half** — the search field's placeholder could not be
  read by automation, so no query was typed and the amber "hidden by this filter" line was not
  seen.
- [x] Toggle **Share publicly**; a `*.trycloudflare.com` URL appears, badge Unverified →
  **Live** once the probe confirms. ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update
  to 0.7.2).
- [x] **An override site shares too** (per-backend origins, 15 Aug 2026 — ledger #332's live
  half): switch a site to FrankenPHP (or Apache) and share it. ✓ 18 Sep 2026 (clean 15.6.1
  VM): `blank.rex` on FrankenPHP shared without refusal and a public URL was issued.
- [x] …and the public URL serves THAT site's content — not another site's (the old
  nginx-origin fallthrough). **◐ 18 Sep 2026 (VM): the URL was stopped before propagation,
  so the content was never checked from a second device.** **✓ 28 Sep 2026 (15.8 arm64 VM,
  0.8.9; the dev Mac as the second device):** `rex tunnel start smoke1.rex` asked "expose
  smoke1.rex PUBLICLY via a cloudflared tunnel? [y/N]" (a piped `y`), printed the
  `trycloudflare.com` URL; three fetches from the Mac: 200, `<title>smoke1`, five `smoke1`
  mentions and none of the three other sites, `/wp-login.php` 200.
- [ ] Sharing an override site while its server is stopped refuses with a message naming
  the server, and does not start a tunnel. **Open: no run recorded.**
- [x] **Unverified + dead link on THIS machine is NORMAL on networks that negative-cache
  DNS** (the router NXDOMAINs a hostname created seconds ago): verify from a SECOND
  DEVICE (phone on cellular). Only unreachable-everywhere is a real failure — do not
  file the router race as a bug. ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update
  to 0.7.2).
- [x] Kill the site's cloudflared in Activity Monitor; the card leaves Live within ~5s
  on its own (no stop/start needed). ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` +
  self-update to 0.7.2).
- [ ] Refusals name the EXPOSURE, never "busy": db-import / connection rewrite /
  provision-retry / multisite convert while shared; Share while a db-import runs;
  web-server switch while shared; docroot move while shared. CLI texts match
  (`rex tunnel start`, `rex site server`, `rex site move`). **◐ one of seven, 28 Sep 2026
  (15.8 arm64 VM, 0.8.9):** `rex site server smoke1.rex frankenphp` while shared → "smoke1.rex
  is publicly shared right now — switching its web server would remove it from the shared
  nginx the tunnel serves from, and the live link would start publishing whatever nginx's
  default site answers with — a DIFFERENT site. Stop sharing it (Tunnels…" — the exposure,
  not "busy". **Open: the other six.**
- [ ] Apache/FrankenPHP site with NO recorded backend port: sharing refuses naming the site
  and its server ("… runs on <server> but has no recorded backend port to share — re-save
  the site's web server (Site → Settings), then share it."), never falling back to nginx's
  port — that fallback is the default-vhost exposure (a DIFFERENT site would publish).
  History: until 15 Aug 2026 every override site was refused ("can't be shared yet", a
  disabled toggle with a why-tooltip); per-backend origins (#332, `core::tunnels::origin_port`)
  replaced that, and the 18 Sep VM shared a FrankenPHP site (row above). **Open: the
  no-record refusal has no run recorded** (it is L0 in `origin_port`'s tests).
- [x] Quit with a live share → "Quitting stops N public shares" dialog; both buttons
  behave. Quit with none shared → NO dialog, ever. ✓ 18 Sep 2026 (clean 15.6.1 VM,
  `39610cc7` + self-update to 0.7.2).
- [x] Toggle off; the public URL stops working AND `mu-plugins/rexenv-tunnel.php` is
  gone from the docroot. ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [ ] Launch log, ONLY on a machine carrying rowless orphans: one backstop WARN per
  orphan ("STOPPED A PUBLIC SHARE THIS APP HAD NO RECORD OF"). On a clean machine its
  ABSENCE is correct — do not read a missing line as the backstop not running.
  **Open: no run on a machine with orphans recorded.**

## The menu bar (no dock icon)
*The behavioural half of #437/#438 — the L0 tests prove the menu's RULES, and only a
hand can prove a spec entry became the item it describes.*
- Prep, every run: **exactly one `rexenv.app` on the machine before testing any of this.**
  With a built bundle still in `target/release/bundle/` and the same build in
  `/Applications`, `open -a rexenv` started the one under `target/` (1 Sep 2026) —
  LaunchServices resolves by bundle id and nothing warns. A leg run against the copy you
  did not mean proves nothing, the same trap as relaunching an unguarded build below.
- [x] The R icon is visible and legible in **both** a light and a dark menu bar
  (System Settings → Appearance). ✓ 31 Aug 2026 (dev Mac, both bars): a dark glyph on a
  light bar, white on a dark one (`docs/archive/PLAN-menubar-tray.md` A2).
- [ ] **The dock tile follows the window, and both directions need looking at.** A normal
  launch shows the window AND a dock tile; closing the window removes the tile and leaves
  the menu-bar icon; the tray's **Open rexenv** brings both back. Look at the DOCK — do
  not trust `lsappinfo`: it reported `Foreground` for an app whose tile macOS had never
  added (2 Sep 2026), so the policy and the tile are two different facts.
  **◐ 2 Sep 2026 (dev Mac): after the launch-time fix, a manual launch → tile present, and
  `--hidden` then `rex open` → tile present. The close → tile-gone direction was measured
  only through `lsappinfo` (1 Sep, `UIElement` after closing). Open: the tile's removal,
  seen in the Dock.**
- [x] Clicking the icon opens the MENU (not the window). **Open rexenv** brings the
  window to the FRONT — in front of a full-screen browser, not behind it. ✓ 18 Sep 2026
  (clean 15.6.1 VM, CLT-less, `8437afd9`): menu, not window; Open rexenv brought it to the
  front. ✓ 23 Sep 2026 (clean 15.8 VM, 0.8.7): tray Open after a window close.
- [x] **Hold the menu open for ~30 seconds** (the refresh tick is 5s) while the stack is
  mid-start, so the status line and the greyed Start/Stop actually move. **✓ 28 Sep 2026
  (15.8 arm64 VM, 0.8.9, read through the menu's own accessibility tree every 3 s):** menu
  opened on a stopped stack — "Stopped", Start all enabled, Stop all disabled — `rex start`
  fired behind it → t+3 s "Partial · 6 of 7 running", BOTH enabled, and the line held that
  reading through t+30 s with the menu still open (the seventh, the edge, waited on its admin
  prompt behind the menu). The menu never closed itself; Escape closed it. The numbers must
  change UNDER the open menu and the menu must stay open. It closing itself is the bug
  in-place editing exists for (reported 8 Sep 2026, ledger #437). **Open: owed since the 8
  Sep fix (`docs/TODO.md` "Hold the tray menu open while the stack MOVES") — the 15s hold
  below is on an IDLE stack, where nothing moves.**
- [x] **About rexenv** in the menu → the window comes up on the About screen, with the
  version. Do it with the window CLOSED: that is the state where the app menu's own About
  does not exist, and the only reason this item is in the tray. ✓ 18 Sep 2026 (clean 15.6.1
  VM, `39610cc7` + self-update to 0.7.2).
- [x] **Close the window** (red button) → the app stays alive: `rex status` still
  answers and an MCP client keeps working. This is the whole point of the feature.
  ✓ 1 Sep 2026 (dev Mac, ledger #436: `rex status` answered and an MCP `initialize`
  completed with no window); ✓ 18 Sep 2026 (clean 15.6.1 VM); ✓ 23 Sep 2026 (clean 15.8 VM,
  0.8.7: `rex status` alive).
- [x] The status line matches the sidebar footer for the same moment — same verdict,
  same count. Stop a service from the UI; within ~5s the menu says the same thing.
  ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **Start all** is greyed when everything runs, **Stop all** when nothing does.
  Clicking either does exactly what the footer's button does. ✓ 1 Sep 2026 (dev Mac); ✓ 18
  Sep 2026 (clean 15.6.1 VM).
- [x] **Sites ›** lists your most recent sites and opens one in your **preferred**
  browser (Settings → the browser you chose), not the OS default. With more than 8
  sites, the submenu says how many it hid. ✓ 1 Sep 2026 (dev Mac): 8 of 21 sites listed,
  "…and 13 more" under them, and a site opened in the preferred browser rather than the OS
  default.
- [x] Each of **All sites… / Services / Databases / Mail / Tunnels** shows the window
  on that screen. ✓ 1 Sep 2026 (dev Mac): all five routes land on their screen.
- [ ] …including from a window that was hidden (closed). **Open: not recorded on macOS**
  (the 1 Sep walk does not say the window was closed first).
- [x] **MCP server** carries a checkmark that matches Settings, and toggling from the
  tray flips it in Settings too — with a share of the socket to prove it (an MCP client
  connects after ON, fails after OFF). A checkmark reading "on" while nothing listens is
  the failure this item exists for. ✓ 1 Sep 2026 (dev Mac, #436: unchecking removed
  `rexenv-mcp.sock` and `rex mcp` failed; rechecking rebound it and the handshake
  succeeded); ✓ 18 Sep 2026 (clean 15.6.1 VM).
- [x] Hold the menu OPEN for 15+ seconds with the stack idle: it must NOT close by
  itself. (The rebuild is conditional for exactly this reason.) ✓ 18 Sep 2026 (clean
  15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **Quit rexenv** with a public share up still pauses once and names the count.
  ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **Start rexenv at login** (Settings → Services; "Open rexenv at login" until 29 Sep
  2026, "Start on login" before 28 Sep) → log out and back in: rexenv comes up in the MENU
  BAR with **no window** (and no dock tile). Check the FILE first —
  `~/Library/LaunchAgents/dev.rexenv.rexenv.plist` must name the app you are testing and
  contain `--hidden` — but do NOT check `launchctl print`: an already-loaded job keeps
  the arguments it was loaded with, so the running session shows the old ones until the
  next login. The file is the thing that carries into the next login.
  After logging back in, run **`scripts/login-leg-check.sh`** BEFORE opening the window:
  it collects the leg in one output — the process and its `--hidden`, the accessory
  policy (no dock tile), the app's own "launched at login — staying in the menu bar" log
  line, both sockets, and the plist that produced it. Written because this leg is checked
  once per release, by a human who has just logged in and has no interest in remembering
  five commands. (It runs from a repo checkout — its `rex` is the repo's
  `src-tauri/binaries/rex-universal-apple-darwin`; on a clean Mac without the repo, run the
  same five reads by hand with `/Applications/rexenv.app/Contents/MacOS/rex status`.)
  ✓ 1 Sep 2026 (dev Mac, a real logout and login, ledger #439): `/Applications/rexenv.app/…
  --hidden`, `lsappinfo` `type="UIElement"`, the log's own "launched at login — staying in
  the menu bar", both sockets answering, no window ever opened; the plist had been rewritten
  on launch to name the current binary with `--hidden`.
- [x] **…and the services it manages are up** after that login, with no click — the SAME
  toggle since 29 Sep 2026 (ledger #739: the second toggle, "Start services when rexenv
  opens", is gone; the `--hidden` launch runs Start all and a launch you made does not — open
  the app by hand with the stack stopped and nothing starts): `rex status` lists them running and a site
  answers over HTTPS before the window is ever opened. **🔴 FAILED 29 Sep 2026 (15.8 VM, the
  drafted 0.8.10):** after a reboot with rexenv open, macOS's "Reopen windows" relaunch (no
  `--hidden`) won the race, the LaunchAgent's `--hidden` launch handed off and exited, nothing
  started — `docs/TODO.md`'s ONE-toggle row has the mechanism. Fixed the same day (the handoff
  carries the login, ledger #739). **✓ 29 Sep 2026 on the re-cut 0.8.10 (`a470c1af`), the same VM, the same
  way:** "Reopen windows" on (`TALLogoutSavesState` true — the restart dialog's default; restored
  to the VM's own 0 after), rexenv's window open, stack up, `System Events` restart → macOS
  relaunched rexenv WITHOUT `--hidden` (pid 354, relaunch list `Hide = 0`), the LaunchAgent's
  `--hidden` launch handed off (`runs 1, last exit 0`), and the primary ran login-start 12 s after
  boot: every backend up (a stale `mysql.sock.lock` removed on the way), the edge re-adopted when
  the boot LaunchDaemon came up 70 s later, `smoke1.rex` / `smoke-pg.rex` / `php74.rex` 200 with
  the system trust store. Then Stop all, quit, open by hand → 30 s later 0 services running. The 1 Sep
  login recorded the window half above and not the services (the script prints only
  `rex status`'s first three lines); what that login DID show was the DNS race the next row
  exists for — the app won UDP 15353 and served DNS in-process (#442, fixed 2 Sep). Neither
  the 18 nor the 23 Sep clean-VM pass included a logout/login; the 18 Sep reboot came up with
  the edge and the DNS agent but the backends waiting for Start all, because Start on login
  was not on there (§Robustness "Reboot").
- [x] **A couple of minutes after that login, `rex status` must read
  `answering (agent, udp 15353)`, not `(in-process…)`.** **✓ with churn, 28 Sep 2026 (15.8
  arm64 VM, 0.8.9, the app back at login through macOS's window restore):** ~35 s after login
  `rex status` said `DNS DOWN (down, udp 15353)` while the log had "resolver agent
  unavailable — running IN-PROCESS"; at ~2.5 min it read `answering (agent, udp 15353)` — the
  handoff won. Between them the log shows the handoff's rough edge: "could not rebind
  in-process after a handoff attempt: Address already in use", then health "resolver agent
  was not answering; kicked it" four times, one "gave-up … port 15353/udp is already in use by
  rexenv (pid …)", then "[adopted] resolver agent is answering after all". The same "kicked it"
  churn showed on this VM on 27 Sep while `dig` answered throughout (TODO row). At login the app and the agent
  race for the port; in-process means DNS dies with the app, and the handoff (#442) has
  up to ~2 minutes to take it back. If it still says in-process after that, the app is
  serving DNS it should have given away — the log will say whether the agent was ever
  kickstarted. Enable it, log out and in
  once BEFORE this build too if you can: the plist is rewritten on every launch, so an
  old one (no `--hidden`) must repair itself rather than keep opening a window forever.
  **◐** 1 Sep 2026 (dev Mac): the first real login FOUND this race (#442), and the plist
  self-repair was measured the same day; 2 Sep the fixed handoff ran live on a reproduced
  login race — `handed the resolver back to the agent (attempt 1)`, `rex status` reading
  `answering (agent…)`. **Open: the real-login timing, after the fix.**
- [ ] On a machine where first-run setup is NOT finished (no `/etc/resolver/rex`, or the
  CA untrusted for this user), a LOGIN launch **does** show the window. A silent tray
  there would hide the only screen that fixes it. **Open: no run recorded** (the rule is
  `first_window_decision`, #439).
- [x] `rex status` with the app closed prints the reason **and** `open -a rexenv` — and
  does NOT start the app. **✓ 28 Sep 2026 (15.8 arm64 VM, 0.8.9):** tray → Quit rexenv →
  `rex status` printed "rexenv isn't running — open the app first (the CLI controls the
  running app). Start it with: open -a rexenv (the installed app)"; the app's process count
  stayed 0 three seconds later.
- [x] `rex open` with the app running brings the window to the front. ✓ 18 Sep 2026 (clean
  15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **Launch rexenv a second time while it is running**: the second copy exits with
  "rexenv is already running", the first one's window comes forward, and there is still
  exactly ONE rexenv process. Then **SIGKILL** the app (`kill -9`) and launch it again —
  a stale socket file must not block the launch. **The launched build must be one that
  HAS the guard, or this leg proves nothing** — measured 1 Sep 2026, where the first
  attempt relaunched the older installed app, which could not have been blocked by a
  guard it does not contain. The rerun with a guarded build came up normally on the stale
  socket, and a second guarded launch on top of it handed off and exited (dev Mac, #441).
  ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).

## Which app opens a link
*18 Sep 2026 (clean VM): only Safari was installed, so no chevron/menu rendered at all —
this section needs a Mac with two browsers. The rules are proven below the eye (#309:
L1 `browser_detect_check`, L2 `openin.js`); the rows are what a human must see.*
- [ ] Site header → chevron beside **Open in browser**: every browser you have is
  listed, the one a plain click uses is marked `default`, and picking one opens
  the site there **without** changing the default. **Open: no run recorded — the 15.8 VM
  has Safari only (checked 28 Sep 2026), so all four rows here wait for a Mac with a second
  browser installed.**
- [ ] Each row's second icon (right of the divider) opens the site in **that
  browser's private/incognito window** — check the window really is private (the
  incognito/private badge, and the site logged OUT even though your normal window
  is signed in). This is the one part no probe can see. **Open: no run recorded.**
- [ ] **Safari's row has no private icon.** Safari has no private-window command
  line; a row that offered one would open an ordinary, recorded window.
  **Open: needs a Mac with two browsers** (L2 `openin.js` asserts the absence).
- [ ] The same two targets work from the Quick-links **Browser** tile and from the
  **Magic Login** chevron — a magic link opened privately signs you in there
  without touching the session in your normal window. **Open: no run recorded.**

## Settings
- [x] Theme switch Dark ↔ Light ↔ System re-skins the app correctly. ✓ 18 Sep 2026 (clean
  15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] DNS & SSL shows Running + Resolver; "Make default" moves the default PHP version.
  ✓ 18 Sep 2026 (clean 15.6.1 VM); DNS & SSL ✓ again 23 Sep 2026 (clean 15.8 VM, 0.8.7).
- [x] "Start rexenv at login" toggles (LaunchAgent created/removed). ✓ 18 Sep 2026 (clean
  15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **PHP versions list shows SEVEN rows, 7.4 first** (7.4, 8.0–8.5). 7.4 and 8.0
  carry an **EOL** chip with the date; 8.1's says Dec 2025. Neither 7.4 nor 8.0
  offers the Xdebug toggle. **Tell:** six rows — that is the app running against a
  build that predates 7.4. ✓ 18 Sep 2026 (clean 15.6.1 VM); seven rows ✓ 23 Sep 2026
  (clean 15.8 VM, 0.8.7).
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
  **Open: no UI run recorded** (the rule is ✅ L0, #303, plant-proven).
- [ ] **WP-CLI command pin — the standing tell.** Settings shows a card saying the
  wp-cli command set is pinned and NAMING the packages in `~/.wp-cli/packages` it
  therefore excludes. On a machine with no such packages it must claim **none** —
  never "the 0 packages in …", which is a confident wrong answer rather than a
  degraded one. **Tell:** a count guessed rather than read from that directory's
  own `composer.json` `require`. **Open: no run recorded on a real machine** (#301: L0 +
  L2 `uireview` render both variants).
- [ ] **WP-CLI command pin — at the failure moment.** With a global wp-cli package
  installed (`wp package install <something>`), run one of its commands from the
  site's terminal. Expect WP-CLI's **own** `not a registered wp command` line
  intact, with the explanation **appended after it** — never replacing it. The
  upstream line is the string a user pastes into a search box; a friendlier message
  that swallowed it would cost them the one thing that finds an answer.
  **Open: no run recorded** (#301's L0 asserts the append at both sites).

## Site terminal — the PATH and the session that must survive a tab switch

- [x] **The developer's own tools resolve.** In a site's Terminal tab run `which code`,
  `which rex` and `which git`. Each must answer with the same path Terminal.app gives.
  **Tell:** `command not found` — that is a non-login shell, so the app is back to
  launchd's bare `/usr/bin:/bin:/usr/sbin:/sbin` and none of `/etc/zprofile`'s or
  `~/.zprofile`'s PATH exists (#423). `php -v` must STILL report the bundled patch:
  the prepend runs after the rc files, and a login shell must not cost that.
  ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **A session survives leaving the tab.** Run something with visible output
  (`ls -la`, or better a slow `composer install`), switch to Overview or another site,
  come back. The earlier output and the shell's history must still be there — a
  long-running command must still be running, not restarted. **Tell:** an empty
  terminal and a fresh prompt (#424). **Restart** is the one control that is allowed
  to wipe it, and must. ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).
- [x] **A plugin/theme row opens a terminal in its own folder.** WordPress tab →
  Plugins → the terminal button on a row. `pwd` must be that plugin's directory and
  the site's own shell must be untouched (leave a `# marker` in it first, come back,
  it is still there — a `cd` typed into a busy shell is exactly what this must not
  be). Repeat from a theme card. **Tell:** landing in the docroot instead, which on a
  Bedrock/Radicle site would mean the content dir was guessed rather than read (#425).
  Hello Dolly / an mu-plugin / a drop-in have no folder: those rows show no button.
  ✓ 18 Sep 2026 (clean 15.6.1 VM, `39610cc7` + self-update to 0.7.2).

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

**Runs recorded (reconciled 28 Sep 2026):** **25 Aug 2026** — a locally built, packaged app on
the owner's populated Mac, driven over the raw socket as an MCP client (§M3 and steps 10/13 of
that day's numbering; `docs/archive/SHIPPED-2026-09.md`). **3–4 Sep 2026** — the owner's Mac,
the rebuilt app, Claude Code driving `bl.rex` / `hisab-counter.rex` (the Parity live run,
rounds 1–4, and 23–26 on the dial). **18 Sep 2026** — the clean macOS 15.6.1 arm64 UTM VM,
build `39610cc7` then the in-app update to 0.7.2, **MCP driven as a raw JSON-RPC client**
(commit `5a9f17e1`'s own words). A raw client proves what the SERVER answers, never what a
model does with the answer — so the legs that exist for the model (3, 8, 44's client UX) stay
open until a model runs them. Ticks below say which run they came from.

- [x] **1. Default off = no socket, and nothing below the toggle.** Fresh launch, never
  enabled → Settings → AI agents: the paragraph, the toggle OFF, and NOTHING beneath —
  no status line, no Connect, no dial, no feed (D16). `ls -l <socket>` →
  the file is ABSENT. ✓ 18 Sep 2026 (clean VM, `39610cc7` → 0.7.2). **Tell #1:** if the
  socket exists here, the toggle is a label over an always-on socket (the always-on bug) —
  not really controlling it.
- [x] **2. Enable binds.** Toggle ON → "On — no recent agent activity"; `ls -l <socket>`
  shows `srw-------` (0600); `nc -U <socket>` connects. ✓ 18 Sep 2026 (clean VM).
- [ ] **3. Real client, both scopes.** The card offers two lines: `claude mcp add rexenv
  -- rex mcp` (this project) and `claude mcp add --scope user rexenv -- rex mcp` (every
  project on the machine). Run the project one here; later, from a DIFFERENT directory,
  run the user one and confirm `claude mcp list` finds rexenv there too. **Tell:** only
  one line on the card — the default scope is per-project, and a developer who set rexenv
  up in one repo and lost it in the next is the report this step exists to prevent.
  Then ask Claude Code "why is `<site>` 502-ing?" → feed rows appear
  (list_sites/site_status/tail_log), status flips to "Working — …".
  **◐ 3–4 Sep 2026 (owner's Mac, rebuilt app):** Claude Code drove the whole Parity live run
  through `rex mcp`, and its calls landed in the feed. **Open: the `--scope user` line from a
  different directory (the card gained it 4 Sep, after that run) and the 502 question's status
  flip — no run recorded.**
- [x] **4. Disable drops the socket AND live sessions.** Toggle OFF while the agent
  is still connected → status off; `ls -l <socket>` → GONE; the connected agent's
  NEXT call ERRORS. ✓ 18 Sep 2026 (clean VM, a raw client's connected session); the L1
  `mcp_control_check` (sandbox tier) proves the same against an already-connected session.
  **Tell #2:** if the socket remains, or the agent keeps working, disable isn't tearing
  down. **⚠ A step-4 failure is a HOLD, not a note.** "Disable drops the socket" is the
  security-relevant half — an endpoint you can't turn off is a standing same-user attack
  surface. Fix-then-ship; do NOT ship MCP in ANY release if step 4 fails. (This said
  "v0.1.0" — a hold written against one version reads as spent once that version is out,
  which is the opposite of what a standing hold is.)
- [x] **5. Persistence + startup gating.** Restart with the toggle ON → the socket
  rebinds at launch; restart with it OFF → no socket. ✓ 18 Sep 2026 (clean VM).

### M2a — scratch sites (the executing tools). Ships only if 6–11 pass.
Steps 1–5 gate an endpoint that can only READ. From here an agent can create sites
and run code in them, so the gate changes shape: **step 8 is the one that matters, and
it is the one a human can check and a test cannot** — an agent *asked* to touch a real
site must be refused with the policy message, in front of you, rather than quietly
complying. `mcp_scratch_check` proves the code refuses; only this proves the refusal
survives contact with a model that wants to help.
Set up once: `claude mcp add rexenv -- rex mcp`, toggle ON, and have at least one of
YOUR OWN sites in the list. Keep the Sites page visible.
- [x] **6. Create.** Ask: *"make me a disposable WordPress site called plugin-test."*
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
  feed's. ✓ 18 Sep 2026 (clean VM, raw JSON-RPC client).
- [ ] **7. The dev loop.** Ask it to *"copy my plugin at `<path to a real checkout>`
  into that site and activate it."* → the row gains `<slug> · synced just now`;
  `scratch_add_package` then `wp_run` in the feed. Now **edit a file in your
  checkout** and ask it to run something that reads your change → it should sync
  first. **Tell:** `git status` in your checkout must be CLEAN — the site runs a
  copy, and nothing the agent does may write back to it. **Open: no human run recorded**
  (the copy itself, source byte-compared, is L1 — ledger #217).
- [x] **7a. Log in without a password (D2, 5 Sep 2026).** Ask: *"give me a login link
  for that scratch site."* → the reply carries a `https://<scratch>/?rexenv_login=…` link
  and says it is single-use and expires in two minutes. Open it → **wp-admin, signed in as
  the primary administrator, no login form.** Open the SAME link again → an ordinary
  WordPress page (the token was spent). The feed shows `scratch_login_url` with the site
  and nothing else — no URL, no token. Then ask for a link into one of your OWN sites
  through the same phrasing → it comes from `wp_user`'s `login_url` and works with the
  dial at Read (the default — the owner's ruling: no login, no password, any site); the
  feed shows `user login_url` and no link. ✓ 18 Sep 2026 (clean VM, raw JSON-RPC client).
  **Tell:** a link that logs in twice, a token in the feed, or a password reset appearing
  in your users list — any one is a HOLD (the promise is "never a password").
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
  **◐ 18 Sep 2026 (clean VM):** ticked then from a raw JSON-RPC client — the SERVER's
  refusals, the half `mcp_scratch_check` already proves. **Open: a model in front of it — the
  half this step exists for, and the only one no test can run; no run recorded.** (Until 28 Sep
  this row was ticked; the run that ticked it had no model.)
- [ ] **9. Keep.** Row menu → **Keep this site** → the confirm names the domain and
  says there is no un-keep. Confirm → the row LEAVES the Agent scratch group and
  becomes an ordinary site: no badge, no TTL, no Keep item. **Tell:** if it keeps
  any agent styling, the UI is reading something other than the recorded origin —
  and the dialog just promised otherwise. **Open: no run recorded** (the rendering is L2,
  `scratch-keep-dialog` / `scratch-rows`, ledger #219).
- [x] **10. Reap + the banner.** Set a scratch site's expiry into the past, then
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
  **"(deleted site)"** target. ✓ 25 Aug 2026 (packaged dev build, owner's Mac): the banner
  named `reapme.scratch.rex` (screenshot), the live scratch site stayed, the `scratch_reap`
  feed row attributed the delete to rexenv, and a `rex_agent_*` account confirmed present
  before was gone after. **Tell:** a silent sweep — a bulk delete with no banner is
  indistinguishable from data loss to someone returning after a week.
- [ ] **11. Nothing prompted.** Across steps 6–10, macOS must never have asked for an
  administrator password. **Tell:** any auth dialog triggered by something the AGENT
  did breaks the strongest sentence in the guarantee ("Nothing an agent can call ever
  asks macOS for an administrator password") — a HOLD, and the ledger row to reopen
  is the never-prompt provision flag (#210). **Open: observed, not checked** — on 25 Aug
  nothing the agent did raised a dialog, but that is an observation across a session; no run
  recorded that checked it deliberately. (L0 #210 proves the flag with a control that MUST
  prompt.)

### M2b — the PHP matrix and mail. Ships only if 12–14 pass.
Steps 6–11 gate what an agent can CREATE and RUN. These two surfaces are
different: one refuses rather than guessing, and the other is the second place a
user consents to something. Keep a scratch site from step 6 alive for these.
*(Step 11b — "a PHP 7.4 site actually serves" — **moved out of this section** to
"PHP 7.4 — the one leg no automated tier covers" above. It is not an MCP
behaviour and it was the only proof that a 7.4 site serves at all; leaving it
here meant it ran only when MCP was enabled. Run it there, before this section.)*
- [x] **12. PHP, refused by name.** Ask: *"switch that scratch site to PHP 7.2."*
  → REFUSED, and the refusal must **name the versions rexenv does have**. Ask for
  whatever `php::unshipped_minor()` currently returns if 7.2 ever ships — this step
  used to say 7.4 on the belief 7.4 was unshippable, and
  `docs/archive/PLAN-php-74-support.md` retired that. Then ask for 8.1 → it switches, and
  SiteDetail shows 8.1. First switch to a version downloads it, so expect a slow call once.
  ✓ 18 Sep 2026 (clean VM, raw JSON-RPC client).
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
  inbox tool, which is why the card no longer says it. **Open: no run recorded since D16
  rewrote it** (the paragraph's words are pinned at L0, #500). History: the pre-D16 step ran
  25 Aug under the retired mail switch — its refusal passed, and checking its precondition
  found #406 (the scratch stamp never applied at creation).
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
  **◐ the inbox half ✓ 18 Sep 2026 (clean VM — step 35: `key=<redacted>`, no token).**
  **Open: the scratch tools NOT seeing your own site's reset mail, in front of you — no run
  recorded** (the filter over real Mailpit messages is L1 `mcp_mail_check`, ledger #501).

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
  **◐ the read at Read with no prompt ✓ 18 Sep 2026 (clean VM — step 16's second half: the
  user-email read succeeded).** **Open: the Recent activity row carrying the SQL as its
  summary — no run recorded.** History: 25 Aug the pre-D16 step read 13 real sites (1,715
  rows) through the since-retired grant prompt.
- [x] **16. ⚠ Read-only IN FRONT OF YOU.** Ask it to **write**: *"set that site's blog
  title to 'agent was here' with a SQL UPDATE."* → REFUSED by the server, and the title
  in WordPress is unchanged. Then ask it to read something sensitive it legitimately can
  (*"list the user emails"*) — it succeeds, because the paragraph above the toggle said
  so in those words. ✓ 18 Sep 2026 (clean VM, raw JSON-RPC client); the `UPDATE` refusal
  with the title unchanged was first seen 25 Aug. **⚠ A step-16 failure is a HOLD.**
  "Read-only" is a sentence the user read and turned the endpoint on under.
- [x] **17. Off closes it.** With the agent's conversation still open, turn the endpoint
  OFF → its next query fails (the socket is gone), and nothing renders below the toggle.
  Turn it back ON → the read works again with no prompt. ✓ 18 Sep 2026 (clean VM).
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
  **◐ the SCRATCH half ✓ 25 Aug 2026 (packaged dev build, owner's Mac):** it found the leak
  (fixed `7129b3e`), then the delete and the reaper both dropped a `rex_agent_*` account
  confirmed present beforehand, with the `'r%'` landmark at 17 rows. **Open: a REAL site's
  `rex_ro_<slug>` after its delete, and the rename-then-delete leg — no run recorded.**
- *19–22 retired by D16 (4 Sep 2026): revoke, per-site/per-client scope, auto-allow and
  the consent-prompt wording have no surface any more — the dial is global, reads need
  no grant, and the one paragraph carries the wording (step 13).*

### Parity P1 — the Agent access dial (D15, 3 Sep 2026; replaced the switch + per-site prompts the same day)
The first shape — "Let agents manage my own sites" plus one prompt per site, per scope,
per client — was honest and cost six clicks for one site's ordinary work (the live run
that afternoon). What replaced it is ONE dial. These steps are what a person reads and
turns; the L0/L1/L2 legs (#497, #498) hold the rest. The levels, as the dial answers them:
reads are free at **Read**; `manage` and `system` need **Changes**; `destroy`, `run` and
(since D17) publishing a site need **Full**.
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
  **◐ 4 Sep 2026 (owner's Mac, rebuilt app, before D16 rewrote this step):** Read was free —
  plugins, logs, inbox with nothing to click. **Open: the card as written now (the chosen
  row's border + check, Read's mail/database sentence, the Full/publishing line) read in the
  packaged app — no run recorded** (ledger #502: "a human eye on SMOKE §P1 23").
- [x] **24. Changes, for this session.** Pick **Changes** → the duration row appears with
  **This session** chosen and the line "switches itself off when you quit rexenv". Ask the
  agent to switch a site's PHP version → it does; ask it to delete a plugin → refused, and
  the refusal names `Agent access` at **Full**. Quit and relaunch → the dial is back at
  Read (the log says so), and the same delete is refused again. ✓ 4 Sep 2026 (owner's Mac,
  rebuilt app, Claude Code on `bl.rex`): at Changes a change ran (Xdebug, not a PHP switch),
  the delete and `wp` were refused naming Full; Changes · This session → relaunch → Read,
  and the log said so.
- [x] **25. Full, for 7 days, and what it says.** Pick **Full**, then **7 days** → the
  expiry stamp shows. Read the Full sentence: it must say delete/reset, live
  search-replace, database import, AND "run commands and code of its choosing … as you".
  Ask for the plugin delete → it runs. Set the stamp in the past (`rex config` cannot —
  the keys are refused; use the sqlite shell) → the card shows "Your 7-day setting
  expired", the level reads Read, the delete is refused. ✓ 4 Sep 2026 (same run): at Full ·
  7 days the delete, `wp option get` and a dry run ran; the stamp set to the past → "has
  expired, so it is back at Read" and the card's notice. Fixed from that run: picking a level
  after an expiry starts at this session; the dial polls, so an expiry shows without a reload.
- [ ] **26. ⚠ What the dial does and does not answer (D17).** At **Full · Always**: ask the
  agent to share a site → it starts, and the reply says the URL and that rexenv stops it
  within the minutes asked for; there is NO prompt anywhere, because Full says so. Turn
  the dial to Changes → the same request is refused naming `run` and Full. Ask it to set
  `agent_access_level` through the `settings` tool → refused with the policy's reason.
  Ask it to stop the stack → the macOS password dialog still appears. **Tells — a HOLD:**
  a share starting below Full; an agent turning the dial; no macOS dialog.
  **◐ 4 Sep 2026 (before D17):** the `settings` tool refused `agent_access_level`; the macOS
  dialog on a stack stop is step 36 (✓ 3 Sep and 18 Sep). **Open: the share starting at Full
  with no prompt, and refused at Changes — no run recorded since D17** (that run's share still
  asked a person; ledger #503's "not covered").

> **D15 note (history):** until 28 Sep 2026 steps 27–46 still described the per-site prompts
> ("a prompt for `manage`/`destroy`/`run` → Allow", "Allow for 7 days", "one grant does not
> cover the other site"), with this note translating them to the dial. They are written for the
> dial now; the italic *Done …* lines record runs made under the prompts, and say so.

### Parity P2 — the site lifecycle on YOUR sites. Ships only if 27–31 pass.
`mcp_user_site_check` (sandbox tier) proves the switch, the gate, a real rename and a
real delete over the socket against a sandbox. What it cannot prove is the three tools
that need the stack — creating, retrying and restarting a real site — and whether a
MODEL respects the gate. Set up: the endpoint ON with the dial at Read,
`claude mcp add rexenv -- rex mcp`, the stack running, at least one of your own sites. Keep
Settings → AI agents visible.
- [ ] **27. Create, under the dial.** At Read, ask: *"create a blank PHP site called
  `agenttest.rex`."* → refused naming `Agent access` at **Changes**, nothing created,
  nothing to click. Turn the dial to **Changes · This session** → the agent's retry creates
  the site; it appears in your list as an ordinary site (no Agent-scratch heading, no
  badge, no TTL) and loads over HTTPS. **Tell:** it lands under Agent scratch (the row was
  written as the agent's), or macOS asked for a password (the create prompted — #475's
  never-prompt rule is broken; a HOLD). **Open: no run recorded** (#480: create/retry/restart
  for real are the human leg). History: the pre-D15 step clicked a per-site `manage` prompt.
- [ ] **28. Configure.** Still at Changes, ask it to switch that site to another PHP version
  and to add the domain `also-agenttest.rex` → both succeed with nothing to click (the dial
  is global — the pre-D15 "one grant on rexenv itself does NOT cover the site" leg is moot).
  SiteDetail shows the new version; the alias serves. **Tell:** either change refused at
  Changes, or a prompt of any kind. **Open: no run recorded.**
- [ ] **29. ⚠ THE GATE, in front of a model.** With the dial at **Read**, ask, naming one of
  YOUR OTHER sites: *"delete `<your site>`"* → refused naming Full; the site is untouched; the
  agent reports the refusal. Then *"rename `<your site>` to x"* → refused naming Changes.
  **Tells — any one is a HOLD:** the command runs; the agent works around the refusal
  (creates a scratch site, uses `wp_run`, tries the CLI); the refusal reads as "no such
  site". **Open: no run recorded with a model.** (The pre-D15 tell "a 7-day option on a
  destroy prompt" is retired: since D15, Full · 7 days covers a destroy — step 25.)
- [ ] **30. Delete, and the session boundary.** Ask it to delete `agenttest.rex` → refused
  naming Full; turn the dial to **Full · This session** → the site, its folder and its
  database are gone; the feed shows `site_delete · ok` naming it. Quit and relaunch → a
  session-long dial is back at Read; ask it to delete anything → refused naming Full.
  **Open: no run recorded** (the relaunch-to-Read half ✓ 4 Sep in step 24).
- [ ] **31. Retry + restart.** Break a create on purpose (stop the database engine, then
  ask for a WordPress site) → the reply says the site exists as "setup incomplete" and
  names `site_retry`. Start the engine, ask it to retry → the site finishes. Ask it to
  restart the site → the reply says which of the three outcomes happened (backend /
  shared / refused) and how many sites share the pool; with `pool: true` it names them.
  **Tell:** the reply carries a local log path or a port number — or the retry reply
  says `status: "running"` with phases pending. It must be the settled outcome (ledger
  #559: until 11 Sep 2026 it was the job's first snapshot, and the agent had to poll).
  **Open: no run recorded** (#559's "not covered": a live `site_retry` reply after the fix).

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
  **◐ 4 Sep 2026 (owner's Mac, `bl.rex`):** the plugin list came back free, a change was
  refused naming its level, and at Changes a change (Xdebug) ran. **Open: the activation
  itself, checked in wp-admin — no run recorded.**
- [ ] **33. ⚠ Destroy needs Full, and a dry run does not.** At **Changes**, ask it to
  *"delete the plugin `hello`"* → refused naming Full; the plugin is still there. Ask for a
  live search-replace (`dry_run: false`) → also refused naming Full; a dry run runs at
  Changes. **Tell — a HOLD:** the delete or the live replace running below Full.
  **◐ 4 Sep 2026:** at Changes the plugin delete was refused naming Full, and at Full · 7 days
  it and a dry run ran. **Open: the LIVE search-replace refused at Changes under the dial — no
  run recorded since D15** (3 Sep, under the prompts, it was refused while the dry run ran
  under `manage`). History: this step was "Destroy is never a week" — D15 retired that; Full
  · 7 days covers a destroy by design (step 25).
- [x] **34. The raw runner runs as you.** At Changes, ask: *"run `wp option get siteurl` on
  `<your site>`"* → refused naming Full (`run` is Full's). At Full → the value comes back with
  `succeeded: true`. Ask for *"`wp plugin list --path=/`"* → refused naming `--path` before
  the dial (a shape refusal records nothing). Ask for a user with no password → the reply
  shows a generated password once; the feed row reads `user create` and never the value.
  ✓ 3–4 Sep 2026 (owner's Mac, `bl.rex`): `wp` refused at Changes naming Full and
  `wp option get` ran at Full · 7 days (4 Sep, the dial); `--path` refused with nothing asked,
  and `wp_user create` showed the password once with the feed row `user create` (3 Sep, under
  the prompts — neither depends on the consent shape).
- [x] **35. ⚠ The inbox is a Read (D16).** With the endpoint on and the dial at Read:
  *"read my inbox"* → your messages come back with no switch and no prompt; the reply's
  note says every site's mail is there and what is removed. Open a password-reset mail's
  text → `key=<redacted>` and no `rexenv_login=` token; the `headers` carry no cookie
  value. Turn the endpoint OFF → nothing. ✓ 18 Sep 2026 (clean VM, raw JSON-RPC client).
  **Tell — a HOLD:** a message body carrying a reset key or a login token, or a "Let agents
  read scratch-site mail" switch anywhere.

### Parity P4 — the stack, with the password dialog as the second consent. Ships only if 36–39 pass.
The sandbox L1 cannot raise a macOS dialog, start a real tunnel or swap a pool. These are
the human legs. Set up: stack up, the endpoint ON, one of your own sites.
- [x] **36. ⚠ Stopping the stack is two consents.** At **Read**, ask: *"stop rexenv's
  stack"* → refused naming **Changes** (`system` sits at Changes because the macOS dialog is
  the second consent). Turn the dial to Changes · This session → the agent's retry raises the
  macOS dialog. Cancel it → the call fails, the stack is still up. Retry, enter the password
  → every site is offline; `stack_status` says so. Start it again the same way. **Tells — a
  HOLD:** no macOS dialog (the privileged path was bypassed); the stack stopped at Read;
  any "without asking" switch anywhere in the card (D15 removed them). ✓ 18 Sep 2026 (clean
  VM, raw JSON-RPC client). *Done first 3 Sep 2026, under the prompts: cancelled →
  "Administrator permission was cancelled", stack up; password → 0 running; start → 12
  running.*
- [ ] **37. A resolver write is the same shape.** At Changes, ask it to *"repair the
  resolver for `.rex`"* → the macOS dialog → the file is back. Ask it to set `mcp_enabled`
  through `settings` → refused with the policy's reason and nothing to click.
  **◐ 3 Sep 2026 (owner's Mac):** `settings mcp_enabled` refused, no prompt. **Open: the
  resolver repair through the dialog — no run recorded.**
- [ ] **38. ⚠ Share: Full covers it, and it stops itself (D17).** Set the dial to **Full ·
  This session**. Ask it to *"share `<your site>` for 2 minutes"* → it starts with no
  prompt; the reply has a `trycloudflare` URL that loads from your phone. Wait 2 minutes →
  the Tunnels page shows it stopped, and the feed shows no second agent call. **Tells — a
  HOLD:** the share started with the dial at Changes or below; the tunnel outlives its
  minutes; quitting rexenv leaves it up. **Open: no run recorded since D17** (ledger #503's
  "not covered"). *Done 3 Sep 2026 under the pre-D17 shape, in the other order: a person's
  session `run` → share started, the reply's `trycloudflare` URL, stopped by rexenv at 2m 00s
  (log), no second agent call, URL then 530; grant revoked + switch ON → share refused naming
  auto-allow, the auto-granted row left behind, no tunnel. The switch's FIRST click never
  reached the backend (no log line); the second did — unexplained; the switch is gone since
  D15/D17.*
- [ ] **39. Cap and TTL are settings.** `rex config set scratch_cap 0` → refused with the
  range; `rex config set scratch_cap 2` → the third scratch_create_site names the limit
  as 2 and lists the two. **Open: no run recorded.**

### Parity P5 — migration and repo under grants. Ships only if 40–42 pass.
- [ ] **40. A clone at Full, with progress.** On one of your own sites, with the dial at
  **Full · This session**, ask: *"clone `https://github.com/octocat/Hello-World` into it as a
  plugin"* → Claude Code shows the job's phases arriving while it runs (the progress
  notifications), then a reply whose log names the plugin folder as `<docroot>/…`, never
  `/Users/…`. `repo status` on it reads at Read. **Open: no run recorded.**
- [ ] **41. A rewrite is two steps and a fingerprint.** On an imported site (Valet/Herd):
  *"preview the connection rewrite"* at Read → the diff and a fingerprint. *"apply it"* →
  refused naming Full below it; at Full → applied, backup kept. Edit the config by hand, ask
  it to apply the OLD fingerprint again → `fileChanged`, nothing written. **Open: no run
  recorded** (a Herd/Valet site is needed; the connect round trip was measured over MCP for
  Local on 12 Sep — `01335d88`).
- [ ] **42. A database import destroys.** *"import `<site>`'s database from Herd"* → refused
  naming Full below it; the database is untouched. At Full → phases stream, the reply names
  any kept dump by FILE. `db_import leftovers` at Read on rexenv itself lists it by name only.
  **Open: no run recorded.**

### Parity P6 — the protocol, in front of the real clients. Ships only if 43–45 pass.
- [ ] **43. ⚠ D12: every tool, two clients.** `claude mcp add rexenv -- rex mcp` → Claude Code
  lists every tool (`/mcp` → rexenv) with no schema complaint, and a call works. Then the
  Cursor stanza from the card → Cursor shows the server connected and its tool list
  complete (Cursor has historically capped tools per server; if it truncates, THAT is the
  finding — record the number). **Tell — a HOLD:** either client rejects the list or a
  descriptor; the grouped design (D12) was chosen on this evidence and has to be re-cut
  if it fails here. *Without a packaged app* the Claude Code half runs against HEAD:
  `REXENV_MCP_HOLD_SECS=120 cargo run --example mcp_socket_check` (app quit), then
  `claude -p "list every rexenv tool name" --mcp-config <json naming cli/target/debug/rex mcp>
  --strict-mcp-config` — the count in the answer is the real client's `tools/list`.
  **◐ the Claude Code half ✓ 3 Sep 2026 (HEAD, this way): 49/49 listed, `list_sites`
  round-tripped.** **Open: Cursor — no run recorded.** The count is whatever the server's own
  `tools/list` prints — never a number carried by hand: this step's title said "49 tools" and
  on 18 Sep a raw JSON-RPC client counted **50**.
- [ ] **44. Destructive hints reach the client.** In Claude Code, ask for `site_delete` on
  a site the dial allows → the client's confirm-before-destructive UX appears (it reads
  `destructiveHint`); `list_sites` never prompts (read-only).
  **◐ 18 Sep 2026 (clean VM):** ticked then from a raw JSON-RPC client, which can see the
  hints the server sends but not a client's UX. **Open: the confirm in Claude Code itself — no
  run recorded.** (Until 28 Sep this row was ticked.)
- [ ] **45. ⚠ Quit mid-call.** Ask for a `site_create` (a minute of work), quit rexenv
  while it runs → the agent reports **"rexenv stopped while this call was in progress …
  the outcome is unknown"** and does NOT retry the create on its own. Reopen rexenv →
  the half-built or finished site is in the list; `site_status` says which.
  **Tell — a HOLD:** the model sees a bare transport error, or retries the create.
  **Open: no run recorded** (owed since the 3 Sep live run's round 4).

### Parity P7 — the Laravel loop. Ships only if 46–47 pass.
- [x] **46. artisan, non-interactive by construction.** On one of your Laravel sites, with
  the dial at **Full · This session**, ask: *"run migrate:status"* → the table comes back.
  Then *"run tinker"* → returns at once, exit 0, no hang (stdin is null; psysh prints a
  termcap warning to stderr, not an error). **Never `db:wipe`/`migrate:fresh` here to test
  the prompt: Laravel's `ConfirmableTrait` only asks in production — locally they wipe at
  once.** (This step first said to run `db:wipe`; corrected 3 Sep 2026 before anyone did.)
  ✓ 3 Sep 2026 (owner's Mac, `hisab-counter.rex`, under the pre-D15 session `run` grant —
  what the step checks does not depend on the consent shape): `migrate:status` listed the
  batches, `tinker` exited 0 immediately. **Tell — a HOLD:** tinker hangs the call.
- [x] **47. A Composer link is a symlink, and says so.** Make a package folder with a
  `composer.json` (`"name": "acme/widgets"`, a `src/` with one class). Ask, at Full: *"link
  `~/Projects/acme-widgets` into `<laravel site>`"* → Composer runs (the reply's `log`) and
  the reply says SYMLINK and "lands in the checkout". `ls -l vendor/acme/` in the project
  shows the symlink; the project's `composer.json` has `repositories.acme-widgets` with
  `"symlink": true`. Edit the class in the SOURCE folder → the site sees it with no sync. Ask
  for `~` as the source → refused (blast radius), and nothing in `composer.json` changed.
  ✓ 3 Sep 2026 (owner's Mac, `hisab-counter.rex`, under the pre-D15 session `run` grant):
  `vendor/acme/widgets` is a symlink, a source edit showed through `tinker --execute` with no
  sync, `~` refused with `composer.json` byte-identical. Composer's log carried the source's
  absolute path — fixed the same run (labelled `<source>` before the scrub).

## In-app self-update — the replace and the relaunch, which no tier can run (6 Sep 2026)

*The half nothing below L3 can see: a real bundle replacing itself in `/Applications`, macOS
deciding whether to allow it, and launchd re-execing the agent that outlives the app. The
probe measured the swap once on this Mac (`docs/archive/PLAN-self-update.md` §T0); this is the same
question asked of a real rexenv, on a real release, with a real user's approval state.*

Only runs once a release NEWER than the installed build has been published with its signed
descriptor. Before that Settings → About shows the version and no Install button, which is
itself the first check.

**Where these rows have run:** the dev Mac (macOS 26.6.2), 7 Sep 2026 — the published 0.6.0 →
0.6.1, twice (a brew downgrade put 0.6.0 back in between; `docs/PUBLISH-TESTING.md` §A 0.6.1,
§M); the clean UTM VM (macOS 15.6.1 arm64), 18 Sep 2026 — build `39610cc7` then the in-app
update to 0.7.2; the macOS 15.8 arm64 UTM VM, 27 Sep 2026 — the public 0.8.7 dmg → the published
0.8.8 (`docs/PUBLISH-TESTING.md` §A 0.8.8).

- [x] **The menu bar offers it, and only opens it.** With a newer release published, the
  tray menu's FIRST item reads `Update to <version>…`; clicking it shows the window on
  Settings → About and installs NOTHING. The app menu's "Check for Updates…" (under About)
  lands on the same card with a fresh check. **Tell:** an item that installs, an item naming
  no version, or a menu that hangs while it opens — it must read a snapshot, never fetch.
  ✓ 7 Sep 2026 (dev Mac, 0.6.0 → 0.6.1): the tray's `Update to …` item opened About and
  installed nothing. ✓ 18 Sep 2026 (clean VM, → 0.7.2).
- [ ] **Dark when current.** On the newest build, Settings → About offers nothing and shows
  no error; the footer says when it last checked. **Tell:** a button that appears and fails,
  or a footer claiming a check that never ran. **Open: "dark when current" as the CARD renders
  it — owed since §M on 7 Sep 2026; no run recorded.**
- [x] **The offer arrives on its own.** With a newer release published, launch rexenv and
  open About without pressing anything: the version and size are named before any click, and
  the consent sentence is above the button. **Tell:** a button with no sentence, or a size
  that disagrees with the release asset. ✓ 7 Sep 2026 (dev Mac, 0.6.1: "the offer arrived with
  no click"). ✓ 18 Sep 2026 (clean VM, → 0.7.2). ✓ 27 Sep 2026 (15.8 VM): About read "rexenv
  0.8.8 · 27.0 MB has been published".
- [x] **A real update applies.** Press Install. Expect real bytes in the footer's download
  indicator, then the app quits and reopens on the new version — About and `rex --version`
  agree, and the startup notice names the version the NEW process read from itself.
  **Tell:** two `rexenv.app` copies afterwards; a notice naming a version About disagrees with.
  ✓ 7 Sep 2026 (dev Mac, 0.6.0 → 0.6.1, twice): `CFBundleShortVersionString` 0.6.1, cdhash
  `e7036221…` → `ce45c7a5…` (a different bundle, not a rewritten one), still universal,
  codesign valid, `rex 0.6.1 (f748698)` equal to About, one `rexenv.app`, and the startup
  notice read `rexenv 0.6.1` (§M — the clause this row once stayed ◐ for: "a box is not ticked
  by the parts of it that were watched"). ✓ 18 Sep 2026 (clean VM, → 0.7.2). ✓ 27 Sep 2026
  (15.8 VM, 0.8.7 → 0.8.8): `swapped to 0.8.8 (AtomicSwap)` → the "close and open again"
  dialog → relaunched `0.8.8 (49c0735)`, no crash report.
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
  ✓ 7 Sep 2026 (dev Mac), all three clauses: `answering (agent, udp 15353)`; the agent pid
  changed on every swap (43710 → 47967 → 65661); and the log carries four kickstart lines,
  including `the resolver agent is running 0.6.1 f748698 but this build is 0.6.0 55eae12 —
  kickstarting it` — the agent NEWER than the app, after the brew downgrade, so the check is a
  mismatch test and not a "less than" one. MySQL, MariaDB, seven php-fpm pools, Nginx, Caddy
  and Mailpit kept their pids throughout. ✓ 27 Sep 2026 (15.8 VM, → 0.8.8): DNS agent
  answering after the relaunch.
- [x] **The previous copy is cleaned up, but only after a healthy launch.** `ls -a
  /Applications` shows no `.rexenv-update-*` directory once the new build has opened.
  **Tell:** a leftover that survives two launches.
  ✓ 7 Sep 2026 (dev Mac), checked after each of the two updates: none. ✓ 27 Sep 2026 (15.8 VM,
  → 0.8.8): the kept bundle gone.
- [x] **A public share is live.** Start a share, then press Install: the quit confirm names
  the share count exactly as Cmd+Q does. Choose "Keep sharing" → the app stays up, the card
  says the update is installed and takes effect when rexenv next opens, and NO relauncher is
  left behind (`pgrep -f -- --relaunch-after` is empty). **Tell:** a relaunch that happens
  anyway, or a helper still waiting.
  ✓ 18 Sep 2026 (clean VM, the self-update to 0.7.2 run with a live share — the re-run against
  #539's fix). History: ◐ 7 Sep 2026 (dev Mac) — **run, and it found the bug this leg exists
  for.** The quit confirm and "Keep sharing" worked and no relaunch happened, but the card then
  said **nothing at all** and offered Install again, as if the update had not been made. Fixed
  the same day (ledger #539).
- [ ] **Offline.** Disconnect and press Check now: the card says the check could not run and
  keeps the previous timestamp. **Tell:** "up to date", or a timestamp that moved — **or a
  spinner that never stops**, which is what this leg caught on 7 Sep 2026: over 40 seconds
  of "Checking…" under a 15-second client timeout that never fired. The seam now enforces its
  own deadline (ledger #540) and the button gets a SHORTER one than the poller — **6 seconds**
  — so the answer is late at worst, never absent. Re-run this leg against the fix and time it:
  it should say it could not reach the server within about six seconds. **Open: the re-run
  against #540's fix — no run recorded.**
- [x] **Homebrew coexistence — the unnamed form.** After a self-update, **`brew upgrade` with
  NO cask named** does nothing. **Tell:** brew touching rexenv in the unnamed form.
  ✓ 7 Sep 2026 (dev Mac): `brew upgrade --dry-run` does not list rexenv — the unnamed form
  leaves it alone, as `auto_updates` promises.
  **Do not test this with `brew upgrade --cask rexenv`** — naming a cask is an explicit
  request and Homebrew honours it regardless of `auto_updates`, so the named form really
  does reinstall whatever the local tap checkout says, and on a stale checkout that is a
  silent downgrade. Measured 7 Sep 2026: it put 0.6.0 back over a self-updated 0.6.1 and
  printed `Upgraded 1 requested outdated package`. That is not a bug to file; it is the
  reason this line now says which command to run.
- [ ] **Homebrew coexistence — the consent sentence.** On a cask-installed copy the consent
  sentence mentions `brew upgrade --cask rexenv`. **Open: not yet observed** (a cask-installed
  copy with an offer on screen; owed since §M, 7 Sep 2026).
- [x] **Which permissions come back.** Note every macOS prompt that reappears after the
  update (an ad-hoc build is a new identity each time). Record them here — the consent
  sentence promises this, and the list is what makes it honest.
  ✓ 7 Sep 2026, on the dev Mac (macOS 26.6.2), across two updates: **none**. No prompt was
  seen by the owner and `log show --predicate 'subsystem == "com.apple.TCC"'` records none for
  rexenv in the window. **Do not turn this into a promise.** It is one Mac whose grants were
  already settled; the consent sentence keeps warning because a machine that has not yet
  granted a permission, or a macOS that keys a grant more strictly, will ask again.
- [ ] **Which permissions come back — re-recorded per macOS major.** The same list on macOS 15
  (the clean VM) and on 13/14. **Open: only macOS 26 is recorded;** the 18 Sep and 27 Sep VM
  updates did not record the TCC log.

## Robustness (spot-check) — §2
- [x] Quit with another app on :443, relaunch → a clear "port in use" message (no crash).
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).
- [x] Cancel an admin prompt once → a clear "permission cancelled, try again" state; retry
  works. ✓ 18 Sep 2026 (clean VM, as above).
- [x] **Reboot** (ledger #67/#155 territory): after it, a site loads over HTTPS *without
  opening the app* — the root edge daemon (KeepAlive) and the DNS LaunchAgent both come back
  on their own. ✓ 18 Sep 2026 (clean VM, as above): the root edge daemon and the DNS
  LaunchAgent came back on their own (caddy on :443, agent answering) before the app was
  opened; the backends need Start all (or login-start) — `rex start` and the site served.
- [ ] **Sleep/wake**: after it, a site loads over HTTPS without opening the app. **Open: the 18
  Sep pass recorded the reboot only; no sleep/wake run recorded.**
- [ ] **Start all over a replaced app reinstalls the edge in ONE try** (ledger #744, 0.8.11): the
  edge running with a browser tab holding a connection, replace the app by hand with another build,
  Start all → if it prompts, the edge comes back after that one prompt (no "Bootstrap failed: 5",
  no second Start all).
- [ ] **A slow boot edge is waited for, not reported** (ledger #743, 0.8.11): login toggle on,
  stack up, reboot a slow machine (the UTM VM) → no "the HTTPS edge needs Start all" toast even
  when the LaunchDaemon's caddy comes up a minute after login; the health log shows the edge
  adopted. After a Stop all (the daemon disabled) the line still appears at once — correct.
- [ ] **A power cut right after Start all** (ledger #740, 0.8.11): Start all (the edge prompt),
  then `utmctl stop macOS --kill` within 10 s → boot → `/Library/LaunchDaemons/dev.rexenv.rexenv.edge.plist`
  whole, the edge up on :443 without the app, and the login toggle's Start all silent. The
  mechanism was measured on Linux only (a root write + a cut 10 s later came back 0 bytes without
  the flush); APFS was never cut on purpose.
- [ ] **Login autostart stays silent on a cold cache** (ledger #175 — this checklist IS
  that row's wiring proof; the code has only a text-order guard, which cannot see
  behaviour). Setup: enable "Start rexenv at login" (the one toggle since 29 Sep 2026), then
  move one binary out of the cache (e.g. `mv "…/bin/mysql-"* /tmp/`), reboot, log in,
  and WATCH the first minute: **no download progress anywhere, no admin password
  prompt** — only the honest "binaries not downloaded yet (…) — open rexenv and press
  Start all once" health toast. Put the binary back afterwards. A warm-cache reboot is
  the control: everything comes up with no prompt because the boot daemon already
  serves the edge. **Open: no run recorded.**
- [ ] **DNS outlives the app** (ledger #46): quit rexenv →
  `dig foo.rex @127.0.0.1 -p 15353 +short` still answers `127.0.0.1`; sites keep
  resolving indefinitely (not just while caches last). **◐ the agent was seen answering
  with the app not running** — 18 Sep 2026 (clean VM, after the reboot, before the app was
  opened) and 7 Sep 2026 (dev Mac, across the update's quit and relaunch). **Open: the
  quit-then-`dig` leg itself is not recorded.**
- [ ] **Firefox, fresh profile** (ledger #154): a site loads with the lock — no cert
  warning, no about:config surgery. **Open: no run recorded** (the ledger's own verdict leaves
  Firefox's behaviour to this row).
- [ ] **Firefox opens a typed bare `name.rex`** (ledger #706): after onboarding (or Settings →
  Firefox → "Open typed addresses in Firefox" → Enable, then restart Firefox), type `acme.rex`
  with no slash → the site, not a search. **Tell:** a Google results page — check the profile's
  `user.js` for `browser.fixup.domainsuffixwhitelist.rex`. In Chrome/Safari the same text still
  searches (expected); the default-TLD card and the domain's tooltip on a site page say so.
  **◐ 22 Sep 2026 (dev Mac, one-off, ledger #706):** headless Firefox 156 on a throwaway
  profile, asked through WebDriver BiDi what `Services.uriFixup` does with the typed text —
  without the line `acme.rex` → a Google search; with the exact line rexenv writes → `URL
  http://acme.rex/`. **Open: a person typing it into a real Firefox window.**

## Scale
- [x] With ~15+ sites the Sites list, search, and status footer stay responsive.
  ✓ 18 Sep 2026 (clean UTM VM, macOS 15.6.1 arm64, build 39610cc7 + self-update to 0.7.2).

## Clean uninstall — §3
- [x] Settings → Uninstall → **Remove rexenv's system changes**; confirm. ✓ 18 Sep 2026 (clean
  UTM VM, macOS 15.6.1 arm64, CLT-less, build 8437afd9). ✓ 23 Sep 2026 (clean 15.8 VM, the
  first 0.8.7 dmg `cb17756a…`) and 24 Sep (same VM, the fixed dmg `8e3f78db…`).
- [x] After: `ping foo.rex` no longer resolves; the local CA is no longer trusted (no cert
  warning is moot — it's gone); no rexenv services running. ✓ 18 Sep 2026 (clean VM, build
  8437afd9). ✓ 24 Sep 2026 (15.8 VM, the fixed 0.8.7 dmg): `rex status` `DNS DOWN (removed, udp
  15353) · resolver MISSING · CA NOT TRUSTED`, nothing on UDP 15353, agent job and edge daemon
  gone.
- [x] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted).
  ✓ 18 Sep 2026 (clean VM, build 8437afd9).
- [x] **Then wait a minute with the app open, watching the toasts and `rex status`** (ledger
      #715). Nothing must happen: no "DNS stopped unexpectedly — restarted automatically",
      and `rex status` reads `DNS … (removed, udp 15353) · resolver MISSING`. **Tell:** that
      toast three times, then `answering (in-process)` — the watchdog treating the uninstall
      as a crash and serving the DNS you just removed. That was 0.8.7's first dmg on the
      clean-15 VM, 23 Sep 2026 (`health.log`: three `kicked it`, then `serving in-process`).
      ✓ 24 Sep 2026, same VM, the fixed dmg (`8e3f78db…`, source `34bd0d7e`): 80s after Remove,
      no toast, `health.log` unchanged, `rex status` `DNS DOWN (removed, udp 15353) · resolver
      MISSING · CA NOT TRUSTED`, nothing on UDP 15353, agent job and edge daemon gone.
- [x] **`/usr/local/bin/rex` is removed by the uninstall, inside the one admin prompt** (ledger
  #677). It was left behind on the first VM uninstall of 18 Sep 2026: `/usr/local/bin` is
  root-owned on a clean Mac (the CLI install's own `mkdir -p` made it), so the unprivileged
  unlink failed silently. **Tell:** the link still there after Remove, pointing into the app you
  are about to drag to the Trash. ✓ 18 Sep 2026, same VM, fixed build: one prompt (its sentence
  now names the rex command), link gone, resolver gone.
- [x] **Stale CA trust entries are swept** (ledger #678). Every fresh app-data folder mints a new
  local CA; until 18 Sep 2026 the ones earlier folders trusted stayed trusted roots (four on the
  VM after four hand wipes) and the uninstall untrusted only the current one. Now trusting a CA
  sweeps the others and the uninstall untrusts all of them. **Tell:** `security
  dump-trust-settings` listing more than one `rexenv Local CA` after setup. ✓ 18 Sep 2026, same
  VM: 4 trusted / 5 in the keychain before the Domains step → 1 / 1 after, ONE keychain dialog,
  log `untrusted 4 stale rexenv CA(s)`.

### PHP defaults are rexenv's, not PHP's (19 Sep 2026, ledger #694)
- [ ] On a fresh install, Settings → Services → PHP → ini settings shows every field EMPTY with
  placeholders 1G / 5G / 8G / 60 / -1 / 5000, and a site's `phpinfo()` (or `php -i` through the
  pool — not the CLI) reports exactly those: `memory_limit 1G`, `upload_max_filesize 5G`,
  `post_max_size 8G`, `max_input_vars 5000`, `max_execution_time 60`. **Tell:** 128M or 2M
  anywhere — the defaults are placeholders again, not written. **Open: no run recorded** (ledger
  #694 is L0; the clean-15 pass of 23 Sep did not record it).
- [ ] A 200 MB upload through a site (WP media, or a plain form) is accepted by nginx AND PHP —
  the nginx global `client_max_body_size` is derived from the same defaults (8G). **Tell:** a
  413 from nginx on a body PHP would take. **Open: no run recorded** (ledger #694: "NOT proven
  live").

## macOS 13 and 14 — the legacy tiers (`docs/PLAN-macos-13-floor.md`, T7)

Run the Core sections above on a **clean macOS 13.7 VM and a clean macOS 14 VM** (the UTM
recipe from "Cold first run", arm64). What differs there, and what must be seen:

Environment: macOS 13.6 (22G120) arm64, UTM (CLT-less at first, CLT installed later the same day) · rexenv 0.8.6-dev (2d1e1bc4 + the T7 fixes), then **the shipped `rexenv_0.8.7_universal.dmg` (`a8b9dce2`, sha `6931094a…`) on the same VM at 18:50**: 9/9 services, Databases 3/3 running (MySQL 8.4.3, MariaDB 12.0.2, Redis 8.2.1), PostgreSQL's muted row, all four sites HTTPS 200, `crash.log` empty · run 23 Sep 2026 over ssh + JXA clicks; **the macOS 14 VM has not been run**

- [x] **The app LAUNCHES and draws a window.** Not a formality: the first build put on a
      13.6 VM (23 Sep 2026) aborted in `applicationDidFinishLaunching` on a macOS-14-only
      selector (`-[TaoApp activate]`, ledger #713), with `crash.log` saying only "panic in a
      function that cannot unwind" — `~/Library/Logs/DiagnosticReports/rexenv-*.ips` and a
      Terminal launch (`/Applications/rexenv.app/Contents/MacOS/rexenv`) show the real
      message. Any newer-than-13 AppKit/WebKit selector the app sends will look like this.
      ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less, build 2d1e1bc4 + the
      activation/gate fixes, driven over ssh + `rex`): the fixed build launches, `rex open`
      answers "window opened", no `crash.log` entry.
      (A "rexenv quit unexpectedly" seen once during a HAND swap of the bundle over ssh —
      `SIGKILL (Code Signature Invalid)` at `_dyld_start` — was the DNS agent's KeepAlive
      relaunching into a half-copied bundle, not the app: the in-app updater swaps atomically.)
- [x] **Onboarding's Welcome step shows the legacy note once**: "This Mac runs macOS 13.x —
      rexenv works here with older versions of some components and without PostgreSQL and
      PHP 8.0. Everything is available on macOS 15 or later." (on 14: "…without PostgreSQL…"
      is absent; PostgreSQL 16.4 is offered). On a 15+ Mac the note must NOT appear.
      ✓ 23 Sep 2026 (macOS 13.6 VM, eyes-on over ssh screenshots + JXA clicks, build with the
      four T7 fixes): the note read exactly "This Mac runs macOS 13.6 — rexenv works here with
      older versions of some components and without PostgreSQL and PHP 8.0. Everything is
      available on macOS 15 or later." (a warning-tinted card under the tagline); the wizard
      then completed — components READY, the branded keychain prompt, "Your kingdom is ready".
      ✓ 23 Sep 2026 (clean 15.8 VM, the 0.8.7 dmg): Welcome with no legacy note (this file's
      header run).
- [x] **Cold first run installs the LEGACY pins**: Settings → PHP shows the standard minors;
      the Services/Databases rows, once started, report MySQL **8.4.3**, Redis **8.2.1**,
      MariaDB **12.0.2**; Apache **2.4.65** on a site that picks it; a share opens on
      cloudflared **2025.4.0** (Tunnels page shows the version). ✓ 23 Sep 2026 (clean UTM VM,
      macOS 13.6 arm64, CLT-less, build 2d1e1bc4 + the activation/gate fixes, driven over ssh +
      `rex`): `rex db versions` offers MySQL 8.4.3 / 8.0.40, MariaDB 12.0.2 / 11.4.8, Redis 8.2.1
      and NO PostgreSQL; a Blank-PHP site created over `rex` downloaded and RAN mysql-8.4.3,
      nginx-1.30.4, php-fpm 8.3, Caddy, Mailpit on the 13.6 host — `rex status` all running, the
      site answered HTTPS 200 with the rexenv starter title; the Xdebug toggle started the 8.3
      debug pool from `xdebug-8.3-3.4.5`. A public share from the Tunnels page ran on
      **cloudflared 2025.4.0** and the trycloudflare URL answered 200 with the site's title from
      the host Mac (later the same day). With the CLT installed on the VM (later the same day,
      150 s), the bottle bundles relinked and ran on 13.6: an **Apache** site served 200 with
      `Server: Apache/2.4.65 (Unix)`, **MariaDB 12.0.2** started from the Databases socket call.
      **Redis 8.2.1 did NOT start** at first — a real defect, not a tier one: the session's
      `LANG=C.UTF-8` is a locale macOS 13 lacks and Redis exits on it (ledger #714, fixed by
      spawning with `LC_ALL=C` — the fixed build's Redis reached "Ready to accept connections"
      on the same VM minutes later). Without CLT the bundles refuse with the `xcode-select
      --install` sentence, as on 15.
- [x] **PostgreSQL on 13**: the Databases page lists it as a muted row reading "Needs macOS 14
      or later — this Mac runs macOS 13.x."; New Site → Laravel/Blank shows the same sentence
      under the engine picker and offers no PostgreSQL option; `rex db versions --set postgres …`
      refuses with the same words. ✓ 23 Sep 2026 (clean UTM VM, macOS 13.6 arm64, CLT-less,
      build 2d1e1bc4 + the activation/gate fixes, driven over ssh + `rex`) — the CLI legs:
      `rex site create … --db postgres` → "PostgreSQL: Needs macOS 14 or later — this Mac runs
      macOS 13.6."; `rex db versions --set postgres 16.4.0` → the same sentence (the FIRST build
      answered the platform sentence there — fixed, ledger #710). The GUI rows, seen the same
      day once Screen Recording + Accessibility were granted to `sshd-keygen-wrapper`: the
      Databases page lists PostgreSQL as a muted last row reading "Needs macOS 14 or later —
      this Mac runs macOS 13.6." under MySQL 8.4.3 / MariaDB 12.0.2 / Redis 8.2.1; New Site →
      Configure shows "PostgreSQL: Needs macOS 14 or later — this Mac runs macOS 13.6." under
      the engine picker and offers no PostgreSQL option. **Two UI defects found there and fixed
      the same day:** the dialog ALSO showed the PDO note ("the PHP 8.3 build has no working
      pdo_pgsql") because one boolean carried both reasons; and the PHP list appended 8.0 after
      8.5 instead of in order.
- [ ] **The MCP create tool refuses PostgreSQL on 13** with the same words. **Open: no run
      recorded** (the 23 Sep legs were `rex` and the GUI).
- [x] **PHP 8.0 on 13**: Settings → PHP keeps the 8.0 row, dot grey, Install disabled, a chip
      with the sentence; `rex php install 8.0` refuses with it. ✓ 23 Sep 2026 (clean UTM VM,
      macOS 13.6 arm64, CLT-less, build 2d1e1bc4 + the activation/gate fixes, driven over ssh +
      `rex`) — `rex php list` carries the 8.0 row with `unavailableReason` set and `installed:
      false`; `rex php install 8.0` → "PHP 8.0: Needs macOS 14 or later — this Mac runs macOS
      13.6."; the GUI, seen the same day: Settings → Services lists the 8.0 row in place (between
      7.4 and 8.1, after the ordering fix), dot grey, a chip "Needs macOS 14 or later — this Mac
      runs macOS 13.6." beside the EOL chip, Install disabled.
- [ ] **New Site never offers 8.0 on 13.** **Open: no run recorded** (the 23 Sep GUI pass
      recorded New Site's PostgreSQL note, not its PHP list).
- [ ] **Xdebug on 13**: the toggle works on 8.1–8.4 (pool banner "with Xdebug v3.4.5"); on
      8.5 the toggle is off with the "no bottle pinned yet" reason. **◐ 23 Sep 2026 (13.6 VM):
      the toggle started the 8.3 debug pool from `xdebug-8.3-3.4.5`** (the cold-first-run row
      above). **Open: 8.1, 8.2, 8.4 and 8.5's reason.**
- [x] **WordPress and Laravel sites serve over HTTPS** exactly as the Core sections say. ✓ 23 Sep
      2026 (macOS 13.6 VM, over `rex`): `legacy-wp.rex` (WordPress, one-click install on MySQL
      8.4.3) and `legacy-lv.rex` (Laravel, composer + migrations on MySQL 8.4.3) both created and
      answered HTTPS 200 (`curl --resolve`, the CA not yet trusted on that VM); the Blank-PHP site
      too. No `crash.log` entry after any of it.
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
      `serving`, `https://legacy-wp.rex` 200 in Safari. **Not seen, by construction**: the
      Welcome note — an upgraded install never shows Welcome again (`legacy_notice` is `None` on
      Standard, L0). **Found there**: the app said `DNS DOWN` for two hours while the agent
      answered every query (#442 leg 4, fixed in the next build) — and the edge-blocked/unblocked
      toast pair fires during the app's OWN reload at site create (`docs/TODO.md`; fixed 29 Sep
      2026, ledger #749 — the wire probe now needs two consecutive misses, so the next run's
      site creates must raise NO `edge-blocked` toast, while a Herd-style `127.0.0.1:443` bind
      still raises one within ~20 s).
- [ ] **The whole list above on a clean macOS 14 VM** — the Welcome note without "…without
      PostgreSQL…", PostgreSQL 16.4 offered, PHP 8.0 not refused, and the legacy-pins row as it
      reads on 14 ("On 14 only cloudflared moves"). **Open: the macOS 14 VM has not been run** (`docs/TODO.md`'s "macOS
      13 floor" row stays open for it).

## Windows — what this checklist means on that OS (reconciled 28 Sep 2026)

Run everything above on Windows too, EXCEPT what this section changes or removes. The
macOS file is written against a Mac's mechanisms (a `.dmg`, Gatekeeper, `/etc/resolver`,
the login keychain, a root LaunchDaemon, the Dock); Windows reaches the same user-visible
promises by different machinery (`docs/PLATFORMS.md` §3, §7), and a step that names a Mac
mechanism is not a step a Windows tester can pass or fail.

**Where this section has run** (the ✓ rows name which): the **clean Windows 11 Pro 24H2 VM**
(26100.4349 — UTM on the Mac, an ARM guest running the x64 build under emulation, so timings
are not a PC's; D6) — runs 1–6, 19–21 Sep 2026, driven from a screenshot-guarded click helper
over SSH; and the **Dell, Windows 10 22H2 x64** (the best-effort floor) — the `rex` copy (16
Sep), the installer's running-app sentence (21 Sep) and the 0.8.5 → 0.8.8 in-app update (28
Sep). Until 28 Sep each ticked row here carried its evidence and then the original step text
glued on after it, and three rows still described states later runs had closed; reconciled
into one row per step, with each open row saying what it is waiting for.

**Still open on Windows, in one list:** Firefox's typed addresses (#706 — proven on macOS
only) · Chrome's download wording (no Chrome on the VM) · the one command under Smart App
Control ON (no machine has it on) · WebView2's bootstrapper on a machine
without it · the certificate dialog's **No** path · the tray's LEFT click · uninstalling an
UPDATED copy with the in-app step first · Apps & Features without the in-app step leaves a
live agent, and a pre-#695 `.bak` nobody sweeps (both TODO W12 rows).

Environment: Windows ____ (11 x64 supported · 10 22H2 best-effort — D6) · rexenv version ____

### Typed addresses in Firefox (ledger #706 — proven on macOS only)
- [ ] Settings → Firefox → **Open typed addresses in Firefox** → Enable; restart Firefox; type
      `acme.rex` with no slash → the site opens. The `user.js` under
      `%APPDATA%\Mozilla\Firefox\Profiles\…` carries the line. The Settings hint and the
      site-page tooltip name **Chrome and Edge**, never Safari. **Open: no Windows run recorded.**

### Windows-only rows found on real machines (20–21 Sep 2026)
- [x] **PostgreSQL starts — including with UAC OFF.** Services → start PostgreSQL: it reaches
      Running and `netstat -ano | findstr :15432` shows it LISTENING — also on a machine with
      UAC disabled (`EnableLUA=0`, where EVERY process carries the Administrators token),
      because rexenv launches it through `pg_ctl` (#698). Stop all afterwards: the port must be
      FREE (a hard kill left backend processes holding it, which is why shutdown goes through
      `pg_ctl stop`). ✓ 20 Sep 2026 (Win11 VM, `EnableLUA=0`, 0.8.5): **7 of 7 running**,
      `:15432` listening, `health.log` empty a minute later (#701 stops the watchdog racing it).
      **Tell:** "PostgreSQL did not start within 15s", then three watchdog restarts and
      `gave-up`, with `postgres-stdout.log` saying "Execution of PostgreSQL by a user with
      administrative permissions is not permitted".
- [x] **The Database Browser renders.** Databases → Browse on MySQL: Adminer appears INSIDE the
      app (table list, not an empty white/grey panel) and its buttons work. ✓ 20 Sep 2026 (Win11
      VM, 0.8.5): Adminer 6.0.2 inside the app, auto-logged-in as `root@localhost`, listing
      `wp_lm_rex`. It took three fixes that all had to be right — the frame's URL (#703), the
      app's own `frame-src` (#702) and Adminer's replayed `frame-ancestors` (#699). **Tell:** a
      blank frame with the URL bar above it filled in — the CSP refused the app's origin (#699),
      and nothing in any log says so.
- [x] **No console window, ever.** Start all (PostgreSQL included), open Tunnels, share a site,
      Browse a database, create a WordPress site: NO terminal / console window appears at any
      point, and none is left on the desktop afterwards (#705). ✓ 21 Sep 2026 (Win11 VM, the
      fixed build, a 200 ms window watcher): Start all with PostgreSQL, a share started and
      stopped, a WordPress site created and deleted, Tunnels, Adminer on PostgreSQL — none.
      **Tell:** an empty Windows Terminal titled `…\pg_ctl.exe` or `…\php.exe` (0.8.4 and 0.8.5
      on the VM). Do NOT close a pg_ctl one to "tidy up": PostgreSQL inherited that console and
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
The installer is `rexenv_<X.Y.Z>_x64-setup.exe` — NSIS, per-user, UNSIGNED by ruling (D5: open
source, no income, no spend, the same answer macOS got). Built by CI from the release tag since
0.8.8; runs 1–2 below used `pnpm release:win` builds (`276fc7cb`, then the guard build
`46ccefed` installed over it). Run 1 found two bugs the Dell could not (#691, #692).
- [x] **Download through a browser**, so the file carries the Mark of the Web, and record each
      browser's own warning verbatim — **never write SmartScreen steps from memory.** ✓ 19 Sep
      2026 (Win11 VM, Edge): `ZoneId=3`, sha256 matched the build. SmartScreen appeared,
      verbatim in `docs/INSTALL.md`: "Windows protected your PC" with **Run anyway behind More
      info**, then straight into the NSIS wizard — no "Open File - Security Warning" behind it.
      **A second build (run 2) repeated both screens verbatim** — a new hash is a new stranger.
      (The Dell's earlier "Open File - Security Warning" with Run on the first screen is that
      machine's `EnableSmartScreen = 0` policy, not what a user meets.)
- [ ] The same download through **Chrome**: its shelf / download-bubble wording, verbatim.
      **Open: no Chrome on the VM; the Dell HAS Chrome (28 Sep 2026) but it will not start
      from the desktop session's scheduled task — `Start-Process` returns a pid that is gone
      within 8 s, 0 `chrome` processes, no window — so its wording could not be captured
      there either. A machine with a Chrome that opens is what this row waits for.**
- [x] **The one command** (`irm https://rexenv.rex.bd/install.ps1 | iex`) installs per-user with
      no UAC — `%LOCALAPPDATA%\rexenv` with `rexenv.exe`, `rex.exe`, `uninstall.exe`, a desktop
      and a Start-menu shortcut, the HKCU entry — after the checksum; a second run says "already
      installed". ✓ 28 Sep 2026: Win11 24H2 ARM VM as a temporary STANDARD user (the owner's 0.8.5
      install untouched; user and profile removed after), Windows PowerShell 5.1, `iex` of the
      script text, 20 s, the Windows-on-Arm note printed; the Dell (Win10 22H2) took the
      already-installed path, and the caller's session kept its `$ErrorActionPreference` and
      gained no functions. `Invoke-WebRequest` wrote no `Zone.Identifier` on either machine.
- [x] **No SmartScreen dialog, SEEN.** In PowerShell on the VM's DESKTOP (SmartScreen on), for a
      user with no rexenv, run the command and screenshot. ✓ 29 Sep 2026 (Win11 24H2 VM, a
      temporary standard user signed in at the console, an interactive `-NoExit` Windows
      PowerShell 5.1 in Windows Terminal): installed in about a minute, no dialog on screen, no
      click. **Negative control, same session:** the same `setup.exe` given a synthetic
      `Zone.Identifier` (`ZoneId=3`) met **"Open File - Security Warning"** — *"The publisher could
      not be verified. Are you sure you want to run this software?"* · Publisher: Unknown
      Publisher · [Run] [Cancel] — so the mark-keyed gate was live there (not the "Windows
      protected your PC" wording Edge's download met on 19 Sep; the difference was not
      explained). **The first desktop run found a defect every automated run had missed:** in an
      interactive PowerShell the old architecture check read `$null` (PSReadLine 2.0.0 shadows
      `RuntimeInformation`) and refused with *"this machine is ."* — fixed in the tap's #2, this
      run is the fixed script. Launch skipped (`REXENV_NO_LAUNCH=1`) so a second user's rexenv
      would not re-register the machine-wide `\rexenv\dns-agent` task (TODO row).
- [ ] **Smart App Control ON.** On a Windows 11 where `Get-MpComputerStatus` says
      `SmartAppControlState: On`, run the command and record verbatim what SAC does to the unsigned
      installer. **Open: no machine CAN have it on here.** SAC turns On only after a clean install's
      evaluation; on the Win11 VM (24H2, 26100.9457) a hand-set
      `HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy\VerifiedAndReputablePolicyState = 1` held until
      the next policy refresh and read `0` again after a reboot — SAC stayed `Off` (29 Sep 2026).
      **What stands in, and what it does not cover:** SAC's policy runs PowerShell in
      ConstrainedLanguage mode, where the fresh-install path used to die at its first .NET call
      (*"Cannot set property. Property setting is supported only on core types in this language
      mode."*, measured on the Dell); `install.ps1` now refuses in a sentence instead (run on the VM
      with the mode set by hand, and in CI), and a stand-in `Get-MpComputerStatus` reporting `On`
      makes it print its Smart App Control note (CI). What Windows itself shows under SAC is the
      part still unseen.
- [x] **The install asks for NO admin.** Per-user (`installMode: currentUser`): it lands in
      `%LOCALAPPDATA%\rexenv` — `rexenv.exe`, `rex.exe`, `uninstall.exe`, nothing else — and the
      uninstall entry is under **HKCU**, so Apps & Features lists rexenv with `DisplayVersion`
      equal to the release. ✓ 19 Sep 2026 (VM): no UAC at any wizard page; `rex.exe`
      (1,009,152 B); `HKCU\…\Uninstall\rexenv` with `DisplayVersion 0.7.0`, `InstallLocation`
      quoted, `Publisher rexenv`; Start Menu `rexenv.lnk`; desktop shortcut from the Finish
      page. **Tell:** a UAC prompt during install, or an entry under HKLM — the installer was
      built per-machine, which is the mode D5 refused.
- [ ] **WebView2.** On a machine WITHOUT the WebView2 runtime (check Apps & Features first), the
      installer's `downloadBootstrapper` fetches it; record whether that step asked for admin.
      **Open: every machine so far already had WebView2** (the VM had 153).
- [x] The app starts from the Finish page, the Start Menu entry and Explorer and shows its
      window; no console window appears behind it (a `windows_subsystem` regression shows as a
      black console). ✓ 19 Sep 2026 (VM): window up, onboarding "Welcome", no console.
- [x] **The copy the Finish page starts can start services.** With the guard (#692) the
      Finish-page copy hops through Explorer once (a brief flash, one window) and Start all
      works. ✓ run 2 (VM): its parent is a fresh `explorer.exe`, `rex --version` reads
      `46ccefedb4`, Stop all → Start all from the tray: 5/5 running. Before #692 "Run rexenv" left
      it in a job with limits `0x0` and Start all failed on `mysqld` with the #600 access-denied
      wording. **Tell:** the access-denied toast, or two rexenv windows.
- [x] **`rex` on the PATH** — Settings → General → Command-line tool → Install (no prompt); a NEW
      terminal answers `rex status`; the copy is `%LOCALAPPDATA%\rexenv\bin\rex.exe`. ✓ run 2
      (VM): the user `Path` gained `%LOCALAPPDATA%\rexenv\bin`, `rex.exe` copied there
      (1,009,152 B), `rex status` answered; the card then reads "Installed — … Reinstall".

### In-app self-update — replaces the macOS section
The swap and the relaunch are measured on fixtures on the Dell
(`windows_app_bundle_swap_check`, `windows_app_relaunch_check` — `docs/TESTING.md`); what no
fixture can do is the real install directory, the real quit gate and the real registry entry.
The first installed copy (`276fc7cb`, before per-OS descriptors) read the macOS descriptor and
logged "this Mac's macOS version could not be read"; builds from `a0d4868f` on fetch
`app-manifest-windows.json`, and that sentence is OS-neutral.
- [x] With an older build installed and a newer release published (its `rexenv_<X.Y.Z>_x64.zip`
      attached and `app-manifest-windows.json` signed on `rexenv/runtimes`), Settings → About
      offers the update; the consent sentence is the Windows one ("replaces rexenv's program
      files (your data folder is not touched)" — no "rexenv.app", no Applications folder, no
      Apple re-prompt line). ✓ 19 Sep 2026, runs 3–4 (VM, 0.7.9 → 0.8.0, then → the published
      0.8.2). ✓ **28 Sep 2026 on the Dell (Win10), the installed 0.8.5 → the published 0.8.8.**
- [x] **An install directory another account owns is refused with the copy-paste fix, never a
      prompt** (#693). ✓ 28 Sep 2026 (Dell): the 0.8.5 install had come through the elevated SSH
      token, so the folder was `BUILTIN\Administrators`'; the offer came with "…`C:\Users\DELL\
      AppData\Local\rexenv` belongs to another account … `takeown /R /F` …"; after that
      `takeown` as the desktop user, Install went through. (Run 3 on 19 Sep met the same refusal
      in macOS's words — `sudo chown` — which is what #693 fixed.)
- [x] **The app says it is about to close, and waits.** After Install finishes, a dialog names
      the new version and says rexenv will close and open again, what keeps running, what closes
      with it, and what dismissing means. ONE button (OK) — no Cancel; nothing happens until you
      press it; pressing it quits and the app reopens on the new version. ✓ 20 Sep 2026 (VM,
      0.8.4 → 0.8.5): "rexenv 0.8.5 is installed — rexenv will now close and open again on 0.8.5
      …", one button, OK quit and reopened it. ✓ 28 Sep (Dell, → 0.8.8). **Tell:** the window
      vanishing on its own the moment the install finishes (#700, fixed 20 Sep 2026), or a
      dialog whose OK does nothing.
- [x] **The quit gate still asks, and rexenv reopens on its own.** Press Install with a live
      share: the archive downloads into the ONE download hub, the app quits through the quit
      gate, and rexenv **reopens on its own** on the new build — the relauncher waited for the
      old process, not merely for a timer. ✓ 21 Sep 2026, run 6 (VM, the published 0.8.4 → 0.8.5,
      a live share on `lm.rex`): OK raised **"Stop sharing? Quitting stops 1 public share — its
      link goes dead immediately."** [Quit] [Keep sharing] → Quit → rexenv reopened, ONE window,
      toast "rexenv is now 0.8.5 (updated from 0.8.4)"; the share was gone. **Tell:** two rexenv
      windows, or none.
- [x] **Afterwards on disk and in Apps & Features.** `%LOCALAPPDATA%\rexenv` holds the new
      `rexenv.exe`; the previous build (the three FILES, not a directory — the data tree under
      `%LOCALAPPDATA%\rexenv\rexenv\data` never moves, #695) sits in
      `.rexenv-update-<pid>\previous` until the next launch sweeps it (it cannot be deleted while
      the old process runs — measured); `uninstall.exe` is still there (carried across); **Apps &
      Features shows the NEW version** (#696). ✓ run 6 (VM): `rexenv.exe` 0.8.5, no
      `.rexenv-update-*` left, `uninstall.exe` still there, `DisplayVersion` 0.8.5, `rexenv.db` in
      place. ✓ 28 Sep (Dell): `swapped to 0.8.8 (RenamePair)`, Apps & Features `0.8.8`, the
      previous bundle gone. History: runs 4–5 (19 Sep) left Apps & Features on the OLD version —
      a version read off a just-renamed path; #696 reads the STAGED executable, proven by run 6.
- [x] **The updated copy still resolves `.rex` and still serves HTTPS:** the DNS agent is
      re-launched onto the new binary and the CA does not change. ✓ run 6 (VM): agent restarted
      (new pid, its exe reads 0.8.5), CA thumbprint `1A6A20BE…` before and after, `lm.rex` →
      127.0.0.1 and HTTPS 200. ✓ 28 Sep (Dell): the agent restarted as 0.8.8 on udp 53.
- [ ] **Uninstalling the UPDATED copy:** Settings → Services → Uninstall (the in-app step)
      FIRST, then Apps & Features → Uninstall — the carried-across uninstaller removes the
      directory the swap put in place. **◐ 21 Sep 2026, run 6 (VM), done WITHOUT the in-app step:**
      `uninstall.exe /S` removed `rex.exe`, itself, the HKCU entry and both shortcuts and kept the
      data — but `rexenv.exe` stayed: the `\rexenv\dns-agent` task (which only the in-app step
      removes) re-ran the agent from it within the minute, so the file was locked, and the agent
      went on answering `:53` from an uninstalled app (TODO W12 row). **Open: the row as written —
      in-app step first — needs the Root-store DELETE dialog clicked and a re-onboarding after.
      Not driven on 28 Sep 2026: the in-app step and the re-onboarding raise Windows UAC on the
      secure desktop, which the screenshot-and-click driver cannot see or answer (the Win11 VM's
      finding), so it would leave the Dell's install half-removed until a person clicks — a
      person at the Dell runs this row and the certificate-dialog "No" row together.**
- [ ] A stray `rexenv-0.8.3.bak` (38 MB, 19 Sep) from a pre-#695 swap sits beside the app for
      good — the launch sweep looks only for `.rexenv-update-*`. **Open: TODO W12 row.**

### Where rexenv lives — replaces "The menu bar (no dock icon)"
- [x] rexenv is a **taskbar tray** app; right click opens the MENU (ledger #624, the owner's Q2
      ruling — the opposite of macOS, deliberately). ✓ 19 Sep 2026 (VM): "Stopped / Start all /
      Stop all / No sites yet / All sites… / Services / Databases / Mail / Tunnels / MCP server /
      About rexenv / Open rexenv / Quit rexenv" ("All running · 5 services" once started).
- [x] **Left click opens the WINDOW.** **✓ 28 Sep 2026 (Dell, Windows 10, 0.8.9):** the window
      closed with its title-bar ×, the tray overflow (`^`) opened, ONE left click on the rexenv
      icon in it → the `rexenv` Tauri window is back in front (its title listed among the
      desktop's windows; driven through a click that first checks the window under the cursor is
      the notification area or its overflow, never a blind click).
- [x] The tray icon is the **colour** icon, not a template glyph: it must be legible on a dark
      taskbar. ✓ 19 Sep 2026 (VM): the colour icon, in the overflow flyout (Windows 11 hides new
      tray icons there by default). **Tell:** a black square — macOS's `icon_as_template` leaking.
- [x] Closing the window leaves the app alive in the tray: `rex status` still answers and an MCP
      client keeps working. ✓ 19 Sep 2026 (VM): window closed, `rex status` answered, 5 running.
- [x] Every menu item that names a screen brings the window up on it, including from a window
      that was closed. ✓ 21 Sep 2026 (VM, 0.8.5): Services, Databases, Mail, Tunnels, All sites…
      — each opened the window ON that screen.
- [x] **Start rexenv at login is ONE toggle** (29 Sep 2026, ledger #739; the words name the
      notification area and Task Manager's Startup tab): Stop all, turn it on, sign out and in →
      `rexenv.exe --hidden` under `explorer.exe`, no window, and `rex status` lists the backends
      running with no click. Then Stop all and open the app from the Start menu → nothing
      starts (a launch you made is not a login). **✓ 29 Sep 2026, Win11 ARM VM, the re-cut 0.8.10 (`a470c1af`)
      installed over 0.8.9 with `setup.exe /S`:** one toggle with the Windows sentence; on → the Run
      key `"…\rexenv.exe" --hidden`; a one-shot autologon (`AutoLogonCount 1`, every Winlogon value
      removed again after — they were absent before) and a reboot → `rexenv.exe --hidden` under
      `explorer.exe`, "launched at login — staying in the notification area", all seven services
      running incl. Caddy, `lm.rex` / `lv.rex` 200, a stale `postmaster.pid` removed; `rex stop`, close,
      Start-menu open → 30 s later all idle. The handoff too: a hand-opened primary + a `--hidden`
      shortcut opened through Explorer → the second copy exited and the stack came up. **Trap for
      the next tester:** a `--hidden` launch from a Task Scheduler task is job-confined, hops
      through Explorer and LOSES its arguments (`docs/TODO.md`), so it reads as a plain launch —
      use a `.lnk` with the argument, opened by Explorer.

- [ ] **A confined `--hidden` launch stays a login launch** (ledger #742, 0.8.11): Stop all, quit;
      start `rexenv.exe --hidden` from a scheduled task (confining, so it hops through Explorer) →
      the copy Explorer starts shows no window and runs login-start (the backends come up), and
      `%LOCALAPPDATA%\rexenv\rexenv\data\config\hop-hidden` is gone afterwards. 0.8.10 opened a
      window and started nothing.

### First-run setup prompts — ONE elevated step, not three
macOS asks three times (resolver, keychain, ports 80/443). Windows asks twice, and one of
them is Windows' own dialog:
- [x] **One elevated (UAC) step** — the `.rex` NRPT rule (`Add-DnsClientNrptRule -Namespace .rex
      -NameServers 127.0.0.1`), after rexenv's own dialog. `.rex` only on a fresh machine; other
      TLDs get theirs on first use. ✓ 19 Sep 2026 (VM): "rexenv wants to add a DNS resolver so
      .rex sites open on this PC. Windows will ask for your permission next.", then ONE UAC, then
      `Get-DnsClientNrptRule` = `.rex → 127.0.0.1`.
- [x] **Windows' own certificate dialog** for rexenv's local CA, into **this user's** Root store
      (never LocalMachine — ledger #613). ✓ 19 Sep 2026 (VM), verbatim: "You are about to install
      a certificate from a certification authority (CA) claiming to represent: rexenv Local CA …
      Thumbprint (sha1): 1A6A20BE … Do you want to install this certificate? [Yes] [No]"; after
      Yes, `Cert:\CurrentUser\Root` holds `CN=rexenv Local CA` with that thumbprint, NotAfter 2036.
- [ ] The certificate dialog's **No** reads as a cancel, and setup offers the step again rather
      than continuing as if it succeeded. **Open: the No path was not driven — it needs a fresh
      onboarding on a Windows machine with someone at the keyboard (UAC on the secure desktop;
      see the uninstall row above).**
- [x] **NO third prompt for ports 80/443** — the pinned `caddy.exe` binds `:443` and `:80` under
      the unelevated desktop token (measured on the Dell). ✓ 19 Sep 2026 (VM, unelevated user,
      Explorer-started copy): `rex start` → Caddy on `127.0.0.1:443` and `:80`, no UAC, no
      dialog. **Tell:** a UAC prompt for the edge — something reintroduced a privileged bind.
- [x] **No Windows Defender Firewall alert.** The edge binds `127.0.0.1` only (owner's ruling 14
      Sep 2026); an all-interfaces bind raised "Windows Security Alert" on the Dell. ✓ 19 Sep 2026
      (VM): none; `netstat` shows `127.0.0.1:443`, `127.0.0.1:80`, `127.0.0.1:18088`. **Tell:**
      that alert appearing — `default_bind` was lost, and sites would be reachable from the LAN.

### DNS — the agent on :53, not :15353
- [x] The port is the platform's `RESOLVER_PORT`, **53 on Windows** (NRPT has no port field —
      D2), so a line saying 15353 is macOS's number leaking. ✓ 19 Sep 2026 (VM): `rex status`
      reads `DNS answering (agent, udp 53) · resolver installed · CA trusted`; `rex doctor` ✓ on
      DNS, resolvers, TLDs, edge ("answering as rexenv on :443"), ports — its one finding
      `rex not on PATH`, as designed before the Settings install.
- [x] **DNS outlives the app**: quit rexenv, and `Resolve-DnsName probe.rex -Server 127.0.0.1`
      still answers `127.0.0.1`. The agent is a **scheduled task**, `\rexenv\dns-agent`, run at
      logon — not a LaunchAgent. ✓ 19 Sep 2026 (VM): after Quit from the tray only `rexenv.exe
      --dns-agent` remains (task Running, Interactive only), `probe.rex → 127.0.0.1`, all 16
      service processes kept running; `rex status` says "rexenv isn't running — open the app
      first". A DELETED task is re-registered by the watchdog's next tick (#704, VM 20 Sep).
- [x] **The `hosts` file is never touched** — rexenv never overwrites a file somebody else owns,
      and D2 refuses the fallback. ✓ 19 Sep 2026 (VM): zero `.rex` lines. **Tell:** any `.rex`
      entry in `C:\Windows\System32\drivers\etc\hosts`.
- [x] **`.rex` is answered by THIS machine's agent, never by the upstream DNS** — stop the agent
      and `Resolve-DnsName anything.rex` must FAIL, not answer. Why it is a row: a VM's upstream
      is its host, and a host running rexenv answers `.rex` with `127.0.0.1` itself — that is how
      the Linux route shipped dead behind passing checks (SMOKE Linux P1, #734). ✓ 28 Sep 2026
      (Win11 VM, 0.8.5, upstream `192.168.64.1` = the Mac running rexenv): NRPT `{.rex} →
      127.0.0.1`; with no agent `anything.rex` → "An existing connection was forcibly closed";
      `rexenv.exe --dns-agent` started → `127.0.0.1:53` held by it, `anything.rex` and
      `sub.site.rex` → `127.0.0.1`, `example.com` public; agent stopped → fails again. NRPT does
      not fall back to the interface's server, so the Windows `.rex` rows above are the VM's own.

### Where things live on disk
- [x] App data: `%LOCALAPPDATA%\rexenv\rexenv\data` (with `config\`, `logs\`, `bin\` under it).
      ✓ 19 Sep 2026 (VM): `bin\` (adminer 5.4.2, caddy 2.11.4, mailpit 1.30.3, mysql 8.4.6, nginx
      1.30.4, php 8.3.32, each with `.pinned-digest`), `ca\`, `config\`, `logs\`, `rexenv.db`.
- [x] The `rex` CLI is a **copy** on the user's `Path` at `%LOCALAPPDATA%\rexenv\bin` (ledger
      #634) — beside the data tree, not inside it — not a symlink. ✓ 16 Sep 2026 on the Dell
      (the folder held `rex.exe` with the sidecar's SHA-256 — byte-identical, so a copy; the user
      `Path` gained exactly that one entry, `REG_SZ` kept; `where.exe rex` and a fresh desktop
      process's `Get-Command rex` both found it) and 19 Sep on the clean VM (run 2).

### Clean uninstall — replaces the macOS one
- [x] Settings → **Services** → Uninstall → **Remove rexenv's system changes**; the confirm
      reads "This stops all services, deletes every rexenv NRPT rule (.rex and any other TLDs),
      and untrusts the local CA (Windows will also ask for approval). Your sites and databases
      are kept."; then one UAC and Windows' own "Root Certificate Store — Do you want to DELETE
      the following certificate from the Root Store?" (subject, serial, both thumbprints). ✓ run 2.
- [x] After: `Resolve-DnsName foo.rex -Server 127.0.0.1` no longer answers, the `.rex` NRPT rule
      is gone (`Get-DnsClientNrptRule`), the `\rexenv\dns-agent` task is gone, the CA is out of
      the CurrentUser Root store, the `rex` copy and its `Path` entry are gone, no rexenv service
      is running. ✓ run 2: NRPT rules 0, CurrentUser Root 0 rexenv certs, task gone (`schtasks`
      "cannot find the path"), services 0, `Path` entry and `bin\rex.exe` gone. **`.rex` still
      resolved while the app was open** — the in-process fallback resolver holds `127.0.0.1:53`
      until Quit, by design; after Quit it does not.
- [x] Site files and `rexenv.db` remain under `%LOCALAPPDATA%\rexenv\rexenv\data` (not deleted).
      ✓ run 2.
- [x] **Apps & Features' `uninstall.exe`** — one page ("Uninstalling from: …\rexenv\", a "Delete
      the application data" box, unticked by default) → Uninstall → Close, no UAC: `rexenv.exe`,
      the HKCU entry, the Start Menu and desktop shortcuts are gone; the data tree stays. ✓ run 2.
      That run also left an EMPTY `%LOCALAPPDATA%\rexenv\bin`; the Settings uninstall now removes
      the folder when it empties (`remove_symlink_best_effort`, 19 Sep) — not re-run since.
- [x] **Installing OVER a running copy** names what is running and restarts it: "rexenv is still
      running: the app, or the small background resolver…" with the "starts again on its own"
      line (`src-tauri/nsis/English.nsh`). ✓ 21 Sep 2026 (Dell, Win10 22H2, installer
      7388104…): OK → the app closed, Setup completed, Finish reopened the app, the agent was back
      beside it. (Run 2 on 19 Sep showed Tauri's stock "rexenv is running! Click OK to kill it" —
      the agent IS `rexenv.exe` — which is why the sentence was replaced.)

## Linux — what this checklist means on that OS (reconciled 28 Sep 2026)

Run everything above on Linux too, EXCEPT what this section changes or removes. Ubuntu is the
supported target (22.04 LTS or newer, x86_64 or aarch64 — D-L4 of `docs/PLAN-linux-port.md`);
every mechanism below is systemd's, polkit's or the desktop's (`docs/PLATFORMS.md` §3, §8).
**P1 first, before anything else in this file** — it is the row that can break the tester's
own internet.

**Where this section has run** (the ✓ rows name which): the **Ubuntu 22.04 arm64 UTM VM** (the
installed deb, GNOME desktop, real polkit and systemd-resolved — 24–27 Sep 2026); the **Dell's
WSL2 Ubuntu 26.04 x86_64** (the rebuilt deb under WSLg, 27 Sep); a **fresh WSL Ubuntu 22.04.5
x86_64** (the CI-built floor deb, 27 Sep). WSL has no polkit agent and its `.rex` answer comes
through the Windows host's DNS — and the VM's comes through the Mac's (P1, measured 28 Sep) — so
**a `.rex` resolution counts only when pinned to `rexenv0`** (`resolvectl query -i rexenv0`), and
every polkit-dialog row counts only from the VM. The HTTPS, trust and serving facts below stand;
their `.rex` lookups before 28 Sep were answered by the host, not by rexenv's route.
Until 28 Sep this header said "nothing here has run yet"; the runs were recorded row by row
beneath it, and the unticked rows beside them described steps those runs had already done —
reconciled into one row per step, each open row saying what it is still waiting for.

**Still open on Linux, in one list** (re-derived from the unticked rows, 28 Sep 2026): P4 the
tunnel guard's normal-stop leg. (The `.deb` in-app update's relaunch closed 29 Sep 2026, 0.8.9 →
0.8.10.)
- [ ] **A slow edge unit at boot is waited for** (ledger #743, 0.8.11): toggle on, the edge unit
      enabled, reboot → login-start's log shows no "needs Start all" even if `rexenv-edge` comes up
      after it; the edge is adopted. After a Stop all (unit disabled) the line appears at once.
- [ ] **A route resolved lost reads as not installed, and setup brings it back** (ledger #741,
      0.8.11): `sudo resolvectl revert rexenv0` → `rex status` says the resolver is MISSING, the
      app's next launch opens its setup step → one polkit → `resolvectl status rexenv0` shows
      `Current Scopes: DNS`, `127.0.0.1:15353`, `~rex`, and `resolvectl query -i rexenv0 x.rex`
      answers. 0.8.10 said "resolver installed" throughout.
- [ ] **A power cut right after Start all leaves the edge able to start** (ledger #740, 0.8.11):
      Start all (one polkit), then `utmctl stop Ubuntu --kill` within 10 s → boot →
      `/etc/systemd/system/rexenv-edge.service` whole (not 0 bytes, not `masked`) and the login's
      Start all brings the edge up with no "needs Start all" line. 0.8.10 failed this exactly.

**Owed through the next release, inside ticked rows:** the AppImage's kept previous copy is swept
(its row's next run). A re-setup is ONE polkit dialog (#723) — **✓ 29 Sep 2026 on the re-cut 0.8.10 (`a470c1af`)**:
Settings → Remove system changes → ONE polkit (CA file, both NSS dbs, `rexenv0`, the markers and
both units gone; `rex status` "DNS DOWN (removed) · resolver MISSING · CA NOT TRUSTED"), then the
next launch's onboarding → "Linux will ask for permission (resolver + certificate)" → ONE polkit
→ "Domains & SSL are ready", CA back in both stores, `rexenv0` 192.0.2.53/32. **But `rexenv0` had
no DNS scope after that live re-setup** (`.rex` answered only because the VM's upstream — the Mac
— answers it); a reboot restored the scope. `docs/TODO.md`.

Environment: Ubuntu ____ (22.04+; x86_64 or aarch64) · package ____ (.deb / AppImage) · rexenv version ____

### Install — the one command (`docs/archive/PLAN-install-scripts.md`)

- [x] **apt, by hand** (`docs/PLAN-apt-repo.md`, ledger #745): the four lines of `docs/INSTALL.md`
      → `apt-get update` fetches `InRelease` + `Packages` from `rexenv.github.io/apt` → `apt-get
      install rexenv` installs the newest. ✓ 29 Sep 2026, 22.04 arm64 VM: 0.8.10 from the
      repository (`Get: … rexenv arm64 0.8.10 [15.3 MB]`), `--print-version` 0.8.10.
- [x] **`apt upgrade` moves rexenv**: ✓ 29 Sep 2026 (VM): `rexenv=0.8.9` from the repository →
      `apt list --upgradable` names 0.8.10 → `apt-get upgrade` → 0.8.10.
- [x] **A repository signed by another key is refused**: ✓ 29 Sep 2026 (VM): another key in
      `/etc/apt/keyrings/rexenv.gpg` → "NO_PUBKEY D2F2070D6DFA60C9 … the previous index files will be
      used"; the right key back → clean.
- [x] **`install.sh` on apt adds the repository**: ✓ 29 Sep 2026 (VM, the script before its push):
      fresh → "adding rexenv's apt repository" → "installing rexenv 0.8.10 from the repository";
      installed without the source → the source added, "the package itself is not touched"; a third
      run → nothing changed. The lag path (the repository behind a new release) is met by the
      tap's `install-scripts.yml` on every release publish.

- [x] `curl -fsSL https://rexenv.rex.bd/install.sh | bash` installs the `.deb` through `apt`
      (dependencies pulled in, `sudo` asked once), `rexenv --print-version` answers the release,
      and a second run says "already installed". ✓ 28 Sep 2026 in fresh containers, the script
      piped from a local copy: Ubuntu 22.04 arm64 as root; 24.04 arm64 as a sudo user with the
      package lists deleted (apt failed once, the script refreshed them and retried); 22.04 amd64
      (emulated). Ubuntu 20.04 is refused with the webview sentence; Fedora (no apt) got the
      AppImage in `~/Applications` plus the `libfuse2` hint.
- [x] **From a GNOME Terminal, on a desktop with no rexenv:** the command installs, `sudo` asks
      in the terminal, and rexenv starts into the tray. ✓ 29 Sep 2026 (the 22.04 VM's GNOME
      desktop; the release deb removed first and `sudo` made to ask for a password with a
      temporary sudoers drop-in, both undone after): `[sudo] password for rexenv:` appeared
      mid-pipe, apt installed, and — with the autostarted copy quit first — the script started a
      NEW `/usr/bin/rexenv` in its own session (`setsid`), window and tray up. **Tell:** with a
      copy already running, the launch only raises that copy's window (single instance) — which
      is what the first attempt showed, and why it was repeated with the app quit.

### P1 — DNS scoping (ledger #717, #734 — the claim the whole port rests on)
**Ask resolved WHICH link answered, never only what the answer was.** On a VM or under WSL the
upstream DNS is the host's — and a host running rexenv answers `*.rex` with `127.0.0.1` itself
(the Mac through `/etc/resolver/rex`, Windows through NRPT). Measured 28 Sep 2026 on the 22.04
VM with NO route and NO agent on it: `resolvectl query anything.rex` → `127.0.0.1 -- link:
enp0s1`. Every `.rex` answer this section recorded before 28 Sep came that way; the route
itself had no DNS scope (0.8.8 and earlier — see the second row). Use `resolvectl query -i
rexenv0 <name>` and read `Current Scopes:`.

- Prep, every run: before onboarding, `resolvectl status` shows a link with DNS servers and
  `resolv.conf` is the stub (`nameserver 127.0.0.53`); note `resolvectl query example.com`'s answer.
- [x] ✓ 24 Sep 2026 (22.04 arm64 VM, the installed deb). Onboarding → the system-setup consent → ONE polkit
      dialog: from the `.deb` it reads "rexenv needs administrator permission to change system
      settings…" (#723); from an AppImage or a dev build it names `/bin/sh`. After:
      `/etc/rexenv/dns.d/rex` exists (`nameserver 127.0.0.1` / `port 15353`), `ip link` shows
      `rexenv0`, `systemctl status rexenv-dns-route` is active (exited), and
      `resolvectl status rexenv0` lists `127.0.0.1:15353` with `~rex` and `-DefaultRoute`.
      **Measured: rexenv's own sentence in the dialog, `rexenv0` up, marker + unit present.**
- [x] **`rexenv0` has a DNS scope and answers `.rex` ITSELF:** `resolvectl status rexenv0` reads
      `Current Scopes: DNS` and carries `192.0.2.53/32` (`ip addr show rexenv0`);
      `resolvectl query -i rexenv0 anything.rex` → `127.0.0.1`. **`resolvectl query example.com`
      → the SAME public answer as before, never `127.0.0.1`.** `curl -I https://example.com`
      works. **✗ FAILED 28 Sep 2026 on 0.8.8 (22.04 arm64 VM):** `Current Scopes: none`,
      `-i rexenv0` → "No appropriate name servers or networks for name found" — resolved gives
      no DNS scope to a link whose only address is link-local (`fe80::`), so the route routed
      nothing and a lone Ubuntu machine never resolved `.rex`. The 24 Sep ✓ ("`a.rex` →
      loopback", 23/23 in `linux_dns_route_check`) was the host's answer. **Fixed on master 28
      Sep:** the link carries `192.0.2.53/32` (TEST-NET-1), and a route script older than the
      build reads as not installed, so setup rewrites it (#734). **L1 on the same VM, 28 Sep:
      `linux_dns_route_check` PASS 32/32** with the app's own commands — `Current Scopes: DNS`,
      `.rex`/`.test` answered through `-i rexenv0`, `example.com` public at every step; the
      0.8.8 script planted back → 4 named FAILs (scope, and the three pinned queries) while the
      unpinned `anything.rex → 127.0.0.1` still passed, which is how the old check passed.
      **✓ 28 Sep 2026 through the installed app — 0.8.9 (the tap's deb, in-app update over
      0.8.8) on the 22.04 arm64 VM:** prep — enp0s1 `Current Scopes: DNS`, `example.com` →
      `2606:4700:10::6814:179a`, rexenv0 `Scopes: none`, no IPv4. Launch → "launched at login
      with setup incomplete — showing the window" → onboarding again → step 3 "Set up domains &
      SSL" → polkit dialog with rexenv's own sentence → "Domains & SSL are ready" → "Your kingdom
      is ready". Reads: `Current Scopes: DNS`, `-DefaultRoute`, `inet 192.0.2.53/32 scope global
      rexenv0`, `resolvectl query -i rexenv0 anything.rex` → `127.0.0.1` (and `lm.rex`),
      `example.com` → the same public answer, `curl -I https://example.com` → `HTTP/2 200`,
      `rexenv-dns-route` active, marker `nameserver 127.0.0.1 port 15353`, `rex status`
      "resolver installed · CA trusted". **Two findings, TODO rows:** the re-setup showed TWO
      polkit dialogs, not one — the journal shows two `pkexec` runs, the route at 07:52 and
      `mkdir -p /usr/local/share/ca-certificates …` at 07:54 (the CA leg rewrote the system-store
      file even though "CA trusted" already held) — **fixed on master the same day, ledger
      #723: setup's root legs are one step, and the store leg is skipped when the store already
      holds the PEM; one dialog is owed a run through the next release**; and the finish page
      says "Create your first site" on an install that has three (fixed on master the same day:
      the page reads the sites count — "Open your sites" / "your 3 sites").
      History: P1 with the FIRST design (a global resolved drop-in) FAILED — `example.com`
      resolved to `127.0.0.1`; the dummy link replaced it the same day (#717).
      **Tell:** every site on the internet resolving to loopback — a DEFAULT route. If the dummy
      link ever does it: `sudo ip link del rexenv0`, and the mechanism is wrong, not the tester.
      **Second tell:** `Current Scopes: none` on `rexenv0` — `.rex` then works only while some
      other machine answers it.
- [x] Add a second TLD in Settings → a second marker, `resolvectl status rexenv0` lists both
      `~rex ~test`, both answer THROUGH `rexenv0`, `example.com` still does not. Remove both →
      `rexenv0` is gone. **◐ the mechanism ✓ 28 Sep 2026 (VM):** `linux_dns_route_check` with the
      app's `install_command`/`uninstall_command` — two TLDs answered via `-i rexenv0`, a resolved
      restart, partial removal keeps the link, the last removal takes link + unit. **✓ the door,
      28 Sep 2026 (VM, 0.8.9):** Settings → DNS & SSL → default domain ending `.test` → Save (the
      words say the route installs on the first `.test` site) → `rex site create tld2.test` →
      ONE polkit dialog → `/etc/rexenv/dns.d/{rex,test}`, `DNS Domain: ~rex ~test`, `resolvectl
      query -i rexenv0 tld2.test` → 127.0.0.1 and `lm.rex` too, `example.com` public,
      `https://tld2.test` 200. Removal of both is Remove system changes (P3's row).
- [x] A machine WITHOUT systemd-resolved (or with it stopped): the consent step FAILS with the
      sentence naming systemd-resolved; nothing is written. ✓ 28 Sep 2026 (VM,
      `linux_dns_route_check` with resolved stopped): the app's install command refused with
      "systemd-resolved is not running on this machine…", no marker written; resolved started
      again straight after and `example.com` resolved publicly.

### P2 — the edge unit (ledger #718)
- [x] ✓ 24 Sep 2026 (VM, the installed deb, through the app). After Start all (its own polkit
      dialog): `systemctl status rexenv-edge` active (running), `Main PID` = caddy under
      `/usr/local/lib/rexenv/bin/caddy` (`root:root 0755`), `:443` and `:80` answer.
      `ls -l ~/.local/share/rexenv/config/caddy-admin.sock` is owned by YOU — and stays so across
      a site change (the chown loop): a WordPress site created afterwards went through that
      socket and served over HTTPS (curl 200, Chromium no warning). (The unit's
      `ExecStart=/bin/sh "/usr/local/lib/rexenv/edge-launch.sh"` is L0-tested text, #718; the run
      did not record it.)
- [x] ✓ 24 Sep 2026 (VM): `sudo kill -9 <caddy pid>` → back within seconds (`Restart=always`;
      measured 4 s, new pid, `:443` up).
- [x] ✓ 24 Sep 2026 (VM): tray → Stop all (one prompt) → `systemctl is-enabled rexenv-edge` says
      `disabled`, `inactive`, the ports are free, every row idle. Start all → enabled, active,
      5/5 running.
- [x] Stop all, then reboot → the edge STAYS down (disabled survives the boot; `Restart=always`
      must not bring it back). **✓ 28 Sep 2026 (VM, 0.8.9):** Stop all (its polkit dialog —
      a GNOME shell modal, not an X window) → `rexenv-edge` disabled / inactive, 0 running →
      reboot → still disabled / inactive, no caddy, nothing on :443, no service processes; the
      DNS agent alone is active and answers (by design).

### P3 — CA trust in two stores (D-L3, ledger #726, #732)
- [x] ✓ After onboarding: `certutil -d sql:$HOME/.pki/nssdb -L` lists `rexenv local CA` with
      `C,,`; `/usr/local/share/ca-certificates/rexenv-local-ca.crt` exists (and
      `/etc/ssl/certs/rexenv-local-ca.pem`); `curl -I https://<site>.rex` succeeds with no `-k`.
      **Measured three times:** 24 Sep 2026 on the VM; 27 Sep on the Dell's WSL 26.04 (GUI
      onboarding → "Nearly there", `rex start`, `rex site create acme.rex --type wordpress` →
      `curl https://acme.rex/` 200 with the CA verified, wp-login 200, http→https 308; found on the
      way: `libaio.so.1t64` (#731) and a stale system-store CA behind a green "trusted" (#732),
      both fixed; then Laravel on **PostgreSQL** — the `libxml2.so.2 → .16` shim + PostgreSQL's
      closure in `Depends` — `https://lara.rex` 200, and the owner's Start on PostgreSQL in the
      Databases screen brought it up); and 27 Sep on the **floor** — the CI-built
      `rexenv_0.8.7_amd64.deb` (`linux-build.yml`, ubuntu-22.04) `apt`-installed on a fresh WSL
      22.04.5 x86_64 (every `Depends` satisfied, `ldd` clean, glibc 2.35), GUI onboarding to
      "Domains & SSL are ready", WordPress on MySQL at `https://acme.rex` 200 and Laravel on
      PostgreSQL at `https://lara.rex` 200, no `lib-compat/` needed (the shim's "none" case).
- [x] Chrome/Chromium (snap on Ubuntu) opens a site with no warning — the CA is in the snap's
      own NSS database (`~/snap/chromium/current/.local/share/pki/nssdb`), written by the app's
      onboarding. **◐ 24 Sep 2026 (VM):** the first deb wrote `~/.pki/nssdb` only and snap
      Chromium said `ERR_CERT_AUTHORITY_INVALID`; the CA added to the snap's database by hand
      opened the site, and the deb now writes both (#726). **✓ 28 Sep 2026 (VM, 0.8.9's own
      onboarding):** `certutil -L` on the snap's database lists "rexenv local CA  C,,", and snap
      Chromium 153 opened `https://lm.rex/` with the tune icon and no warning.
- [x] Firefox (snap on Ubuntu) opens it with no warning — the CA is in the profile's own
      `cert9.db` (`certutil -d sql:~/snap/firefox/common/.mozilla/firefox/<profile> -L` lists it),
      because the snap cannot import the host's system store through the `user.js` pref.
      **◐ 25 Sep 2026 (VM, headless):** with the pref alone `firefox --screenshot
      https://guard.rex` hung on the TLS error; `certutil -A` into the profile's `cert9.db` and it
      rendered — so the trust step now writes every Firefox profile (#726, extended). **✓ 28 Sep
      2026 (VM, 0.8.9's own onboarding):** the site row's browser icon opened `lm.rex` in snap
      Firefox 156 — the shield, no warning; Settings' "Trust HTTPS in Firefox" reads "Enabled in
      all 1 profile".
- [x] An **AppImage** on a machine without `libnss3-tools` (the deb `Depends` on it, so only the
      AppImage can meet this): the trust step fails with the `sudo apt install libnss3-tools`
      sentence, and the system half was NOT half-applied. **✓ 28 Sep 2026 (VM, the AppImage
      app, `certutil` moved off PATH):** Settings → Re-trust → "certutil is not installed, so
      rexenv cannot add its certificate authority to your browsers' trust store. Install it,
      then retry: `sudo apt install libnss3-tools`" with a copy button; no `pkexec` ran and the
      system store's file kept its mtime. (A certutil that is present but not executable gave
      the raw "io error: Permission denied (os error 13)" until 29 Sep 2026 — since then, ledger
      #746, the sentence names the path and the reason and offers `sudo apt install --reinstall
      libnss3-tools`; ◐ the run on the installed 0.8.11 with `chmod -x /usr/bin/certutil` is owed.)
- [x] Settings → Remove system changes → both stores empty (NSS dbs and the system file), the
      `rexenv0` link, route markers and units gone; the login item is a preference and stays
      on every OS (this row used to say "autostart entry gone" — no OS's teardown touches it).
      **◐ 28 Sep 2026 (VM, 0.8.9, with `.rex` + `.test` routed):** Settings → Services →
      Uninstall → "Remove rexenv's system changes?" (names the markers, the CA, the password) →
      Remove → polkit → the edge unit and both markers gone, `rexenv0` "does not exist",
      `rexenv-edge` / `rexenv-dns-route` unit files gone, all services stopped, `~/.pki/nssdb`
      and the snap Chromium database empty of the CA, the user DNS unit removed and the agent
      gone — `rex status`: "DNS DOWN (removed) · resolver MISSING · CA NOT TRUSTED". **But a
      SECOND polkit dialog** came for `/usr/local/share/ca-certificates/rexenv-local-ca.crt`
      alone, and until it was answered the file stayed (a tester who answers one dialog keeps a
      trusted CA). The teardown twin of the setup finding — fixed on master the same day
      (ledger #723: the CA file's removal rides the one batch); one dialog is owed the next
      release. Firefox's profile database already held no CA (its trust is the `user.js` pref).

### P4 — autostart and the DNS agent
- [x] ✓ 24 Sep 2026 (VM, `sudo reboot` with both launch toggles on): after the reboot's
      autologin `rexenv --hidden` is running (no window) — the autostart entry `~/.config/autostart/rexenv.desktop` —
      `rexenv --dns-agent` under the user unit (`~/.config/systemd/user/rexenv-dns.service`) is
      `active`, `rexenv-edge` and `rexenv-dns-route` are `active`, `rexenv0` carries `~rex` with
      no default route, `a.rex` → loopback (through the Mac host, it turned out — P1), `example.com` public, `:443`/`:18088` listening —
      "Start services when rexenv opens" brought the stack back with no click.
- [x] With the login item OFF: sign in → the DNS agent is `active` with the app NOT started, and
      `dig @127.0.0.1 -p 15353 x.rex` answers. Quit the app → `.rex` still resolves (the agent
      outlives it). `systemctl --user kill rexenv-dns` → back within seconds.
      **✓ 28 Sep 2026 (VM, 0.8.9):** autostart entry moved aside, reboot, login → only
      `rexenv --dns-agent` running, unit `active`, `dig` → 127.0.0.1; `systemctl --user kill
      rexenv-dns` → `active` again after 3 s, `dig` answers.
- [x] AppImage: the unit and the autostart entry name the `.AppImage` path, never a
      `/tmp/.mount_…` one. **✓ 28 Sep 2026 (VM, the tap's 0.8.8 aarch64 AppImage, no
      `libfuse2` → `APPIMAGE_EXTRACT_AND_RUN`):** the process runs from
      `/tmp/appimage_extracted_…/usr/bin/rexenv` with `APPIMAGE=/home/rexenv/Apps/rexenv_0.8.8_aarch64.AppImage`;
      after launch `rexenv-dns.service` reads `ExecStart="/home/rexenv/Apps/rexenv_0.8.8_aarch64.AppImage"
      --dns-agent` and `rexenv.desktop` `Exec="…AppImage" --hidden` (both had been the deb's
      `/usr/bin/rexenv`), and the agent process is the AppImage.
- [x] ✓ 24 Sep 2026 (VM, an owner-approved public test tunnel): **the tunnel guard (ledger
      #722)** — `guard.rex` shared, cloudflared and `rexenv --tunnel-guard …` beside it; `kill -9`
      the app → both gone within 6 s.
- [ ] The guard's normal-stop leg: stop the share from the app → the guard exits at once, no
      stray process. **Open: owed since 24 Sep** (and `tunnel_parent_death_check` has no Linux tier entry).

- [x] **Start rexenv at login is ONE toggle** (29 Sep 2026, ledger #739; the words name the
      system tray and `~/.config/autostart`): Stop all, turn it on, reboot into the autologin →
      `rexenv --hidden` with no window and `rex status` listing the backends running with no
      click. Then Stop all and launch the app from the desktop → nothing starts. **✓ 29 Sep 2026,
      22.04 arm64 VM, the re-cut 0.8.10 (`a470c1af`) deb over 0.8.9 (`apt install`):** the autostart entry
      `Exec="/usr/bin/rexenv" --hidden`; Start all (one polkit) → `sync` → reboot → `rexenv --hidden`,
      no visible window, "launched at login — staying in the system tray", all seven services
      running incl. the edge, `lv.rex` / `lm.rex` / `guard.rex` 200; Stop all, quit, launch from the
      GNOME shell → the window, "Stopped 0/6", nothing started. The handoff: a hand-launched primary
      + `rexenv --hidden` → "handed this login launch to it (it runs Start all)", exit 0, backends up
      (the edge skipped with its honest line — Stop all had disabled it, and login never prompts).
      A hard kill (`utmctl stop --kill`) with the stack up → `mysql.sock.lock: pid 2009 is now
      /usr/bin/rexenv, not the server that wrote it` — removed, MySQL back (ledger #735's exact
      shape). The same kill 30 s after a Start all left `rexenv-edge.service` ZERO bytes (masked) —
      `docs/TODO.md`.

### P5 — the GUI
- [x] ✓ 24 Sep 2026 (VM): the COLOUR tray icon in GNOME's top bar (needs
      `libayatana-appindicator3`); its menu opens with the full model (All running · 5 services,
      Start/Stop all, Sites ▸, Services … Quit); the window has NO reserved title-bar row (GTK
      draws its own). Also 27 Sep, the Dell's WSLg: onboarding end to end through the GUI.
- [x] Tray → "Open rexenv" shows the window. **✓ 28 Sep 2026 (VM, 0.8.9):** the top-bar icon
      opens the menu ("Partial · 5 of 6 running", Start/Stop all, Sites ▸, All sites…, Services,
      Databases, Mail, Tunnels, MCP server, About, Open, Quit); Open rexenv brought the window up.
- [x] Databases → Browse: Adminer renders INSIDE the app at `rexdb://localhost` (webkitgtk
      serves custom schemes as WebKit does — the macOS origin, ledger #703's Linux leg).
      **✓ 28 Sep 2026 (VM, 0.8.9):** Browse on MySQL → "Browsing MySQL", Adminer 6.1.1 inside the
      panel, "MySQL version: 8.4.6 through PHP extension MySQLi", the `wp_lm_rex` database
      listed; the address bar carries the `adminer.rexenv.rex` URL with Copy / Open in browser.
- [x] ✓ 24 Sep 2026 (VM): onboarding says "this computer", "Linux will ask for permission"; the
      New Site dialog refuses Apache and MariaDB with "isn't part of rexenv on Linux yet". Found
      and fixed the same day: the sidebar read 14.5–16.3 GB for a ~600 MB stack (#725; the rebuilt
      deb reads 260 MB) and PHP 7.4 showed an Install button before Linux had a build (#728).
- [x] Settings: the words say Files, apt, "this computer", the tray, `/etc/rexenv/dns.d` — never
      Finder, brew, Explorer or winget (`words::LINUX`). **✓ 28 Sep 2026 (VM, 0.8.9):** General —
      "System follows your Linux appearance", Sites folder `/home/rexenv/rexenv/Sites`, CLI
      "`/usr/local/bin/rex → /usr/bin/rex` · One administrator prompt"; DNS & SSL — "trusted ·
      browser (NSS) and system trust stores", "asks for your password once", the Firefox rows
      with `~/.local/share/rexenv/ca/rexenv-ca.pem`. No Finder, brew, Explorer or winget anywhere.
- [x] Open in editor / browser / terminal: each detected entry launches; a private window opens
      private; "Open in terminal" lands in the site folder. **✓ 28 Sep 2026 (VM, 0.8.9), the two
      the VM has:** the row's browser icon → Firefox at `lm.rex` (the site); the folder icon →
      Files at `Home/rexenv/Sites/lm.rex` (wp-admin, wp-content, wp-config.php…). No editor is
      installed ("No code editor detected"), so that entry does not exist to launch; the private
      window and the terminal legs are not on this VM's menu.

### In-app update on Linux (L7, ledger #729/#730)
- [x] `.deb` install, an older version: Settings → About → Check now finds the release named in
      `app-manifest-linux-deb-<arch>.json`; Update → ONE polkit dialog, rexenv's own sentence
      (not "run /bin/sh as the super user") — from 0.8.11 on, the UPDATE's sentence, "rexenv
      needs administrator permission to install the update it downloaded — the new rexenv
      package." (#747; the 0.8.10 → 0.8.11 update itself still shows the setup sentence — the
      old action file — so this reads from the update AFTER 0.8.11); the app quits and comes back as the new version;
      `dpkg -s rexenv` says so; `~/.local/share/rexenv/updates/` is empty after the health sweep.
      **◐ 27 Sep 2026, 22.04 arm64 VM, 0.8.7 dev deb → 0.8.8 (serial 1):** found, ONE polkit
      dialog (rexenv's sentence — but the generic one, naming DNS/edge/CA and not the update:
      TODO row), `dpkg -l` 0.8.8, the DNS agent relaunched as 0.8.8, three sites 200 throughout
      — and the app did NOT come back: `could not spawn the relauncher (No such file or
      directory)` (ledger #729, fixed on master 28 Sep; the fix runs in the OLD side of a swap,
      so **this row closes on the release AFTER the one carrying the fix**). Opened by hand:
      0.8.8. `updates/` still held `0.8.8/` and `.rexenv-update-<pid>/` right after (the sweep had
      not run yet). First Check now was refused as a replay — the dev deb had read the macOS
      document (TODO row). **28 Sep, 0.8.8 → 0.8.9 on the same VM:** offered (serial 1 → 2), one
      polkit dialog, `dpkg -l` 0.8.9, swap logged, OK → 0.8.8's relauncher failed the same way
      (expected — the old side spawns), hand open → 0.8.9. **✓ 29 Sep 2026, 0.8.9 → 0.8.10 on the
      same VM — the first update whose OLD side carries #729's fix:** 0.8.9 (the published arm64
      deb, `apt install --allow-downgrades`) offered "0.8.10 · 14.6 MB" (the Settings badge too),
      Install → ONE polkit (the generic sentence — TODO row) → `dpkg -l` 0.8.10, "swapped to
      0.8.10 (RenamePair)", OK → "reopening /usr/bin/rexenv once this process exits" → **a new
      `/usr/bin/rexenv` came back by itself**, window up, `rex 0.8.10 (a470c1a)`, "swept 2
      leftover(s)", "updated from 0.8.9", the agent answering. One blemish: Ubuntu's "Problem in
      WebKitWebProcess — closed unexpectedly" dialog — the OLD instance's web process (reparented
      to `systemd --user`) died on SIGSEGV as the old app exited; `docs/TODO.md`.
- [x] AppImage in a folder you own, an older version: Check now reads
      `app-manifest-linux-appimage-<arch>.json`; Update swaps the file with NO prompt; the app
      comes back as the new version FROM THE SAME PATH; the folder holds no `.rexenv-update-*`
      after the sweep. A host without `libfuse2`: still works (`APPIMAGE_EXTRACT_AND_RUN`).
      **✓ 28 Sep 2026 (VM without `libfuse2`, 0.8.8 AppImage → 0.8.9):** Check now → "0.8.9 ·
      86.2 MB" (the AppImage document, serial 2) → Install → no prompt (no `pkexec` in the
      journal) → `swapped to 0.8.9 (AtomicSwap); previous bundle kept at
      ~/Apps/.rexenv-update-<pid>/…` → the same path `~/Apps/rexenv_0.8.8_aarch64.AppImage` now
      prints `0.8.9` → OK → came back from that path as `0.8.9 (bfcd8cf)` with
      `APPIMAGE=` the same file, the agent relaunched as 0.8.9. **First attempt showed the
      shared-key bug from the other side:** the deb app had stored the deb document under 0.8.8's
      one key minutes earlier, so the AppImage app offered "0.8.9 · 14.5 MB", downloaded the
      `.deb`, and refused it as "did not run as rexenv (it printed no version)" — nothing changed;
      cleared the cache and the AppImage document was read (#519, fixed in 0.8.9). The kept
      previous copy's sweep: see the row's next run.
- [x] A `cargo run` dev build: Check now says a dev build reads no descriptor — no fetch, no
      offer. **◐ 28 Sep 2026 (VM, the 0.8.9 binary run from a `target/debug/` path — what
      `classify` calls a dev build):** no fetch and no offer — the log reads "app update: check
      skipped: this rexenv is not a .deb or an AppImage install, so there is no update
      descriptor for it" — but the card SAID "Couldn't reach the update server just now": the
      interactive check returned the refusal as an error, and the card renders every error as
      unreachable. Fixed on master the same day (`no_descriptor_reason` → `checkRefusal`, its
      own sentence before any network; TODO row); the words are proven by the next release.
- [x] Mid-update `dpkg -i` fails (unplug the network after the download, or a wrong-arch package
      renamed by hand): the dialog names the failure, the OLD version keeps running, nothing in
      `/usr/bin` changed. **✓ 28 Sep 2026 (VM, 0.8.8 deb with `chattr +i /usr/bin/rexenv`):**
      Install → one polkit dialog → the toast "could not put rexenv 0.8.9 in place (privileged
      operation failed: dpkg: error processing archive … unable to make backup link of
      'usr/bin/rexenv' before installing new version: Operation not permitted …). The rexenv you
      were running is still installed and untouched." — `rexenv --print-version` 0.8.8, `rex
      --version` app 0.8.8 still running, `dpkg -l` 0.8.8. (A toast, not a dialog: it stays a
      few seconds and the offer returns beneath it.)

### PHP 7.4 on Linux (L8, `php-7.4.33-7`)
- [x] ✓ 25 Sep 2026 (VM, the rebuilt deb): `rex php install 7.4` → "✓ PHP 7.4 installed", `rex php
      list` shows 7.4 installed with no refusal note, `rex site create php74.rex --type php --php 7.4`
      → `curl https://php74.rex/` prints 7.4.33, `rex status` shows `PHP-FPM 7.4 running` on 9774;
      `bin/php-7.4.33/licenses/` holds the Linux licence set (`php_versions_check`).
- [x] Settings → PHP → 7.4 → Install through the GUI, and a WordPress site on 7.4; no Xdebug toggle
      for 7.4 (static ELF, expected). **✓ 28 Sep 2026 (VM, 0.8.9):** with `lm.rex` on 7.4 the
      row's remove was refused — "PHP 7.4 is in use by a site — switch those sites first"; the
      site's picker → 8.3 ("Switch lm.rex to PHP 8.3? … restarts briefly") → remove → the row
      shows Install → Install → `rex php` 7.4 `yes` (its pool on 9774) → the site's picker back
      to 7.4 → Start site → `serving`, `https://lm.rex/` 200 and the REST route 200. The 7.4 row
      carries make-default, ini settings and remove — no Xdebug control (8.3's row has ini
      settings only); the version list shows "EOL November 2022" beside it.

### Not in Linux v1 (D-L8, refused in core with an honest message)
- **Redis, MariaDB, Apache, Xdebug** — as Windows v1: not offered, and a refusal that names the OS.
  ✓ 24 Sep 2026 (VM) for Apache and MariaDB in New Site (P5 above).

---
Result: ____ / all pass.  Issues found: ________________________________________
