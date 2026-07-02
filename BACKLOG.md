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

- [~] **M5 — FrankenPHP override ignores subdirectory-multisite rewrites** — backend fix
  LANDED. `reconcile_overrides` now threads `sites::rewrite_mode_for(s.multisite)` into
  `frankenphp::write_config` (was hardcoded `RewriteMode::Single`). The FrankenPHP
  `SubdirectoryMultisite` Caddy template — never exercised before, and buggy: a placeholder
  typo (`{http.regexp.wpsubph.2}` for a regexp named `wpsubphp` → empty rewrite) plus a
  missing `wp-admin` redirect — was rewritten to faithfully mirror the proven nginx rules
  (redirect → strip `/wp-*` → strip `*.php`, each `not file`-guarded, ordered in a `route {}`
  block). Validated with `frankenphp adapt`/`validate` (`Valid configuration`) + a unit test
  + `examples/frankenphp_subdir_validate.rs`. **The §2.6 UI guard STAYS** (see M5-verify) —
  the routing hasn't been exercised by a real request yet.

- [ ] **M5-verify — live-test FrankenPHP subdirectory multisite, then remove the §2.6 guard.**
  On a machine with a free `:443`: one-click a WP site, convert to subdirectory-multisite,
  switch it to FrankenPHP, and confirm a sub-site page **and** its `/wp-admin` redirect route
  correctly (couldn't be done in-repo — DBngin holds `:443`). Then remove the 2.6 UI guard:
  the `FRANKENPHP_SUBDIR_BLOCK` guards in `SiteDetail.tsx` + `NewSiteDialog.tsx` and the
  New-Site invariant `useEffect`.

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

- [ ] **Per-site serving granularity** (follow-up to TASKS-FIXES §2.1 / H1). 2.1 makes
  the site status honest by deriving it from *stack-level* state (`global_status` — up vs
  stopped), so all sites read the same status. A "partial" stack (e.g. a FrankenPHP
  backend down while nginx serves) is shown optimistically as running. *Fix (later):*
  derive true per-site serving — the site's own upstream (its php-fpm pool or FrankenPHP
  backend) is up AND the edge is up AND it's present in the generated config. This is also
  the natural home if real per-site enable/disable (fork A) is ever wanted.

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
