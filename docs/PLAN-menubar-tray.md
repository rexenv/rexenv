# PLAN — the menu-bar app (tray) — rexenv lives in the menu bar, not in the dock

**Status:** planned 31 Aug 2026, not started. Approved shape: **menu-bar only, no dock
icon** (owner's ruling, 31 Aug 2026).

## 1. The problem, stated as the user hit it

`rex` and the MCP server are **remote controls for a running app**. Both are unix
sockets opened by the app process (`cli_server::spawn`, `mcp_server::spawn_if_enabled`
in `lib.rs` setup) and both die with it. The CLI says so in one line:

> `rexenv isn't running — open the rexenv app, then reconnect. No MCP server is
> available until rexenv is running.` — `cli/src/main.rs:33`

That is deliberate and stays. A headless CLI- or MCP-spawned backend is the
second-writer bug class the stack guard exists for (`cli_server.rs:8-10`,
`PLAN-mcp-server.md` §2.3): a second process opening the same SQLite and spawning the
same services means two ServiceManagers disagreeing about what is running, which is the
one thing this codebase has refused to build since Phase 1.

So the fix is not a daemon. **The fix is that closing the window stops being a quit.**
Today it is one:

```rust
// lib.rs:29-38 — closing the window quits the app, which stops live public shares
if let tauri::WindowEvent::CloseRequested { api, .. } = event { … }
```

Services already outlive the app, DNS already outlives the app — the *control plane*
(CLI + MCP) is the one thing that does not, and it does not because the process exits
when the last window closes. A menu-bar item keeps the process, and therefore both
sockets, alive for the whole login session.

## 2. The ruling: menu-bar only

macOS `NSApplication.ActivationPolicy::Accessory` — no dock icon, no app switcher entry,
a status item and nothing else. This is what Herd does and what was asked for.

**What Accessory costs, stated up front because it is not free:**

- **The app menu goes away.** With no dock icon there is no menu bar for the app, so
  `install_about_menu_item` (`lib.rs:1170`) has nowhere to install, **Cmd+Q is gone**,
  and — the one that actually bites — **Cmd+C / Cmd+V / Cmd+Z inside the webview are
  gone**, because on macOS those come from the Edit menu, not from the webview. That
  menu was deliberately preserved once already: the About item was built by *editing*
  the default menu rather than replacing it, precisely so the clipboard shortcuts
  survived. Accessory removes the whole thing.
- **A modal prompt from a non-activated app can hide behind other windows.** The
  privileged prompt already has a foreground requirement (a backgrounded `osascript`
  cannot show its auth dialog at all); an Accessory app must call
  `activate(ignoringOtherApps:)` before any native prompt or the resolver/CA prompt is a
  dialog nobody sees.

**Decision ladder, in order — do not skip the measurement:**

- **A1 (built first, then narrowed — see the two notes below):** always Accessory. Tray
  icon only, no dock icon, ever.
- **Then measure** (Phase A verification, below): open the window, focus the site
  Terminal tab and Adminer, and try Cmd+C / Cmd+V. If the clipboard still works,
  A1 ships.
- **A2 (fallback, one line):** Accessory while the window is hidden, Regular while it is
  visible. The dock icon then exists only while a window is open — still "no dock icon"
  in the state the user cares about (idle, menu-bar-resident) — and the Edit menu comes
  back exactly when there is something to type into.

A2 is written down now, with A1, so that finding the clipboard broken is a *switch*
and not a redesign. **Whichever one ships, the doc records which, and why** — a plan
that lists two options and never says which one is in the binary is the staleness this
project has already paid for twice.

> **Measured 31 Aug 2026 — A1 SHIPS.** On a dev build of this branch, on the owner's
> machine: Cmd+C and Cmd+V still work in the site Terminal tab and in Adminer with no
> application menu present. So the Edit menu is not what the webview's clipboard depends
> on, the cost this section feared is not real, and **A2 was not taken**. It stays
> written down as the fallback if a future macOS changes that. The rest of the ladder
> held too: the tray menu opens, **Open rexenv** brings the window to the FRONT (the
> `activate_app` leg — an accessory app that merely `show`s a window comes up behind
> whatever was in front), and closing the window leaves `rex status` answering.

> **A2 SHIPS AFTER ALL — 1 Sep 2026, and for a reason this section did not anticipate.**
> The ladder above only ever asked whether Accessory costs the clipboard. It does not.
> What it costs is the DOCK: with the window open and no tile, the window cannot be
> Cmd-Tabbed to and reads as a window belonging to no app — the owner hit it the first
> day of use. So the policy now follows the window (`dock_follows_window`): Regular while
> a window is up, Accessory the moment it closes. "No dock icon" still holds in the state
> the ruling is about — idle, menu-bar-resident — and A1's measurement stays true and
> stays here, because it is why the switch is a two-line function instead of the whole
> design. **Ordering is load-bearing on both edges**: show the window BEFORE the switch
> to Accessory and the switch takes it away (measured three times, 31 Aug), so hiding
> sets the policy after the hide and showing sets it before the show. Verified live on
> one process (pid 5602): `lsappinfo` read `Foreground` with the window open and
> `UIElement` after closing it, with the CLI socket answering throughout.

## 3. What the tray must never do

Three rules inherited from the code it will display:

1. **"Running" = ownership AND liveness, never a port-listen.** The tray's status line
   reads a `ServiceManager` snapshot through the same path `commands/services.rs`
   status uses. A tray that probes ports would be a *second* answer to "is it running",
   next to the one the UI shows, and they would disagree.
2. **Never hold the services lock across a wait.** The menu is rebuilt from a
   `try_lock` snapshot. If the lock is busy the menu keeps the last snapshot and shows
   it — a stale-but-labelled menu beats a menu that blocks the menu bar.
3. **Hiding the window stops nothing.** No service, no tunnel, no job. The tray is a
   window control, not a lifecycle control.

## 4. Phases

> **STATUS — all four phases shipped (31 Aug – 1 Sep 2026).** A1–A9, B1–B7, C1–C3 and
> D1–D7 are done, with A7 closed alongside C1 as promised. Ledger rows #436–#441 carry
> the verdicts. Two things arrived after the plan was written and are recorded where they
> happened rather than smoothed away: **the dock now follows the window** (§2's A2, taken
> for a reason the ladder never considered — a tile-less window cannot be Cmd-Tabbed to),
> and **a single-instance guard** (#441), because a menu-bar app made a second copy of
> itself invisible. What is still unrun is named, not assumed: the login-launch legs need
> a logout, and they live in `SMOKE-TEST.md` under "The menu bar".


### Phase A — the process survives the window (this alone closes the user's goal) — ✅ SHIPPED 31 Aug 2026

| # | Task | Done when |
|---|---|---|
| A1 | `Cargo.toml`: `tauri` features `tray-icon` + `image-png` | `cargo build` green, no other change |
| A2 | Menu-bar template icon, **derived from the app icon** by `scripts/make-menubar-icon.py` (alpha only, `icon_as_template(true)`) | ✅ **seen 31 Aug 2026 on a real machine, BOTH bars**: dark glyph on a light bar, white on a dark one |
| A3 | Build the tray in `lib.rs` setup: icon + a menu with **Open rexenv** and **Quit rexenv** | tray visible; both items work |
| A4 | `ActivationPolicy::Accessory` at startup (A1 of §2) | no dock icon, no app-switcher entry |
| A5 | `CloseRequested` → `window.hide()` + `prevent_close()`; **no tunnel prompt on close** (nothing dies) | closing the window leaves `rex status` and `rex mcp` working — the whole point |
| A6 | Real quit = tray **Quit rexenv** only → keeps `confirm_quit_or_prompt` (tunnels DO die there) | quit with a share up still pauses once and names the count |
| A7 | First run must not be invisible: if onboarding is incomplete, show the window on launch | ✅ **closed WITH C1, as promised** (31 Aug 2026): `first_window_decision` — resolver file + per-user CA trust, the two facts `FirstRunGate` routes on, plus the init-failure case |
| A8 | Activate before any native prompt (`activate(ignoringOtherApps:)`) — resolver, CA trust, quit confirm | privileged prompt appears in front, from a hidden-window app |
| A9 | **Measure the Accessory cost**: Cmd+C / Cmd+V in the Terminal tab and in Adminer | ✅ **measured 31 Aug 2026 — the clipboard survives, so A1 stands and A2 is not needed** |

**Two things the build settled, recorded here because the plan had said otherwise:**

- **Clicking the icon opens the MENU, not the window.** The plan said left-click shows
  the window; that is not the shape this feature was asked for (Herd's icon opens a
  quick menu) and it would leave Phase B's menu reachable only by right-click. The
  window is reached through the menu's **Open rexenv**.
- **One icon file, at 2x, not a `@2x` pair.** `tray-icon` normalises whatever it is
  given to an 18pt height (`tray-icon-0.24.1` macos/mod.rs: `let icon_height: f64 =
  18.0`) before handing it to `NSImage`, so the pixel size is only about crispness and
  the canvas only decides the margin — a 20pt canvas holding an 18pt mark draws at
  ~16pt, the size Apple's own status items use. And the icon is **derived from
  `icons/icon.png`'s alpha**, by a stdlib-only generator, rather than hand-drawn: a
  second mark drifts from the first the day the brand changes, and a generator needing
  a toolchain this machine does not have (no PIL, no ImageMagick, no rsvg) is a
  generator that gets replaced by a hand-drawn PNG the first time someone runs it.

**Phase A verdict — SHIPPED 31 Aug 2026, verified on the owner's machine:** window
closed, dock empty (`lsappinfo` reports `type="UIElement"`), tray present in both a
light and a dark menu bar, **Open rexenv** brings the window to the front, `rex status`
answers with the window closed, and the clipboard survives the missing app menu (A9).
The only box left open is A7, deliberately — see its row.

### Phase B — the quick menu — ✅ SHIPPED 31 Aug 2026

The menu is a **pure function** `TrayModel { services, sites, mcp_on } -> MenuSpec` in
`core/tray.rs`; the Tauri half only renders it. That is what makes it L0-testable —
the menu is the part with rules in it, and rendering is not.

| # | Task | Done when |
|---|---|---|
| B1 | `core/tray.rs`: the model + `MenuSpec`, no Tauri types | L0 tests over the state matrix |
| B2 | Status line (disabled item): `Edge · Nginx · PHP 8.3 · MySQL — 4 running`, from the ServiceManager snapshot | matches the Services screen for the same instant |
| B3 | **Start all / Stop all** | same commands the UI calls, no second path |
| B4 | **Sites ›** — recent N, click opens `https://<domain>` via the browser preference (`open_in_browser`) | respects the preferred browser, not LaunchServices |
| B5 | **New site…**, **Mail**, **Databases**, **Services**, **Tunnels** → show window + route | each lands on its route |
| B6 | **MCP: on/off** checkmark bound to `mcp_enabled` | toggling from the tray flips the same setting the Settings screen shows |
| B7 | Rebuild triggers: service-state change + a coalesced ~5s tick, snapshot only | no lock held across a wait (rule 2, §3) |

### Phase C — always-on — ✅ SHIPPED 31 Aug 2026 (the login legs are walked at release)

`AutostartManager` already exists (per-user LaunchAgent `dev.rexenv.rexenv.plist`,
`RunAtLoad`, ARCHITECTURE §Autostart) — this phase does not build autostart, it makes
autostart *quiet*.

| # | Task | Done when |
|---|---|---|
| C1 | **Start hidden in the menu bar** — login launch opens no window (unless A7 applies) | ✅ 31 Aug 2026, #439 — `visible: false` + `--hidden` in the plist; a USER launch shows the window at once, a login launch asks `first_window_decision` |
| C2 | Login rules unchanged: never download, never prompt at login (ledger #175) | ✅ 31 Aug 2026 — `auto_start_services` untouched, #175's guard passes |
| C3 | `rex`: when the socket is missing, *offer* `open -a rexenv` — never auto-spawn | ✅ 31 Aug 2026, #440 — plant-proven |

**What C1 cost, recorded because it is not obvious:** the flag is the easy half; DELIVERING
it is not. A user who enabled autostart before this change has a plist naming an older
binary with no `--hidden` in it, and nothing in the app would ever have looked at it
again — so the plist is now rewritten on every launch while autostart is on, exactly as
the DNS agent's already was. A feature that only works for people who toggle the setting
again after upgrading is a feature that does not work.

### Phase D — docs and proof, in the same commits — ✅ SHIPPED 31 Aug–1 Sep 2026

| # | Task |
|---|---|
| D1 | `ARCHITECTURE.md`: a section on app lifetime — what dies with the app (tunnels, repo jobs) vs what outlives it (services, DNS) vs what is now *kept alive by the tray* (CLI + MCP sockets) |
| D2 | `MAP.md` + README structure tree: `core/tray.rs` and the platform wiring |
| D3 | `CLAIM-LEDGER.md` rows + verdicts, same commit as the code: (i) closing the window stops no service and keeps both sockets; (ii) tray status is a ServiceManager snapshot, never a port probe; (iii) the menu build never holds the services lock |
| D4 | `DESIGN.md`: template-icon rule (monochrome, both menu-bar themes), honest status copy |
| D5 | `TESTING.md`: the L0 menu-model tests — and **the L1 example measured away, not written**. `tray_lifetime_check` needed a way to hide the window from outside the app; the only one would be a `rex` verb invented for the test. See TESTING §5 and #436 |
| D6 | `SMOKE-TEST.md`: the manual legs — tray click, Cmd+C in the webview, login-hidden start, quit-with-a-share |
| D7 | `INSTALL.md`: first-run wording now that there is no dock icon |
| D8 | `TODO.md` ticks with ✓ evidence, per task |

## 5. What is deliberately NOT in scope

- **No headless mode, no daemon, no CLI autostart.** §1. The app is the one writer.
- **No tray-driven service control beyond Start all / Stop all.** Per-service toggles
  belong on the Services screen, where the failure text has room to say why.
- **Tray clicks are not L1-provable on this machine** — no blind synthetic clicks on a
  live desktop. They ride `SMOKE-TEST.md` (D6), stated as a manual leg rather than
  claimed as automated.

## 6. Proof limits, written before the work

`tray_lifetime_check` proves the socket survives a *programmatic* `hide()`. It does not
prove a human clicking the red button reaches the same code path — that is the same
gap `CloseRequested` has always had, and it is a smoke-test leg, not a claim.
