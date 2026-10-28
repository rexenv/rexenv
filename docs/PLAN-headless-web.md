# PLAN — headless rexenv on a server, driven from a browser and from `rex`

**Status:** IN PROGRESS — 26 Oct 2026. T0 measured and **T1 built and run on the Ubuntu VM the same day**
(§6); T2 onward not started. The facts in §2 were read from the tree at `14ab9608`;
**T0 was measured the same day on the Ubuntu 22.04 arm64 VM (§2a)**, and the shape waits on the owner's
rulings D1–D7 (§5). Re-ordered the
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
    `DevUiReview.tsx`), in about 22 `listen` calls. Some channels are fixed (`service-health`,
    `download-progress`, `scratch-reaped`, the updates events, `WATCH_GLOBAL_EVENT`); others are
    per job (`db-import://state/<id>`, `live-sync://state/<id>`, `valet-import://row`, …). So the
    `/events` socket must carry the channel name, not a fixed list. The tray and menu events have no
    meaning on a server.
  - Native dialogs: three folder pickers in `src/lib/ipc/index.ts:78/87/104`.
  - Window: `src/lib/window-focus.ts` already renders with no Tauri window (the wk-checks path).
    `src/lib/window-drag.ts` becomes a no-op.
- **The custom URL schemes do not exist in a browser.** The Database Browser (`rexdb:`) and the
  phpMyAdmin console (`rexpma:`) are custom protocols (see `tauri.conf.json` `frame-src`). A browser
  cannot load them, so they need HTTP routes on the web server. This is the #699/#702/#703 lesson: all
  three ends of the frame policy must be right (our CSP, the replayed `frame-ancestors`, the frame URL).
- **Linux privilege needs a desktop.** Linux privilege goes through `pkexec`/polkit
  (`PrivilegeManager`), which needs an authentication agent. A headless server has none.
