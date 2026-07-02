# rexenv — Read-Only Code Review (Findings of Record)

> Structured, read-only audit of the rexenv codebase (macOS-first Tauri 2 / Rust +
> React/TS local dev environment) at the "limited release" stage. Judged against
> CLAUDE.md, PROJECT_SPEC.md, and the TASKS files — against the intended
> architecture, not generic ideals. **No code was changed to produce this report.**
>
> This file is the permanent record of the audit. The actionable pre-release subset
> lives in **TASKS-FIXES.md**; deferred items in **BACKLOG.md**. Finding IDs
> (C1, H1…, M1…, L1…) are stable and cross-referenced from those files.

---

## Summary

The **architecture is genuinely well executed** — the commands → core → platform
layering holds, `core/` contains no OS-specific code, everything OS-specific is
behind traits (Windows/Linux are clean `todo!()` stubs), and unit/example test
coverage on pure functions is strong. The problems are not structural. They cluster
in two places:

1. **Wiring gaps** between the (correct) backend logic and the UI — features that
   look done but aren't connected.
2. A pervasive **"port is listening = my service is running" / "DB flag = reality"**
   conflation — the source-of-truth class specifically hunted for.

### Top 5 to fix before sharing the .dmg
1. **C1** — First-run system setup is never actually run → `.test` never resolves on a clean Mac. Release blocker.
2. **H1** — Per-site Start/Stop is fiction (flips a DB column; all sites are served regardless).
3. **H2** — Service "running" = "something is on the port," conflated with process ownership.
4. **H3** — Startup DB/CA failure leaves the app a silent black hole (every command panics).
5. **M2** — Broken buttons ship visible (Mail delete/mark-read call unregistered commands; Services per-row toggle is a no-op).

---

## Coverage

- **Backend (Rust): ~100%.** Every source file read in full — all of `core/`, all
  of `commands/`, `state/` (db, models, store, app), `platform/traits.rs`, the full
  macOS impl, both Windows/Linux stubs, `lib.rs`, `error.rs`.
- **Frontend: ~70%.** Read in full: the IPC bridge (`lib/ipc/index.ts`), `types/index.ts`,
  `App.tsx`, `AppShell.tsx`, `Onboarding.tsx`, `Sites.tsx`, `SiteDetail.tsx`,
  `Services.tsx`, `Mail.tsx` (top half), `StatusFooter.tsx`. Read via targeted grep:
  `Settings.tsx`, `NewSiteDialog.tsx`.
  **Blind spots:** `WordPressManager.tsx` (1051 lines — only its IPC call sites seen),
  `NewSiteDialog.tsx` (partial), `Databases.tsx`, `Tunnels.tsx`, and small UI primitives.
  Given the review's focus (architecture, state-truth, risky subsystems, security),
  the backend is where the risk lives and it's fully covered.
- **Not run:** `cargo build` / `cargo test` (read-only review); no live/browser testing.

---

## CRITICAL

### C1 — System setup (resolver + CA trust) is never wired to the app
- **Where:** `src/routes/Onboarding.tsx:226-231`; also `src-tauri/src/core/setup.rs:26`
  (0 non-example callers), `src-tauri/src/lib.rs:71-150` (no command registered),
  `src/App.tsx:17-31` (no first-run redirect).
- **Severity:** Critical (release blocker).
- **Why it matters:** The correct implementation exists — `core::setup::run_system_setup`
  installs `/etc/resolver/test` via one admin prompt, then trusts the local CA. But
  nothing invokes it. The Onboarding "Domains & SSL" step is
  `setTimeout(() => setState("done"), 1800)` with a literal `// TODO: wire to run_system_setup`.
  There is no `system_setup`/`configure_resolver` command in `lib.rs`, no IPC wrapper,
  and `/onboarding` is never navigated to (index and `*` both redirect to `/sites`).
  `start_services` starts the root Caddy on :443 but does **not** install the resolver
  or trust the CA. Without `/etc/resolver/test`, macOS sends `*.test` to public DNS →
  NXDOMAIN, so on a clean machine the product's entire premise (`https://mysite.test`)
  is unreachable through the shipping UI. It works on the dev Mac only because an
  example binary (`examples/system_setup.rs` / `wp_real443_setup.rs`) installed the
  resolver once. Directly corroborated by TASKS-RELEASE task 1.5 (cold run on a second
  Mac) being unchecked.
