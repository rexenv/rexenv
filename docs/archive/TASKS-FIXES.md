> ⚠️ **HISTORICAL** — reflects plans/state at the time of writing, NOT current code.
> Current truth = `docs/ARCHITECTURE.md` + `docs/TODO.md`. Do not trust any "fact" here
> without checking the code.

# TASKS — Pre-Release Fixes (from the read-only audit)

> The actionable subset of **FINDINGS.md** that must land before the .dmg goes to
> trusted testers. Two tiers, in agreed priority order: **§1 Release-blockers** (the
> app is broken/opaque without them) and **§2 Correctness + security** (ship-quality
> before sharing). Everything else is in **BACKLOG.md**.
>
> Same discipline as the other TASKS files: work top-to-bottom, one task at a time,
> keep changes surgical, and check the box only when "Done when" passes — prefer a
> real check (a cold run, `codesign`/`security`/`curl` output, a live UI click) over
> "looks right". Finding IDs (C1, H1…) cross-reference FINDINGS.md.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[D]` deferred

---

## 1. Release-blockers

> Without these, a fresh install either doesn't work at all (C1) or fails opaquely
> (H3), or ships controls that visibly break (M2).

- [x] **1.1 (C1) Wire first-run system setup** — ✓ done (commit `96fc297`). Added an
  async `system_setup` command → `core::setup::run_system_setup`, registered in `lib.rs`,
  exposed as `ipc.systemSetup`; Onboarding's "Set up domains & SSL" now calls it with real
  busy/done/error + Try-again (no `setTimeout`); `FirstRunGate` routes to `/onboarding`
  when `dns_status().resolverInstalled` is false, else `/sites`. **Verified cold-start**
  on a clean account with no `/etc/resolver/test`: onboarding wrote the resolver + trusted
  the CA through the UI (no example binary), and `https://<name>.test` loaded with a valid
  local-CA lock end-to-end. `cargo check --lib` clean; `tsc` + `vite build` clean.
  *Done when:* a `system_setup` Tauri command calls `core::setup::run_system_setup`; a
  typed IPC wrapper exists; the Onboarding button calls it with real busy/done/error
  states (no `setTimeout`); first launch routes to onboarding (or the setup step) when
  `dns_status().resolverInstalled` is false; and on a machine where `/etc/resolver/test`
  does **not** exist, completing onboarding through the UI writes it and trusts the CA —
  verified end-to-end with **no example binary**: `cat /etc/resolver/test` shows the
  resolver, `security dump-trust-settings` lists the rexenv CA, and `https://<name>.test`
  loads with a valid local-CA lock. (Pairs with TASKS-RELEASE 1.5.)

- [x] **1.2 (H3) No silent startup panic on DB/CA failure** — ✓ done. On a fatal
  DB-open / CA-load failure `lib.rs` now records a human-readable reason in an
  ALWAYS-managed `InitError` (new `init_error` command) instead of leaving `AppState`
  unmanaged; `App.tsx` reads it FIRST and renders a terminal `FatalError` screen that
  drives no AppState-backed commands, so the "state not managed" panic path is never
  reached. No existing command changed. **Verified**: with the app-data folder chmod'd
  `000`, the app shows "rexenv couldn't start" (DB message), stays responsive, no panic;
  restoring perms returns to normal. `cargo check --lib` clean; `tsc` + `vite build` clean.
  *Done when:* a DB-open or CA-load failure produces a clear, human-readable error in the
  UI (blocking window / error screen) and the app stays responsive — no
  "state not managed" panics. Verified by forcing the failure (point app-data at an
  unwritable path, or corrupt the CA key file) and confirming the app shows the error
  instead of cryptic per-action failures.

- [x] **1.3 (M2) No broken visible controls** — ✓ done (chose: **remove** each broken
  affordance; kept every working one). `tsc` + `vite build` clean.
  - **(a)** ✓ Removed the Mail per-message trash button and the "Mark all read" button,
    plus their orphaned mutations and the `mailpitDelete`/`mailpitMarkAllRead` IPC wrappers
    (both targeted commands the backend never registered). "Clear all" (`mailpit_clear`),
    Open Mailpit, search, and preview tabs still work.
  - **(b)** ✓ Removed the Services per-row Start/Stop toggle (was a `() => {}` no-op) and
    its plumbing; Start-all/Stop-all remain, and the working "Set default" PHP action stays.
  - **(c)** ✓ Removed the dead "default"/"edge router" badges and the always-`"—"` version
    line (the DTO never sends `isDefault`/`isRouter`/`version`). Type fields left in place
    (read defensively) so the DTO can be populated later without a UI change.

---

## 2. Correctness + security (before sharing)

> The app runs after §1, but these are honesty/trust and safety issues a tester would
> hit or a reviewer would flag.

