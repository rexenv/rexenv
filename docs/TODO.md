# TODO — the single active-work file

Everything open lives here. Completed phases + audit history: `docs/archive/`
(historical — don't trust as current). When you finish an item, tick it here with a
one-line ✓ evidence note (same convention as the archived TASKS files).

## Actionable now

- [x] **Supervisor-aware fix-it commands + status-pill ghost fix** (`2fc0b37`/`d55f3dd`).
  (1) Port-conflict messages carry a copyable command matched to how the holder is
  MANAGED (toast CommandBlock): app-supervised → `osascript -e 'quit app "Herd"'`
  (live-tested: quits Herd, frees 127.0.0.1:443); brew binary → `brew services stop F
  || sudo …` (Valet's nginx lands here); else `sudo kill <master-pid>`; wired into
  ensure_free, Start-all wire gate, watchdog edge-blocked, bind-conflict error.
  (2) Status-badge overlap root-caused in two layers: exact-fit pill widths (86px <
  natural "Running" 88.3/"Stopped" 89.9 in WebKit metrics — SF Pro, NOT an
  Inter/WKWebView delta; the font stack puts -apple-system first) AND a WKWebView
  compositing ghost — `transition-opacity` layerizes the row while the label swaps,
  and →Idle stops all animation so the stale "Running" snapshot lingers till the next
  poll (→Running mounts the ping dot, forcing recomposition — hence the asymmetry).
  Fixed: row dims instantly (no opacity transition), all pills `min-w-[92px]` +
  nowrap + flex-none markers (incl. the DNS pill still at 86px), so no future label
  can wrap or clip. ✓ WebKit-measured (8 pills × 92.0px, single line); ✓
  **live-verified in the packaged app** (Jul 13): Caddy Running↔Idle toggled
  repeatedly, clean both directions.
- [x] **Herd/port-conflict honesty + opt-in login-start** (post-reboot 502 report,
  Jul 13). (1) `friendly_holder` attributes app-bundled listeners to the owning app
  ("Herd (nginx, pid 1234)"); `start_edge_daemon` timeout and the watchdog edge-down
  give-up now NAME the :443/:80 holder instead of a vague "socket never came up"
  (`c3c1236`). (2) New default-off setting `start_services_on_launch`: app launch runs
  Start-all after adoption — with app-autostart on, the stack returns after reboot
  promptlessly (boot daemon has the edge up → adopted). Login-safe: never downloads
  (cold cache → honest service-health event), never prompts (privileged edge plan →
  skipped + surfaced). Settings UI: old "Start services on login" toggle renamed to
  the truthful "Open rexenv at login" (it only installed the login item); new real
  toggle added (`3fa1753`). ✓ 234 lib tests, examples, tsc. **Live test of (1) found the
  REAL failure mode** (`d4e5892`): Herd binds `127.0.0.1:443` SPECIFICALLY, coexisting
  with our wildcard `*:443` bind — NO bind error anywhere (netstat: both listeners;
  caddy-start.log clean), kernel routes loopback to the most-specific listener = Herd →
  all-green UI while Herd 404s every site (TLS issuer: Laravel Valet CA). Process
  identity (admin unix socket) cannot catch this — fixed with positive WIRE identity:
  every site block stamps `header X-Rexenv-Edge "1"`; `proxy::edge_answers_as_ours`
  probes 127.0.0.1:443 (DNS-free, marker-checked; `Server: Caddy` fallback); watchdog
  flips `edge_blocked` on transitions (`edge-blocked`/`edge-unblocked` events naming
  the holder); `status()` folds it in (Caddy + site dots read NOT running while
  shadowed); Start-all/auto-start gate on the probe and fail naming the interceptor.
  ✓ **Live-verified (A)** (Jul 13): with Herd running — Start-all fails naming Herd,
  Caddy row flips red ≤10s, recovery on Herd quit; holder attribution fixed en route
  (`c6200c7`: master pid not worker, real exe via txt FD not the rewritten ps title,
  `.app`/Application-Support attribution → "quit Herd", honest name+pid+path degrade).
  ✓ **Live-verified (B)** (Jul 13): reboot with both
  toggles on, untouched — edge up 16s after boot (pre-login), DNS agent at login,
  auto-start spawned MySQL/fpm/nginx promptlessly, `https://tr.rex`/`lm.rex` → 200,
  wire identity ours (`x-rexenv-edge: 1`). health.log also captured the whole Herd
  suite firing in production: `edge-blocked` naming "Herd (nginx-arm64, pid …)" +
  copyable quit command, `edge-unblocked` on Herd quit.
- [x] **Sites die 1–2h after quitting the app — DNS must survive the app.** Root cause
  (evidence-first diagnosis): the data plane (nginx/fpm/MySQL/edge, all detached or
  launchd-owned) survives a quit indefinitely — but the resolver was an IN-PROCESS tokio
  task that died the instant the app quit. Sites coasted ~1h40m on client caches +
  persistent H2 connections (access log: steady 60s polls 18:37→20:17, flapping
  revivals, final 58-min outage ending 34s after app relaunch with ZERO server
  restarts — same pids throughout). Sleep exonerated (`pmset -g log`: awake through all
  outages); idle timeouts exonerated (fpm/MySQL kill workers/connections, never
  masters); macOS never reaps orphans. ✓ **Fixed:** DNS now served by a per-user
  LaunchAgent `dev.rexenv.rexenv.dns` (`<binary> --dns-agent`, KeepAlive + RunAtLoad,
  all unprivileged — new 11th trait `DnsAgentManager`); app launch = adopt
  (`answers_as_ours` wire probe, H2) or install/refresh (plist tracks current binary,
  dev↔installed hand off; busy-port retry every 10s hands off from an old in-process
  holder) or IN-PROCESS fallback (never regress); watchdog kickstarts a dead agent
  (bounded 3) then one in-process fallback; `dns_status.mode` = agent | in-process |
  down; teardown uninstalls the agent. ✓ 233 lib tests green (plist invariants +
  `answers_as_ours` live probe), examples build, tsc clean. ✓ **Live-verified with the
  app CLOSED** (Jul 12): OS-chain resolution (dscacheutil → /etc/resolver/rex → agent)
  answers 127.0.0.1; `https://tr.rex`/`lm.rex` → 200; SIGTERM'd agent relaunched by
  launchd in ~3s and kept resolving. Follow-up: Settings doesn't RENDER `dns_status.mode`
  yet (backend field + TS type only).
- [x] **Xdebug per-site toggle (§8.2) — UNBLOCKED 16 Jul 2026, built.** The
  original blocker (no hosted Xdebug for static PHP) dissolved: shivammathur's
  homebrew-extensions ghcr bottles (`xdebug@8.1`–`@8.5`, Xdebug 3.5.3 — the same
  content-addressed digest-pinned bottle class as Redis/MariaDB/Apache; the tap
  powering GitHub Actions setup-php) **dlopen directly into our EXISTING static-php
  binaries** — no debug PHP build, no extension-parity problem, extension set stays
  identical by construction. Feasibility proven first (cli+fpm load on 8.1–8.5,
  `xdebug_info()` functional, DBGp init on a live socket; sonoma bottles load on
  Tahoe; `.so` links only system libs; old tags retained → pins 404-but-never-
  drift). **8.0.30 excluded** (see Blocked). ✓ **Built** in 5 commits `617d050`/
  `c424322`/`a731311`/`cb28b13`/`99e9fce` + example: per-minor one-part bundles
  (all 10 digests downloaded + load-tested at pin time); per-minor DEBUG pools
  (same fpm binary, `-d zend_extension` + `xdebug.mode=debug,develop`, ports
  `9981–9985`, spawned only for toggled sites, adopted/watchdog-respawned/orphan-
  swept/settings-restarted like normal pools, status row `PHP-FPM x.y (Xdebug)`);
  **every debug spawn gated on a real load probe** (PHP treats a bad
  zend_extension as a warning — the gate makes it an error, never a silently
  Xdebug-less pool); v11 `sites.xdebug`; `pool_port_for_site` = the single
  routing seam (nginx vhost, Apache override, site_serving; stale flag on 8.0
  falls back to the normal pool); core-validated toggle (FrankenPHP + 8.0
  refused with real reasons, disable always allowed); toggle carries across a
  PHP-version switch; Start-all prefetches toggled minors' bundles; SiteDetail
  Settings card with IDE hints. ✓ 266 lib tests, clippy (no new), examples,
  tsc, vite. ✓ **Live-verified via `examples/xdebug_pool_check`** (real cache,
  production paths): bundle download→verify→relink→sign→publish, loads-clean +
  codesign pass, load-probe gate Ok, **full DBGp handshake**, debug pool via
  `start_fpm_xdebug` accepting. ✓ **Human-verified in-app** (16 Jul 2026): the
  per-site toggle works on a real site.
- [x] **Watchdog races an in-flight Start-all.** Observed live (health.log 12:25:46Z,
  during the edge-daemon verification): a watchdog tick landed between `start_core`
  spawning MySQL/fpm and their readiness, saw "port closed", and killed + respawned
  them mid-start (benign outcome, but a needless kill of a healthy starting child —
  and a slow-to-boot MySQL could be respawn-looped into `gave-up`). The edge branch
  was already race-free (bootout-first stop, `aa79c93`). ✓ **Fixed** with the
  per-spawn-timestamp option (self-clearing — no caller has to remember to clear a
  flag on error paths, and it covers EVERY spawn path incl. the watchdog's own
  respawns, so a slow starter can't be respawn-looped): `Proc::Child` now carries
  its spawn `Instant` (stamped in `From<Child>`, compile-enforced everywhere);
  `Proc::starting()` = within `START_GRACE` (30s = 2× the longest readiness budget
  of 30×500ms); adopted survivors get NO grace (they were already serving). All
  five non-edge watchdog branches (DB engines, fpm pools incl. debug, override
  backends, Mailpit, nginx) now treat port-closed as dead only when the master is
  gone OR the grace has lapsed — a dead master is still reaped instantly, grace or
  not (a crash during start must restart), so the orphaned-workers detection is
  untouched. ✓ 268 lib tests (+2: grace semantics in proc.rs; a reap_dead sweep
  proving a live just-spawned pool with a closed port survives while a dead master
  is reaped despite grace), clippy (no new), examples build. Observe on the next
  few Start-alls: health.log should show no "restarted … port closed" events
  during startup.

- [x] **Configurable TLD (v1: default-TLD-for-new-sites)** — stored `default_tld`
  setting; policy-driven `validate_domain` (hard-block `.local`/gTLDs/2-letter in CORE —
  refused even via direct invoke; warn tier for non-RFC-2606 TLDs); answer-all DNS
  handler + one `/etc/resolver/<tld>` per TLD (first-use prompt, no DNS restart);
  uninstall sweeps all rexenv resolver files by content signature; `.test` backbone
  permanent; wp-login mu-plugin allow-list now per-site domain; Settings picker +
  TLD-aware dialogs. ✓ **Done** in 5 commits `bb1ba89`/`509ec95`/`7dad2de`/`dc6cb57`/
  `b1e103d`. **Corrected 12 Jul** (`e9a87d9`/`9b14cbe` + docs): `.rex` is the backbone —
  onboarding installs ONLY `/etc/resolver/rex`, v9 migration flips the default,
  `ADMINER_HOST` → `adminer.rexenv.rex`; `.test` is an ordinary safe-set choice whose
  resolver installs on demand. All self-checks green (225 lib tests, clippy, examples,
  tsc, vite); **awaiting human runtime verification** — Done-when checklist in
  `docs/TLD-FEATURE-REPORT.md`.
- [x] **Edge "stops by itself" — adopt a live edge, never kill it.** Root cause of the
  `health.log` `edge-down` entries: every edge death was rexenv's own `caddy stop` —
  (a) `prepare_edge` treated a LIVE edge as a stale leftover (`recover_stale_edge`)
  whenever the manager's state said stopped, so Start-all killed the healthy root edge
  and re-prompted for the password; (b) the watchdog was one-way (running→stopped,
  never back), so one stale edge-down left the UI lying and invited exactly that
  Start-all; (c) live-check examples share the real app-data dir and stop the edge via
  the shared admin socket (`health_watchdog_check` artifacts match the Jul 7 event
  exactly — see item below). ✓ **Done:** `prepare_edge` adopts+reloads a live edge
  (fresh-start path only if the reload is refused); `reconcile_health` re-adopts a live
  edge (`"adopted"` event, surfaced as an info toast); verified live against the running
  root edge — pid unchanged through both flows (`examples/edge_adopt_reload_check.rs`),
  `cargo test --lib` 153 green, site 200 through the edge after.
- [x] **Edge stays up unless explicitly stopped — root LaunchDaemon KeepAlive.** The
  remaining "Caddy stopped by itself" case was NOT rexenv: an external OS `SIGTERM` to
  the root edge (graceful `exit_code 0`, confirmed via `caddy-start.log`; not a crash,
  not sleep/wake, not a port conflict). The edge was the ONE service the health watchdog
  never auto-restarts (a privileged `:443` start needs an admin prompt), so any external
  kill left every site unreachable until a manual Start-all — and the `edge-down` toast
  only showed if the app window was open. ✓ **Done:** new `EdgeSupervisor` trait (10th
  platform trait) → macOS root LaunchDaemon `dev.rexenv.rexenv.edge` (`KeepAlive=true` +
  `RunAtLoad=true`); launchd relaunches the edge on any death/sleep/reboot.
  `proxy::start_edge_daemon` (stage unprivileged → one privileged `cp`-into-root-tree +
  `bootstrap`), `stop_edge_daemon` (`disable`+`bootout`, out of lock — Stop-all now costs
  one prompt), `CaddyHandle::Daemon` across start/adopt/stop/status, watchdog
  `edge-restarting` (info) instead of `edge-down` for a Daemon edge. **Security:** the
  daemon execs a `root:wheel 0755` COPY of caddy (never the user-writable cache — LPE
  guard); plist `root:wheel 0644`. `cargo test --lib` 229 green (+4 edge-daemon tests),
  examples build, `tsc` clean. Commits `4f45ff7`/`c15680d`. NOTE: the daemon installs on
  the next Start-all (first run re-prompts once); supersedes the osascript start path
  (kept only for the `caddy_443`/`service_manager_demo` examples). Partially overlaps the
  deferred **SMAppService** item below (that would fold the remaining setup prompts into
  one registration). **Follow-up (live Stop-all exposed 3 state-machine defects, fixed):**
  (1) Stop-all raced the watchdog — `stop_all` cleared the handle, the watchdog re-adopted
  the still-serving edge during the auth prompt, the bootout then landed → stale `Daemon`
  handle (health.log 11:56:08 `adopted` + 11:56:11 SIGTERM) → `stop_services` now boots
  out BEFORE touching manager state; (2) `edge-restarting` was an unbounded every-10s
  reassurance even for an edge never coming back → bounded: announce once, 3-poll grace,
  then DIAGNOSED `edge-down` (uninstalled/disabled via new `EdgeSupervisor::is_enabled`/
  blocked) + handle→Stopped; (3) `prepare_edge` trusted a stale non-Stopped handle → every
  Start-all silently skipped the edge — now liveness-checked (H2), stale handle falls
  through to a fresh start. Also `chown -h` in the launcher loop (root chown on a
  user-controlled path must not follow symlinks). ✓ 231 lib tests green incl. 2 new
  state-machine regression tests (mock platform). ✓ **Live-verified full cycle**
  (Jul 12): Start-all from a `disabled` label (re-enable+bootstrap) → external
  `sudo kill` self-healed in ~1s with ZERO health events → Stop-all prompt-first,
  edge stayed down, zero events across 35s (3+ polls) → Start-all-after-stop restored
  the edge, sites 200 over `:443` (`lm.rex`/`tr.rex`/`adminer.rexenv.rex`).
- [x] **Isolate live-check examples from the real stack.** `examples/*.rs` use
  `platform::current()` → the REAL app-data dir: their `start_all`/`stop_all`/
  `recover_stale_edge` stop the USER'S running edge over the shared admin socket (and
  restart shared services). Adopt-don't-kill removed the worst path, but an example's
  explicit `stop_all` still tears the stack down. Options: env-var app-data override for
  example runs, or a guard that refuses `stop_all` when the edge wasn't started by the
  example. ✓ **Done (guard option)** — sharing the real app-data is the POINT of a
  live check (cache + adopt paths), so the fix is provenance, not isolation:
  `core::stack_guard` — a non-app process may stop only what it SPAWNED
  (`Proc::Child`). Guarded chokepoints: `proxy::stop_edge` (skip+log),
  `recover_stale_edge` (refuse with a real error), `stop_stale_owned` orphan sweep
  (skip), and adopted-`Proc` skips in `stop_all` + `PhpFpmPools::stop_all` (Drop
  paths were already adopted-safe). App opens the guard via `mark_app_process()`
  in `lib::run`; deliberate utilities (`stack_stop`, `adopt_check`,
  `caddy_recovery_demo`, `service_manager_demo`, `mail_adopt_settings_check`)
  call `allow_real_stack_control()`; ad-hoc override `REXENV_CONTROL_REAL_STACK=1`.
  ✓ 269 lib tests, examples build. ✓ **Live-verified** (16 Jul,
  `examples/stack_guard_check` — no opt-in, real running stack): with edge +
  MySQL + MariaDB + 6 fpm pools serving, `recover_stale_edge` refused naming the
  guard, `stop_edge` skipped (admin socket stayed live), adopt (11 services) +
  `stop_all` left everything serving; `https://tr.rex` → 200 with
  `x-rexenv-edge: 1` after.
- [x] **L7 — move shared Nginx off 8088** (from BACKLOG). `8088` collides with Hadoop
  YARN / common dev proxies; moved to `18088`. Low risk: loopback-only, bind-tested at
  start. ✓ **Done:** `core/services.rs` `NGINX_HTTP_PORT` = 18088; configs regenerate
  from the constant on every stack start (`rebuild_configs_for`); tests/examples/mock/
  docs updated; `cargo test --lib` green. Transition note: a stack left running by a
  pre-change build keeps its old nginx on 8088 — it is not adopted (adoption keys on the
  new port) and not auto-reaped; kill it manually or via the old build's Stop all.
- [x] **In-flight download dedup** (download-manager follow-up, NOT a release blocker):
  add a per-(name,version) async once-lock (single-flight map) in `core/binaries.rs`
  `resolve*` so concurrent callers await the same download instead of racing — today the
  same bytes download twice and the hub progress bar jitters between the two streams.
  Pre-existing race (any two commands resolving the same binary); onboarding's
  auto-prefetch makes it easier to trigger. Correctness is fine (atomic staging/publish
  keeps one winner) — this is efficiency/polish. ✓ **Done:** `in_flight(name, version)`
  single-flight guard acquired first in `resolve`/`resolve_file`/`resolve_dir` — the
  second caller waits, then hits the cached-path early return; a failed first attempt
  lets the waiter download (natural retry). New serialization unit test; 167 lib tests
  green, clippy clean.
- [x] **Release 1.5 — cold first run on a second Mac / clean account** (from TASKS-RELEASE).
  Hands-on: install the .dmg on a machine that has never seen rexenv, run the full first-run
  flow. Pairs with the next item. ✓ **Done:** verified by a real fresh-account cold run on
  10 July 2026 — onboarding system setup, live binary downloads, WordPress site over HTTPS
  with a valid lock, rest of the app all worked end to end.
- [ ] **Release 5.4 — execute the clean-Mac smoke test** — checklist already written:
  `docs/SMOKE-TEST.md`. First pass (fresh-account, 10 Jul 2026) all green except the
  multisite-convert item, untestable because the convert UI didn't exist — fixed below;
  re-verify converted-multisite items + onboarding fixes on the next cold run.

## Smoke-test fallout (10 Jul 2026 fresh-account run) — all fixed

- [x] **Multisite convert missing from the UI** (spec §2.1 "one-click enable/convert").
  Never surfaced, not a regression: §10.1 shipped backend+IPC only, §10.3's Network tab
  was gated multisite-only, and 12.3's toggle was create-time only — the convert-an-
  existing-site seam fell between the three. ✓ **Done** `f72ee5c`: Network sub-tab
  always shows; single sites get a convert panel (subdomain/subdirectory cards, wp-config
  + URL-structure warning, confirm) that flips to the network manager on success.
  Verified live both modes: subdirectory sub-site at `/site1`, subdomain sub-site over
  HTTPS with a valid lock (wildcard cert/route end-to-end), and Reset on a converted
  site returns a clean single-site install.
- [x] **Onboarding "Domains & SSL" was skippable** → app where no site loads (no
  resolver/CA). ✓ **Done** `fbe72be`: Continue locked until `dns_status` reports
  `resolverInstalled && caTrusted` (real state, refetched after every setup attempt);
  Welcome's "Skip setup" removed (same hole). No dead-end: setup stays retryable with
  the friendly cancelled-prompt error, window stays quittable. Logic verified; locked
  visual to be eyeballed on the next cold run (warm machines always show it unlocked).
- [x] **Onboarding window not draggable** (renders outside AppShell → no drag-region
  header). ✓ **Done** `d99d095`: 60px title-bar drag strip wired to the shared
  `onTitleBarMouseDown` (drag + double-click maximize). Verified via `#/onboarding` on
  tauri dev.
- [x] **Onboarding content clipped at small window heights** — flex-1 without min-h-0
  pushed the footer (Continue) out of the overflow-hidden root, no scroll. ✓ **Done**
  `6031a34`: content scrolls under a pinned footer; inner min-h-full wrapper keeps steps
  centered when there's room. Verified at the 640px minimum on the Install step.
- [x] **DB-export toast: "Show in Finder"** (nicety). ✓ **Done** `2d243f8`: new
  `ShellRunner::reveal` (macOS `open -R`, win/linux `todo!()`), thin `reveal_path`
  command, toasts gained an optional action button (10s TTL); both export call sites
  (Tools + Reset dialog) reveal the written `.sql`. Verified live from both.

## ✓ Shipped — 11 Jul 2026 session (Site Settings + WP Manager expansion)

Each line = one feature, live-verified before its commit.

- [x] Site Settings tab v1 — rename, site info (type / DB name / multisite), HTTPS
  cert card (issued/expires/SANs/folder via `ssl::site_cert_info`) `5f091c1`
- [x] Maintenance mode toggle (Tools) `edd3255`
- [x] Debug-constant toggles — WP_DEBUG_LOG / WP_DEBUG_DISPLAY / SCRIPT_DEBUG,
  core-whitelisted `baae862`
- [x] Permalink structure picker (stock presets, honest Custom display) `780c8c2`
- [x] Cache flush + delete-all-transients `8b69ffb`
- [x] Per-user role dropdown; primary-admin guard enforced in core `b4416ce`
- [x] Cron viewer + run-due + per-hook forceful run `3b95253`
- [x] Core checksum verify with benign/real triage (exit code never drives the
  verdict) `6b31503`
- [x] DB import — typed confirm + backup-first, bundled mysql over stdin `fe7654b`
- [x] WXR content export to Downloads `59c400c`
- [x] Tools regrouped: Backup & restore / Core / Maintenance cards `66d528e`
- [x] Cert regenerate — per-site Regenerate in the SiteDetail Settings-tab cert card,
  plus a fix for the global Settings button: a byte-identical Caddyfile makes plain
  `caddy reload` a no-op, so re-issued certs were never served until an edge restart —
  both paths now force-reload; re-issue is atomic (temp-write + rename, never
  delete-first), so a failure leaves the old cert intact and served `57b7281`
- [x] Site language switch — Tools Language card: picker (Installed/Available) from
  `wp language core list`, install-if-needed + `wp site switch-language` in one action;
  success gated on `language core is-installed`, NOT install's exit code (a failed/
  offline download still exits 0 — same trap class as checksum verify); install capped
  at 60s wall-clock (WP's download_url waits 300s/attempt — offline that froze the
  spinner; timed-out child is SIGKILLed, error surfaces, old language kept); core
  translations only, multisite note (main site only) shown honestly `ed7d74e`
- [x] Checksum "Clean up macOS system files" — deletes the panel's benign list with
  four backend guards per file (is_os_noise basename reused, relative/no-`..`, lstat
  regular-file — the guard that stops a `.DS_Store` symlink from deleting its TARGET —
  canonicalize+prefix inside docroot); skip-not-abort with reasons; auto re-verify
  refreshes the panel in the same round-trip `24d35bd`
- [x] Site options editor — curated 11-option scalar whitelist (default-deny: siteurl/
  home/active_plugins/serialized options unreachable by construction, `OPTION_FIELDS`
  enum in core enforces name + per-kind value on every write); non-scalar values
  refused at read AND write; typed inputs (timezone/role pickers fed live), confirm
  old → new, form re-reads from the site after save `fe80fb4`
- [x] Core version switch / downgrade — picker from the stable-check API (831
  releases, ≥6.0, insecure marked), `wp core update --version --force` under a 300s
  cap, success gated on `wp core version == target` (never the command's claim),
  post-switch db_version probe returns dbUpdateRequired so the panel states the
  "Database Update Required" screen explicitly; honest confirm + Export-DB-first in
  the flow `0d3a6e5`

## PHP-versions gap vs Herd/Valet (scoped 11 Jul 2026; #3 built, #1/#2 deferred)

- [x] **Per-version PHP settings (#3)** — memory_limit / upload_max_filesize /
  post_max_size / max_execution_time / max_input_time / max_input_vars, per minor
  (matches shared per-version pools). Whitelisted+typed validation (`core::php::SETTINGS`,
  default-deny like the options editor), `php-fpm -t` gate on a `.conf.candidate` before
  any restart, values as `php_value[…]` lines in the pool conf, SQLite `php_settings`
  table (migration v5). Both gotchas handled: per-site nginx `client_max_body_size`
  mirrors max(upload, post) of the site's version + upload>post rejected as a set;
  `request_terminate_timeout` rises to max_execution_time (300s floor, cap surfaced in
  the UI note). FrankenPHP sites unaffected (own embedded PHP — stated in the UI).
  Settings → PHP versions → per-installed-version "Settings" editor. ✓ **Done**
  `70266a7`: live-verified on a throwaway site — 512M via ini_get; 64M upload/post lets
  a >2M wp-admin Media upload through with `client_max_body_size 67108864` in that
  site's nginx block; upload-alone rejected (cross-field); "banana" rejected with no
  restart; cleared field reverts to default. 195 lib tests, clippy clean.
- [x] **More PHP versions (#1):** 8.0.30 / 8.4.23 / 8.5.8 pinned — offered set is now
  8.0–8.5. **7.4 stays absent:** static-php never shipped it — needs self-build +
  self-hosting, same blocked path as the Xdebug debug build. ✓ **Done** `ac19d31`: all
  12 artifacts downloaded, hashed, extracted, arch-checked, and RUN (arm64 native +
  x86_64 Rosetta; `php -v` + mysqli verified on each — no guessed hashes); registry/
  ports/pools/hub/UI flowed through untouched. Live-verified: 8.0 + 8.4 installed from
  the UI with real download progress, sites assigned show the right version, WordPress
  loads on 8.0 (bulk extension set OK on the oldest). Ongoing risk (accepted): upstream
  rebuilds in place → 24 pins to babysit.
- [x] **PHP patch updates (#2): Option A only** — pins ride app releases; NO in-app
  TOFU updater (static-php publishes no checksums — runtime update-discovery would move
  pin trust from the signed app binary to the user's machine). ✓ **Done** `ef0465d`:
  `seed_registry` reports stored-patch ≠ build-pin (one-shot, `installed` preserved);
  startup task prefetches the new patch via the hub BEFORE any lock (offline = old pool
  keeps serving, retried next launch), restarts bumped minors' live pools through the
  same `restart_php_pool` path as the settings editor, then GCs `php-<oldpatch>/`
  caches (narrow name rule — never debug builds/staging/other binaries/unpinned
  minors). Live-verified via DB simulation: pool restarted on the pin, site kept
  serving, fake stale cache GC'd + logged, second relaunch a no-op. Maintainer
  release procedure = bump `PHP_VERSIONS` + re-verify pins (binaries.rs:96) + update
  `docs/PORTS.md`; everything else is automatic.

## Parked (deliberate — needs explicit go; don't pick up silently)

- [x] **Change domain** — cross-cutting: cert re-issue + config regen + WP search-replace,
  and the DB name derives from the domain (L). ✓ **Done** in two commits:
  `688142a` (prerequisite) stores `db_name` on the site row (v6 migration backfills,
  every runtime reader switched — user-verified reset + export/import round-trip on a
  pre-migration site); `b0975b3` — `change_site_domain` orchestrates preflight (validate +
  uniqueness + multisite REFUSED honestly) → mandatory Downloads backup (abort on
  fail) → new-domain cert (additive) → `wp search-replace` dry-run gate then two real
  passes (`https://old→https://new`, bare `old→new`, `--all-tables`) → SQLite domain
  flip (docroot + db_name untouched by design) → config regen + forced edge reload →
  best-effort old-artifact cleanup (cert dir, tunnel, FrankenPHP config/logs). DNS
  needs nothing (wildcard `*.test`). UI: Settings → Domain card (disabled on
  multisite), destructive dialog with backup/email-rewrite/reversal notes; frontend
  `siteDbName()` derivation deleted in favor of stored `site.dbName`. Live-verified
  on a throwaway site (post + image): myapp.test → myshop.test served with valid
  SANs, no redirect to the old domain, serialized/attachment URLs rewritten, old
  domain no longer routes, reset works after the change (db_name consistent),
  multisite card disabled, old cert dir removed, backup in Downloads.
- [x] **Move site / custom docroot** — fixed `sites_dir` scheme + nginx root regen (M/L).
  ✓ **Done** `c098077`: `move_site_docroot` — preflight rejections before any file is
  touched → same-volume rename / cross-volume copy+VERIFY (partial cleaned up) → row
  flips only after files exist at target → regen+reload → old tree deleted LAST.
  `sites.path` confirmed the single runtime source (no db_name-style trap). Also fixed
  a real reconcile bug: running FrankenPHP overrides never restarted on a changed
  docroot/rewrite — now desired config is diffed against the on-disk file. UI:
  Settings → Site folder card (picker + confirm with caveats). Live-verified:
  same-volume + cross-volume (hdiutil image) moves, reset-after-move, rejection
  errors, FrankenPHP serving from the new root post-move.
- [x] **Per-site env vars** — conflicts with per-VERSION shared php-fpm pools; no clean
  seam (L). ✓ **Done** `7cc2751`: the seam existed after all — vars ride the REQUEST
  (per-site nginx `fastcgi_param` lines), pools untouched; FrankenPHP overrides get
  config `env` lines + real process env at spawn (own process per site;
  `spawn_logged_env`, trait default errors so stubs can't drop vars). `core::site_env`
  trust boundary: reserved-name list test-locked to `TEMPLATE_FCGI_PARAMS`, reject the
  unescapable (`$`, `{}`, control chars), escape `\`/`"`. v7 `site_env` table (CASCADE),
  manager map mirrors php_settings, Settings-tab editor with honest note. Live-verified
  probe on BOTH servers: getenv()/$_SERVER/$_ENV all set; injection strings + reserved
  names rejected; watchdog respawn keeps env. Footguns (variables_order EGPCS-no-ini,
  FrankenPHP spawn env) recorded in ARCHITECTURE.md.

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output, not a bug — intentionally not filtered.

## Blocked on external work

- [ ] **Xdebug on PHP 8.0** (corner of §8.2) — the Nov 2024 static-php 8.0.30 build
  exports ZERO Zend symbols (`nm -gU` = 0; dlopen of any xdebug.so fails with
  `symbol not found: _OnUpdateBool`), and upstream still serves that exact build
  (re-verified 16 Jul 2026). 8.1+ solved via the bottle path (see Actionable). Fix
  needs an upstream rebuild of 8.0.30 with symbols, or the self-build recipe
  (`docs/xdebug-debug-build.md`, `php-debug` variant wiring in `core/binaries.rs`
  kept as the fallback path). PHP 8.0 is EOL — acceptable to leave excluded.
- [ ] **SMAppService privileged helper** (Phase 1 §10.4) — single-prompt system setup.
  Needs a signed + notarized bundle → packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** (Release 1.6) — needs paid Apple account.
- [ ] **OpenLiteSpeed override server** (researched 15 Jul 2026 — moved here from
  "Deferred services": this is NOT a bundling problem, there is no macOS binary to
  bundle). Evidence: upstream releases ship Linux tarballs only
  (`openlitespeed-1.9.1-{aarch64,x86_64}-linux.tgz`); homebrew-core has NO formula
  (nothing content-addressed to pin); the only community path is a third-party
  source-build tap (`puleeno/homebrew-openlitespeed`) frozen at EOL 1.4.51 — fails
  the trust model (unpinned third party, on-machine compilation) AND version quality.
  Modern source (1.9.1) DOES carry Darwin branches in `build.sh` (brew/port dep
  detection, mod_security forced OFF, CMakeLists sed-patched at build time, clones
  `litespeedtech/third-party` for vendored deps) — so a MAINTAINER self-build +
  self-host is plausible but unproven; it needs the same missing infra as the Xdebug
  debug build (artifact hosting + checksum pin, ideally after Developer ID signing).
  Code is ready and honest today: `ensure_server_available` in CORE refuses OLS at
  create AND switch (no IPC path can make a phantom OLS site), UI never offers it,
  and the manager's `OverrideKind` seam means enabling it later = one new arm +
  config template, not a restructure. NOTE for Phase 4: on Linux this is CHEAP —
  official upstream tarballs exist.

## Deferred services (the dylib-tree-bundling step now EXISTS — Redis proved it)

- [x] **macOS dylib-tree bundling** (the family's shared blocker) — `4392a57`:
  `core/binaries.rs` `bundle_manifest`/`resolve_bundle` assemble Homebrew-bottle ghcr
  blobs (content-addressed: the URL embeds the pinned digest — pins can 404 but never
  drift, unlike static-php/FrankenPHP rebuilds; anonymous bearer) into ONE cached tree
  (include-filtered strip-2 extract; same staging→prepare→atomic-publish as
  `resolve_dir`), and the new `BinaryProvider::prepare_binary_tree` (macOS) rewrites
  every Mach-O's non-system load command (`@@HOMEBREW_*@@`) to `@loader_path`-relative
  paths into `lib/`, errors loudly on an unbundled dep, verifies post-relink, ad-hoc
  re-signs LAST. ✓ 248 lib tests (+7), clippy, tsc.
- [x] **Redis engine** — `cb1e9ac`: `core/redis.rs` (argv-only config, data under
  app-data — the `--dir` path doubles as the adoption marker), `DbEngine::Redis`
  available on macOS (Services row, watchdog, adoption, ports all via existing
  `available()` plumbing), Databases row shows a `redis-cli -p 16379` hint instead of a
  dead Browse (no Adminer driver). ✓ **Live-verified** (`examples/redis_bundle_check`,
  Jul 14): both bottles downloaded + published to the real cache; all 4 Mach-Os
  loads-clean (`otool -L` = system/@loader_path only) + `codesign --verify --strict`
  pass; served on :16379; PING→PONG + SET/GET round-trip through the bundled
  redis-cli; clean stop. x86_64 bottle digests are Homebrew-published — re-verify on
  the next Intel smoke run.
- [x] **MariaDB engine + site→engine selection at create** — `62c498a`/`b5861c4`.
  The feared dep closure collapsed on inspection: `mariadbd`/clients link ONLY
  openssl@3 + pcre2 (groonga/lz4/lzo/xz/zstd are mroonga/connect PLUGIN deps; plugins
  excluded ⇒ libs never bundled). Bundle = mariadb bottle (server + `mariadb` +
  `mariadb-dump` + bootstrap SQL + errmsg/charsets — the 221MB `bin/`, plugins,
  baked-brew-path scripts all excluded) + openssl@3 + pcre2. `core/mariadb.rs` init =
  `mariadbd --bootstrap` fed the bundled SQL over stdin (`@auth_root_socket=NULL` →
  passwordless root, the MySQL model; NO install-db script — it's a shell script full
  of baked brew paths); explicit `--lc-messages-dir`/`--character-sets-dir` (compiled
  defaults are placeholders). Site seam: v10 `sites.db_engine` (default mysql),
  `core/database.rs` fns now take the client/dump BINARY (MariaDB = same protocol),
  every site DB op dispatches on the site's engine (create/reset/export/import/
  change-domain backup/delete/sizes-per-engine), Start-all spawns MariaDB exactly when
  a site lives there, New-Site dialog's Database field is a real MySQL/MariaDB picker,
  Adminer browses 13307 via the MySQL driver. ✓ **Live-verified**
  (`examples/mariadb_bundle_check` + `examples/mariadb_site_check`, Jul 15): 3 bottles
  → merged tree, all 6 Mach-Os loads-clean + strictly signed; fresh bootstrap; served
  :13307, `SELECT VERSION()` = 12.3.2-MariaDB; then a full WP site: `wp core install`
  over 13307, siteurl round-trip through php mysqli, `mariadb-dump` export (91KB) +
  re-import, `reset_site` drop+reinstall — all green. 253 lib tests, clippy, examples,
  tsc. **Human verify next:** create a MariaDB site from the dialog, site over HTTPS,
  per-site Adminer Browse.
- [x] **Apache (httpd) override server** — the closure shrank on inspection again:
  `bin/httpd` links ONLY apr + apr-util + pcre2 (+ system expat/iconv); openssl/
  brotli/nghttp2 belong to mod_ssl/mod_brotli/mod_http2, which are excluded (TLS/H2
  are the edge's job) — bundle = httpd (server + ONLY the 10 conf-loaded modules +
  the real `mime.types` from the bottle's staged etc/) + apr + apr-util + pcre2.
  `core/apache.rs` mirrors frankenphp.rs: loopback backend on 8300–8399 (same FNV,
  own base — a server switch can't collide with itself), NEVER the edge; `.php` →
  the site's SHARED php-fpm pool via mod_proxy_fcgi (per-version PHP settings apply
  identically; env vars ride the request as `SetEnv`, same delivery class as nginx's
  `fastcgi_param`); `AllowOverride All` — `.htaccess` works, the point of Apache;
  subdirectory-multisite mirrors WP's canonical network rules in server context.
  Manager's override machinery generalized to KINDS (`OverrideKind`: reconcile with
  kind-change stop, config-diff, spawn, watchdog respawn, adoption, status rows,
  ports, serving probe — one seam, OLS drops in later). UI pickers offer
  "Apache (.htaccess)". ✓ **Live-verified** (`examples/apache_site_check`, Jul 15):
  4 bottles → merged tree, httpd + dylibs + modules loads-clean; `httpd -t` Syntax
  OK; served a probe site on :8329 against a throwaway fpm pool — PHP-via-fpm ✓,
  SetEnv env var per-request ✓, pretty-URL front-controller fallback ✓, css mime
  from the bundled map ✓, `.htaccess` RewriteRule 302 ✓. 258 lib tests, clippy,
  examples, tsc. **Human verify next:** create/switch a site to Apache in-app,
  site over HTTPS, plugin `.htaccess` rules.
- [x] **Per-engine DB version switch** — the family's last item. Offered sets:
  MySQL 8.4.6/8.0.44 · PostgreSQL 18.4.0/17.10.0/16.14.0 · MariaDB 12.3.2/11.4.12 LTS
  (Redis single — picker hides). **Per-SERIES datadirs** are the core design: never
  an in-place upgrade/downgrade (PG major datadirs are mutually incompatible;
  MySQL/MariaDB downgrades unsupported) — the default series keeps the legacy
  `<engine>/data` path (existing data never moves), other series live under
  `<engine>/<series>/data`; a selection orphaned by a pin bump falls back to the
  default, its datadir left intact. Selection = `db_version_<engine>` settings KV
  (validated in core, no migration); manager mirrors it (watchdog respawns on the
  SELECTED version); every site DB op resolves the effective version's client bins;
  Start-all prefetches the selected versions. Databases-row picker with an honest
  confirm (per-version data dirs named; running engine restarts). ✓ 261 lib tests
  (+ series/datadir/effective-version), clippy, examples, tsc. ✓ **Live-verified**
  (`examples/db_version_switch_check`, Jul 15): PG 17 → fresh `postgres/17/data`,
  marker DB created → switch to 16 → own fresh datadir, marker NOT visible
  (isolation) → back to 17 → marker still there (data survives the round-trip);
  MariaDB 11.4.12 + MySQL 8.0.44 resolved into the real cache and RUN (`--version`;
  their ports were serving the live stack). **Human verify:** switch a version from
  the Databases row in-app.

FrankenPHP + PostgreSQL prove the override/engine patterns; Redis proves the BUNDLE
pattern. **Adding a bundled DB engine:** mirror `core/redis.rs` (or `core/postgres.rs`
for init-style engines) — pin the bottles in `bundle_manifest` (`core/binaries.rs`),
fill the engine's arm in `core/db.rs`, wire `plan_for_engine`/`resolve_any`
(`core/downloads.rs`), update `docs/PORTS.md`. **Adding an override server:** mirror
`core/frankenphp.rs` (loopback backend, never the edge).

## Phase 4+ (next era)

- [ ] Windows platform impls — fill the `todo!()` stubs in `platform/windows/mod.rs`
  (trait-by-trait; architecture requires no restructuring)
- [ ] Linux platform impls — same, `platform/linux/mod.rs`
- [ ] Packaging polish: Tauri updater (Release 6.1, optional), public distribution