- **~~Secrets need a keyring.~~ WRONG, corrected 26 Oct 2026.** This line was written from the live-sync
  PLAN's open question, not the code: §11 Q4 was decided on 10 Oct 2026, and the pairing lives in an
  owner-only `0600` file under app data on every OS (`core/live_sync/secrets.rs` — "a Linux box without
  one (headless, a minimal WM) still has to work"). There is no `SecretStore` and no Secret Service
  dependency, so headless live sync needs nothing here. (Off 443 it is refused anyway, #917.)
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
- **The Linux catalog has had Redis, MariaDB and Apache httpd since 28 Oct 2026** — rexenv's own
  builds (`docs/PORTS.md` §Linux; D-L8 of `docs/PLAN-linux-port.md` closed for them). Until then
  `ships_on` refused them on Linux, which is what this line said when it was written.
- **The edge is loopback-scoped by design.** It runs auto-HTTPS off, with leaves from the local CA,
  and `.rex` resolves only on the machine itself. So a site on the VPS is reachable from the
  developer's own browser only through a tunnel, a proxy, or a real domain (§4 Phase 3).

## 2a. T0 — measured on the Ubuntu VM, 26 Oct 2026

The installed 0.9.2 deb (arm64) on the UTM VM, with the desktop (`gdm`) stopped so the box looks like a
VPS, driven over SSH. Every site and process the run made was its own and was removed after; the VM was
handed back with `gdm` running and the GUI app relaunched by its autostart.

- **No display → the app panics before anything starts.** `env -u DISPLAY -u WAYLAND_DISPLAY
  rexenv --hidden` exits 101: `Failed to initialize gtk backend!: BoolError { message: "Failed to
  initialize GTK" … }` from `tao-0.35.3/src/platform_impl/linux/event_loop.rs:217`. tao calls
  `gtk::init()` while building the event loop, before Tauri's `setup` and before any window exists, so
  "a Tauri app with no window" (D1 option a) does not exist on Linux without patching tao. The panic
  left nothing behind: the next launch claimed the CLI socket normally.
- **Under Xvfb the whole app runs headless.** `xvfb-run -a -s "-screen 0 1280x800x24" rexenv --hidden`,
  started from SSH with no desktop session: it adopted the running stack, `rex status` showed all seven
  services, and `rex site create t0probe.rex --type wordpress` made a WordPress site in 9.7 s that
  answered `200` over HTTPS. `rex site delete … --yes` removed it. Xvfb's RSS was about 63 MB. `xvfb` +
  `xauth` were installed with apt on the VM (they stay; harmless).
- **Stop all needs polkit on every run.** `rex stop` failed with `This account is not allowed to
  administer the computer (polkit refused), or no authentication agent is running.` and stopped NOTHING
  (all services still running after, the refusal precedes the stops). The reason is
  `core/proxy.rs` `stop_edge_daemon`: the edge is a root system unit (`rexenv-edge.service`), and stopping
  it is a `run_privileged` call each time. `start_edge_daemon` (install/refresh of the unit) is the same.
  `rex start` worked only because the edge was already up. The full list of `run_privileged` callers
  on Linux:
  - `core/proxy.rs` — `start_privileged`, `start_edge_daemon`, `stop_edge_daemon` (the edge);
  - `core/dns.rs` — the resolver route install, removal and `hand_back_resolver`;
  - `core/setup.rs` — `run_system_setup` and `run_system_teardown`;
  - `core/cli.rs` — `install` (the `rex` symlink when unprivileged fails);
  - `platform/linux/mod.rs` — `trust_ca` / `untrust_ca`, and the self-update's `dpkg -i`.
  So `--headless-setup` alone is not enough: Stop all and edge refreshes recur at runtime. Phase 1a needs
  a runtime path for them (D7's sibling, see T1).
- **`rex mcp` cannot be turned on headless.** `rex mcp` answered `rexenv is running, but its AI agent
  (MCP) endpoint is off — turn it on in rexenv → Settings → AI agents (MCP), then reconnect.` That is the
  opt-in of ledger #203, and `rex config` deliberately cannot write it or the agent-access dial (ledger
  #397, `core/settings_access.rs`), because the toggle's sentence IS the consent. A headless box has no
  Settings screen, so issue #7's ask 3 needs a consent path that is not the GUI (D7). The scratch-site
  run (T3) therefore was not reached in T0.
- **Reboot: nothing of the user side comes back on a server.** Read from the code, not rebooted:
  the app's autostart is an XDG `.desktop` file (`platform/linux/mod.rs` `AutostartManager::enable`),
  which only a graphical login runs; the DNS agent is a user unit `WantedBy=default.target`
  (`platform/linux/units.rs:55`), which needs a session or lingering (`Linger=no` on the VM); only the
  edge (`WantedBy=multi-user.target`) starts at boot. The services the app spawns stay down until the
  app runs. During the run the DNS agent survived the SSH logouts only because the app's own session
  scope kept the user manager alive.
- **There is no one place that builds a site URL.** `https://{domain}` is formatted in 16 Rust files
  (`core/proxy.rs`, `core/wordpress.rs`, `core/tunnels.rs`, `core/wp_login.rs`, `core/worktree.rs`,
  `core/live_sync/{pull,push}.rs`, `commands/site_provision.rs`, `mcp_server/scratch.rs`, …) and 5 TSX
  files (`Sites.tsx`, `SiteDetail.tsx`, `LiveTab.tsx`, `lib/adminer.ts`, `lib/phpmyadmin.ts`). T2's first
  step is one `site_url(domain)` that knows the port, and a TEXT test that no other `https://{}` remains.
- **The frontend without Tauri**: the dev and wk-check builds already run with `isTauri()` false
  (`src/lib/ipc/index.ts:41`) against `@/lib/mock`, so a browser renders the shell. What that path does
  not cover is the real transport; T6 still owes it.

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
- **Secrets.** Nothing to do — live sync's pairing is a `0600` file already (see §2, corrected).

### Not in this plan — Redis, MariaDB and Apache on Linux (issue #7 ask 5)

Catalog work in `rexenv/runtimes` and `core/binaries.rs`, under D-L8 of `docs/PLAN-linux-port.md` —
**pinned on 28 Oct 2026** (rexenv's own builds, both arches). Their Linux run is tracked in their own
`docs/TODO.md` row. Headless mode does not depend on them, and they do not depend on headless mode.

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

- **D1 — how a window-less Tauri app runs.** T0 settled (a): it does not exist on Linux (§2a, tao
  panics in `gtk::init`). Recommended now: **(b) for Phase 1a**, because it ran the whole stack on the VM
  with no code change, and (c) later only if Xvfb proves a burden on real servers.
  - (a) ~~The Tauri app with no window created~~ — measured impossible without patching tao.
  - (b) The same under a virtual display (Xvfb). One apt dependency (`xvfb`, ~63 MB RSS), no code change
    to run; `--headless` becomes "start under Xvfb with no window shown".
  - (c) Refactor `commands/` and the socket servers off `AppHandle` onto an app-owned host that Tauri,
    the CLI, the MCP and the web server all hold. This is the cleanest, but it touches 17 command
    files and both servers.
- **D2 — dispatch generation:** a macro that emits both `generate_handler!` and the web table from
  one list (recommended), vs. a build-time check over hand-written arms. **Ruled 28 Oct 2026: the
  macro.** Built in T6 as `lib.rs` `for_each_command!` → `app_invoke_handler` (the desktop's
  `generate_handler!`) and `COMMAND_PATHS` / `command_names()` (the web's table). What the web does
  with a name is not a second dispatcher: it hands the request to the main webview's own
  `on_message`, so the ACL, `State` and argument decoding are the window's.
- **D3 — the other two OSes.** The three-OS rule applies. Recommended: Linux first, because that is
  the VPS, with macOS (LaunchDaemon) and Windows (service / boot-time task) filled in the same change
  as `Error::Unported` plus a sentence in `platform/words.rs`, and a TODO row each. The web transport
  itself is common code and works on all three as soon as their headless mode exists.
- **D4 — secrets without a keyring:** **moot, 26 Oct 2026.** The owner ruled "honest refusal" on the
  question as this plan framed it — and the framing was wrong: live sync never used a keyring; its
  pairing has been a `0600` file under app data on every OS since 10 Oct (`core/live_sync/secrets.rs`,
  `docs/PLAN-wp-live-sync.md` §11 Q4). A refusal would have removed a working feature on headless
  boxes for a problem that does not exist, so none was built; the owner was told.
- **D6 — the edge port setting's shape.** Recommended: global settings (one HTTP port, one HTTPS port,
  one bind address) rather than per site, changeable only while no site exists in the first version.
  The alternative is a full rewrite of every site's URLs on change, as a later task.
- **D7 — consent on a box with no Settings screen.** The MCP opt-in and the agent-access dial are GUI
  consents by rule (ledger #203, #397), and the edge's Stop/refresh need polkit at runtime (§2a).
  Recommended: `sudo rexenv --headless-setup` asks both in the terminal, as full sentences the same Rust
  owns for the toggles, with `--enable-mcp --agent-access <level>` for unattended installs (the
  flags ARE the consent, typed by an admin). Phase 1b's web Settings then carries the same toggles.
  **Ruled 26 Oct 2026, with the privilege half CORRECTED before any code:** as first written here, D7
  also installed "a polkit rule letting the service user run rexenv's own privileged step without an
  agent". That would have been passwordless root for every process of that user, because
  `--privileged-step` runs ANY script (`linux::run_step`), and the user's processes include every site's
  PHP-FPM and every plugin under test. The owner re-ruled it to a CLOSED set: the edge unit's
  `systemctl enable` / `restart` / `disable --now`, three fixed argv in a sudoers drop-in, with every
  other privileged step refused, naming the setup (T1, ledger #911). The consent half (MCP, agent access)
  is T3's.
- **D5 — the web UI's port.** **Ruled 28 Oct 2026: `9443`** (`web_rules::WEB_PORT`, `docs/PORTS.md`). It goes into `docs/PORTS.md` and is port-gated through
  `core/ports::ensure_free` like every other service.

## 6. Tasks

Phase 1a first (T0–T5), then Phase 1b (T6–T9).

- **T0 — measure (no code shipped).** ✓ 26 Oct 2026, the Ubuntu VM — answers in §2a. Was: done when
  every answer is written into §2:
  - Does a Tauri 2 app start on Ubuntu 22.04 with no `DISPLAY`/`WAYLAND_DISPLAY` and no window built?
    Under Xvfb?
  - The full list of `PrivilegeManager` calls on Linux, and of anything else that assumes a desktop
    session.
  - Every place that builds a site URL or assumes port 443 (for T2).
  - What breaks in the frontend with the Tauri internals absent, beyond §2's list. The wk-checks
    already render without Tauri, so start there.
  - Run on the Ubuntu VM (no desktop session), not the dev Mac.
- **T1 — headless mode.** ✓ 26 Oct 2026 — built and run on the Ubuntu VM (ledger #911–#913, SMOKE Linux
  "Headless on a server"). What it took beyond the sketch: the app starts its OWN Xvfb (no `xvfb-run`
  wrapper; `-nolisten tcp`, a cookie, dies with the app); GTK gets NO session bus, because with one it
  waited ~25 s on xdg-desktop-portal's missing GNOME backend (31 s → 1 s); the passwordless set is
  `sudo -n` + a sudoers drop-in, not polkit (D7 as corrected); a Linux edge start with identical root
  files is a restart, or `rex start` after `rex stop` would have needed the install's copy; the
  hand-off note and `rex open` learned the holder can be headless. Was: `--headless` boots the app with
  no window, tray or menu (under Xvfb per D1).
  Done when `rex status`, `rex site create`, `rex start/stop` work against it on the VM over SSH with no
  desktop session, a second launch hands off to it (ledger #441), and `rex stop` no longer needs a polkit
  agent (D7's rule).
- **T2 — configurable edge ports and bind address (D6).** Done when, on the VM with a foreign nginx
  holding `:80`/`:443`, the stack starts on `8080`/`8443`, a WordPress site and a subdirectory multisite
  load with the port in their URLs, login cookies hold, and the HTTP→HTTPS redirect keeps the port. Also
  run on macOS and Windows, because the setting is common code.

  **T2 design (D6 ruled 26 Oct 2026: global, changeable only while no site exists).** Read from the tree
  the same day:
  - Most of the plumbing exists, unused. `ServiceManager` already holds `Ports { http, https, nginx }`,
    `CaddyConfig` already renders `http_port`/`https_port`/`default_bind`, and an unprivileged port
    already takes the CHILD edge path (`prepare_edge`: `privileged = port_needs_privilege(https)`).
    That path is Windows' production edge today: a user process, adopted after a relaunch through its
    admin socket, stopped through the admin API, with no privilege anywhere. So on macOS and Linux an
    edge on `8443` needs no root unit and no sudo at all — issue #7's server needs neither.
  - What is missing: the setting itself; `Ports` filled from it instead of `Ports::default()`;
    `default_bind` overridable (today only the platform answers it); the runtime readers that still
    name `DEFAULT_HTTPS_PORT` (the port gate's table `core/ports.rs:191`, the conflict help in
    `commands/{services,system,site_provision,valet_import}.rs` and `cli_server.rs`'s doctor,
    `core/dbconsole_proxy.rs:93`, `core/php.rs:1761`); and one `site_url(domain)` that knows the
    port, replacing `https://{domain}` in 16 Rust and 5 TSX files (§2a).
  - **The edge's kind follows the port.** A privileged port keeps the root unit; an unprivileged one
    gets the child edge. A change takes effect at the next Start all, through Caddy's reload (it
    rebinds), so the running edge never has to know. Two transitions need care: a root daemon left
    installed when the port moves above 1024 (adoption would still call it `Daemon`), and a child
    edge when the port moves below 1024 (it cannot bind it). The first version stops the running edge
    and starts the right kind when Start all finds the kind changed.
  - **Multisite with a port: measure, do not assume.** WordPress used to refuse a network on a
    non-standard port ("You cannot use port numbers such as…"). That check is gone from current core
    (read in `wp-admin/includes/network.php`, 26 Oct 2026), but whether a subdirectory network then
    WORKS with `:8443` in its domain is unknown. T2's VM run answers it, and if it fails, a multisite
    on a non-443 port is refused with a sentence, never created broken.

  Slices, one commit each (**S1 ✓ 26 Oct 2026**, ledger #914/#915; **S2 ✓ the same day**, #916/#917,
  with the URL-rewriting flows REFUSED off 443 as a first-version limit; **S3 ✓ the same day**, #918 —
  most transitions already held through reload + `recover_stale_edge`; the one that did not was an
  installed-but-stopped daemon beside a child on 8443, fixed by keeping an installed daemon the edge
  on any port; **S4 ✓ on Linux the same day** — the Settings card, Onboarding's port, and the VM run
  on a fresh headless account beside a foreign nginx, which found Caddy's redirect dropping the port
  (#919) and a fresh account's missing log folder; **macOS and Windows run 27 Oct 2026** on fresh
  `t2rex` accounts — the same checks and the card, all passing): **S1** the three settings (`edge_http_port`, `edge_https_port`,
  `edge_bind`) as gated keys with validating setters (range, distinct, not a port rexenv already
  uses, refused while any site exists), `Ports` and the bind filled from them at launch (before
  adoption) and at every Start all, and the runtime readers above switched to the configured port.
  **S2** `site_url` + the 21 files, with a TEXT guard that no other `https://{domain}` remains; the
  WordPress install writes the port. **S3** the edge-kind transitions. **S4** the Settings control
  (all three OSes), Onboarding's edge-conflict sentence (it says "port 443" in TSX — the port belongs
  in Rust's answer, `setup_edge_conflict`), and the VM / macOS / Windows runs of the done-when.
- **T3 — `rex mcp` + scratch sites on headless Linux.** **Code half ✓ 26 Oct 2026** (ledger #920):
  `sudo rexenv --headless-setup [--enable-mcp] [--agent-access <read|changes|full>]`; with neither
  flag it asks in the terminal, opening with the card's paragraph — which lived in the TSX and moved
  into `mcp_server::ENABLE_CONSENT` so both doors read one copy — and the levels' `what_it_allows`;
  with no terminal it changes neither. A level above Read is set for always. The write runs in a
  child forked from the root setup that drops to the user first (the database stays theirs, a link
  they planted is not followed as root); there is deliberately no argv mode that writes the keys,
  since an agent can run anything the user can. Run in the cloud session's container (flags, pty,
  ownership, a planted link); **run on the VM 26 Oct 2026** (ledger #920): the consents by flag, pty and no-tty,
  and over `rex mcp` a scratch create → plugin add + `wp_run` activate → the plugin's output → delete, nothing
  left behind. ✓. Was: done when an MCP client over `rex mcp` on the VM
  runs `scratch_create_site` → a plugin install → `scratch_delete_site`, with nothing left behind, and
  `docs/INSTALL.md` documents it.
- **T4 — install + setup.** **Unit + lingering + INSTALL ✓ 26 Oct 2026; run on the VM the same day** (ledger
  #921): `--headless-setup` writes and enables `rexenv-headless.service` (`User=`, `/usr/bin/rexenv
  --headless`, `Restart=on-failure`, `KillMode=process` so the stack outlives the app,
  `XDG_RUNTIME_DIR` + `After=user@<uid>.service` for the DNS agent's user unit) and runs `loginctl
  enable-linger`; it refuses without the `.deb`'s `/usr/bin/rexenv`. INSTALL § Linux "Server
  (headless)". The VM run (#921): restart keeps the stack, `kill -9` is restarted, a hand-launched second
  copy hands off, and a boot with no one logged in brings the service, the user manager, the DNS agent
  and the stack up. The deb's post-install note ✓ (print-only, #922). **A FRESH server ✓ the same day**
  (#923): the setup runs the desktop's own route/trust/edge code as the account and relays its root
  scripts to itself; run on a fresh account with the VM's rexenv state parked. Was open: a FRESH server —
  the edge install, the `.rex` route and the CA trust are root steps the closed set refuses (#911),
  so they need either the setup to do them or a GUI-less first setup. The systemd unit, `--headless-setup`, lingering, the deb's post-install note.
  `docs/INSTALL.md` gets a "Server (headless)" section, and `docs/SMOKE-TEST.md` gets the rows. After T4,
  Phase 1a is shippable, and issue #7's reporter gets a build to test on Ubuntu 24.04 x86_64.
- **T5 — the other OSes per D3.**
- **T6 — web transport.** **Built 28 Oct 2026, run in the cloud container — NOT on the VM** (ledger
  #929): `rexenv --web` serves the embedded bundle on `127.0.0.1:9443` (`web_server.rs`, rules in
  `web_rules.rs`); `POST /ipc/<cmd>` goes through `Webview::on_message` on the hidden main webview,
  only for a name on the one list; `GET /events` is server-sent events (not a WebSocket — the push
  is one-way, and SSE needs no second crate) where a browser `listen(name)` takes a Tauri
  `listen_any` on that name for its stream; the frontend picks the transport from a `<meta>` the
  server injects (`src/lib/ipc/transport.ts`). In the container a headless instance served the
  Sites screen to Chromium with real data, the New site form's create reached the backend and its
  refusal came back as the toast, and an event crossed the stream. **No auth until T7** — the
  branch carries both before it ships. Not done: the SSH-tunnel run on the VM and a site actually
  created (the container cannot download binaries). Was: The `invoke`/`listen` switch, `/ipc` + `/events`, and the generated dispatch
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
