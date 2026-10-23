# PLAN — One update channel: every "something newer exists" is noticed the same way and installed from one place

Status: **BUILT — T0–T7 landed 23 Oct 2026 (ledger #901–#907), every leg L0 + WebKit on the dev Mac, Windows and Linux compiled; OWED: the three SMOKE rows live on each OS (they need a published manifest naming a newer PHP patch than the pin, i.e. the first release after this); the Adminer kind ran on Windows on the 0.9.1 release build (24 Oct 2026), D1 built the same day (ledger #908).** Owner ask (23 Oct
2026): "I want every kind of update to notify the same way — Adminer, PHP, the app itself — so
the user notices them all alike and updates quickly." §8's three questions were answered by the
owner the same day, all three as recommended.

Builds on: `docs/archive/PLAN-self-update.md` (the app's signed channel, D3 "how loud"),
`docs/archive/PLAN-binary-updates.md` (the PHP/Adminer/phpMyAdmin manifest), `docs/archive/
PLAN-update-notice.md` (the pills, the toast, the tray item — ledger #884/#885). Those rules
are kept, not reopened; this plan widens their SURFACE from one family to all of them.

---

## 0. The complaint, in one line

A new rexenv release is impossible to miss (toast, two pills, a tray item). A new PHP patch,
Adminer or phpMyAdmin is impossible to NOTICE: the offer renders only when its own screen is
open — Settings → PHP for PHP, Databases for the consoles — and nothing polls for it between
launches. Three families, three cards, three vocabularies ("Install rexenv 0.6.0" / "8.2.32"
button / "Update to 6.1.2"), and a user has to visit three screens to learn what is pending.

## 1. What exists today (read, not remembered — 23 Oct 2026)

| Family | Manifest + check cadence | Where the offer shows | Install | Skip |
|---|---|---|---|---|
| **App** | `app-manifest*.json` + `.sig`; at launch and every 6 h (`lib.rs` `spawn_app_update_poller`), gated by `app_update_auto_check`; emits `app-update` | toast "rexenv X can be installed" → Show update; TopBar pill `↑ Update to X`; sidebar pill `Update ready`; tray "Update to X…"; `rex status` line; `stack_status.app_update` (MCP, read-only) | Settings → About card only: consent sentence from Rust, Install, restart prompt | `app_update_skipped` (a version) |
| **PHP patch** | `manifest.json` (families Php/Adminer/PhpMyAdmin), at launch + when Settings → PHP or Databases mounts (5-min stale); **no poller, no event, ignores the auto-check setting** | Settings → PHP row button with the patch number; "· X exists" chip from php.net (a fact, not an offer) | `php_update_apply` — download, swap the pool, revert if it does not come up; toast says the MEASURED effect | none |
| **Adminer** | same manifest, same cadence | Databases → `AdminerVersionCard` "Update to X" | `adminer_update_apply` — download, probe the login gate + frame protections, restage | none |
| **phpMyAdmin** | same | Databases → `DbConsoleCard` "Update to X" | `phpmyadmin_update_apply` — download, verify, restage | none |
| DB engines | pinned sets, change only with an app release | picker only | — | — |
| Other binaries (caddy, nginx, mysql, wp-cli, composer, mailpit, cloudflared…) | compiled-in pins; `launch_refetch_stack` re-downloads re-pinned ones at launch | nothing | app release | — |
| WordPress core/plugins/themes | per site, wordpress.org, when the site's WordPress panel opens | that panel's amber "Updates" tab; `rex wp … list` | per row | — |

Infrastructure: `src/lib/toast.ts` (zustand, kinds error/success/info, ≤5 stacked); the
startup-notice queue (`commands/system.rs`, drained once by `StartupNoticeHost`); backend →
frontend events: only `app-update` and `menu://check-updates`. **One bug found on the way:**
every `warn` startup notice gets an "Open Tunnels" action (`App.tsx` ~263), including the
self-update's "did not take" warning.

## 2. Rules already decided, kept here (do not relitigate)

- P1 **Own signed channel.** Two documents, one `RELEASE_PUBKEY`, `updates::verify_signed_bytes`
  is the ONE signature check; each document carries its own monotonic serial; re-verified on
  every read; manifests are committed files on `rexenv/runtimes` main, never release assets (#368).
- P2 **Families are an allowlist** (Php, Adminer, PhpMyAdmin); the app is NOT a family — its
  grant is strictly larger. Compiled-in pins are a floor; a manifest only moves forward.
- P3 **Offers are computed live**, never a stored flag. Skip is stored as a version so a later
  release still offers itself.
- P4 **Nothing installs without the user.** The notice surfaces (pills, toast, tray) are DOORS,
  never installers (#884, #885). Auto-check is a setting read before any I/O; absent = on.
- P5 **Sentences that describe a rule live in Rust** (consent, restart, refusal — `copy_scan`),
  and **a toast may only write the effect the backend measured** (DESIGN: "Saved" ≠ "now running").
- P6 **MCP cannot install the app** (`Never(UPDATE_FLOW)`, #530: it replaces the process that
  enforces the agent dial); PHP/console installs stay at the `system` level.

## 3. The design — one inventory, one poller, one notice, one page

### 3.1 Common (once, `core/` + the frontend)

**`core/update_hub.rs` — the inventory.** One pure function over the two cached manifests and
the installed facts:

```rust
pub enum UpdateKind { App, Php { minor: String }, Adminer, PhpMyAdmin }
pub struct UpdateOffer {
    pub kind: UpdateKind,
    pub installed: String,        // "8.3.27", "6.1.1", "0.9.0"
    pub offered: String,          // "8.3.28"
    pub size_bytes: Option<u64>,
    pub consent: String,          // the family's sentence, from Rust (P5)
    pub effect: EffectSentence,   // what install will DO: "restarts the 8.3 pool", "restages the console", "restarts rexenv"
    pub skipped: bool,            // this (kind, offered) was set aside
    pub needs_restart_of_app: bool, // App only — ordered LAST by Update all
}
pub struct UpdateInventory { pub offers: Vec<UpdateOffer>, pub checked_at: Option<i64>, pub reachable: bool }
pub fn inventory(conn) -> UpdateInventory   // live (P3): app_update::current_offer + updates::cached().newer_than(each family) + skip keys
```

Skip becomes per family: `update_skipped:<kind>` = version (the app's `app_update_skipped`
is read as `update_skipped:app` through a one-line alias, no migration of the value).

**One poller.** `spawn_app_update_poller` becomes `spawn_update_poller`: the same 6 h tick,
the same `app_update_auto_check` gate (renamed in the UI copy only: "Check for updates
automatically" — the key stays, `rex config` whitelist unchanged), and it now fetches BOTH
documents (`app_update::fetch` + `updates::fetch`/`accept`). After either document moves, it
emits ONE event `updates-changed` (the existing `app-update` event keeps firing for one release
so an old frontend bundle after a swap is not deaf — then it goes). The per-screen refetches in
Settings → PHP and Databases stay as "Check now" presses only; they stop fetching on mount.

**One notice.** `UpdatePill.tsx` reads the inventory instead of the app state:
- one offer → `↑ Update to <thing> <version>` where thing ∈ {rexenv, PHP 8.3, Adminer, phpMyAdmin};
- several → `↑ 3 updates`; rail: `Updates ready`; app installed-pending wins the slot:
  `Restart to finish <v>` (unchanged).
- The toast (`UpdateWatch`) fires once per (kind, version) per run: "PHP 8.3.28 can be
  installed" / "3 updates can be installed" → **Show updates**. Same kind, same lifetime.
- The tray item becomes "Updates (3)…" / "Update to PHP 8.3.28…", one item, opens the page.
- `rex status` prints one line per offer; `stack_status.updates` (MCP, read-only) is the
  inventory. The old `app_update` field stays for one release.

**One page.** Settings → **Updates** (the About card's update half moves here; About keeps
version + licences). One row per offer, the SAME shape for all four kinds:

```
[icon] PHP 8.3        8.3.27 → 8.3.28 · 31 MB     <consent sentence>        [Install]  Skip
[icon] Adminer        6.1.1  → 6.1.2  · 0.5 MB    <consent sentence>        [Install]  Skip
[icon] rexenv         0.9.0  → 0.9.1  · 29.9 MB   <consent sentence>        [Install]  Skip
                                                              [Update all (3)]
Check for updates automatically ☑ · Release list from rexenv, checked 2h ago · [Check now]
```

**Update all = quick update.** Sequential, in this order: PHP patches (each pool swap verified
or reverted, as today), Adminer, phpMyAdmin, then the app LAST — because its install ends in a
restart prompt and everything before it must have reported its measured effect first. The
first failure STOPS the chain and leaves the row red with the backend's sentence; nothing after
it starts (the bar freezes where the work stopped — the `importbar.js` rule). Progress is one
`UpdateJob` state the page polls, shaped like `DbImportJobState` (phases per offer, a
`running` cursor, `settled` rows), with the same honesty: a row says `done` only when the
backend returned the effect it measured.

Every single-row Install stays exactly what it is today (the same four `*_apply` commands);
the page is a new front over the existing backs, not a new installer.

### 3.2 Per OS (one answer each; all three filled in the same change)

| Difference | macOS | Windows | Linux |
|---|---|---|---|
| App install step inside Update all | bundle swap + relaunch (as today) | NSIS `/S` + relaunch (as today) | `.deb` under polkit / AppImage exchange (as today); an **apt-installed** rexenv's row says "update with apt" and has no Install button (the row is refused, not hidden) |
| Where "Check for updates…" lives | app menu + tray | tray only | tray only |
| Tray item text | `PlatformWords` — one field, three constants (the tray draws differently per OS) | same | same |
| The notice pills / page | common React | same | same |

PHP, Adminer and phpMyAdmin installs are already per-OS inside `binaries::manifest` arms; the
hub adds no OS fact. If a per-OS refusal sentence is needed (apt row), it is a `PlatformWords`
field, never inline.

### 3.3 What is NOT in scope

- **Auto-install.** Never (P4). The "quick" is one button, not zero.
- **Other pinned binaries** (caddy, nginx, mysql, wp-cli…): they move with the app release and
  stay out of the allowlist (P2); the app's own row covers them.
- **WordPress core/plugin/theme updates**: a different trust root (wordpress.org), a different
  blast radius (a site, not the stack), already their own panel. §8 Q1 asks whether the page
  should show a read-only line per site ("ab12.rex: 3 plugin updates → open"); the default here
  is NO row in Update all, ever.

## 4. Tasks

| # | Task | Proof |
|---|---|---|
| **T0** | Fix the startup-notice bug: the "Open Tunnels" action attaches only to tunnel notices (give `startup_notice` an optional `action` enum in Rust; the frontend renders what it is sent) | L0 test on the notice struct; L2 `uireview.js` scenario with a non-tunnel warn → no button |
| **T1** | `core/update_hub.rs`: `inventory()`, `UpdateKind`, per-kind skip keys with the app alias; `update_skip`/`update_unskip` commands | L0: live computation (a stored flag planted → red); skip of one kind leaves the others; the app's old key read through the alias |
| **T2** | One poller: fetch both documents, emit `updates-changed`; the PHP/console screens stop fetching on mount; `updates_check_now` command (6 s interactive deadline, both documents) | L0 source-order scan (`auto_check` read before any I/O for BOTH fetches); example `update_hub_check` (network tier) against the real manifests |
| **T3** | `UpdatePill` + toast + tray over the inventory (`PlatformWords` tray field ×3) | L2 `appupdate.js` widened: one-offer / many-offer / installed-pending states; tray text unit-tested per platform |
| **T4** | Settings → Updates page: rows, per-row Install (existing commands), Skip/Undo, auto-check toggle, footer | L2 scenarios per row kind + the skipped row; `copy_scan` for every sentence |
| **T5** | `UpdateJob` + Update all (sequential, app last, stop-on-first-failure, measured effects) | L0: order + stop rule (plant: the app not last → red; a failure that lets the next start → red); L2 `updateall.js` driven through a scripted job |
| **T6** | `rex status` lines, `rex updates` (list; `apply <kind>` for php/adminer/phpmyadmin only — the app stays Settings-only), MCP `stack_status.updates` + a read-only `updates` tool; parity rows | `cli` tests; MCP parity test (`app` kind still `Never`) |
| **T7** | Docs: ARCHITECTURE (the update paragraphs become one section), DESIGN (one vocabulary for offers), PLATFORMS §3 row, INSTALL "Updating" per OS, SMOKE-TEST rows per OS, CLI-ROADMAP, ledger rows, PUBLISH-TESTING §M widened to all four kinds | `verify.sh` all green incl. both compile gates; the GUI rows run on the dev Mac, the Win11 VM and the Ubuntu VM |

Order: T0 → T1 → T2 → T3 → T4 → T5 → T6 → T7. T3 and T4 can go in one commit; T5 is its own.

## 5. Ledger rows this will add

- "The inventory is computed live from the two signed manifests and the installed facts — never a stored offer" (extends #52x's rule to every kind).
- "Update all installs the app LAST and stops at the first failure; no row says done without a measured effect."
- "Both manifest fetches are gated by the same auto-check setting, read before any I/O."
- "The notice surfaces (pill, toast, tray, `rex status`) are doors to Settings → Updates; none of them installs or checks" (#884/#885 widened).
- "A `warn` startup notice carries an action only when the backend attached one."

## 6. Risks, named

- **A frontend swapped under a running app.** After an app install the OLD bundle's JS keeps
  running until restart; it must not break on the new event name. Hence `app-update` keeps
  firing for one release.
- **Update all across a pool swap.** `php_update_apply` restarts a pool; a site answering 503
  for a second is expected and already how the single button behaves. The job's row must say
  "restarted the 8.3 pool" or "nothing was running to restart" — the measured effect, never
  "is now on".
- **The apt-installed Linux app.** The in-app installer must not fight dpkg's record; the row
  is refused with the apt sentence (already the self-update's rule, #745) — the hub just has to
  carry the refusal instead of hiding the row.
- **Toast fatigue.** Once per (kind, version) per RUN, as today for the app — a run that sees
  three offers shows ONE toast ("3 updates can be installed"), not three.

## 7. Done when

A user on 0.9.1 with PHP 8.3.27, Adminer 6.1.1 and phpMyAdmin 5.2.2 installed, and a runtimes
manifest naming 8.3.28, 6.1.2 and 5.2.3, sees within 6 h (or at next launch) ONE toast "3
updates can be installed → Show updates", the pill `↑ 3 updates`, the tray "Updates (3)…";
opens Settings → Updates, reads three rows in one shape, presses **Update all**, watches the
three rows settle with their measured effects, and the pill goes away. The same on macOS,
Windows (Win11 VM) and Linux (Ubuntu VM), with screenshots in SMOKE-TEST. `rex status` and
`stack_status` tell the same three before, none after.

## 8. Owner decisions (asked and answered 23 Oct 2026)

- **D1 — WordPress updates on the page: YES, read-only.** One line per site ("ab12.rex: 3
  plugin updates → open the site"), behind the existing two-pass check so no extra
  wordpress.org traffic; never in Update all.
- **D2 — Update all includes the app: YES, last**, with its own restart prompt as today.
- **D3 — The page is Settings → Updates**, a new section; About keeps version and licences.

## 9. Build log

- **T0 — 23 Oct 2026.** `StartupNotice.action: Option<NoticeAction>` (label + route), `push_with`;
  the two tunnel sweeps attach `NoticeAction::TUNNELS`, the self-update's launch WARN attaches
  `NoticeAction::ABOUT`, the info none. `StartupNoticeHost` moved to its own file and renders the
  door it is sent. L0 planted; L2 `uireview.js` `notices` planted (the probe finds the three
  toasts by text — the shell toasts other things in the harness, and a count read 5). Ledger #901.
- **T1 — 23 Oct 2026.** `core/update_hub.rs`: `UpdateKind` (app · php{minor} · adminer · phpMyAdmin),
  `UpdateOffer` (installed → offered, size, Rust consent + effect sentences, `skipped`),
  `UpdateInventory` (offers in Update-all order, `restart_pending`, `checked_at`, `auto_check`,
  `pending()` for the pill count), `inventory(conn, &Facts)` pure over the four families' own facts,
  per-kind skip keys `update_skipped:<kind>` with the app's read through `core::app_update`'s key.
  Commands `updates_inventory` / `updates_skip` + TS types. The IPC wrappers wait for their first
  caller (T3): `every_ipc_wrapper_is_actually_called` refused them uncalled, which is the guard
  doing its job. MCP parity: `updates_inventory` is a `Gap` until T6, `updates_skip` `Never`.
  Four L0 tests, two plants. Ledger #902. Nothing renders it yet (T3/T4).
- **T2 — 23 Oct 2026.** `lib.rs`: the launch sweep reads `auto_check_enabled` ONCE and then fetches
  both documents (`check_for_runtime_updates` is the PHP/console manifest on the app check's
  contract: fetch unlocked, accept under one lock, `update_hub::store_checked`, tell the window);
  `spawn_app_update_poller` → `spawn_update_poller`, both documents per tick; `UPDATES_EVENT =
  "updates-changed"` emitted after either acceptance (`app-update` kept for one release). Command
  `updates_check_now` (both on the 6 s deadline; `check_problem` on the inventory; `updates::
  fetch_interactive`). Frontend: `onUpdatesChanged` → `AppUpdateEventWatch` invalidates
  `["updates"]`, `["php-versions"]`, `["adminer-status"]`, `["db-console"]`. The three screens'
  mount-time fetches stay until T4 replaces them (their wrappers would otherwise be uncalled).
  Scan test widened to both checks, planted. Ledger #903.
- **T3 — 23 Oct 2026.** `UpdatePill` reads the ONE inventory beside the app state: one pending
  offer by its short `display` ("Update to PHP 8.3.28"; the app keeps its bare version), several as
  "3 updates" (count in mono, `data-count`), rail "Updates ready"; `UpdateWatch` toasts once per SET
  of offers ("PHP 8.3.28 can be installed" / "3 updates can be installed → Show updates"); the tray
  item is `TrayUpdate::One(display)` / `Many(n)` over `update_hub::snapshot()` — published by the
  background checks, the command and a skip (`publish`), never read from the database while drawing
  (the lib.rs scan now demands the hub's snapshot and bans `gather`/`inventory` there). `gather()`
  moved fact-gathering into core so the command, the checks and the tray compute one list. Found
  on the way: "Update to PHP 8.3 8.3.28" overflowed the bar by 11 px → one `display` per offer and
  the TopBar search floor 100 → 80 px, with a fit leg in `appupdate.js`. IPC `updatesInventory`
  lands with its first callers. The tray item and the pills still open About (`UPDATES_ROUTE`, one
  constant) until T4. Ledger #904.
- **T4 — 23 Oct 2026.** `src/components/settings/UpdatesSection.tsx` replaces `AppUpdateCard`:
  Settings gains an **Updates** section (About keeps version + licences); one row per offer in one
  shape (label · `installed → offered · size` · Rust consent · **Install <display>** titled with the
  effect · **Skip this version**; skipped → "was set aside … Undo"), the app row keeping its refusal,
  download progress and restart states under the same `data-probe="app-update"`/`data-phase`; one
  auto-check switch; one footer (`check_problem` / `check_refusal` / "checked N ago" / "Not checked
  yet"); "Check now" = `updates_check_now` under the pill's mutation key; `check=1` handled here.
  Every door points at `/settings?section=updates` (`ABOUT_PATH`'s value, `UPDATES_ROUTE`,
  `NoticeAction::UPDATES`). The three family screens stop fetching the manifest on mount; their
  check wrappers and the card-only `appUpdateCheck`/`appUpdateSkip` wrappers are deleted
  (`every_ipc_wrapper_is_actually_called`). The copy scans follow the file. Found: the About card's
  passive `["downloads"]` read logged a "no queryFn" error on every render. D1 (read-only WordPress
  lines) deferred to after T5 — it needs a per-site update count the two-pass check only produces
  when a site's panel is open. Ledger #905.
- **T5 — 23 Oct 2026.** `core::update_hub::{plan_all, RunState}` own the order and the bookkeeping
  (one row at a time, the app last, the first failure → later rows `notReached`, `pct` frozen);
  `commands::update_hub::updates_apply_all` spawns `run_all`, which drives each row through the
  SAME family command its own Install uses (`php_update_apply`, `adminer_update_apply`,
  `phpmyadmin_update_apply`, `app_update_apply`) and republishes the tray snapshot + `updates-changed`
  when it settles; `updates_apply_all_state` is polled. The page: **Update all (n)** with more than one
  pending row, per-row verdicts (spinner / measured effect / the family's sentence / "Not reached"),
  a stop sentence above the rows, the restart dialog once when the app swapped. Two bugs found by the
  L2: the in-flight poll overwriting the seed (now cancelled first) and verdicts hidden on `ok`. MCP
  parity: both commands `Never`. Ledger #906.
- **T6 — 23 Oct 2026.** `PendingSummary` carries every pending `display` and `to_json()` (one
  shape: `pending`, `offers`, `restartPending`). `rex status` prints one line for one offer, a count
  with the names for several, the reopen sentence for a swapped build; `--json` `update` is that
  shape; `rex db console`'s offer line points at Settings → Updates. MCP `stack_status.updates` is
  the same JSON (`app_update` kept one release); parity `updates_inventory → Tool("stack_status")`.
  No `rex updates` verb and no CLI apply: the read rides `status`, installs stay the GUI's and the
  MCP `php`/stack apply actions (ledger #530's stance). Ledger #907.
- **T7 — 23 Oct 2026.** Docs landed with each task (ARCHITECTURE, DESIGN, MAP, TESTING, INSTALL,
  README, SMOKE rows ×3 per OS, CLI-ROADMAP, PLATFORMS §3 rows, ledger #901–#907); PUBLISH-TESTING
  §M widened to every kind from 0.9.1. **Owed:** the three SMOKE rows live on macOS, Windows and
  Linux — they need a published manifest naming a PHP patch newer than the pin, which the first
  release after this is; and D1.
- **D1 — 23 Oct 2026.** `WordPressUpdatesNote` on the Updates page: one read-only line per WordPress
  site with plugin updates, from the site panel's `["wp-plugins", id, "updates"]` cache alone (every
  query `enabled: false`), linking to `/sites/<id>/wordpress`; never a row, never counted. The
  harness seeds the cache (`?wp=1`) and records `wp_plugins` calls so the probe can assert zero —
  the first plant passed silently until it did. Ledger #908.