- [x] **2.1 (H1) Per-site status reflects actual serving** — ✓ done (fork **B**: removed
  the per-site toggle, derive the badge from real stack state). Removed the
  `start_site`/`stop_site` commands + IPC wrappers + the per-site Start/Stop toggle on
  both Sites and SiteDetail (they only flipped the fictional `sites.status`). Displayed
  status now derives from a live `global_status` query (`summary !== "stopped"` →
  running), and the Sites badge, header count, filter tabs, and sort all read that same
  truth — the same source the footer uses, so they can't disagree. `core::sites::set_status`
  kept (tested domain primitive). Partial-stack per-site granularity deferred to BACKLOG.
  **Verified**: Stop-all → every badge Stopped / "0 running" / site doesn't load; Start-all
  → every badge Running / site loads; no per-site power control remains. `cargo check --lib`
  + 13 sites tests pass; `tsc` + `vite build` clean.
  *Done when:* a site's running/stopped state reflects whether it is actually served
  (stack up + present in the generated config). **Either** per-site enable/disable is
  real — toggling filters `rebuild_configs_for` and reloads the edge, so a "stopped"
  site stops serving while a "running" one serves — **or** the per-site toggle is removed
  and the badge is derived from actual serving. The Sites header count and
  All/Running/Stopped tabs read the same truth. Verified: toggling a site changes whether
  `https://<domain>` serves, and with the stack stopped no site shows "running".

- [x] **2.2 (H2) "Running" means an owned process is alive, not a bare port-listen** —
  ✓ done. `status()` + `db_status()` now gate each service's `running` on manager-handle
  ownership AND liveness instead of a raw port probe: DB engines `self.dbs.contains_key &&
  engine.running()`; Nginx `self.nginx.is_some() && nginx_running`; Mailpit
  `self.mailpit.is_some() && mail::running()`; Caddy `self.caddy != Stopped &&
  proxy::admin_in_use()` — probed via the admin endpoint (:2019), never raw :443. (PHP
  pools / FrankenPHP were already list-only-when-tracked.) Handle presence is a stronger
  ownership signal than an `owned_listeners` marker scan; a stopped manager now
  short-circuits (zero probes). **Verified**: with DBngin holding :443, Services + footer
  show Caddy **Stopped** (was falsely Running); after freeing :443 + Start-all it's Running;
  Stop-all leaves DBngin untouched. 116/116 lib tests pass (two strengthened to pin
  "stopped ⇒ nothing running, regardless of foreign listeners").
  *Done when:* each service's `running` is true only when a rexenv-owned process is up —
  tracked-child liveness (`try_wait`) and/or `owned_listeners(port, app_data_marker)`;
  the root Caddy edge is probed via its admin identity, not raw :443. Verified: with
  rexenv stopped but DBngin holding :443/:3306 (or a system MySQL running), Services and
  the footer show Caddy/MySQL as **stopped**, and Start-all/Stop-all act only on rexenv's
  own processes.

- [x] **2.3 (H5) Lock down the root Caddy admin API** — ✓ done. rexenv drives Caddy's
  admin over a **unix socket** (`<config_dir>/caddy-admin.sock`) instead of the default
  unauthenticated TCP `:2019`: `generate_caddyfile` emits `admin "unix//<path>|0600"`;
  `start_privileged` chowns the root-created socket to the invoking user (uid from the
  app-data owner) so reload/stop stay promptless while no other process/user can reach it;
  `reload`/`stop_admin` use `--address unix//<sock>`; `admin_in_use()` (probed :2019) →
  `admin_alive(platform)` (connects to the socket, since the file persists after a crash);
  `recover_stale_edge`/`stop_edge` use it; `CADDY_ADMIN_PORT` removed. Caddy `running` is now
  the handle gate alone. **Verified** against the pinned caddy 2.11.4 (unix admin works with
  `run`+`start`, socket `srw------- 0600`, reload/stop over the socket, `:2019` refused,
  spaced paths validate, stale socket file auto-replaced) + live root-edge check (curl :2019
  refused, socket user-owned 0600, reload/stop still work). 116/116 lib tests pass (added
  `admin_binds_unix_socket_not_tcp`). Bonus: rexenv no longer touches :2019 at all, which
  also delivers most of 2.4 (M1).
  *Done when:* the root edge no longer exposes an unauthenticated TCP admin endpoint —
  admin is bound to a unix socket with owner-only (0600) perms (or disabled, with
  reload/stop driven another way), and reload / stop / stale-edge recovery still work
  through that channel. Verified: `curl 127.0.0.1:2019/...` no longer controls the edge
  (connection refused / not the admin), while start/reload/stop-all still function.

