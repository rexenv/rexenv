# PLAN — headless rexenv on a server, driven from a browser and from `rex`

**Status:** PLANNED — 26 Oct 2026. Nothing built. The facts in §2 were read from the tree at `14ab9608`;
the open measurements are T0's (§6), and the shape waits on the owner's rulings D1–D6 (§5). Re-ordered the
same day after GitHub issue rexenv/rexenv#7: Phase 1a (headless + `rex`/`rex mcp` + configurable edge
ports) ships before the web UI, because it alone is what that user needs.
Tracked as the "Headless rexenv on a VPS" row in `docs/TODO.md` (Phase 4+).

## 1. The ask

Owner, 26 Oct 2026: installing rexenv on a local PC is all it takes today. Installing it on a
remote VPS should be the same. A VPS has no display, so the app's GUI should be served on a port of the
VPS, and opening that port in a browser should give the whole app. `rex` should keep working on the VPS too.

The same day a user asked for the same thing from the other end, in GitHub issue rexenv/rexenv#7
("Linux: run headless on a server (no GUI, configurable ports, `rex mcp`)"). Their use is automated
pull-request QA. An agent (Claude Code CLI) on an Ubuntu 24.04 x86_64 server creates a fresh WordPress
site per PR, installs the plugin build, tests it through `rex` and `rex mcp`, and deletes the site. They
need no browser UI at all. They list five asks:
1. a headless systemd service controlled only through `rex` and `rex mcp`;
2. configurable HTTP/HTTPS ports or a bind address, because their nginx already holds 80 and 443;
3. `rex mcp` on Linux, documented;
4. scratch sites created and deleted per run on Linux;
5. Redis and MariaDB on Linux, and Apache if practical (they test a caching plugin and `.htaccess`).

Asks 1–4 are this plan (§4 Phase 1a). Ask 5 is a catalog question, outside it (§4 "Not in this plan").

What this is NOT: a hosting panel, a multi-user/team product, or a way to put the developer's sites
on the public internet. It is ONE developer's rexenv, running on a machine they reach over the network.

## 2. What the tree says today (read at `14ab9608`)

