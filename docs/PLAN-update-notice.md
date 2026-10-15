# PLAN — Making an app update impossible to miss (and "Check for Updates" reachable from any screen)

**Status:** T1–T4 BUILT 15 Oct 2026 on branch `claude/update-notice` (not merged): the `app-update`
push (ledger #884), the TopBar + sidebar pills (#885), the tray "Check for Updates…" (#535
widened), docs + SMOKE rows. §7 resolved as recommended (toast "Show update"; icon-only idle
control; rail pill kept — on its own line above Settings, since beside the label it overlapped
it at 220 px). **Proven:** L0 tests (plant-proven), `verify.sh` green on a Linux sandbox
(windows-check SKIPPED; macOS not compiled there), and `appupdate.js` run + plant-proven in
**Chromium only**. **Owed:** the WebKit run (`verify-full.sh` on the Mac) and every §6 row below
on its OS — none has run.
**Builds on:** `docs/archive/PLAN-self-update.md` (the update machinery, D3 "how loud"),
`docs/archive/PLAN-menubar-tray.md` (the tray's rules, ledger #534).

## 0. The complaint, in one line

The only always-visible sign of a new release is a bare mono `0.6.0` beside **Settings** in
the sidebar. Users read it as "this is the app's version", not "a newer version exists". And
"Check for Updates…" lives only in the macOS app menu and under Settings → About — nothing in
the app's own chrome, and nothing at all on Windows/Linux, which have no native app menu.
(This plan's references to "#534" for the tray rule mean ledger **#535** — #534 is the About
card's copy row; corrected 15 Oct 2026 when T3 widened #535.)

## 1. What exists today (read, not remembered — 15 Oct 2026)

| Surface | Where | Behaviour | Problem |
|---|---|---|---|
| Sidebar badge | `src/components/shell/Sidebar.tsx:103-114` | `liveBadges["/settings"] = update?.offered?.version` → mono `0.6.0`, same style as the Sites/Services counts | Reads as a version label; no verb, no colour, no affordance; clicking Settings lands on **General**, not About |
| Toast | `src/App.tsx:161` `UpdateWatch` | `toast.info("rexenv X can be installed", {label: "Open About"})`, once per version per run, 10 s | Fine, but 10 s and gone; label says a place, not an action |
| Tray item | `core/tray.rs` `TrayAction::UpdateTo(v)` → "Update to X" | Shows window on About (never installs, #534) | Only when an offer exists; **no "Check for Updates…" in the tray** |
| App menu | `lib.rs:2583` `install_about_menu_item` (`cfg(macos)`) | "Check for Updates…" under About → shows window, emits `menu://check-updates` → `CheckUpdatesMenuWatch` navigates `/settings?section=about` + invalidates | **macOS only.** Windows/Linux get no menu (GTK has no app menu, `PLAN-linux-port.md` §1.1; Windows draws no menu bar) |
| About card | `src/components/settings/AppUpdateCard.tsx` | The one place that installs; consent/refusal sentences from Rust | Correct; stays the only installer |
| TopBar | `src/components/shell/TopBar.tsx` | title / search / per-route action | Nothing about updates |

**A second defect found while reading, which the owner did not ask about but the plan must fix
or the new surfaces will lie:** `check_for_app_update` (`lib.rs:1871`, run by the launch sweep and
the 6 h poller) stores the result and logs it, but **emits no event to the window**. The Sidebar
and `UpdateWatch` read `["app-update"]` with `staleTime: Infinity` and no refetch; only the About
card's observer (default staleTime) refetches, and only while About is mounted. So with the window
open, a release found by the poller reaches the badge and the toast **only after the user opens
About** — the badge was telling people about the previous session's check. Any new always-visible
surface inherits this unless the backend pushes.

## 2. Design

### 2.1 Principles (DESIGN.md rules this must keep)

- **Honest UI.** A surface renders only a fact the backend holds: `offered` (verified descriptor,
  compared live), `installedPending`, `skipped`. Nothing says "up to date" (unprovable; the card's
  own rule). Nothing renders when `enabled` is false (no pinned key — dev builds).
- **One installer.** Every new surface is a *door to About*; none installs, none fetches. The tray
  keeps #534 (snapshot only, never fetches, never locks).
- **Rule sentences live in Rust.** The new surfaces use *labels* ("Update to 0.6.0", "Check for
  Updates…", "Restart to finish"), not consent/refusal sentences — `copy_scan` stays untouched.
- **Mono for technical values** (the version), Inter for the verb.
- **Common once, per-OS where it differs.** The tray spec is `core/` → one change covers three
  OSes. The in-app chrome is React → one change covers three. The only per-OS fact is the native
  menu, already handled: macOS has it, the others don't, and the in-app control is the equaliser.

### 2.2 The new surfaces

**S1 — TopBar update control (every screen, all three OSes).** A fixed slot at the right of
`TopBar`'s action cluster, rendered by `TopBar` itself (not per route, so no route changes):

| State (from `["app-update"]` + readiness) | Render | Click |
|---|---|---|
| `!enabled` | nothing | — |
| idle (no offer) | icon-only ghost button (lucide `RefreshCw` or `ArrowDownToLine`), `title="Check for Updates…"` | `useCheckForUpdates()`: navigate to About **and** invalidate (same as the menu item; one hook, three callers) |
| checking (card's mutation pending) | spinner in the same slot | disabled |
| `offered` (phase offered/refused) | brand-tint pill: `↑ Update to` + mono `0.6.0` | navigate `/settings?section=about` |
| `offered` && `skipped === offered.version` | back to idle icon (the user set it aside; the card's Undo is where that lives) | as idle |
| `installedPending` | pill, neutral-strong tokens: `Restart to finish` + mono `0.6.0` | navigate to About (the card explains quit-and-reopen) |

Width-safe at the narrowest window (`min-w-0`, the pill truncates the label, never the version).

**S2 — Sidebar badge becomes a pill, and it goes to About.** Replace the bare version with the
same `UpdatePill` component in compact form: brand dot + `Update` (no version — S1 names it; a
220 px rail has no room for both and the version alone is what confused people). Rendered as a
**sibling** of the Settings `NavLink`, not inside it (a link inside a link is invalid HTML and
WKWebView makes the inner one dead); its `to` is `/settings?section=about`. Settings itself keeps
going to General — someone opening Settings for the TLD must not be hijacked.

**S3 — Tray gains "Check for Updates…" (all three OSes).** New `TrayAction::CheckUpdates`
(`tray:check-updates`), placed next to "Update to X" / where it would be, enabled always (even
when `enabled`-false? **No** — when `TrayModel.update_enabled` is false the item is omitted; a dev
build has nothing to check against). Handler: show + focus window, emit `menu://check-updates` —
the **same event** the macOS menu emits, so `CheckUpdatesMenuWatch` is the one place that decides
what "the user asked" does. The tray still fetches nothing (#534 unchanged; the must-not scan gains
nothing because the handler calls no `app_update_*`).

**S4 — The backend pushes.** After a successful `accept` in `check_for_app_update` (and after
`app_update_check`/`_skip`/`_apply` already return fresh state, so only the background path
needs it): `app.emit("app-update", ())` + `refresh_tray`. Frontend: one listener in `App.tsx`
(beside `CheckUpdatesMenuWatch`) that invalidates `["app-update"]` and
`["app-update-readiness"]`. Sidebar/TopBar/UpdateWatch keep `staleTime: Infinity` — they are
told, they do not poll. **Without S4, S1–S3 show yesterday's answer; S4 is not optional.**

**S5 — Toast label.** `"Open About"` → `"Show update"`. Same behaviour. (Owner may prefer
`"Install…"`; the toast must then still only *navigate* — the consent sentence is on the card.)

### 2.3 Rejected, and why

- **A full-width banner across the main area.** Loud, and the `Accessory` policy + Sparkle's
  convention (D3) argue against windows that shout; S1's pill is on every screen already.
- **Hijacking the Settings nav to About while an offer exists.** Punishes everyone who wanted
  General.
- **Showing the version in the sidebar pill.** That is the thing that misread.
- **A native menu bar on Windows/Linux.** Windows draws its own title bar above the page and a
  Tauri menu bar there would be the only app on the machine with one; GTK has no app menu.
- **TopBar control running the check itself.** A second place deciding what a failed check looks
  like (the card owns that mutation and its copy — `CheckUpdatesMenuWatch`'s own comment).

## 3. Files

| File | Change |
|---|---|
| `src/components/shell/UpdatePill.tsx` **(new)** | `UpdatePill({variant: "bar" \| "rail"})` — reads `["app-update"]` + `["app-update-readiness"]` from cache, renders the S1/S2 table, `data-probe="update-pill"` with `data-phase` |
| `src/components/shell/TopBar.tsx` | mount `<UpdatePill variant="bar" />` after `{action}`; idle = the check button |
| `src/components/shell/Sidebar.tsx` | drop `"/settings": { badge: … }`; footer row renders `NavButton` + sibling `<UpdatePill variant="rail" />` |
| `src/lib/useCheckForUpdates.ts` **(new)** | the navigate-and-invalidate pair; used by `CheckUpdatesMenuWatch`, S1 idle button |
| `src/App.tsx` | `AppUpdateEventWatch` (S4 listener); `UpdateWatch` label (S5) |
| `src/lib/ipc/index.ts` | `onAppUpdate(cb)` listener wrapper for the `app-update` event |
| `src-tauri/src/lib.rs` | emit `app-update` + `refresh_tray` after accept in `check_for_app_update`; tray handler arm for `TrayAction::CheckUpdates` (show, focus, emit `menu://check-updates`) |
| `src-tauri/src/core/tray.rs` | `TrayAction::CheckUpdates`, id `tray:check-updates`, `build()` places it, `TrayModel.update_enabled: bool`; tests: id round-trip, omitted when disabled, `same_shape` unaffected by toggling it |
| `src/lib/mock.ts`, `src/routes/DevUiReview.tsx` | pill in every phase for the WebKit review view |
| `scripts/wk-checks/appupdate.js` | asserts the pill's `data-phase` per mocked state; asserts the rail pill links to `?section=about`; asserts no `<a>` inside `<a>` |

## 4. Tasks (each its own commit, verify green before each)

- **T1 — S4 backend push + listener.** Done when: a mocked background check with the window open
  flips the Sidebar within one tick (L2: `appupdate.js` drives the event); `refresh_tray` is
  called from the same spot (L0 source test beside the existing "exactly two call sites" test).
- **T2 — `UpdatePill` + TopBar + Sidebar.** Done when: tsc/eslint green; `DevUiReview` shows all
  six states; wk-check passes; the Sidebar never renders a bare version.
- **T3 — tray "Check for Updates…".** Done when: tray tests green; the must-not scan still passes;
  clicking it on each OS lands on About with the footer timestamp refreshed.
- **T4 — docs + SMOKE rows + ledger.** See §5.

## 5. Docs touched (same commits)

- `docs/ARCHITECTURE.md` — the self-update "other surfaces" paragraph: pill (bar + rail), tray
  Check item, the `app-update` push; **correct** the sentence that currently says the badge shows
  the version.
- `docs/DESIGN.md` — new rule under honest UI: *a bare value is not a badge* — the sidebar `0.6.0`
  was read as the running version by the people it was meant to alert (owner, 15 Oct 2026); a
  notice needs a verb or an icon that says what the value is for.
- `docs/SMOKE-TEST.md` — §"In-app self-update": the pill on a screen that is not Settings, the
  tray Check item; the **Windows and Linux sections** say the tray item is their only
  menu-equivalent (no app menu) and must be pressed there.
- `docs/CLAIM-LEDGER.md` — #534 widened (the tray's Check item emits an event, calls no
  `app_update_*`); new row: *the TopBar/Sidebar pills read the cache and never run a check*
  (L0 source test: `UpdatePill.tsx` imports no `appUpdateCheck`/`appUpdateApply`).
- `docs/MAP.md` — `UpdatePill.tsx`, `useCheckForUpdates.ts`.
- `docs/archive/PLAN-self-update.md` D3 — one line pointing here (the badge-as-version lesson).
- `docs/TODO.md` — one row for this plan; `./scripts/status.py --write`.

## 6. Proof per OS

| OS | Host | What is pressed |
|---|---|---|
| macOS | 15.8 UTM VM, release build — **owed: run on macOS VM** | pill from Sites; tray Check; app-menu Check still works; S4: leave the window open across a forced poll (test-pinned manifest) |
| Windows | Dell (Win10 22H2), `tauri build` installer — **owed: run on Dell** | pill; **tray Check is the only menu path** — press it; title-bar-above-content layout leaves the pill visible |
| Linux | Ubuntu 22.04 UTM VM, deb — **owed: run on Ubuntu VM** | pill; tray Check via libappindicator (plain items, no icons expected) |

A row proven on one OS only is `◐`.

## 7. Open questions for the owner

1. Toast action label: `"Show update"` (navigate only — recommended) or `"Install…"` (still only
   navigates; the word promises more than the click does)?
2. Idle TopBar control: icon-only (recommended, quiet on eight screens) or a text button
   "Check for updates"?
3. Should the sidebar rail pill exist at all once S1 is on every TopBar? Recommended yes: the
   rail is the one thing visible when a route hides its TopBar (none do today).