- [x] **2.4 (M1) rexenv must not stop a user's own Caddy** — ✓ done. The functional fix
  landed with 2.3 (H5): rexenv's admin moved off TCP `:2019` onto a private unix socket,
  so `recover_stale_edge`/`stop_edge` now probe/stop via `admin_alive()` (OUR socket) —
  a foreign Caddy on `:2019` is invisible to us — and `stop_edge`'s reap is gated to our
  own caddy binary path (`owned_pids(caddy_bin)`); `reconcile_startup`'s port sweep already
  excludes `:2019`/`:443`. This commit pins the invariant in the docs/comments
  (`recover_stale_edge`, `stop_edge`, `prepare_edge`). **Verified live**: a foreign caddy
  from a different binary path is NOT selected by our reap marker (`owned_pids` miss), and
  `:2019` is never targeted. 116/116 lib tests pass.
  *Done when:* with a user's own Caddy running on `:2019`, rexenv launch and Stop-all do
  **not** stop it — the stop is ownership-gated (via `owned_listeners(2019, app_data_marker)`
  and/or a rexenv config marker) — while rexenv's own edge is still managed (start / reload /
  stop-all work). Verified live: start a foreign Caddy on `:2019`, launch rexenv + run
  Stop-all, and confirm the foreign Caddy is still up while rexenv's edge behaves normally.

- [x] **2.5 (H4) Atomic binary prepare — no poisoned cache** — ✓ done. `resolve`,
  `resolve_file`, and `resolve_dir` now extract + prepare into a unique hidden staging
  dir (`.staging-<name>-<version>-<pid>-<seq>`) on the same filesystem, then `publish()`
  atomically renames it into the cache ONLY on full success; on any failure the staging
  dir is removed, so the cached path is never created from an unprepared/partial binary.
  `publish` also un-poisons a stale/partial leftover from a prior crash (replaces a dir
  missing its final marker) and yields to a concurrent-resolve winner. **Verified**: 3 new
  `publish` unit tests (target-absent / race-winner / stale-partial) + a codesign-shim live
  recipe (forced prepare failure leaves no cached `caddy-2.11.4` and no `.staging-*`; a
  retry resolves cleanly). 119/119 lib tests pass.
  *Done when:* a failure during `set_executable`/`prepare_binary` never leaves a
  usable-looking-but-unprepared binary in the cache — `resolve` (and `resolve_dir`)
  extract to a temp path, prepare, then atomically rename into place (or `remove_dir_all`
  the target on any failure). Verified: forcing a prepare failure (e.g. an unrelinkable
  dylib dep) leaves no cached file, and a retry re-downloads and prepares cleanly rather
  than returning the poisoned binary.

- [x] **2.6 (M5-interim) Block the FrankenPHP switch for subdirectory-multisite sites** —
  ✓ done (UI-only; full fix stays in BACKLOG M5). Guarded **both** UI entry points that
  can reach the broken combo: (1) **SiteDetail** server switcher — the FrankenPHP option
  is `disabled` (relabelled + `title` tooltip) when `site.multisite === "subdirectory"`;
  Nginx stays enabled so an already-broken site can escape. (2) **New Site dialog** —
  bidirectional mutual lock: FrankenPHP disabled in the server select when Subdirectory is
  chosen, and the Subdirectory card disabled (dimmed + note) when the server is FrankenPHP;
  plus an invariant effect that reverts to Subdomain if a blueprint sets FrankenPHP
  programmatically while Subdirectory is active. No backend change. **Verified:** `pnpm
  build` passes (strict tsc) + manual click-through of both screens (incl. the blueprint
  edge). `reconcile_overrides` still hardcodes `RewriteMode::Single`, so the full fix
  (pass the real `rewrite_mode_for(s.multisite)` into `frankenphp::write_config`) remains
  in BACKLOG.md.
  *Done when:* a subdirectory-multisite site cannot be switched to FrankenPHP from the UI
  (the FrankenPHP option is disabled with a tooltip explaining why); every other site can
  still switch freely. No backend change required.

---

## 3. Live-testing fixes (found after the audit)

- [x] **3.1 Always-on DNS resolver out of the Stop-all contract** — ✓ done (commit
  `ec2a8b0`). The embedded resolver (in-process task, app-lifetime, watchdog-restarted —
  deliberately NOT in `stop_all`) was listed as a stoppable Services row, so after Stop
  all the footer stayed "Partial" and never offered "Start all" — a "running" the button
  couldn't clear (same source-of-truth class as 2.1/H1). Chose the intentional-keep-running
  fix: dropped the DNS row from `enriched_status` (running/total/summary + both global
  toggle labels now count only manager-owned services) and surfaced the resolver as a
  separate **"Always on"** indicator on Services fed by `dns_status` — green when up, red
  **"Down"** (error pill, not gray "Idle") when the task dies. Resolver lifecycle +
  watchdog untouched. **Verified:** 152/152 lib tests (`summarize(&[false,…]) == "stopped"`
  pins the footer); live — stack fully stopped (`lsof` shows :443/:8088/:9783/:13306/:11025
  free) while the resolver holds UDP `:15353`, footer reads Stopped/"Start all", DNS
  indicator green; confirmed in-app by the user.
  *Done when:* UI and reality agree — no "running" that Stop-all can't clear; the DNS
  indicator stays visible (and goes red on a dead resolver) without ever affecting the
  global Start/Stop-all state.

