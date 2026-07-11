# TODO — the single active-work file

Everything open lives here. Completed phases + audit history: `docs/archive/`
(historical — don't trust as current). When you finish an item, tick it here with a
one-line ✓ evidence note (same convention as the archived TASKS files).

## Actionable now

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
- [ ] **Isolate live-check examples from the real stack.** `examples/*.rs` use
  `platform::current()` → the REAL app-data dir: their `start_all`/`stop_all`/
  `recover_stale_edge` stop the USER'S running edge over the shared admin socket (and
  restart shared services). Adopt-don't-kill removed the worst path, but an example's
  explicit `stop_all` still tears the stack down. Options: env-var app-data override for
  example runs, or a guard that refuses `stop_all` when the edge wasn't started by the
  example.
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
  pre-migration site); `change_site_domain` orchestrates preflight (validate +
  uniqueness + multisite REFUSED honestly) → mandatory Downloads backup (abort on
  fail) → new-domain cert (additive) → `wp search-replace` dry-run gate then two real
  passes (`https://old→https://new`, bare `old→new`, `--all-tables`) → SQLite domain
  flip (docroot + db_name untouched by design) → config regen + forced edge reload →
  best-effort old-artifact cleanup (cert dir, tunnel, FrankenPHP config/logs). DNS
  needs nothing (wildcard `*.test`). UI: Settings → Domain card (disabled on
  multisite), destructive dialog with backup/email-rewrite/reversal notes; frontend
  `siteDbName()` derivation deleted in favor of stored `site.dbName`.
- **Move site / custom docroot** — fixed `sites_dir` scheme + nginx root regen (M/L).
- **Per-site env vars** — conflicts with per-VERSION shared php-fpm pools; no clean
  seam (L).

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output, not a bug — intentionally not filtered.

## Blocked on external work

- [ ] **Xdebug per-site toggle** (Phase 3 §8.2) — blocked on §11.2: no hosted
  Xdebug-enabled static-php build exists. Recipe + wiring done (`docs/xdebug-debug-build.md`,
  `core/binaries.rs` `php-debug` variant, checksums intentionally empty). Needs: maintainer
  build + host + checksum pin.
- [ ] **SMAppService privileged helper** (Phase 1 §10.4) — single-prompt system setup.
  Needs a signed + notarized bundle → packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** (Release 1.6) — needs paid Apple account.

## Deferred services (need a macOS dylib-tree-bundling step; none has a clean portable binary)

- [ ] Apache (httpd) override server — links non-system dylibs (apr, openssl@3)
- [ ] OpenLiteSpeed override server
- [ ] MariaDB engine (+ site→engine selection at create) — ships no portable macOS binary
- [ ] Redis engine — needs dylib bundle
- [ ] Per-engine DB version switch (multi-version DBs)

FrankenPHP + PostgreSQL prove the override/engine patterns.
**Adding a DB engine** (once a portable binary exists): mirror `core/postgres.rs`
(the template — `TarGzTree` dir binary, TCP-only on its `core/db.rs` port, init/start/
stop/running), fill that engine's stubbed arm in `core/db.rs`, pin the binary in
`core/binaries.rs`, register the port in `core/ports.rs` `default_ports()`, and update
`docs/PORTS.md`. **Adding an override server:** mirror `core/frankenphp.rs` (loopback
backend, never the edge).

## Phase 4+ (next era)

- [ ] Windows platform impls — fill the `todo!()` stubs in `platform/windows/mod.rs`
  (trait-by-trait; architecture requires no restructuring)
- [ ] Linux platform impls — same, `platform/linux/mod.rs`
- [ ] Packaging polish: Tauri updater (Release 6.1, optional), public distribution