- **Suggested fix:** Add a `system_setup` command that calls `core::setup::run_system_setup`;
  add a typed IPC wrapper; wire the Onboarding button to it with real busy/done/error
  states; and gate first launch — if `dns_status().resolverInstalled` is false, route
  to `/onboarding`. Add a re-run/repair affordance in Settings.

---

## HIGH

### H1 — Per-site status is decoupled from whether the site is served
- **Where:** `src-tauri/src/commands/sites.rs:29-40` (`start_site`/`stop_site` → `set_status` only);
  `src-tauri/src/core/sites.rs:305-346` (`rebuild_configs_for` serves all sites, no status filter);
  `src/routes/Sites.tsx:273` (header/tab counts read `site.status`).
- **Severity:** High.
- **Why it matters:** `start_site`/`stop_site` write only `sites.status`. A site is
  actually served purely by the shared nginx/Caddy config, which includes **every**
  site whenever the stack runs. So a "stopped" site is still fully reachable over
  HTTPS, a "running" site is dead when the stack is stopped, and nothing reconciles
  the column with reality. The Sites header "N running" and the All/Running/Stopped
  tabs count this fictional column. This is the same footer-vs-Services divergence
  already found, now on the primary screen — users toggle a site "off," see it still
  load, and lose trust in every status indicator.
- **Suggested fix:** Decide whether per-site enable/disable is a real feature. If yes,
  filter `rebuild_configs_for` by an "enabled" flag and reload the edge on toggle
  (like the PHP/server switches already do). If no, remove the per-site toggle and
  derive the badge from actual serving (stack up + site in config). Either way the
  badge must reflect reality.

### H2 — "Running" is a bare port-listen probe, conflated with process ownership
- **Where:** `src-tauri/src/core/service_manager.rs:548-567` (Caddy: `ports::is_listening(https)`;
  Nginx: `nginx_running`; Mailpit: `mail::running`), `src-tauri/src/core/db.rs:126-128`,
  `src-tauri/src/core/database.rs:105`, `src-tauri/src/core/frankenphp.rs:123`.
- **Severity:** High.
- **Why it matters:** Every service's `running` is "can I open a TCP connection to the
  port," never "is *my* tracked child alive and the one bound there." The pid is tracked
  but unused for the liveness decision. The dev machine's DBngin holds :443 (nginx) and
  :3306 — rexenv reports **Caddy running** whenever DBngin's nginx is up, even with
  rexenv fully stopped; a system MySQL makes MySQL show green. `stop_all` then can't
  stop these (not owned), so the UI shows "running" that Stop-all can't clear. False
  status + unactionable Stop.
- **Suggested fix:** Compute `running` from tracked-child liveness (`try_wait`) **and**
  the port, or from `owned_listeners(port, app_data_marker)` (the primitive already
  exists) so status reflects a rexenv-owned process. For the root/privileged Caddy,
  probe its admin API identity rather than raw :443.

### H3 — Backend startup failure leaves the app unusable with no message
- **Where:** `src-tauri/src/lib.rs:47-68`.
- **Severity:** High.
- **Why it matters:** On `open_for_platform` or `ssl::load_or_create` error, the code
  logs and returns `Ok(())` without `app.manage(AppState)`. Every command taking
  `State<'_, AppState>` then panics ("state not managed") on invocation. Clean-machine
  failure modes — unwritable app-data, disk full, CA keygen/permission failure — turn
  the whole app into cryptic per-action panics with nothing shown to the user. §2
  robustness explicitly wants "a clear error, never a crash/silent no-op."
- **Suggested fix:** Manage a fallible state (`AppState` holding `Result`/`Option`,
  commands returning a clean `Error`), or show a blocking error window and refuse to
  proceed. At minimum surface the failure to the UI.

