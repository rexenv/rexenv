# BACKLOG — Post-Release (from the read-only audit)

> Findings from **FINDINGS.md** deferred past the pre-release .dmg. The pre-release
> subset lives in **TASKS-FIXES.md**. Full detail (file:line, why-it-matters, fix) is
> in FINDINGS.md under the same IDs — this file is the tracking list.
>
> ⚠️ **Two items are flagged for a pre-release decision** — they may jump into
> TASKS-FIXES.md §2 before that work starts: **M1** and **M5** (see below).

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[D]` deferred · `[→§2]` pulled into pre-release

---

## Pre-release decisions (resolved)

- [x] **M1 — rexenv can kill a user's *own* Caddy** (`core/proxy.rs`). **DONE** as
  TASKS-FIXES.md §2.4 (committed), resolved by the H5 socket move: rexenv's admin left
  TCP `:2019` for a private unix socket, so recover/stop only ever act on our own edge
  (our socket + our-binary reap marker); a developer's Caddy on `:2019` is never touched.

- [x] **M5 — FrankenPHP override ignores subdirectory-multisite rewrites** — DONE (backend
  fix + live-verified; guard removed). `reconcile_overrides` now threads `sites::rewrite_mode_for(s.multisite)` into
  `frankenphp::write_config` (was hardcoded `RewriteMode::Single`). The FrankenPHP
  `SubdirectoryMultisite` Caddy template — never exercised before, and buggy: a placeholder
  typo (`{http.regexp.wpsubph.2}` for a regexp named `wpsubphp` → empty rewrite) plus a
  missing `wp-admin` redirect — was rewritten to faithfully mirror the proven nginx rules
  (redirect → strip `/wp-*` → strip `*.php`, each `not file`-guarded, ordered in a `route {}`
  block). Validated with `frankenphp adapt`/`validate` (`Valid configuration`) + a unit test
  + `examples/frankenphp_subdir_validate.rs`. **Verified live** (M5-verify): a real
  subdirectory-multisite site switched to FrankenPHP routed a sub-site page + its `/wp-admin`
  redirect correctly, so the §2.6 UI guard has been removed.

- [x] **M5-verify — live-test FrankenPHP subdirectory multisite, then remove the §2.6 guard.**
  ✓ done. Verified live on-device: a subdirectory-multisite WP site switched to FrankenPHP
  served a sub-site page **and** its `/wp-admin` redirect correctly through the override
  backend. Removed the 2.6 UI guard — the `FRANKENPHP_SUBDIR_BLOCK` consts + `blocked`
  option logic in `SiteDetail.tsx` and `NewSiteDialog.tsx`, the Subdirectory card's
  `disabled`/`disabledNote`, and the New-Site invariant `useEffect`. Verified: `pnpm build`
  (strict tsc) clean, no residual guard refs.

---

## Medium

- [x] **M3 — `wait_until` results discarded** (`service_manager.rs`). ✓ done. Added
  `wait_until_ready(service, log, tries, cond) -> Result<()>` (timeout → hard error
  naming the service + its `<key>-stdout.log`) + a `stdout_log` helper; the three
  discarded call sites (`ensure_db`, `ensure_mailpit`, `reconcile_overrides`) now
  `?`-propagate. A service that never starts fails there with an actionable message
  instead of a silent `Ok`. Verified: new unit test (Ok + error-names-service+log
  paths) + 120/120 lib tests. NB: a FrankenPHP override that won't come up now aborts
  the reconcile (was silently skipped); its child is already tracked so `stop_all`
  still reaps it. (Left M4's blocking `std::thread::sleep` untouched — separate item.)

- [x] **M4 — Blocking sleeps hold the services lock** (`service_manager.rs`). ✓ done.
  `wait_until`/`wait_until_ready` are now `async` and the 500ms poll uses
  `tokio::time::sleep(...).await` instead of `std::thread::sleep`, so a slow/never-ready
  service no longer parks a tokio worker (up to 15s) — the worker yields to other tasks
  during the wait. The three call sites (`ensure_db`/`ensure_mailpit`/`reconcile_overrides`)
  `.await` it; the readiness test moved to `#[tokio::test]`. Verified: 120/120 lib tests.

