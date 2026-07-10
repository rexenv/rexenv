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
- [ ] **In-flight download dedup** (download-manager follow-up, NOT a release blocker):
  add a per-(name,version) async once-lock (single-flight map) in `core/binaries.rs`
  `resolve*` so concurrent callers await the same download instead of racing — today the
  same bytes download twice and the hub progress bar jitters between the two streams.
  Pre-existing race (any two commands resolving the same binary); onboarding's
  auto-prefetch makes it easier to trigger. Correctness is fine (atomic staging/publish
  keeps one winner) — this is efficiency/polish.
- [x] **Release 1.5 — cold first run on a second Mac / clean account** (from TASKS-RELEASE).
  Hands-on: install the .dmg on a machine that has never seen rexenv, run the full first-run
  flow. Pairs with the next item. ✓ **Done:** verified by a real fresh-account cold run on
  10 July 2026 — onboarding system setup, live binary downloads, WordPress site over HTTPS
  with a valid lock, rest of the app all worked end to end.
- [ ] **Release 5.4 — execute the clean-Mac smoke test** — checklist already written:
  `docs/SMOKE-TEST.md`.

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