- [x] **3.2 One global Stop all, not two** — ✓ done (commit `a54833a`). Removed the
  Services-header Start/Stop-all — a pure duplicate of the footer control (same
  `start_services`/`stop_services` IPC, nothing page-specific). Kept the **footer** one:
  it lives with the global status + app-total resources and invalidates both the
  `services` and `global-status` queries (the header copy only refreshed `services`).
  The TopBar keeps its "N of M running" subtitle. **Verified:** strict `tsc` clean at the
  commit + browser render (header action gone, single footer control whose label derives
  from live `global_status.running`); confirmed in-app by the user.
  *Done when:* exactly one global start/stop control exists, it drives `stop_all`, and
  its Start/Stop label reflects live service state.

- [x] **3.3 Mail inbox grouped by WordPress site** — ✓ done (commit `6d4caef`). The flat
  message list made it impossible to tell which site sent what. The list pane now groups
  messages by site: a message matches a site by the domain of its sender (then any
  recipient) against `sites.domain`, with subdomains matching too (multisite); unmatched
  mail lands in an **Other** group that always sorts last. Group headers are sticky and
  collapsible (chevron), show site name + `domain`, an unread badge and a total count;
  groups sort by most recent activity, messages within a group newest-first. A site
  dropdown above the list filters to one group ("All sites" default; a stale selection
  auto-resets after Clear all). Grouping is a single O(n) `useMemo` pass client-side —
  Mailpit's server-side content search is untouched, and the preview pane is unchanged.
  **Verified:** strict `tsc` clean; live browser click-through — groups render with
  correct counts (mock inbox: Acme Store 2 / Portfolio 1), collapse hides rows and keeps
  badges, the site filter shows only the chosen group, and a message still opens in the
  preview.
  *Done when:* messages are grouped under site-name + domain headers with per-group
  counts, groups collapse/expand and sort by latest activity (newest-first within),
  unmatched mail falls into an Other group, and a site filter narrows the list — with
  the preview pane behavior unchanged.

- [x] **3.4 Mail preview lands on the part that exists (no more "No HTML part")** —
  ✓ done (commit `e778d0c`). Every WP email opened on "No HTML part": `wp_mail()` sends
  plain-text-only mail, and the preview always started on the HTML tab. Confirmed against
  live Mailpit (real message: `HTML len: 0, Text len: 631`); the Rust `HTML`-field mapping
  was already correct. The preview now falls back to the **Text** tab once per message
  when the HTML part is empty but text exists; a manual HTML click afterwards sticks
  (shows "No HTML part" honestly), and messages with an HTML part still open on HTML.
  One mock message is now text-only so the browser dev build exercises the path.
  **Verified:** strict `tsc` clean; live browser — text-only message opens on Text,
  manual HTML click stays on "No HTML part", an HTML message still opens on the HTML
  iframe.
  *Done when:* selecting a plain-text-only email shows its text body immediately (no
  dead-end "No HTML part" landing), HTML emails still open on the HTML tab, and manual
  tab choices are respected.

---

## Notes

- **Ordering rationale:** §1 first (a fresh install is unusable/opaque without it), then
  §2 (trust + safety before handing the .dmg to anyone). Within §2, H1/H2 are the
  source-of-truth fixes the audit was chartered to find; H5 hardens the root Caddy admin
  API; H4 prevents a class of unrecoverable first-run failures.
- **§2 sub-ordering (fixed):** do **2.3 (H5) then 2.4 (M1) back-to-back** — both live in
  `core/proxy.rs` and touch the same admin-API / edge-ownership code, so they share
  context and one round of testing.
- **Decisions made (were flagged):**
  - **M1** — pulled into §2 as **2.4** (testers are developers likely to run their own
    Caddy; same code area as H5). Marked `[→§2]` in BACKLOG.md.
  - **M5** — full fix stays in BACKLOG.md; a cheap **UI interim** (2.6) ships this release
    so no one hits broken sub-site routing.
- **Verification carries over:** prefer a cold-run / live-click / `curl` / `security`
  check per task over "looks right" (mirrors TASKS-RELEASE).