- [x] **M4-residual — services lock held across the readiness waits.** ✓ done. The spawn
  phase is now split from the wait phase (same shape as the existing `prepare_edge`
  two-phase pattern): `spawn_db`/`spawn_mailpit`/`reconcile_overrides` start children
  under the lock (fast) and return `ReadyCheck` probes; `start_core`/`reload` bubble
  them up, and the COMMANDS `await_ready(checks)` after dropping the services lock —
  concurrently (worst case = slowest probe, not the sum), aggregating every failure
  into one M3-style "named service + log" error. `create_site` additionally stops
  holding the manager across the (long) WordPress install; `start_services` orders
  edge start AFTER readiness so the edge never routes to still-starting backends.
  `ensure_db` remains as spawn+await convenience (examples/tests). Verified: new
  `await_ready_is_concurrent_and_names_every_failure` unit test (empty/ok batches,
  both failures named + log hint kept, 4×500ms probes finish in ~1 probe's time) +
  134/134 lib tests, clippy 0 (lib+examples), all examples compile. **Live-verified**
  (`examples/ready_split_check.rs`, real PostgreSQL): spawn tracks the child +
  returns the check, re-spawn is a no-op, `await_ready` drives it to
  `engine.running()`, `stop_db` cleans up.

- [x] **M6 — Monitor refreshes all processes per service per poll**
  (`monitor.rs` + `commands/services.rs`, `commands/database.rs`). ✓ done. Split the
  refresh from the read: new `Monitor::refresh_processes(&mut self)` does the single
  `refresh_processes(All, true)` sweep; `process(pid)` is now read-only (`&self`). The
  `services_status` / `databases_status` commands call `refresh_processes()` once after
  locking, before the per-service map — so a poll does 1 sweep, not N. Bonus: CPU% now
  spans the full poll interval instead of the ~0 gap between the old back-to-back sweeps.
  Verified: 120/120 lib tests (incl. the updated `process_metrics_for_self`).

- [x] **M7 — No backend domain validation** (`core/sites.rs`). ✓ done. Added
  `validate_domain(&str)` called at the top of both `create` (persistence gate) and
  `provision` (before any docroot/cert/DB use). Enforces a strict `.test` hostname:
  ≤253 chars, ends in `.test`, ≥1 label before it, each label 1–63 chars of `[a-z0-9-]`,
  no leading/trailing hyphen. Blocks path traversal (`../`, `/`), config injection
  (space, `;`, `{`, newline, quotes), wildcards, and uppercase — independent of the UI
  slugging. Verified: 3 new tests (5 accepted shapes, 15 rejected inputs, create-persists-
  nothing on reject) + 123/123 lib tests.

- [x] **Per-site serving granularity** (follow-up to TASKS-FIXES §2.1 / H1). ✓ done.
  Added `service_manager::site_serving(sites, &service_infos)` → `Vec<SiteServing{domain,
  serving}>`: a site is *serving* only when the edge is up AND its own upstream is up — its
  FrankenPHP backend port (override sites) or nginx + the php-fpm pool its version routes to
  (nginx sites, via the extracted `sites::pool_port_for`). Matched off the existing
  non-blocking `service_infos()` snapshot (edge/nginx by stable singleton name, per-site
  upstreams by the same ports the config generator emits — no drift). New `sites_serving`
  command; the frontend (`Sites.tsx` rows/count/filters/sort + `SiteDetail.tsx` header)
  overlays it by domain instead of the blanket `stackRunning`, so a partial stack shows
  honest per-site status. "Present in the generated config" is approximated as persisted ⇒
  in config (configs are regenerated from the site list on every change). Verified: new
  `site_serving_reflects_each_sites_own_upstream` test (full / edge-down / fp-backend-down /
  pool-down / nginx-down) + 133/133 lib tests, 0 clippy errors, `pnpm build` (strict tsc).

---

## Low

- [x] **L1 — `managed_ports()` hardcodes PHP minors** (`service_manager.rs`). ✓ done.
  The orphan-reap sweep now loops `php::all_minors()` (derived from `binaries::PHP_VERSIONS`)
  instead of a hardcoded `["8.1","8.2","8.3"]`, so a future 8.4 pool's port is swept too.
  Verified: new `managed_ports_cover_every_pinned_php_minor` test + 126/126 lib tests.
- [x] **L2 — `extract_tar_gz_tree` has no traversal/symlink guard** (`binaries.rs`). ✓ done.
  We compute the output path ourselves (to strip the top dir), bypassing the `tar` crate's
  guards — re-added them: `safe_join` rejects entry paths with `..`/absolute/prefix
  components, and `link_stays_within` lexically resolves symlink/hardlink targets and
  rejects any that are absolute or climb above `dest`. Allows the 22 legit in-tree `../lib/…`
  dylib symlinks in the real MySQL tree (verified: 0 absolute targets, all `..` stay in-tree).
  Defense-in-depth (archives are checksum-pinned). Verified: 2 new unit tests (real MySQL
  cases accepted, traversal/escape/absolute rejected) + 125/125 lib tests.
- [x] **L3 — `ProcessSupervisor::stop` is SIGTERM-only** (`platform/macos/mod.rs`). ✓ done.
  `stop` now SIGTERMs, polls a ~3s grace (returning as soon as the process exits), then
  SIGKILLs if it's still running — so a signal-ignoring process can't block a caller's
  `wait()` forever. Extracted a testable `stop_pid(pid, grace, interval)` + a zombie-aware
  `process_running` (`ps -o state=`; a clean exit zombies until the caller reaps, and `Z`
  counts as not-running so `stop` returns immediately). Verified: 3 tests (live/gone probe,
  SIGTERM stop, SIGKILL fallback for a TERM-trapping process) + 129/129 lib tests.
- [x] **L4 — `MacosShell::run` ignores exit status + stderr** (`platform/macos/mod.rs`). ✓ done.
  `run` now errors on a non-zero exit, carrying the command + exit status + trimmed stderr
  (was `Ok(stdout)` regardless, so a failure looked like an empty-output success). Made the
  `ShellRunner::run` trait doc state the contract explicitly. Verified: 2 new tests (stdout on
  success; error carries stderr + status on failure) + 132/132 lib tests. Was latent (zero
  callers) — fixed before anything wires it.
- [x] **L5 — `dns_status` opens a raw UDP socket in the command layer**
  (`commands/system.rs`). ✓ done. Moved the UDP bind-in-use probe into
  `core::dns::port_bound(port)` (documented as a liveness proxy, distinct from the
  handle-based `DnsService::is_running`); the command now delegates to it and no longer
  opens a raw socket. Same behavior, thin command. Verified: new `port_bound` test
  (held → bound, released → free) + 130/130 lib tests.
- [x] **L6 — Auto-update Settings card is dead UI** (`Settings.tsx`). ✓ done. Removed the
  entire fake Updates section — the `UpdatesSetting` component (no-op check-for-updates
  spinner, mock update banner, non-functional auto-update toggle), its `"updates"` nav tab
  + sidebar update-dot, the render branch, and the now-unused mock constants
  (`UPDATE_READY`/`NEXT_VERSION`) + imports (`ArrowUp`/`RefreshCw`). The app version still
  shows in About, so nothing real is lost. Re-add when the Tauri updater lands (release §6.1).
  Verified: no dangling refs + `pnpm build` passes (strict tsc).

---

## Notes

- These are recorded, not scheduled — pull into a phase deliberately. FINDINGS.md holds
  the full rationale and suggested fix for each.
- The audit's "what's solid" section (FINDINGS.md) lists correct patterns to preserve
  while doing this work — notably the `global_status`/`services_status` shared-source
  fix, the `wp_login` mu-plugin hardening, parameterized SQL, and the checksum-before-write
  download path.
