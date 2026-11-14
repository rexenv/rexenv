# PLAN — Sites list and site page: one place for each action, and a way from one site to the next

**Status:** ACCEPTED 14 Nov 2026, BEING BUILT — the owner read the plan and said to build all of it, one task at
a time; D1–D7 (§6) are taken as recommended on that word (no row was ruled on its own — each stays the owner's to
reverse at review). **T2 built 14 Nov 2026** (ledger #959–#960, `wk-checks/siteswitch.js`, WebKit only — the
SMOKE row is open on all three OSes). **T3 built 14 Nov 2026** (ledger #961–#962, `wk-checks/sitekey.js` under
macOS and Windows words, the L0 words test on all three; the SMOKE rows per OS are open — no build has run on the
Dell or the Ubuntu VM yet). **T1 built 14 Nov 2026** (ledger #963, `wk-checks/siteactions.js`; SMOKE row open per OS).
**T4 built 14 Nov 2026** (ledger #964–#965, `wk-checks/sitesrow.js`; SMOKE row open per OS; the "Recently opened"
sort came with T5). **T5 built 14 Nov 2026** (ledger #966–#968: the order and the storage L0 on any host, the
window's surfaces `wk-checks/sitepins.js`; the tray and a restart are SMOKE rows, open per OS). **T6 — the per-OS
runs — is the only task open**: no build of this work has run on the Dell or the Ubuntu VM, and `linux-check` was
SKIPPED on every verify of it (Docker was not running on the dev Mac), so Linux is not even compiled yet. The facts in §2 were read from the tree at `919e4a40`. Tracked as the
"Sites list and site page" row in `docs/TODO.md` (Now).

---

## 1. The ask

Owner, 14 Nov 2026, with screenshots of the Sites list and of a site's Overview:

1. Using the sites must be easy.
2. Going from one site to another must be quick. The model named: GitHub's repository switcher — on a repository
   page, the arrow beside the repository name opens a searchable list of the owner's other repositories.
3. Anything that is on the same page more than once is removed.
4. What is needed most is usable straight from the Sites list.

## 2. What the tree says today

### 2.1 Moving between sites

- A site page has one way out: the "‹ All sites" link (`src/routes/SiteDetail.tsx:446`). Site A → site B is
  always: back, find the row among 25, click. Three steps, and the list is re-read by eye each time.
- The tab is lost on the way. Looking at the Database tab of `ea` and wanting the Database tab of `bl` is four
  clicks.
- The Sites list keeps its filter, type filter, sort and search in `useState` (`src/routes/Sites.tsx:711-714`).
  They reset every time the user comes back from a site. A user who filtered to "lms", opened one, and came back
  sees all 25 again.
- There is no keyboard path anywhere. The only `keydown` listeners in `src/` close dialogs and menus.
- Nothing records which sites the user works on. "Recent" in the sort button means `createdAt`
  (`Sites.tsx:905`). With 25 sites and 3 in active use, the 3 are found by reading.

### 2.2 The same thing more than once — site page (Overview)

| Thing | Where it appears | Count |
|---|---|---|
| Open in browser | header split button (`SiteDetail.tsx:529`) + Quick links tile (`:755`) | 2 |
| Magic Login | header split button (`:545`) + Quick links tile (`:767`) | 2 |
| Database | tab + Quick links tile (`:787`) | 2 |
| Terminal | tab + Quick links tile (`:793`) | 2 |
| Open project folder | Paths card's folder icon (`:739`, `openable`) + Quick links tile (`:804`) | 2 |
| PHP version | large number + the select showing the same number (`:651`, `:672`) | 2 |
| Web server | large name + the select showing the same name (`:688`, `:691`) | 2 |
| Way back to the list | "‹ All sites" + the sidebar's Sites item, already highlighted | 2 |

Five of the six Quick links tiles repeat something on the same screen. The sixth, "Open in editor", is the only
action that exists nowhere else on the page — and it is the one most used after "Open in browser".

Missing from the site page: Rename is in Settings, but Duplicate, Copy domain and Delete are reachable only
from the list row's menu (`Sites.tsx:656-693`). A user on a site page who wants to delete it goes back first.

### 2.3 The same thing more than once — Sites list

In the owner's screenshot the number 25 is on screen five times: the sidebar badge, the subtitle ("25 sites ·
25 running"), "All 25", "Running 25", "All types 25". Further:

- "All sites ▾" in the header is the bulk start/stop menu (`Sites.tsx:1005`). It sits beside the "All" filter
  tab and shares its words with the site page's "‹ All sites" link, which does something else.
- Every row carries a lock icon and a "Running" pill. With 25 sites secured and running, that is 50 marks that
  say nothing; the one stopped or insecure site does not stand out from them.
- PHP version and web server are two bordered badges per row (`:453-456`) — 50 more boxes.

### 2.4 What is needed most is the hardest to reach

- The row's quick actions (browser, folder, database, WordPress) are `opacity-0` until hover and `hidden` below
  the `lg` breakpoint (`Sites.tsx:357`). In the owner's screenshot the window is narrower than `lg`: the row
  offers nothing but the `⋮` menu. Nothing on the row says the actions exist.
- Magic Login — the primary button of the site page — is not on the row at all. The row's WordPress icon opens
  the WordPress tab instead.
- "Open in editor" is inside the `⋮` menu.
- Start/Stop site is inside the `⋮` menu, while the row draws a status pill that looks like a control.

### 2.5 A doc that is wrong today

`docs/DESIGN.md` ("A default view is chosen once, not re-chosen by a poll") says Sites opens on Running. The
code has opened on All since 11 Sep 2026 (`Sites.tsx:854-856`). T1 corrects the paragraph whatever else is
ruled.

## 3. Shape

Three rules, each answering one of the asks:

- **One home per action.** The site page's header is the action bar; the tabs are the navigation; a card shows
  facts. Nothing is in two of those.
- **The row shows exceptions, and offers the three things people came for.** A normal site is quiet. A site that
  is stopped, insecure, half-made or expiring is loud.
- **A site is one keystroke or one click from any other site**, and the tab comes along.

### 3.1 The site switcher (ask 2)

The "‹ All sites" link becomes a breadcrumb:

```
Sites  /  [W] ea ▾                                  ← was: ‹ All sites
```

- "Sites" goes to the list (what the link did).
- **Built differently from the first draft:** the switcher does not decide which tab the target has. It
  navigates to the same tab, and the PAGE resolves it (`resolveSiteTab`) — because the page rendered whatever
  its URL said, so the bug was reachable by typing, and a rule in the switcher would have fixed one door.
- The site name with its chevron opens a popover, the GitHub shape: a search field focused on open, then the
  list of sites — type tile, name, domain, a status dot, a check on the current one.
- Order: pinned first, then recently opened, then the rest by name (§3.5). Without D5's storage: by name only.
- Search matches name, domain and extra domains — the same three the list's filter matches (`Sites.tsx:895`),
  so the two boxes never disagree.
- **The tab is kept.** From `/sites/ea/database`, choosing `bl` goes to `/sites/bl/database`. Where the target
  has no such tab (WordPress, Live, Repository on a site that is not one), it opens Overview. The rule is one
  pure function with a test, because "kept the tab" on a site without that tab is a blank page.
- Keys: ↑ ↓ move, Enter opens, Esc closes. Mod+Enter opens the highlighted site in the browser without leaving
  the current page.
- The list is not capped. Past the popover's height it scrolls; a truncated list that looks complete is how
  someone concludes a site is gone (DESIGN.md, the tray's rule).
- A scratch site is listed under the same "Scratch" heading the Sites list uses.

**The same list, from anywhere: Mod+K.** One component (`SiteSwitcher`), two doors — the breadcrumb's chevron and
a global shortcut that works on every route (Mail, Services, a terminal tab excepted — see below). From a
non-site route it opens the site's Overview.

- Inside the Terminal tab the shortcut is not taken: Ctrl+K belongs to the shell there (kill-line). The
  breadcrumb still works.
- The label ("⌘K" / "Ctrl+K") is a per-OS value: it comes from `PlatformWords`, all three constants filled, not
  from a `navigator.platform` test in TSX. Over the web UI the words describe the SERVER's OS, not the
  browser's, so the label there is the neutral "Ctrl/⌘ K" — T2 decides this with a test, not by eye.

Scope held on purpose: the switcher switches sites. It is not a command palette for every action in the app.
That is a larger feature with its own honesty rules (which actions are offered while the stack is down), and
this plan does not need it. §7 records it as not built.

### 3.2 The site page (ask 3)

Header, one row:

```
Sites / [W] ea ▾
[W]  ea   ● Running          [ Open in browser ▾ ] [ Editor ▾ ] [ Magic Login ▾ ]  [ ⋯ ]
     ea.test  ⧉
```

- **Open in editor moves into the header**, as a split button with the editor menu the tile had
  (`useEditorMenu`). Hidden over the web UI, as today (`canOpenOnThisScreen`).
- **`⋯` menu** — what the page did not already have a home for: Start/Stop site, Duplicate, Delete.
  **Built narrower than first drafted**, by the plan's own rule: Rename already lives on this page (Settings →
  Site name), Open folder is the path row's icon, Copy domain is the icon beside the domain, and the external
  terminal list is on the Terminal tab's toolbar — putting any of them in `⋯` would have added the duplicates
  this task removes.
- **Stop site** leaves the button row for the `⋯` menu (D3). It is the least-used of the four and the one the
  header wraps for at 945 px (the Dell, 27 Oct 2026).
- The domain gets a copy icon. The `:443` suffix stays only when the edge port is not 443 — on the default it
  is the same four characters on every site.

Overview, after:

| Card | Holds |
|---|---|
| Environment | PHP: the select alone (it shows the value). Web server: the select alone. SSL. The EOL banner and the FrankenPHP note stay as they are. |
| Site | Project path (copy, open). Config path or URL, as today. Plus the read-only facts now in Settings → "Site info": type, database name and engine, multisite (D4). |
| Recent logs | unchanged |
| Agent activity | unchanged |

- **The Quick links card is deleted.** Browser, Magic Login and Editor are in the header; Database and Terminal
  are tabs; the folder is the Paths row's icon and the `⋯` menu.
- Settings → "Site info" moves to Overview (D4): it holds no setting. Settings then contains only things that
  can be changed.

### 3.3 The Sites list row (asks 3 and 4)

```
before  [W] bl        🔒 [7.4] [nginx] [external] [DB connected] [● Running]  ⋮
        bl.rex

after   ● [W] bl                7.4 · nginx      [🌐] [W↗] [</>]  ⋮
            bl.rex
```

- **Three actions, always visible, at every width:** Open in browser, Magic Login (WordPress sites; the slot is
  empty otherwise so the columns line up), Open in editor. No hover, no breakpoint. Each keeps today's disabled
  rule and its sentence ("Nothing is serving … — Start all first").
- **Everything else in `⋮`, and on right-click of the row** (the same menu): Start/Stop site, Database,
  Terminal, Logs, WordPress, Open folder, Rename, Duplicate, Copy domain, Delete. Database/Folder/WordPress
  leave the hover strip for the menu.
- **Status is a dot at the row's left edge** for the normal case. The words appear only when they say something:
  "Stopped by you", "Setting up", "setup incomplete", and "Stopped" while the stack is down. The two
  stopped-nesses keep their two sentences (DESIGN.md) in the tooltip and in the text.
- **The lock is drawn only when the site is NOT secured** (D2).
- **PHP and server are one muted mono text**, `7.4 · nginx`, not two boxes.
- **Chips are for a state that asks something of the user.** `DB imported` (not connected yet), the scratch TTL,
  `source moved`, `setup incomplete` stay chips. `DB connected` and `external` are settled facts: they become
  the second line's muted text (`bl.rex · external`) and stay in full on the site page (D2).
- The metrics column (`SiteMetrics`, `xl` only) is unchanged.
- Width is measured, not guessed: the row is checked at 945, 1100 and 1440 px with a fixture shaped like the
  owner's list — 25 sites, a long unbroken name, a worktree row, a scratch row with a package and a TTL
  (memory: a review fixture narrower than any real window hid this class twice).

### 3.4 The Sites list header (ask 3)

- Subtitle: `25 sites · 25 running` stays — it is the one place the totals are said. The "All" tabs lose their
  number (they repeat the subtitle); "Running 25 / Stopped 0" keep theirs, because that count is what clicking
  shows (DESIGN.md's rule is kept, not bent).
- The type filter renders only the types the user has, and not at all when there is one type. A user with 25
  WordPress sites sees no "Laravel 0".
- "All sites ▾" is renamed and moved: a `⋯` button at the header's right, holding "Start all sites (N stopped)"
  and "Stop all sites (N started)" with today's counts and wording. The words "All sites" then mean one thing in
  the app.
- Sort gains "Recently opened" (needs D5) and the list's filter, type, sort and search survive a visit to a
  site (a small Zustand store; cleared by "Show all sites" as today). Scroll position is restored on the way
  back.
- Keys on the list: `/` focuses the filter, ↑ ↓ move between rows, Enter opens, Mod+Enter opens in the browser.

### 3.5 Pinned and recently opened (asks 1 and 2)

- A site can be pinned (row menu, site page `⋯`). Pinned sites are listed first in the switcher, get a "Pinned"
  group at the top of the Sites list, and appear under the sidebar's Sites item — up to 5, by name, each one
  click from any route.
- Opening a site page records the time. The switcher lists the last few opened after the pinned ones; the sort
  button's new mode uses the same value.
- **Built as a table, not two columns** (`site_prefs`, v46): the tree's own v45 note argues it — a `sites`
  column is threaded through every `Site { … }` literal for a field most sites lack — and kept off the row, no
  site-edit writer can touch a pin. The first draft said:
- Storage is the ruling D5. Recommended: two nullable columns on `sites` (`pinned`, `last_opened_at`), one
  migration, in `core/`. It is app state (the non-negotiable says SQLite), it survives a reinstall of the
  webview's storage, the web UI and the desktop window agree, and the tray's Sites submenu — which today caps
  the list and says what it hid — can show pinned and recent first.
- Recording an open is a write on navigation. It is fire-and-forget, never awaited by the page, and never
  taken under the services lock.

## 4. All three platforms

Almost all of this is frontend, written once. What differs per OS:

| Difference | Where it goes |
|---|---|
| The shortcut's label and modifier (⌘ on macOS, Ctrl on Windows and Linux) | `platform/words.rs` — `MACOS`, `WINDOWS`, `LINUX` all filled |
| The tray's Sites submenu order (if D5 is accepted) | the existing tray code per OS; the ORDER is computed in `core/`, once |
| The webview engine | WKWebView / WebView2 / WebKitGTK — the popover's focus trap and the global key listener are run on each; WKWebView is the one with history (memory: its quirks are a bug class) |

`core/` gains no `cfg(target_os)`. Proof per OS is a RUN of the built app on that OS (the 15.8 VM or the dev
Mac, the Dell, the Ubuntu VM), not `windows-check` / `linux-check`.

Over the web UI (headless): the editor button and Open folder stay absent (`canOpenOnThisScreen`); the switcher,
the pins and the keys all work, since none of them opens anything on the server's screen.

## 5. What this does not change

- No backend behaviour of start/stop, provisioning, delete, Magic Login. Every button calls what it calls today.
- The honest-UI rules in DESIGN.md for the row: a link the stack is not serving is not offered; a word that
  means "failed" is never shown for "in progress"; a filter tab's count is what clicking it shows; two
  stopped-nesses, two sentences. T4 keeps each and its wk-check (`badges`) stays green.
- Tabs, their order and their routes.

## 6. Owner decisions

| # | Question | Recommended |
|---|---|---|
| D1 | The switcher: breadcrumb popover only, or the popover plus Mod+K from every route? | Both — one component, and the shortcut is what makes "site to site" one step. |
| D2 | May the row go quiet — lock only when insecure, status as a dot, `DB connected` and `external` as muted text instead of chips? | Yes. The exception is what the row is for; all four facts stay in full on the site page. |
| D3 | Stop site: keep the header button, or move it to `⋯`? | Move. If stopping one site is something you do many times a day, keep it — say so. |
| D4 | Move Settings → "Site info" to Overview? | Yes. |
| D5 | Pinned + recently opened: SQLite columns, browser storage, or not at all? | SQLite columns. Browser storage is per-webview and the web UI would disagree with the desktop window. |
| D6 | Pinned sites under the sidebar's Sites item (max 5)? | Yes — it is the only switch that needs no popover at all. |
| D7 | The three always-visible row actions: Browser, Magic Login, Editor? | Yes. Alternative third: Database instead of Editor. |

## 7. Not in this plan

- A command palette for actions (start the stack, new site, open Mail…). §3.1 says why.
- Site groups, tags or folders. Pins cover "the few I work on" at 25 sites; groups are a different feature and
  a schema of their own.
- Any change to the tabs' content (WordPress, Database, Live, Logs, Terminal, Settings other than D4).
- A grid/card view of the Sites list.

## 8. Tasks

Each is one commit, verify green, docs in the same commit. T1–T3 need no ruling beyond D1 and D3/D4; T4 needs
D2 and D7; T5 needs D5 and D6.

| # | Task | Done when |
|---|---|---|
| T1 ✓ | **Site page: one home per action.** Delete Quick links; Editor split button and `⋯` menu in the header; PHP and server as the select alone; "Site info" to Overview (D4); Stop site per D3. Correct DESIGN.md's stale "opens on Running" paragraph. | Each action in §2.2's table appears once. Header measured at 945 px with `smoke1w`-shaped and long names: nothing overlaps, the name truncates. Rename, Duplicate, Copy domain, Delete work from the site page. Web UI: no editor/folder control rendered. |
| T2 ✓ | **`SiteSwitcher` + breadcrumb.** The popover, search over name/domain/extra domains, keys, the tab-keeping rule as a pure function. | *(Built: the tree has no TypeScript unit runner, so the rule is proven through the page — `siteswitch.js` legs 2–3; a site whose folder is a checkout is not in the fixture.)* Unit test: the tab rule for every tab × (WordPress, Laravel, Blank PHP, repository or not). wk-check: open, type, Enter lands on the target's same tab; 25 + scratch rows all reachable by scroll; focus returns to the trigger on Esc. |
| T3 ✓ | **Mod+K from every route.** Global listener, not taken inside the Terminal tab or while a dialog is open; label from `PlatformWords`. | Runs on macOS, Windows and Linux builds: the shortcut opens the switcher on Sites, Mail, Settings and a site's Logs tab; in the Terminal tab Ctrl+K reaches the shell. L0 test that all three `PlatformWords` carry the label. |
| T4 ✓ | **Sites row and header.** Three visible actions, `⋮` + right-click menu, status dot, exception-only lock and chips, `7.4 · nginx`, header per §3.4, list state kept across a site visit, list keys. | wk-check at 945 / 1100 / 1440 with the production-shaped fixture: no row overflows, actions visible at all three. `badges` wk-check green (Setting up / setup incomplete / unknown). Filter → open site → back shows the same filter, sort, search and scroll. Counts rule still holds (existing test). |
| T5 ✓ | **Pinned and recently opened.** Migration, `core/` functions, thin commands, IPC wrappers; pin in both menus; switcher order; "Pinned" group; sidebar entries (D6); sort mode; tray order. | L0: migration on a populated DB keeps every row; ordering function tested. Pin survives an app restart; the web UI and the desktop window show the same pins. Tray submenu lists pinned first on each OS that has a tray. A deleted site leaves no pin behind. |
| T6 | **Per-OS runs and docs close-out.** | SMOKE-TEST rows run on the Mac, the Dell and the Ubuntu VM; ledger rows name the OS each proof came from. |

Suggested order: T2 → T3 (the owner's headline ask, and independent of the rest), then T1, T4, T5, T6.

## 9. Docs this touches when built

- `docs/DESIGN.md` — the three rules of §3 as rules, with what they replaced; the corrected default-view
  paragraph; the row's exception-only rule beside the existing honest-UI rows it must not break.
- `docs/ARCHITECTURE.md` — the Sites screen and site page sections; pins/recents as site state (T5).
- `docs/MAP.md` + README structure tree — `SiteSwitcher`, the list-state store, any new `core/` module.
- `docs/SMOKE-TEST.md` — switcher (click and shortcut), tab kept, row actions, pins; Windows and Linux sections
  for the shortcut's modifier and the tray.
- `docs/TESTING.md` + `scripts/wk-checks/` — the new row-width and switcher probes.
- `docs/CLAIM-LEDGER.md` — any "never"/"always" the build writes (the tab rule, "the switcher is never capped",
  the Terminal-tab exemption).
- `docs/CLI-ROADMAP.md` — only if T5 adds an IPC a `rex` command should mirror (`rex site pin`), as a tagged row.
- `docs/TODO.md` + `./scripts/status.py --write`.