### H4 — Poisoned binary cache on a failed `prepare_binary`
- **Where:** `src-tauri/src/core/binaries.rs:426-455` (`resolve`), same shape in
  `resolve_dir:486-514`.
- **Severity:** High.
- **Why it matters:** `resolve` extracts to the final `bin_path`, *then* runs
  `set_executable` + `prepare_binary` (relink Homebrew dylibs + ad-hoc codesign). If
  `prepare_binary` fails (e.g. `relink_to_system_libs` hits an unknown dylib and errors,
  or codesign fails), the extracted-but-unsigned binary is already on disk. The next
  `resolve` hits `if bin_path.exists() { return Ok }` at line 428 and hands back the
  **unprepared** binary forever — Apple Silicon SIGKILLs an unsigned Mach-O, or it
  can't find its dylibs, with an opaque failure that never self-heals short of a manual
  cache wipe.
- **Suggested fix:** Extract to a temp path, run set_executable + prepare, then
  atomically rename into place; or wrap the post-download steps and `remove_dir_all`
  the target on any failure so a retry re-downloads cleanly. Apply to both `resolve`
  and `resolve_dir`.

### H5 — Root Caddy edge exposes an unauthenticated admin API
- **Where:** `src-tauri/src/core/proxy.rs:125-133` (edge started as root via
  `PrivilegeManager`), admin left on at `:2019` (`CADDY_ADMIN_PORT`).
- **Severity:** High (local privilege escalation; local-access precondition).
- **Why it matters:** The edge runs as root and keeps Caddy's admin endpoint enabled
  (the code relies on it for `reload`/`stop`). Caddy's admin API has no auth — any local
  process can POST a new config to a **root** Caddy, and Caddy config can read/write
  files and set roots arbitrarily → local user→root escalation. `core/frankenphp.rs`
  correctly sets `admin off` on its backends; only the privileged edge leaves it open.
  The Caddyfile is also written under user-writable app-data and executed by root.
  (macOS still requires root to bind :443, so dropping root isn't the fix — locking
  the admin channel is.)
- **Suggested fix:** Bind Caddy's admin to a unix socket with 0600 perms
  (`admin unix//…/caddy-admin.sock` in the global block) instead of TCP :2019, and
  drive reload/stop/stale-edge recovery over that socket; or disable admin and reload
  via config-path + signal. Keep the Caddyfile in a root-only-writable location if
  feasible.

---

## MEDIUM

### M1 — rexenv can kill a user's *own* Caddy
- **Where:** `src-tauri/src/core/proxy.rs:172-194` (`recover_stale_edge`) and `:200-236`
  (`stop_edge`), invoked by `service_manager::reconcile_startup` on **every app launch**
  and by `stop_all`.
- **Severity:** Medium (flagged for a pre-release decision — see BACKLOG.md).
- **Why it matters:** `caddy stop` POSTs to `:2019` without verifying the listener is
  rexenv's process. A developer running their own Caddy (another project) on the default
  admin port gets it stopped by rexenv on launch/stop-all. The `owned_pids` reap is
  correctly guarded by the binary-path marker, but the admin-API stop is not.
- **Suggested fix:** Before `stop_admin`, confirm ownership — check
  `owned_listeners(2019, app_data_marker)` is non-empty, or query the admin API's config
  for a rexenv marker — and skip if it's a foreign Caddy.

### M2 — Half-wired features presented as working
- **Where / Severity:** Medium.
  - `src/lib/ipc/index.ts:221-236` — `mailpitDelete`/`mailpitMarkAllRead` invoke
    `mailpit_delete`/`mailpit_mark_all_read`, **neither registered** in `lib.rs`.
    `src/routes/Mail.tsx:74,81` wires them to the per-message trash button and
    "mark all read" → runtime "command not found," shown via `toast.error`.
    (`mailpit_clear` *is* registered, so "Clear all" works.)
  - `src/routes/Services.tsx:293` — `const onServiceToggle = () => {}`. Every service
    row renders a Start/Stop toggle that does nothing (TODO notes no
    `start_service`/`stop_service` exists).
  - `src-tauri/src/commands/services.rs:9-19` — the `ServiceStatus` DTO omits
    `kind`/`version`/`isDefault`/`isRouter`, but `src/types/index.ts:85-90` and
    `Services.tsx` read them. So the PHP "default" badge, the "edge router" badge, and
    per-row version never render (version shows "—").
