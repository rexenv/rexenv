# TODO — the single active-work file

Everything open lives here. Completed phases + audit history: `docs/archive/`
(historical — don't trust as current). When you finish an item, tick it here with a
one-line ✓ evidence note (same convention as the archived TASKS files).

## Actionable now

- [x] **L7 — move shared Nginx off 8088** (from BACKLOG). `8088` collides with Hadoop
  YARN / common dev proxies; moved to `18088`. Low risk: loopback-only, bind-tested at
  start. ✓ **Done:** `core/services.rs` `NGINX_HTTP_PORT` = 18088; configs regenerate
  from the constant on every stack start (`rebuild_configs_for`); tests/examples/mock/
  docs updated; `cargo test --lib` green. Transition note: a stack left running by a
  pre-change build keeps its old nginx on 8088 — it is not adopted (adoption keys on the
  new port) and not auto-reaped; kill it manually or via the old build's Stop all.
- [ ] **Release 1.5 — cold first run on a second Mac / clean account** (from TASKS-RELEASE).
  Hands-on: install the .dmg on a machine that has never seen rexenv, run the full first-run
  flow. Pairs with the next item.
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
