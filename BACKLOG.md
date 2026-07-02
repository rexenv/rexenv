# BACKLOG — Post-Release (from the read-only audit)

> Findings from **FINDINGS.md** deferred past the pre-release .dmg. The pre-release
> subset lives in **TASKS-FIXES.md**. Full detail (file:line, why-it-matters, fix) is
> in FINDINGS.md under the same IDs — this file is the tracking list.
>
> ⚠️ **Two items are flagged for a pre-release decision** — they may jump into
> TASKS-FIXES.md §2 before that work starts: **M1** and **M5** (see below).

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[D]` deferred · `[→§2]` pulled into pre-release

---

## Flagged for a pre-release decision

- [ ] **M1 — rexenv can kill a user's *own* Caddy** (`core/proxy.rs:172-236`).
  `recover_stale_edge`/`stop_edge` run `caddy stop` against `:2019` without verifying
  the listener is rexenv's — and `reconcile_startup` does this on **every launch**. A
  developer running their own Caddy on the default admin port loses it.
  **Decision:** pull into pre-release (§2) if any tester is likely to run their own
  Caddy; otherwise post-release. *Fix:* gate `stop_admin` on
  `owned_listeners(2019, app_data_marker)` / a rexenv config marker.

- [ ] **M5 — FrankenPHP override ignores subdirectory-multisite rewrites**
  (`core/service_manager.rs:371`). Hardcodes `RewriteMode::Single`; the UI lets a
  subdirectory-multisite network switch to FrankenPHP, breaking sub-site routing.
  **Decision:** pull into pre-release (§2) if multisite + FrankenPHP is a demoed combo;
  otherwise post-release (or block the switch in the UI as an interim). *Fix:* pass the
  site's real `rewrite_mode_for(s.multisite)` into `write_config`.

---

## Medium

- [ ] **M3 — `wait_until` results discarded** (`service_manager.rs:126,309,374`). A
  service that never starts still returns `Ok(())`, so failures surface later and
  confusingly. *Fix:* error on a false return, naming the service + log path.

- [ ] **M4 — Blocking sleeps hold the services lock** (`service_manager.rs:596-604`).
  `std::thread::sleep` inside async `ensure_*` parks a tokio worker (up to 15s) while
  the async mutex is held. *Fix:* `tokio::time::sleep`, or `spawn_blocking`.

- [ ] **M6 — Monitor refreshes all processes per service per poll**
  (`monitor.rs:50-57` + `commands/services.rs`, `commands/database.rs`).
  `refresh_processes(All)` runs N times each 2s poll. *Fix:* refresh once per poll (or
  `ProcessesToUpdate::Some`), then read each pid.

- [ ] **M7 — No backend domain validation** (`core/sites.rs:19-41,197-227`).
  Defense-in-depth: the domain flows into a filesystem path, nginx/Caddy config, cert
  SAN, and DB name; the UI slugs it but core doesn't validate. *Fix:* validate against a
  strict `*.test` pattern in `create`/`provision`.

---

## Low

- [ ] **L1 — `managed_ports()` hardcodes PHP minors** (`service_manager.rs:451`). Use
  `php::all_minors()` so a future 8.4 pool isn't missed by the orphan sweep.
- [ ] **L2 — `extract_tar_gz_tree` has no traversal/symlink guard** (`binaries.rs:666-690`).
  Gated by pinned checksums (defense-in-depth). Reject entries escaping `dest`.
- [ ] **L3 — `ProcessSupervisor::stop` is SIGTERM-only** (`platform/macos/mod.rs:209-219`).
  Add a timed SIGKILL fallback so a TERM-ignoring process can't block `wait()`.
- [ ] **L4 — `MacosShell::run` ignores exit status + stderr** (`platform/macos/mod.rs:361-364`).
  Latent (zero callers today). Fix the silent-failure behavior before anything wires it.
- [ ] **L5 — `dns_status` opens a raw UDP socket in the command layer**
  (`commands/system.rs:137-148`). Layering smell; move the probe behind a core/platform helper.
- [ ] **L6 — Auto-update Settings card is dead UI** (`Settings.tsx:724-747`). Hide until
  the updater (release §6.1) is actually wired.

---

## Notes

- These are recorded, not scheduled — pull into a phase deliberately. FINDINGS.md holds
  the full rationale and suggested fix for each.
- The audit's "what's solid" section (FINDINGS.md) lists correct patterns to preserve
  while doing this work — notably the `global_status`/`services_status` shared-source
  fix, the `wp_login` mu-plugin hardening, parameterized SQL, and the checksum-before-write
  download path.
