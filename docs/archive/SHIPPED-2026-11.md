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


## Reconcile of 7 Nov 2026

### From “Now — actionable code/test work”

- [x] **winget: PR 437674 waits for a moderator** — `scripts/winget-manifest.sh` generated it; the PR
  is OPEN with `Azure-Pipeline-Passed` + `Validation-Completed` (checked 9 Oct 2026). Nothing to do
  from here but watch it merge.
  ✓ MERGED 30 Oct 2026 04:11 UTC as "New package: rexenv.rexenv version 0.9.5" (`gh pr view 437674 -R
  microsoft/winget-pkgs`, checked 7 Nov 2026) — rexenv is in winget. Every later version is its own PR (the
  row below).

- [x] **Windows 7.4/8.0/8.1 re-pin: what reaches users** (opened 5 Nov 2026, from the row above). (1) Every 0.9.5
  Windows install still fails those three minors until a release carries the new pins — the signed
  `manifest-windows.json` lists only 8.2–8.5, so no manifest publish can fix it without adding rows. Open for (1) only.
  ✓ 0.9.6 carries the re-pins (`02712ef0` is in `v0.9.5..v0.9.6`), published 6 Nov 2026 with its in-app update
  descriptors; its Dell gate ran a FRESH PHP 7.4.33 download (the cached tree moved aside) — the re-pinned bytes
  accepted, a 7.4 site served `7.4.33` (PUBLISH-TESTING §A 0.9.6).
  ✓ (2) 6 Nov 2026, owner-approved: rexenv/runtimes `php-windows-legacy-2` mirrors the 25 Oct bytes (runtimes e6bf4f0,
  run 42143994884 — sha, `zend_mm_safe_error_ex` export, `php -m` = php.net's, fixed memory-exhaustion path; immutable).
  ✓ (3) runtimes `check-php-windows.yml` (daily 06:41 UTC) hashes the three zips against php-windows.yml's SET and
  rexenv's `php_windows_sha256` on master, and opens one `php-windows-bytes` issue on drift; first run 42143989415:
  all three match both.

## Judgement moves — reconcile of 7 Nov 2026

Two open rows (shape 3: shipped narrative inside an open row) were rewritten in `docs/TODO.md` to their open reason;
their full text as it stood is kept here.

### “Headless rexenv on a VPS” (Phase 4+), before the rewrite

- [ ] **Headless rexenv on a VPS — Phase 1a (T0–T4) shipped in 0.9.3; open: T5 (macOS/Windows, D3 — its own row below) and Phase 1b (T6–T9: the web UI);
  D2 (the macro) and D5 (9443) ruled 28 Oct 2026.** **T6 built 28 Oct 2026** (#929): `rexenv --web` on
  `127.0.0.1:9443`, `/ipc` through the main webview's own `on_message`, `/events` as SSE, one command
  list for the handler and the web; run in the cloud container only — T6's VM run through an SSH tunnel
  with a site created and deleted done 29 Oct 2026 (SMOKE "The web UI"). **T7 built 28 Oct 2026** (#930): one gate
  before any routing — loopback Host, our Origin, the `rex web url` token → an `HttpOnly;
  SameSite=Strict` session, a CSRF header on every POST; `--headless-setup --enable-web`; T7's VM/tunnel run done 29 Oct 2026, with the
  port/`[::1]`/timeout hardening (#935) and four items still owed (the sign-in hardening row above). **T8 partly built 28 Oct 2026** (#931): the web picker
  (`browse_dir`, home and Sites only), desktop actions refuse on a headless launch, URLs open in the
  developer's browser; the Database Browser on a loopback port per console (9444 Adminer, 9445
  phpMyAdmin, #933); real Adminer and phpMyAdmin run through
  the tunnel 29 Oct 2026; **open in T8: the every-route walk on the VM, and hiding the desktop actions (its row above)**. **T9 built 28 Oct 2026** (#932): the CA certificate download with
  the cost said and each OS's trust steps, name-constrained to the sites' endings (#934); **open in T9: the steps on real macOS/Windows/Linux
  machines and a site through `ssh -D`**. T0 measured 26 Oct 2026 on the Ubuntu
  VM; D1 (Xvfb), D3 (Linux first) and D7 (a closed passwordless set, corrected from a polkit rule that
  would have been passwordless root) ruled the same day. **T1 built and run on the VM 26 Oct 2026**:
  `rexenv --headless` (own Xvfb, no session bus — 31 s → 1 s to `setup` — no window, no tray),
  `sudo rexenv --headless-setup` (the edge's three `systemctl` argv only), a Linux edge start with
  identical root files is a restart, honest hand-off and `rex open` (ledger #911–#913; SMOKE Linux
  "Headless on a server"). **T2 S1 built 26 Oct 2026** (D6 ruled the same day): `edge_http_port` /
  `edge_https_port` / `edge_bind` as gated settings, the manager and every runtime edge check on them
  (#914, #915); **S2 built the same day** — one `site_url` per side, WordPress/login/Bedrock/Laravel
  carry the port, the URL-rewriting flows refuse off 443 (#916, #917); **S3 the same day** — an
  installed root edge stays the edge on any port (#918); **S4 the same day**: the Settings card
  (replacing a card that saved nothing), Onboarding names the port, and the VM run on a fresh headless
  account beside a foreign nginx — which found and fixed Caddy's redirect dropping the port (#919)
  and a fresh account's missing log folder. **Open in T2: the macOS and Windows runs** (a fresh
  account on the 15.8 VM and on the Dell; the card's GUI rows of SMOKE "Edge ports") and the
  443 → 8443 → 443 round trip on one account. **T3's code half built 26 Oct 2026** (D7's consent):
  `sudo rexenv --headless-setup` takes `--enable-mcp` / `--agent-access <level>` or asks in the
  terminal with the card's own sentences (the paragraph moved into Rust, `mcp_server::ENABLE_CONSENT`),
  and writes as the user from a forked child — `rex config` still refuses both (#920; run in the cloud
  container only). **T3's VM run done 26 Oct 2026** (#920): the consents by flag, pty and no-tty, and over `rex mcp` a
  scratch create → plugin add + activate → its output → delete, nothing left. **T4's unit built 26 Oct 2026**: the setup writes
  and enables `rexenv-headless.service` (`KillMode=process`, the user's runtime dir) and lingering
  (#921), INSTALL § Linux "Server (headless)"; **T4's VM run done the same day** (#921: restart keeps the stack, `kill -9` restarted, hand-off, a boot
  with no login brings everything up); **T4's deb note added the same day** (print-only postinst, #922), **and the FRESH server** (#923: the
  setup makes the route, the CA's trust and the root edge through the desktop's own code, run on a
  fresh account with the VM's rexenv state parked). **T2's macOS and Windows runs done 27 Oct 2026** (fresh `t2rex` accounts
  on the 15.8 VM and the Dell; #915–#917, #919 now ✅ on all three; the card seen on both). **Phase 1a
  is complete**; open after it: ~~the 443 → 8443 → 443 round trip on one account~~ (#918 ✅ — **run on Linux and macOS 28 Oct 2026**; macOS's found the reload wedge row in § Now. Linux's on the 24.04 server VM, 0.9.4: the root unit took 8443 and 443 back through `rex restart`, no child edge, adoption kept its pid; `edge_bind 127.0.0.1` moved it to loopback), T5 (headless on
  macOS/Windows, D3), and Phase 1b (the web UI, T6–T9). Was open in T4: a FRESH server — the edge install, the `.rex` route and the CA
  trust are root steps the closed set refuses (#911), so a server where no GUI setup ever ran has
  only the 8080/8443 child edge and no `.rex` names; the setup (already root) is the natural place
  to do them. Owner ask 26 Oct 2026: install rexenv
  on a display-less server, drive it from `rex` and from the same UI served on a port. The same day,
  GitHub issue rexenv/rexenv#7 asked for 1a for automated PR QA on Ubuntu 24.04 x86_64; its reporter
  offered to test a build. A public bind and real site domains are later phases.
  Plan: `docs/PLAN-headless-web.md`.

### “Headless mode on macOS and Windows” (Phase 4+), before the rewrite

- [ ] **Headless mode on macOS and Windows** — `--headless` and `--headless-setup` refuse there with a
  sentence (owner ruling D3, 26 Oct 2026: Linux first; `platform::prepare_headless`). macOS would need a
  LaunchDaemon/LaunchAgent shape for a login-less Mac and its own answer to the window server; Windows a
  boot-time task or service with no desktop, and UAC has no unattended form. Plan:
  `docs/PLAN-headless-web.md` T5. **T0 measured 6 Nov 2026 (plan §2b), waits on the owner's D8:** Windows
  session 0 (SSH, and an S4U boot task) runs the whole backend unchanged but starts no WebView2, so the web
  UI's `/ipc` fails there; a login-less Mac hangs in `NSApplication run` before `setup` (no tokio thread,
  `rex` hangs), while any launch with a user logged in — SSH or a system LaunchDaemon — works, web UI included.
  D8 ruled the same day (two steps). ✓ **W1** 6 Nov 2026: `rexenv.exe --headless` runs on Windows and refuses
  every dialog in words (#943, run on the Dell). ✓ **W3** 6 Nov 2026: `--web` survives the Explorer hop and a
  headless launch never hops (#742). ✓ **W2** 6 Nov 2026 (#944, the Dell): `--headless-setup` registers
  `\rexenv\headless` + the agent as S4U boot tasks; a reboot with nobody logged on brought the stack, `.rex`
  and `rex` up in session 0. Still open from W2: whether an unelevated uninstaller can delete an S4U task,
  and what a silent update does while `\rexenv\headless` keeps restarting rexenv.exe (the in-app update of a
  headless PC is unbuilt); a fresh PC's first run (the CA confirmation on a desktop console). ✓ **M1** 6 Nov 2026 (#945, the 15.8 VM): `rexenv --headless` on macOS runs only
  with this account at the console — no dock tile, the prompt refused in words — and exits 2 with a sentence
  instead of hanging when nobody is logged in. ✓ **M2** 6 Nov 2026 (#945): the daemon context cannot read the login
  keychain (the CA read failed, not the resolver's), so a system-domain launch is refused and a LaunchAgent is the
  shape. ✓ **M3** 6 Nov 2026 (#946, the 15.8 VM): `sudo rexenv --headless-setup` writes the LaunchAgent and the
  consents as the account after checking the app's setup. **T5 step one is built on both OSes.** ✓ W2's uninstaller
  question measured (the unelevated token may end and delete the S4U task, not disable it — #944): the uninstaller
  deletes `\rexenv\headless`, a silent install ends it before the swap and runs it after — ✓ RUN on the Dell
  (3 of 3 `/S` over a running headless PC, exit 0, back at once). Still open: a fresh PC's first run (needs a
  person at the desktop for the CA confirmation — tried by automation 6 Nov 2026: the prompt is a system
  window no user process may answer, so it is the owner's SMOKE row). ✓ The web UI through a tunnel in a real browser (#947). D8's second step
  (the webview-free host — **superseded** 6 Nov 2026: the web UI answers in session 0 once `dispatch` falls
  back to the app's local origin, #947; no command refactor is needed). ✓ macOS reboot with automatic login (#946, the 15.8 VM): only
  `--headless` comes up once a headless launch takes itself out of macOS's reopen-at-login list.

## Reconcile of 7 Nov 2026

### From “Phase 4+ (next era)”

- [x] **Windows: a desktop rexenv does not adopt services a HEADLESS one started** (found 6 Nov 2026 in 0.9.6's Dell
  gate). The stack running from a `\rexenv\headless` (S4U, session 0) launch was "adopted 1" (Caddy only) by the
  desktop app that followed; the same 0.9.6 adopted 9 of a stack the desktop had started. Suspect: the adoption's
  process query (the cmdline marker) cannot read a process whose token is the S4U logon session's. Until fixed,
  INSTALL's "go back" says `rex stop` while the headless one still runs. Measure the query first (which call fails,
  from a desktop token, on an S4U-started `mysqld`), then fix or record it as the rule.
  ✓ 7 Nov 2026 (#948): measured — the desktop token's `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` failed with
  ERROR_ACCESS_DENIED on every headless-started pid (S4U = the FULL admin token: High, DACL without the account);
  fixed — a headless launch sets its token's default DACL to the account + SYSTEM + BA and lowers it to Medium
  before anything starts (`acl::open_children_to_this_account`); RUN on the Dell: "adopted 9", pids unchanged,
  the desktop's `rex stop` stopped them all. INSTALL's "go back" no longer needs `rex stop` first (from the
  release after 0.9.6).


## Reconcile of 7 Nov 2026

### From “Phase 4+ (next era)”

- [x] **The web UI still shows desktop actions** (found 29 Oct 2026, the 24.04 VM through a tunnel): the
  Sites row's folder icon, "Open in editor", and the `revealPath` buttons in toasts (WordPressManager,
  RepoPanel, LiveTab, SiteDetail) render over the web transport and answer with `NO_SCREEN` when
  pressed. Honest, but an action that can only fail should not be offered: hide them when
  `transport() === "web"` (or make the wrappers' callers ask one `canReveal()`), all call sites at once.
  ✓ 7 Nov 2026 (#949): `canOpenOnThisScreen()` + `openPath` in `src/lib/ipc`; every opener's file asks it
  (Sites, SiteDetail, SiteLogs, WordPressManager ×5 toasts, RepoPanel, LiveTab, open-in's menus, useEditor,
  useTerminalApp), held by `desktop_only_openers_are_offered_only_with_a_screen`; RUN over the web on the Dell
  (Playwright WebKit): no folder/editor action anywhere it looked, no console error.


## Reconcile of 7 Nov 2026

### From “Release gates (human, scripted — see the docs named)”

- [x] **Intel spot-run**: x86_64 bottle digests + MySQL 8.0.44 x86_64 were hashed
  from real downloads but never RUN (PORTS.md caveat) — run-verify on the next
  Intel machine. No physical Mac is needed: a GitHub-hosted Intel macOS runner (15.7.9) was
  already used on 29 Sep 2026 (SMOKE-TEST). This row also carries the floor's run half (the folded
  "both slices" row).
  ✓ 7 Nov 2026: `.github/workflows/intel-spot-run.yml` on `macos-15-intel`, run 42329020012 — 14 of 14 live
  examples PASS (seven PHP minors, php-fpm, nginx, Caddy, Mailpit, MySQL 8.4 + 8.0.44 x86_64, PostgreSQL,
  FrankenPHP, the MariaDB and Redis bottle bundles, `macos_floor_check` on both arches); 192 downloaded
  Mach-O files, all x86_64 (PORTS.md). Two runs before it died on the workflow itself (missing aarch64
  target; no `timeout(1)` on macOS), fixed in the file.


## Reconcile of 7 Nov 2026

### From “Now — actionable code/test work”

- [x] **The first request after `site move` fails once on Windows** (found 7 Nov 2026 by #953's run on the
  Dell): with the fix in, `rex site move` returns, the first GET answers 404 "No input file specified." (or
  no connection) and the next ones 200 — the command returns before nginx's reload has taken effect. Measure
  where (`move_site_docroot` awaits `ReadyCheck`s — which, and do they cover the reload?) before changing it;
  macOS/Linux unmeasured.
  ✓ 7 Nov 2026 (#955): measured — no check covered the reload (`nginx -s reload` returns once the master is told);
  fixed — `ServiceManager::reload` waits, outside the lock, for a new nginx worker generation; RUN on the Dell: a
  fresh site moved, GETs 1–3 all 200.

