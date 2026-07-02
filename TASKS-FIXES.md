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

- [ ] **1.1 (C1) Wire first-run system setup** — `run_system_setup` exists but nothing
  calls it; the Onboarding "Domains & SSL" step is a `setTimeout` fake
  (`Onboarding.tsx:226`, `// TODO: wire to run_system_setup`), no command is registered,
  and `/onboarding` is never reached on first launch. Result: `/etc/resolver/test` is
  never installed, so `.test` never resolves on a clean Mac.
  *Done when:* a `system_setup` Tauri command calls `core::setup::run_system_setup`; a
  typed IPC wrapper exists; the Onboarding button calls it with real busy/done/error
  states (no `setTimeout`); first launch routes to onboarding (or the setup step) when
  `dns_status().resolverInstalled` is false; and on a machine where `/etc/resolver/test`
  does **not** exist, completing onboarding through the UI writes it and trusts the CA —
  verified end-to-end with **no example binary**: `cat /etc/resolver/test` shows the
  resolver, `security dump-trust-settings` lists the rexenv CA, and `https://<name>.test`
  loads with a valid local-CA lock. (Pairs with TASKS-RELEASE 1.5.)

- [ ] **1.2 (H3) No silent startup panic on DB/CA failure** — if `open_for_platform` or
  `ssl::load_or_create` fails, `AppState` is never managed (`lib.rs:47-68`) and every
  command that takes `State<AppState>` panics with a cryptic error and no UI message.
  *Done when:* a DB-open or CA-load failure produces a clear, human-readable error in the
  UI (blocking window / error screen) and the app stays responsive — no
  "state not managed" panics. Verified by forcing the failure (point app-data at an
  unwritable path, or corrupt the CA key file) and confirming the app shows the error
  instead of cryptic per-action failures.

- [ ] **1.3 (M2) No broken visible controls** — three UI affordances invoke a command
  that isn't registered or is a no-op.
  *Done when:* every control on Mail and Services does what it says, with no
  "command not found" toast:
  - **(a)** Mail per-message delete + "mark all read" either work (`mailpit_delete` /
    `mailpit_mark_all_read` registered in `lib.rs` + backed by `core::mail`) **or** are
    removed. (`mailpit_clear` already works.)
  - **(b)** The Services per-row Start/Stop toggle (`Services.tsx:293` `() => {}`) either
    performs a real per-service start/stop **or** is removed, leaving Start-all/Stop-all.
  - **(c)** The `services_status` DTO includes `kind`/`version`/`isDefault`/`isRouter`
    so the "default" + "edge router" badges and per-row version render **or** those UI
    affordances are removed.

---

## 2. Correctness + security (before sharing)

> The app runs after §1, but these are honesty/trust and safety issues a tester would
> hit or a reviewer would flag.

- [ ] **2.1 (H1) Per-site status reflects actual serving** — `start_site`/`stop_site`
  only flip `sites.status`; `rebuild_configs_for` serves every site regardless, so the
  badge, the Sites header count, and the filter tabs describe a state disconnected from
  reality.
  *Done when:* a site's running/stopped state reflects whether it is actually served
  (stack up + present in the generated config). **Either** per-site enable/disable is
  real — toggling filters `rebuild_configs_for` and reloads the edge, so a "stopped"
  site stops serving while a "running" one serves — **or** the per-site toggle is removed
  and the badge is derived from actual serving. The Sites header count and
  All/Running/Stopped tabs read the same truth. Verified: toggling a site changes whether
  `https://<domain>` serves, and with the stack stopped no site shows "running".

- [ ] **2.2 (H2) "Running" means an owned process is alive, not a bare port-listen** —
  every service's `running` is currently a TCP-connect probe, so a foreign listener
  (the dev's DBngin on :443/:3306, a system MySQL) reads as rexenv's service being up,
  and Stop-all can't clear it.
  *Done when:* each service's `running` is true only when a rexenv-owned process is up —
  tracked-child liveness (`try_wait`) and/or `owned_listeners(port, app_data_marker)`;
  the root Caddy edge is probed via its admin identity, not raw :443. Verified: with
  rexenv stopped but DBngin holding :443/:3306 (or a system MySQL running), Services and
  the footer show Caddy/MySQL as **stopped**, and Start-all/Stop-all act only on rexenv's
  own processes.

- [ ] **2.3 (H5) Lock down the root Caddy admin API** — the edge runs as root with
  Caddy's admin endpoint on TCP `:2019`, unauthenticated; any local process can POST
  config to a root Caddy (arbitrary file read/write as root). FrankenPHP backends already
  set `admin off`; only the edge is exposed.
  *Done when:* the root edge no longer exposes an unauthenticated TCP admin endpoint —
  admin is bound to a unix socket with owner-only (0600) perms (or disabled, with
  reload/stop driven another way), and reload / stop / stale-edge recovery still work
  through that channel. Verified: `curl 127.0.0.1:2019/...` no longer controls the edge
  (connection refused / not the admin), while start/reload/stop-all still function.

- [ ] **2.4 (H4) Atomic binary prepare — no poisoned cache** — `resolve` extracts to the
  final path then runs `set_executable` + `prepare_binary`; if prepare fails, the
  unsigned/unrelinked binary is cached and every later `resolve` returns it via the
  `exists()` short-circuit (Apple Silicon SIGKILLs it; never self-heals).
  *Done when:* a failure during `set_executable`/`prepare_binary` never leaves a
  usable-looking-but-unprepared binary in the cache — `resolve` (and `resolve_dir`)
  extract to a temp path, prepare, then atomically rename into place (or `remove_dir_all`
  the target on any failure). Verified: forcing a prepare failure (e.g. an unrelinkable
  dylib dep) leaves no cached file, and a retry re-downloads and prepares cleanly rather
  than returning the poisoned binary.

---

## Notes

- **Ordering rationale:** §1 first (a fresh install is unusable/opaque without it), then
  §2 (trust + safety before handing the .dmg to anyone). Within §2, H1/H2 are the
  source-of-truth fixes the audit was chartered to find; H5 is the one real security
  hardening item; H4 prevents a class of unrecoverable first-run failures.
- **Watch for a decision:** BACKLOG.md M1 (rexenv killing a user's own Caddy) and M5
  (FrankenPHP ignoring subdirectory-multisite rewrites) are candidates to pull into §2
  — decide before starting §2.
- **Verification carries over:** prefer a cold-run / live-click / `curl` / `security`
  check per task over "looks right" (mirrors TASKS-RELEASE).