- **The backend lives inside the GUI process.** `rex` is remote-control only. With no app running it
  exits 2 with "open the app first" (`docs/ARCHITECTURE.md` §8.2). The reason is written there: a
  CLI-spawned backend would be a second ServiceManager/SQLite writer racing the GUI. A headless mode
  of the SAME app (one instance, same single-instance socket lock, ledger #441) does not break that
  rule, because it is the one brain without a window.
- **The commands are bound to Tauri's runtime.** There are 291 `#[tauri::command]` fns, and 17 files
  under `commands/` take an `AppHandle`. `cli_server.rs` reaches them by holding a
  `tauri::AppHandle<R>` and pulling `tauri::State<AppState>` (+ the job states) out of it
  (`cli_server.rs` `app_state`, `provision_jobs_state`, …). The MCP server does the same. So any
  server mode needs a live Tauri app, or a refactor away from `AppHandle` (D1).
- **The frontend touches Tauri in very few places.** The frontend already has ONE way into the
  backend, and swapping it is the cheap half of this plan:
  - Every backend call goes through ONE `invoke` (`src/lib/ipc/index.ts:46`). It covers 269 command
    names.
  - Events: `@tauri-apps/api/event` is used only in `src/lib/ipc/index.ts` (and the dev-only
    `DevUiReview.tsx`). The backend emits `service-health`, `download-progress`, `scratch-reaped`,
    the updates events (`UPDATES_EVENT`, `APP_UPDATE_EVENT`), `WATCH_GLOBAL_EVENT`, plus the tray
    and menu events, which have no meaning on a server.
  - Native dialogs: three folder pickers in `src/lib/ipc/index.ts:78/87/104`.
  - Window: `src/lib/window-focus.ts` already renders with no Tauri window (the wk-checks path).
    `src/lib/window-drag.ts` becomes a no-op.
- **The custom URL schemes do not exist in a browser.** The Database Browser (`rexdb:`) and the
  phpMyAdmin console (`rexpma:`) are custom protocols (see `tauri.conf.json` `frame-src`). A browser
  cannot load them, so they need HTTP routes on the web server. This is the #699/#702/#703 lesson: all
  three ends of the frame policy must be right (our CSP, the replayed `frame-ancestors`, the frame URL).
- **Linux privilege needs a desktop.** Linux privilege goes through `pkexec`/polkit
  (`PrivilegeManager`), which needs an authentication agent. A headless server has none.
- **Secrets need a keyring.** The live-sync credentials go through `SecretStore`. On Linux that is the
  Secret Service, which a headless box does not run; `docs/PLAN-wp-live-sync.md` §11 Q4 leaves the
  fallback open.
- **The DNS agent is a per-user systemd unit.** A user unit stops at logout unless the user is
  lingering (`loginctl enable-linger`).
- **The edge's ports are fixed.** `core/service_manager.rs:35` takes `proxy::DEFAULT_HTTP_PORT` (80) and
  `proxy::DEFAULT_HTTPS_PORT` (443). There is no setting. On macOS and Linux the edge binds the wildcard
  `*:443` (`core/proxy.rs:137`); Windows binds `127.0.0.1` only. A server whose nginx holds `:443` fails
  the `core/ports::ensure_free` gate, which is issue #7's ask 2.
- **`rex mcp` and scratch sites are common code.** `rex mcp` is a bridge in `cli/src/main.rs` to the MCP
  socket (a unix socket on Linux, `LocalIpc`). `scratch_create_site`/`scratch_delete_site` live in
  `mcp_server/scratch.rs` with no OS branch. Both compile for Linux, but no Linux run and no Linux doc of
  either was found. That is a missing proof, not a missing feature.
- **The Linux catalog has no Redis, MariaDB or Apache httpd** (`docs/PORTS.md` §Linux, D-L8 of
  `docs/PLAN-linux-port.md`). `ships_on` refuses them on Linux.
- **The edge is loopback-scoped by design.** It runs auto-HTTPS off, with leaves from the local CA,
  and `.rex` resolves only on the machine itself. So a site on the VPS is reachable from the
  developer's own browser only through a tunnel, a proxy, or a real domain (§4 Phase 3).

## 3. Shape

```
rexenv --headless [--web 127.0.0.1:9443]
  ├─ the same app: ServiceManager, SQLite, watchdog, cli_server socket, MCP socket — no window
  ├─ web server (new, app-shell like cli_server.rs)
  │    GET  /            → the built frontend (dist/), same bundle as the desktop app
  │    POST /ipc/<cmd>   → the same commands::* fns the webview invokes
  │    WS   /events      → every backend emit, pushed
  │    GET  /db/…, /pma/… → the rexdb:/rexpma: handlers over HTTP
  └─ started by the OS service manager at boot (systemd on Linux — D3 for the other two)
```

- **Frontend transport.** `invoke` in `src/lib/ipc/index.ts` picks the Tauri transport when
  `window.__TAURI_INTERNALS__` exists, otherwise `fetch('/ipc/<cmd>')`. `listen` does the same over
  the `/events` WebSocket. Components do not change. That is the existing rule that "UI never calls
  raw `invoke`" paying off.
- **One dispatch list.** The web server must reach EVERY command the webview can, and nothing else.
  It is generated from the same list as `tauri::generate_handler!` (D2), with a test that the two
  lists are equal. A command added to the desktop app then cannot be missing from the web UI, and a
  hand-written arm cannot drift. This guarantee comes from the structure, not from remembering.
- **The folder pickers** become a server-side directory browser: a new `browse_dir` command listing
  directories under the user's home and the Sites root, rendered by a small React picker in the
  browser. The Tauri dialog stays for the desktop.
- **Desktop-only surfaces** (tray, menu, window chrome, "open in Finder/Explorer", the self-update's
  relaunch) answer from the platform. The answer is a capability saying "not on a headless install"
  plus a sentence in `platform/words.rs`, never a silent no-op. The UI hides what the capability says
  is absent.

## 4. Phases

### Phase 1a — headless, driven by `rex` and `rex mcp` (the first ship; issue #7 asks 1–4)

- `rexenv --headless` runs the full backend without a window. `rex` and `rex mcp` work against it
  unchanged, which answers the first half of the owner's ask and all of issue #7's asks 1, 3 and 4.
- **Configurable edge ports and bind address.** New settings for the HTTP port, the HTTPS port and the
  edge bind address (default `80`/`443`/the OS's current bind, so nothing changes for a desktop user).
  Every start stays port-gated through `core/ports::ensure_free` on the chosen ports. This is common code
  and lands on all three OSes, desktop included. It is not only a config value:
  - A non-443 port makes a site's URL carry it (`https://x.rex:8443`). WordPress stores it in `siteurl`/
    `home`, so creating, importing, renaming a domain and live sync must all write the port. Cookies,
    multisite (subdomain and subdirectory) and the edge's HTTP→HTTPS redirect must be tested with it.
  - Changing the port with sites already present means rewriting those URLs, the same job as a domain
    change. The first version may refuse the change while sites exist, with a sentence saying why.
  - The Share/tunnel and every place that prints a site URL (`rex site info`, MCP `site_info`, the UI)
    must take the port from one place, never assume 443.
- **Linux proof of `rex mcp` and scratch sites** on the headless instance: an agent creates, uses and
  deletes a scratch site end to end, and `docs/INSTALL.md`'s Linux section documents `rex mcp`.
- Privilege, secrets and the DNS agent's lingering follow the rules in Phase 1b below; 1a needs them
  first.

### Phase 1b — the web UI on loopback, reached over SSH

- The web UI binds **`127.0.0.1` only**. The developer runs `ssh -L 9443:localhost:9443 vps` and opens
  `http://localhost:9443`. Browsers treat `localhost` as a secure context, so the tunnel needs no TLS.
- **Auth even on loopback.** Loopback on a VPS is not private: every local user and every process on
  the box can reach it, a WordPress plugin's `wp_remote_get` included. The rules:
  - A random token is written `0600` to app-data. `rex web url` prints `http://localhost:9443/?t=<token>`.
  - The first request trades the token for an `HttpOnly; SameSite=Strict` session cookie.
  - Every `/ipc` call must carry the cookie AND a custom header (CSRF).
  - The `Host` header must be `localhost`/`127.0.0.1` (DNS rebinding).
  - The `Origin` must be our own.
- **The sites, from the developer's browser:**
  - (a) `ssh -D 1080 vps` with the browser's SOCKS proxy set to resolve DNS remotely. `.rex` then
    resolves ON the VPS. The local CA needs trusting on the developer's machine, via a CA download in
    Settings plus a sentence per OS.
  - (b) the existing Share/tunnel flow, unchanged.
- **Privilege.** A one-time `sudo rexenv --headless-setup` does every privileged step the desktop does
  through pkexec: the `rexenv0` DNS link, CA trust into the system store, whatever the edge's `:443`
  needs (T0 lists them), and `loginctl enable-linger` for the DNS agent's user unit. At runtime the
  service runs as the user, never as root, and a privileged op that turns up later fails with a sentence
  naming `sudo rexenv --headless-setup`.
- **Secrets.** `SecretStore` on a box without Secret Service, as D4 rules.

### Not in this plan — Redis, MariaDB and Apache on Linux (issue #7 ask 5)

These are catalog work in `rexenv/runtimes` and `core/binaries.rs`, under D-L8 of
`docs/PLAN-linux-port.md`: a build or a relinked bottle per arch, pinned and checksum-locked, with
`ships_on` opened once a Linux run passes. They are tracked as their own row in `docs/TODO.md`. Headless
mode does not depend on them, and they do not depend on headless mode.

### Phase 2 — the web UI on a public address

- `--web 0.0.0.0:9443 --public`. It is an explicit flag, and the UI shows a consent sentence that lives
  in Rust beside the rule (the `copy_scan` rule).
- TLS is required: an ACME certificate for a hostname the owner points at the VPS, or a self-signed
  certificate whose fingerprint `rex web url` prints. Plain HTTP on a public bind is refused.
- A password login (argon2id hash in SQLite, set with `rex web passwd`) replaces the token, with
  per-IP backoff on failures. The session and CSRF rules of Phase 1 stay.
- **Security, said plainly:** this UI can read and write files, run PHP and WP-CLI, query every
  database and change service config. Anyone who gets past the login owns the VPS user, and through
  the setup grants more. Phase 2 does not ship until the auth layer has its ledger rows proven and
  a review that did not write it has read it.

### Phase 3 — sites on real domains (separate, larger)

- A wildcard such as `*.dev.example.com`, served by an edge mode with ACME (auto-HTTPS ON for that
  TLD) and a non-loopback bind, behind the same opt-in.
- It changes the edge's non-negotiables ("auto-HTTPS disabled", the local CA, loopback scope), so it
  gets its own plan and its own owner ruling before any code.

## 5. Owner decisions

- **D1 — how a window-less Tauri app runs.** Recommended: (a) if T0 proves it works, else (b).
  - (a) The Tauri app with no window created, if the event loop starts with no display. T0 measures this.
  - (b) The same under a virtual display (Xvfb). Needs an extra package on the server, but touches no code.
  - (c) Refactor `commands/` and the socket servers off `AppHandle` onto an app-owned host that Tauri,
    the CLI, the MCP and the web server all hold. This is the cleanest, but it touches 17 command
    files and both servers.
- **D2 — dispatch generation:** a macro that emits both `generate_handler!` and the web table from
  one list (recommended), vs. a build-time check over hand-written arms.
- **D3 — the other two OSes.** The three-OS rule applies. Recommended: Linux first, because that is
  the VPS, with macOS (LaunchDaemon) and Windows (service / boot-time task) filled in the same change
  as `Error::Unported` plus a sentence in `platform/words.rs`, and a TODO row each. The web transport
  itself is common code and works on all three as soon as their headless mode exists.
- **D4 — secrets without a keyring:** an honest refusal for live sync, or a `0600` file under
  app-data with the sentence saying so. Same question as `docs/PLAN-wp-live-sync.md` §11 Q4; one
  ruling should answer both.
- **D6 — the edge port setting's shape.** Recommended: global settings (one HTTP port, one HTTPS port,
  one bind address) rather than per site, changeable only while no site exists in the first version.
  The alternative is a full rewrite of every site's URLs on change, as a later task.
- **D5 — the web UI's port.** `9443` is a proposal. It goes into `docs/PORTS.md` and is port-gated through
  `core/ports::ensure_free` like every other service.

## 6. Tasks

Phase 1a first (T0–T5), then Phase 1b (T6–T9).

- **T0 — measure (no code shipped).** Done when every answer is written into §2:
  - Does a Tauri 2 app start on Ubuntu 22.04 with no `DISPLAY`/`WAYLAND_DISPLAY` and no window built?
    Under Xvfb?
  - The full list of `PrivilegeManager` calls on Linux, and of anything else that assumes a desktop
    session.
  - Every place that builds a site URL or assumes port 443 (for T2).
  - What breaks in the frontend with the Tauri internals absent, beyond §2's list. The wk-checks
    already render without Tauri, so start there.
  - Run on the Ubuntu VM (no desktop session), not the dev Mac.
- **T1 — headless mode.** `--headless` boots the app with no window, tray or menu. Done when `rex status`,
  `rex site create`, `rex start/stop` work against it on the VM over SSH, and a second launch hands off
  to it (ledger #441).
- **T2 — configurable edge ports and bind address (D6).** Done when, on the VM with a foreign nginx
  holding `:80`/`:443`, the stack starts on `8080`/`8443`, a WordPress site and a subdirectory multisite
  load with the port in their URLs, login cookies hold, and the HTTP→HTTPS redirect keeps the port. Also
  run on macOS and Windows, because the setting is common code.
- **T3 — `rex mcp` + scratch sites on headless Linux.** Done when an MCP client over `rex mcp` on the VM
  runs `scratch_create_site` → a plugin install → `scratch_delete_site`, with nothing left behind, and
  `docs/INSTALL.md` documents it.
- **T4 — install + setup.** The systemd unit, `--headless-setup`, lingering, the deb's post-install note.
  `docs/INSTALL.md` gets a "Server (headless)" section, and `docs/SMOKE-TEST.md` gets the rows. After T4,
  Phase 1a is shippable, and issue #7's reporter gets a build to test on Ubuntu 24.04 x86_64.
- **T5 — the other OSes per D3.**
- **T6 — web transport.** The `invoke`/`listen` switch, `/ipc` + `/events`, and the generated dispatch
  with the equal-lists test. Done when the Sites screen lists and creates a site in a browser through an
  SSH tunnel, and the test fails when a command is missing from either list (plant it).
- **T7 — auth.** The token, cookie, CSRF header, and `Host`/`Origin` checks. Each check gets a ledger
  row and a plant that proves the request is refused without it.
- **T8 — the rest of the screens.** The folder picker (`browse_dir`), `/db` and `/pma`, the mail
  viewer, and the desktop-only capabilities. Done when every route in `src/routes/` is walked in a
  browser on the VM.
- **T9 — the CA download** for viewing sites through `ssh -D`, with a sentence per OS.
- **Phase 2 and Phase 3 get their task lists when Phase 1 ships.**

## 7. Docs this touches when built

`docs/ARCHITECTURE.md` (§8.2's "app not running" rule gains the headless instance; a new §
for the web server), `docs/MAP.md`, `docs/PORTS.md` (D5, and the edge's ports becoming settings), `docs/INSTALL.md`,
`docs/SMOKE-TEST.md`, `docs/CLI-ROADMAP.md` (`rex web url`, `rex web passwd`),
`docs/PLATFORMS.md` §3 (the per-OS service manager and the headless capability),
`docs/CLAIM-LEDGER.md` (the auth checks, the equal-lists guard), and `docs/PLAN-linux-port.md`
§1.1 for any `cfg(target_os)` added in `lib.rs`/`main.rs`.