- **Suggested fix:** For Mail — add both commands (delete → `DELETE /api/v1/message/{id}`;
  mark-read → Mailpit's read API) or hide the buttons. For the per-row toggle — implement
  per-service control or remove it, keeping Start-all/Stop-all. For the DTO — populate
  `kind`/`version`/`isDefault`/`isRouter` (the manager knows the default minor, the
  router, and pool versions) or remove those UI affordances.

### M3 — `wait_until` results discarded — startup failures become confusing later failures
- **Where:** `src-tauri/src/core/service_manager.rs:126` (`ensure_db`), `:309`
  (`ensure_mailpit`), `:374` (`reconcile_overrides`).
- **Severity:** Medium.
- **Why it matters:** Each calls `wait_until(cond, N)` and ignores the returned bool.
  If MySQL/Mailpit/FrankenPHP never comes up in the window, the method still returns
  `Ok(())` and keeps the child handle, so the WordPress install or edge reload fails
  downstream with a misleading error instead of "MySQL failed to start — see log."
- **Suggested fix:** Treat a false return as an error naming the service, with a
  log-path hint.

### M4 — Blocking sleeps inside async paths hold the services lock
- **Where:** `src-tauri/src/core/service_manager.rs:596-604` (`wait_until` uses
  `std::thread::sleep`), called from `async fn ensure_db`/`ensure_mailpit`/`reconcile_overrides`
  while `services` is locked (up to 15s DB, 10s Mailpit, 10s per FrankenPHP backend).
- **Severity:** Medium.
- **Why it matters:** Parks a tokio worker thread and holds the async mutex the whole
  time. Status polls are shielded by the `try_lock` cache (good), but other
  service-mutating commands queue behind it.
- **Suggested fix:** Use `tokio::time::sleep` in the async callers (make the wait async),
  or run the blocking waits via `spawn_blocking`.

### M5 — FrankenPHP override ignores multisite rewrite mode
- **Where:** `src-tauri/src/core/service_manager.rs:371` — `reconcile_overrides`
  hardcodes `services::RewriteMode::Single` when writing every FrankenPHP backend
  config, though `frankenphp::site_body` supports the subdirectory-multisite template.
  The UI (`src/routes/SiteDetail.tsx:48-51`) lets any site — including a
  subdirectory-multisite network — switch to FrankenPHP.
- **Severity:** Medium (flagged for a pre-release decision — see BACKLOG.md).
- **Why it matters:** A subdirectory-multisite site switched to FrankenPHP loses its
  network path rewrites → sub-site routing breaks.
- **Suggested fix:** Pass the site's real `rewrite_mode_for(s.multisite)` into
  `write_config`, mirroring the nginx path.

### M6 — Resource monitor refreshes all processes once per service per poll
- **Where:** `src-tauri/src/core/monitor.rs:50-57` + `src-tauri/src/commands/services.rs:78-91`
  / `src-tauri/src/commands/database.rs:46-61`.
- **Severity:** Medium (performance).
- **Why it matters:** `monitor.process(pid)` calls `refresh_processes(ProcessesToUpdate::All, true)`
  on every call, inside a `.map` over every service. Each 2s Services poll runs a full
  all-process refresh N times (N services); the footer and Databases view do it too.
- **Suggested fix:** Refresh once per poll (or once for the exact pid set via
  `ProcessesToUpdate::Some`), then read each pid. Add a `sample_processes(&[pid])` on `Monitor`.

### M7 — Backend performs no domain validation (config injection / path traversal at the trust boundary)
- **Where:** `src-tauri/src/core/sites.rs:19-41` (`create`) and `:197-227` (`provision`).
- **Severity:** Medium (defense-in-depth).
- **Why it matters:** The domain is trusted as-is. It flows into
  `docroot = sites_dir.join(&domain)` (a `../` escapes the sites dir), into nginx
  `server_name` and the Caddy host block (a space or `{` injects directives), the cert
  SAN, and the DB name. The UI slugs the domain safely (`NewSiteDialog.tsx:129`), so
  this isn't reachable through normal use — but `core` is the real trust boundary and
  does no validation.
- **Suggested fix:** Validate the domain in `create`/`provision` against a strict pattern
  (e.g. `^[a-z0-9]([a-z0-9-]*\.)+test$`) before it touches the filesystem or generated
  configs.

---

## LOW

### L1 — `managed_ports()` hardcodes PHP minors
- **Where:** `src-tauri/src/core/service_manager.rs:451` — `["8.1","8.2","8.3"]` instead
  of `php::all_minors()`.
- **Severity:** Low. A future 8.4 pool would escape the orphan sweep.
- **Suggested fix:** Use the shared `php::all_minors()` list.

### L2 — `extract_tar_gz_tree` has no traversal/symlink guard
- **Where:** `src-tauri/src/core/binaries.rs:666-690` — `components().skip(1)` keeps `..`;
  `entry.unpack` follows symlinks.
- **Severity:** Low (gated by pinned checksums — only exploitable via a malicious pinned
  artifact; defense-in-depth).
- **Suggested fix:** Reject entries whose normalized path escapes `dest`.

### L3 — `ProcessSupervisor::stop` is SIGTERM-only, no SIGKILL escalation
- **Where:** `src-tauri/src/platform/macos/mod.rs:209-219`.
- **Severity:** Low. Callers then `child.wait()`, which could block on a process that
  ignores TERM. In practice mysqld/nginx/php-fpm handle TERM.
- **Suggested fix:** Add a timed SIGKILL fallback.

### L4 — `MacosShell::run` ignores exit status + stderr
- **Where:** `src-tauri/src/platform/macos/mod.rs:361-364` — returns stdout regardless
  of exit code/stderr.
- **Severity:** Low. **Zero callers today** (only `open()` is used), so latent.
- **Suggested fix:** Fix the silent-failure behavior before anything wires it.

### L5 — `dns_status` opens a raw UDP socket in the command layer
- **Where:** `src-tauri/src/commands/system.rs:137-148`.
- **Severity:** Low (layering smell — a probe, not OS-divergent logic).
- **Suggested fix:** Move the probe behind a platform/core helper.

### L6 — Auto-update Settings card is dead UI
- **Where:** `src/routes/Settings.tsx:724-747` ("Install & restart" / "Install updates
  automatically") — controls for the deferred updater (release §6.1).
- **Severity:** Low. If non-functional, it implies capability that isn't there.
- **Suggested fix:** Hide until the updater is wired.

---

## What's solid (do not relitigate)

- **Layering** is faithfully thin-commands → platform-agnostic-core → trait-based-platform;
  no OS code leaked into `core/`; Windows/Linux are clean `todo!()` stubs as intended.
- **Security done right in several risky spots:** the "Log in as" mu-plugin
  (`core/wp_login.rs`) is genuinely careful — SHA-256-hashed single-use token, short
  TTL, `hash_equals`, loopback + `.test`-host + CF-header gating; WP-CLI is invoked via
  `Command` args (no shell → no shell injection); SQL is fully parameterized
  (`state/store.rs`); the log tailer validates keys against traversal; tunnels are
  scoped to one Host and Adminer is deliberately not a site so it can't be tunneled.
- **Download path** verifies checksum on in-memory bytes before writing, with sane
  retry/permanent-error classification and connectivity-aware messages (§2.2/2.4 hold).
- **The `global_status`/`services_status` shared-source fix is correct** — both read
  `AppState::service_infos`, and `summarize` is unit-pinned. (The remaining divergence
  is Sites' `site.status` — H1 — a *different* source that was never reconciled.)
