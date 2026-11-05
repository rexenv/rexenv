# Shipped — November 2026

⚠️ **Historical.** Completed rows moved out of `docs/TODO.md` by `scripts/todo-reconcile.py`, with their ✓ evidence as written at the time. May contradict current code — the live description is `docs/ARCHITECTURE.md`.

## Reconcile of 6 Nov 2026

### From “Now — actionable code/test work”

- [x] **macOS: a login-launched rexenv's self-update stopped MySQL, MariaDB, Nginx and Mailpit** ✓ 29 Oct 2026 —
  launchd SIGTERMs a LaunchAgent's process group when the app exits; services + relauncher now get their own group,
  the login plist `AbandonProcessGroup` (ledger #936, L0 + 3 plants; SMOKE row owed live).

- [x] **Linux: an in-app update apply that pkexec refuses leaves NO trace** ✓ 4 Nov 2026 — every failed apply now
  writes `app update: the install did not happen — <why>` to `rexenv.log` (`app_update_apply` is a wrapper whose
  one job is that line; common code, all three OSes), ledger #937, TEXT test + plant. The toast was never missing
  in code (`toastBackendError`) — it was gone before the screenshot. **Re-run on the VM ✓ 5 Nov 2026:** a
  0.9.4-labelled container build of `097fd74d` swapped into `/usr/bin/rexenv`, launched from ssh (`session-7.scope`,
  no polkit agent), Settings → Updates → Install rexenv 0.9.5 → `rexenv.log` 03:50:28 `WARN … app update: the install
  did not happen — could not put rexenv 0.9.5 in place (… polkit refused), or no authentication agent is running …)`;
  `/usr/bin/rexenv` unchanged (sha256 before = after); the 0.9.5 binary restored after. (found 24 Oct 2026, 0.9.2's §M on the
  22.04 VM — `docs/PUBLISH-TESTING.md` §A 0.9.2): with the app in an ssh login session (no polkit agent) the
  download ran to 15.9 MB and the Updates row silently went back to `Install rexenv 0.9.2` — no `rexenv.log` line,
  no toast in a screenshot 90 s later. 0.9.1's run of the same shape showed the honest toast ("could not put rexenv
  … no authentication agent is running"). Either the toast is too short-lived to count or this path skips both;
  the log line is owed in every case (an apply that did nothing must say so somewhere a reader can find).

- [x] **Windows: the php-cgi churn breaker stopped two groups mid-swap on the Win11 ARM VM** (found 24 Oct 2026
  running 0.9.1's §M). After the in-app relaunch onto 0.9.1 moved the 8.2 and 8.3 groups onto their re-pinned
  builds, `health: [gave-up] PHP-FPM 8.2/8.3: stopped — its php-cgi parent was using most of a CPU core in a
  loop` fired for BOTH within ~25 s of the swap (`php_cgi::SPIN_CPU_SHARE` 0.25 over ≥ 5 s, ledger #605), the
  group's output log empty, then "8.3's patch-updated pool did not become ready" — every 8.3 site 502 until
  `rex start` (4 s, then fine). The VM emulates x64 php-cgi on ARM, so a parent spawning 11 workers may hold a
  quarter of a core for 5 s legitimately; the Dell's 16 Oct run of the same swap did not trip it. **Open:**
  reproduce on the Dell (native x64) — if it trips there too, the breaker needs to exempt a group's first N
  seconds after a start the app itself ordered, or read the spin against the WORKER count rather than wall CPU.
  `docs/PUBLISH-TESTING.md` §A 0.9.1 §M has the log.
  **Dell, 5 Nov 2026 — does NOT trip on native x64:** 5 × `rex restart` + 1 cold start with the 8.3 tree moved
  aside (re-downloaded, the swap's "new binaries" shape) + 1 more restart, every group PARENT's CPU sampled each second
  for 60 s: 39 parents (7.4, 8.2, 8.3, 8.4), each 0–203 ms of CPU for its whole start incl. 10 workers; the worst
  5-s window 2.8 % of a core (the breaker needs 25 % = 1250 ms); no `gave-up` in `health.log`. So the margin on
  native hardware is ~6× even counting the start the baseline skips. **What stays open:** Windows on ARM runs these
  same x64 zips emulated (the catalog's arm64 rows point at the x64 zip), which is where it tripped — no ARM host
  exists since the VM's deletion (27 Oct). Without one, the low-risk change is to require the share over TWO
  consecutive windows (a spawn burst ends, a spin does not) — owner's call before building it.
  ✓ **Built 5 Nov 2026 (owner said build it):** `php_cgi::judge` counts consecutive spinning windows
  (`Pool::hot`), trip at `SPIN_WINDOWS` = 2, a calm window clears the count. L0 + 3/3 plants (ledger #605);
  the Dell's `windows_cgi_breaker_check` `PASS`, the spin stopped on the 2nd tick (20 s), legitimate load
  (46,672 requests) and a worker-killing script untouched. **Not proven:** that this clears the ARM
  swap burst itself — no ARM host; a Windows-on-ARM user who meets a `gave-up` after an update is the signal.

- [x] **Windows: two per-OS answers still read off macOS builds** ✓ 4 Nov 2026 — both now ask the platform's pool
  model: a php-cgi group answers `pdo_pgsql` from the extensions it loads (Windows: every minor) and curl's resolver
  `None` (unmeasured), ledger #939, L0 + 2 plants. **Dell ✓ 5 Nov 2026:** `pdo_pgsql` in `php -m` on every minor,
  `pg74.rex` (7.4 + PostgreSQL, `--starter-db`) accepted and reading PostgreSQL 18.6 through its php-cgi group, and
  curl's resolver measured threaded on all seven pinned zips — the group now answers those rows (plant red). Was: `php::pdo_pgsql_supported` →
  `binaries::php_has_pdo_pgsql` is keyed on the macOS self-hosted tag (so Windows refuses PostgreSQL
  on 7.4/8.0 although `platform/windows/mod.rs` enables `pdo_pgsql`), and `wp_dns::resolver_for(minor)`
  carries macOS measurements with no OS. Xdebug and the update catalogs are already per-OS. Split
  out of the Windows launch row 9 Oct 2026 (plan §3a Q3).

- [x] **Linux: the tunnel guard's normal-stop leg** ✓ 4 Nov 2026 — run on the 22.04 VM (0.9.5 deb, owner-approved
  public tunnel): `rex tunnel stop` → cloudflared and the guard both gone in 130 ms, nothing left; and
  `tunnel_parent_death_check` joined the Linux tier (`macos,linux`) — ALL PASS on the VM, a plant (the guard's
  `stop_child` removed) red on legs 1 and 4. Its stand-ins leaked `sleep 300` orphans on both OSes; fixed with a
  process group per stand-in (TESTING). Was: SMOKE § Linux's last unticked row; never run, and
  `tunnel_parent_death_check` has no Linux tier in `scripts/live-checks.sh`. Split out of the Linux launch
  row 9 Oct 2026.

- [x] **The `verify (Windows)` CI job was red on every push since 16013d9f (4 Nov 2026)** ✓ 6 Nov 2026 — found
  when the owner noticed the push action failing. Three lib tests (`an_update_that_removes_postgresql_says_so_and_names_the_sites`,
  `postgres_is_refused_at_the_insert_chokepoint_for_two_different_reasons`,
  `a_site_refused_for_its_shape_leaves_no_folder_behind`) asked the HOST's pool model for the pdo_pgsql answer
  while asserting the php-fpm platforms' — and #939 made the Windows group answer "yes" on every minor, so on
  the Windows runner nothing was refused ("a cost" / "PostgreSQL needs PDO" panics, and the third reached the
  certificate step). Not a product bug: the product's Windows answer is the measured one. Fix: `update_cost_on` /
  `ensure_engine_supports_on` take the model, the tests state the php-fpm answer through `PoolModel::Fpm` and the
  group's answer explicitly, and the provision test keeps only the shapes the running platform refuses. The
  Mac's `verify.sh` could not catch it: `windows-check` compiles Windows, it does not run its tests.

- [x] **Windows: PHP 7.4/8.0/8.1 failed their checksum on every fresh install since 25 Oct** ✓ 5 Nov 2026 — found
  creating a 7.4 site on the Dell (`checksum mismatch for …/php-builder-windows/releases/download/php7.4/…`).
  shivammathur rebuilt all three backport zips IN PLACE on 25 Oct (no backport commit after 19 Oct), the case
  `binaries::php_windows_backport_tag` predicts; 0.9.5 ships the 21 Oct hashes. Re-pinned (`b57eb4c0…`, `47b17c93…`,
  `6fcfeb11…`) after re-measuring: each `php*.dll` still exports `zend_mm_safe_error_ex`, and on the Dell `php -m`
  loads the group's set with `pdo_pgsql`, memory exhaustion exits 1 with no shutdown function. Proven by the
  re-pinned build installing 7.4 on the Dell (`pg74.rex`). `docs/PORTS.md` row.

- [x] **macOS: the root edge daemon recreates the app's `logs/` as ROOT when the data folder vanishes** ✓ 4 Nov
  2026 — the daemon logs to `/Library/Logs/rexenv/caddy-start.log` (`EdgeSupervisor::start_log_path`; the install
  makes the folder; every "check the start log" sentence names the platform's path), ledger #938, L0 + 2 plants.
  Linux needs nothing (systemd fails `209/STDOUT` rather than creating the folder); Windows runs no root daemon.
  SMOKE "Moving the app data aside with the edge live…" ✓ on the 15.8 VM — **15.8 VM, 5 Nov 2026** (a `tauri build --debug` bundle of `3ccab205` — this Mac's ld-27106 writes release proc-macro dylibs dyld refuses, `mis-aligned LINKEDIT string pool`): `rex stop`/`rex start` reinstalled the daemon (plist now names `/Library/Logs/rexenv/caddy-start.log`, the folder root:wheel), `wtp.rex` 200; app quit, data folder moved aside, `launchctl kickstart -k` the daemon → caddy failed 6 times on the missing Caddyfile over ~40 s and the folder never came back; moved back → 0 root-owned files in it, 200, the app re-adopted the edge.
  (found 18 Oct 2026 running 0.9.0's §M on the 15.8 VM). With the edge LaunchDaemon live, renaming
  `~/Library/Application Support/dev.rexenv.rexenv` aside made launchd recreate `…/dev.rexenv.rexenv/logs`
  root-owned within a second — the daemon plist's `StandardOutPath`/`StandardErrorPath` point into the
  user's app data (`platform/macos/mod.rs`, the edge plist) — and the next app start died on
  `mkdir …/ca: Permission denied` until the folder was chowned by hand. Not a 0.9.0 regression; reached
  by a user who deletes or moves the app data while the edge runs (a reset, a cleanup tool). Open: point
  the daemon's log outside the user tree, or have the app refuse that state with the fix instead of a
  bare EACCES; check what Linux's `rexenv-edge` unit and Windows do in the same move.


### From “Ledger-driven proof backlog”

- [x] **22 live-check examples are tagged `all` in `scripts/live-checks.sh` but cannot run on Windows** (found
  5 Nov 2026 on the Dell): each does `binaries::resolve(&*plat, "php", ...)` and unwraps it, and Windows' php is a
  directory distribution, so the first line of real work panics — `wp_plugins_check` 5/5: `Other("php is a
  directory distribution — use resolve_dir")` at `examples/wp_plugins_check.rs:36`. None of the 22 mentions
  `target_os`, `resolve_dir(.."php")` or the pool model (grep, 5 Nov): `adminer_check`, `adminer_deeplink_check`,
  `adminer_update_check`, `blueprint_check`, `cli_wp_install_check`, `laravel_postgres_check`, `multisite_check`,
  `network_check`, `php_tools_check`, `php_fetch`, `repo_install_check`, `starter_seed_check`,
  `tunnel_muplugin_check`, `wp_premium_update_check`, `wpcli_check`, `wp_login_check`, `wp_packages_check`,
  `wp_themes_check`, `wp_info_check`, `wp_plugins_check`, `wp_tools_check`, `wp_noise_check`. Only one was RUN on
  Windows; the other 21 are by source. **Why it matters:** the os column is what says a check proves something on an
  OS, and `all` here claims Windows coverage that does not exist. Fix: one `common` helper that resolves the PHP CLI
  per OS (the file on macOS/Linux, `php.exe` inside the directory on Windows), used by all 22, then a Dell run of
  each; until then retag them `macos,linux`. Owner's call which.
  ✓ **Fixed 5 Nov 2026 (owner: build the helper).** The helper already existed — `binaries::resolve_program`
  answers the executable on every OS — so every example's `binaries::resolve(` of `php`, `nginx` or
  `frankenphp` now calls it (42 lines in 40 files, plus one wrapped call the sweep missed and the guard
  caught), held by `no_example_resolves_a_program_that_is_a_tree_on_some_os` (ledger #940). **Run on the
  Dell, stack stopped, one at a time:** 17 of 22 pass — adminer_check, adminer_update_check, blueprint_check,
  cli_wp_install_check, multisite_check, network_check, php_fetch, php_tools_check, starter_seed_check,
  tunnel_muplugin_check, wp_info_check, wp_login_check, wp_noise_check, wp_packages_check,
  wp_premium_update_check, wp_tools_check, wpcli_check. Three of those needed a second fix the run found:
  `cli_wp_install_check` overflowed Windows' 1 MB main-thread stack (its body now runs on an 8 MB thread) and
  the abort leaked its mysqld onto :13306, which made the next four checks refuse to start;
  `repo_install_check`'s fixture commit timed out on the Dell's global `commit.gpgsign` (fixtures now pass
  `commit.gpgsign=false`, as does `repo_git_ops_check`); `wp_packages_check` read `wp` where Windows writes
  `wp.cmd`. The run also found PHP 8.5's opcache warning (next row). The five that do not pass: next-but-one row.

- [x] **Windows: every PHP 8.5 CLI run printed `Warning: Failed loading Zend extension 'opcache'` on stdout**
  (found 5 Nov 2026 by `laravel_postgres_check` on the Dell; in its logs since 8.5.8, 15 Sep). 8.5's zip builds
  OPcache in and ships no `php_opcache.dll`, and the ini wrote `zend_extension = opcache` for every minor — so
  the warning led every 8.5 WP-CLI, Composer and terminal `php` run, and the group's preflight let it through
  (php-cgi's `<b>Warning</b>: Failed loading …` matched none of its patterns). `windows_php_cli_check` had
  "passed" 8.5.8 on 8 Oct because it looked only for `PHP Startup`.
  ✓ `ExtChange::BuiltInFrom("8.5")` on the Windows model, `CgiGroup::zend_lines` applying `per_version` to Zend
  extensions, the preflight refusing `Failed loading`, the check refusing any `Warning`. L0 + 3/3 plants; on the
  Dell the check built without the rule failed both 8.5.12 rows and with it passes every minor 7.4–8.5 (ledger
  #601, #603). Not run: an 8.5 site served end to end on Windows.

- [x] **All 22 Windows-capable examples pass on the Dell (five failed at first)** (5 Nov 2026, after the row above):
  (1) ✓ `adminer_deeplink_check` — **not a Windows bug, and not a product bug:** Adminer 6.2's login form carries
  a hidden CSRF `token`, and the check posted the auth fields without it (true of the Adminer it was written
  against), so Adminer answered its login page again — 403, no message. The Dell had 6.2.0 from a binary update.
  Driving the Dell's live Adminer by hand: the same POST without the token → login page; with it → logged in,
  `wp_options` listed. The app's auto-submit script submits the page's real form, token included, so the deep
  link itself worked throughout. The check now sends the token when the form has one (as
  `adminer_proxy_check` already did) and prints Adminer's own message on a failure: Dell `ALL GOOD`, auth POST
  302, `wp_*` tables listed (5 Nov 2026); (2) ✓ `laravel_postgres_check` — two fixture faults, neither the
  product's: it spawned `postgres.exe` itself, which refuses to run under an administrator token (every SSH
  session on the Dell) — the product starts it through `pg_ctl`'s restricted token (`postgres::start`), which
  the check now calls; and it gave Composer an environment of `COMPOSER_HOME` ALONE, and `spawn_streamed` clears
  the rest, so a Windows child with no `SystemRoot` could not resolve `repo.packagist.org` — it now passes the
  login environment the product passes. Dell: `laravel on postgres: all green` (all seven minors load
  pdo_pgsql, PDO connects, `create-project` + `artisan migrate` on PostgreSQL) (6 Nov 2026); (3) ✓
  `repo_install_check` — the Dell's GLOBAL Composer config carries the PHPCS installer plugin
  (`dealerdirect/phpcodesniffer-composer-installer`), never allowed in allow-plugins, so Composer refused every
  install — and the user-facing message was the last three lines of Composer's wrapped box plus its `install
  [--prefer-source] …` usage line, the plugin's name cut off. **The message was the product bug** (a common WP
  developer setup): `map_composer_error` now names the plugin, says whether it is the user's global or the
  repo's, and gives the one `composer [global] config allow-plugins.<name> true` command; and `last_lines` no
  longer counts usage synopses or blank box padding (ledger #942). The check now gets its own `COMPOSER_HOME`,
  and its bun row asserts the platform's install line (`PlatformWords::bun_install`) instead of macOS's `brew
  install`: Dell `ALL PASS` (6 Nov 2026); (4) `wp_plugins_check` never reached its checks: `wp core download` of `latest.zip`
  timed out at 600 s with 36.9 of 37.2 MB received, twice — the Dell's link to wordpress.org, not the code (the
  same download passed inside other checks); (5) `wp_themes_check` has no verdict — one run refused (the leaked
  mysqld on :13306), the next was cut off when the SSH session dropped mid-run, its log empty. ✓ Also
  `repo_run_all_check` (tagged `all`, not one of the 22): it did overflow Windows' 1 MB main thread on the
  Dell, then failed its gpg-signed fixture commit, then its composer step on the same global plugin — now on
  `common::main_on_big_stack`, signing off, its own `COMPOSER_HOME`: Dell `ALL PASS`; `git_site_clone_check`
  (also `all`, also signing) `PASS`. Two guards keep the classes shut (ledger #941). ✓ (4) and (5) passed once the
  Dell's wordpress.org link was fast again (6 Nov 2026, ~2 min each): `wp_plugins_check` and `wp_themes_check`
  `ALL GOOD` — no code change was needed. **22 of 22 now pass on Windows.** Not looked at:
  `laravel_postgres_check` left one `postgres` process behind on the Dell after a PASS (the adopted
  postmaster's reap).

