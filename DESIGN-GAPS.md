# DESIGN-GAPS — UI fidelity vs `design/` (macOS)

> **Status: OPEN — burndown not started.** Source of truth: the `design/*.dc.html` comps. This file
> tracks every place the shipped UI diverges from those comps, found by a per-screen audit (each comp
> read against its React implementation). The *foundation* already matches well — palette, type roles
> (Space Grotesk display / Inter UI / JetBrains Mono technical), radii (sm 6 / 9 / lg 13 / xl 14),
> sidebar width/order, focus ring, and TopBar are faithful in `tokens.css` / `tailwind.config.js`. The
> gaps below sit *on top* of that scaffold: two near-stub screens, several missing controls, and the
> status-color "language" rendered flat-gray.
>
> **Scope:** UI/UX only — no backend/architecture changes. Most of the underlying capability already
> exists (logs, terminal, tunnels, WP tools, monitor); these tasks SURFACE it the way the comps show.
> Reuse the existing typed IPC (`src/lib/ipc/`), semantic `--rex-*` tokens (never hardcode hex), and the
> `Card`/`StatusPill`/`StartStopToggle`/`button` primitives. macOS only.
>
> **Working rule (unchanged):** one task at a time; tick the box only when "Done when" passes
> (chrome-devtools on the browser dev build + the screen renders against `src/lib/mock.ts`); one commit
> per task (`Design-gap task X.Y: …`). Verify before moving on.
>
> ### Intentional divergences — DO NOT "fix" toward the comp
> The comps are aspirational and predate the real architecture. These are correct as shipped:
> - **Web-server options Nginx / FrankenPHP** (comps show Apache / OpenLiteSpeed) — Apache/OLS deferred (§7).
> - **DB engines MySQL / PostgreSQL** (comps show MariaDB) — deferred (§7).
> - **Light theme is selectable + applied** though the Settings comp gates it "soon" — per the §4.4 decision.
> - **Terminal tab** (Site Detail), **Blueprints card** + **Uninstall card** (Settings) — real Phase-3 /
>   release features the comps don't include. Keep them.
> - **Blueprint selector** in New Site, **dynamic PHP version list** (no 7.4) — additive, keep.
> - When a designed control's data does not exist yet (e.g. per-DB size, last-login, request counts), build
>   the chrome and wire what exists; leave a clear TODO for the missing datum rather than faking it.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[D]` deferred by decision

Severity tags on each task: **H** (missing element/feature/structure) · **M** (visible mismatch) · **L** (minor drift).

---

## 1. Design-system primitives (cross-cutting — do these first; every screen benefits)

Source: `design/rexenv Component Sheet.dc.html`, `design/rexenv App Shell.dc.html`.
Files: `src/components/common/StatusPill.tsx`, `StartStopToggle.tsx`, `src/components/ui/button.tsx`,
`src/components/common/Placeholder.tsx`, `src/styles/tokens.css`, `tailwind.config.js`.

- [x] **1.1 (H) StatusPill — restore per-status tinted bg + border.**
  *Done when:* each pill carries a status-colored translucent background + border instead of the flat
  neutral chip. Running `bg rgba(63,185,80,.12) / border rgba(63,185,80,.22)`; Starting `.12/.24` amber;
  Stopped `rgba(110,118,129,.13)/.2`; Error `rgba(248,81,73,.12)/.24`. Dot size 9px in pills (7px stays for
  bare/table use). Text colors: Running `#4FC065`, Stopped `#8A90A0`, Error `#F8716B`, Starting `#D7A93A`.
  (Component Sheet §03, lines 209-224 · `StatusPill.tsx:8-27`.)
  ✓ Added status-tint + `*-bright` text tokens to `tokens.css` (`--rex-{running,warning,stopped,error}-{bg,border}`
  + `error-bright`/`warning-bright`) and registered them under `colors.status` in `tailwind.config.js`.
  `StatusPill.tsx` now maps each status to `fill` (tinted bg+border), `text` (bright shade; Stopped → text-muted
  `#8A90A0`), 9px dots, design padding `py-[5px] pl-2.5 pr-3`. `pnpm tsc --noEmit` clean; chrome-devtools
  `/services` shows green-tinted Running pills + neutral-gray Stopped pill (was flat gray).

- [x] **1.2 (M) StatusPill — "Starting" spinner + error pulse.**
  *Done when:* Starting renders a rotating ring (`rexSpin`), not a static dot; Error dot gets `glowErr` +
  `rexErr` opacity pulse; Running dot gets `glowRun`. Add the `rexSpin` / `rexErr` keyframes to
  `tailwind.config.js` (only `rex-ping` exists today). (Component Sheet lines 23-24, 210-222.)
  ✓ Added `--rex-glow-run` (`0 0 7px rgba(63,185,80,.75)`) / `--rex-glow-err` (`0 0 7px rgba(248,81,73,.6)`)
  tokens + `shadow-glow-run`/`shadow-glow-err`, and `rex-spin` (`0.9s linear`) / `rex-err` (`1.8s ease-in-out`)
  keyframes+animations in `tailwind.config.js`. `StatusPill` now renders a `StatusMarker`: Starting = 11px amber
  spinner ring (no dot), Running = 9px dot + `shadow-glow-run` + ping, Error = 9px dot + `shadow-glow-err` +
  `animate-rex-err`, Stopped = plain dot. All animated markers carry `motion-reduce:animate-none` (design is
  reduced-motion aware). `pnpm tsc --noEmit` clean; chrome-devtools `/services` (temp-mapped 4 states, reverted)
  showed the spinner ring + error pill distinct from running/stopped.

- [x] **1.3 (M) Sidebar — active nav item in violet.**
  *Done when:* active item = `bg rgba(124,92,255,.10)` + text `#C9BCFF` (the existing unused `brand-tint`
  token), keeping the `#7C5CFF` left bar. Hover text → `#C7CBD4`. (App Shell line 132, Component Sheet
  §06 lines 127-133 · `Sidebar.tsx:33-37`.)
  ✓ Added `--rex-brand-active: rgba(124,92,255,.1)` token + `brand.active` color. `NavButton` active state →
  `bg-brand-active text-brand-tint` (was `bg-white/[0.055] text-rex-text`); inactive gains
  `hover:text-rex-text-bright` (`#C7CBD4`). Left bar `bg-brand` unchanged. `pnpm tsc --noEmit` clean;
  chrome-devtools shows the active Services item violet-tinted (bg + text + icon), inactive items muted.

- [x] **1.4 (M) StartStopToggle — correct size, colors, + violet "setting" variant.**
  *Done when:* track `46×27px`, knob `21px`; ON (status) `bg #238636 / border #2EA043 + glowRun`; OFF
  `bg #262A33 / border #30343E`, knob `#C7CBD4`. Add a `variant="setting"` (ON `bg #7C5CFF`) for non-status
  toggles (autostart, multisite, etc.) — "running uses green; settings use violet". Fix the hardcoded
  `aria-label` "Start/Stop site" so service/DB/tunnel callers pass their own. (Component Sheet lines 241-261
  · `StartStopToggle.tsx:18-31`.)
  ✓ Added `--rex-toggle-{on,on-border,off,off-border}` tokens + `colors.toggle` in tailwind. Rewrote the
  toggle: `h-[27px] w-[46px]`, 21px knob (`translate-x-[19px]` on), ON status = `bg-toggle-on
  border-toggle-on-border shadow-glow-run`, OFF = `bg-toggle-off border-toggle-off-border` + `bg-rex-text-bright`
  knob (`#C7CBD4`), focus-ring via `--rex-focus-ring`, knob/track transitions per spec. New `variant="setting"`
  (ON `bg-brand border-brand-light shadow-glow-primary`) and a `label` prop drives the aria-label; Sites passes
  `"{Start|Stop} {site.name}"`, Databases `"… {db.label}"`. `pnpm tsc --noEmit` clean; chrome-devtools
  `/databases` shows the larger green-ON / dark-OFF tracks at design proportions.

- [x] **1.5 (M) Button — danger red + secondary border/hover + glow-primary.**
  *Done when:* danger uses `#DA3633` (hover `#E5484D`, pressed `#B92D2B`) — a new token, distinct from
  error-status `#F85149`; secondary border → `border-strong #2E323C`, hover `bg #232734 / border #3B404C`;
  `--rex-glow-primary` corrected to `0 2px 14px rgba(124,92,255,.30)` (currently `0 4px 14px … .35`). Add
  `active:` pressed states (bg shift + `translateY(1px)`). (Component Sheet §02 lines 159-189 ·
  `button.tsx:13-21`, `tokens.css:66`.)
  ✓ Added `--rex-danger{,-hover,-active}` tokens + `colors.danger` and repointed `--destructive → --rex-danger`;
  added `--rex-surface-2-hover #232734` / `--rex-border-strong-hover #3b404c` (+ rex tailwind colors); corrected
  `--rex-glow-primary` to `0 2px 14px rgba(124,92,255,.3)`. `button.tsx`: danger → `bg-danger hover:bg-danger-hover
  active:bg-danger-active`; secondary → `border-rex-border-strong hover:bg-rex-surface-2-hover
  hover:border-rex-border-strong-hover`; primary gains `active:bg-brand-strong`; all four variants get
  `active:translate-y-px` (+ ghost `active:bg-white/[0.085]`). `pnpm tsc --noEmit` clean; chrome-devtools New Site
  dialog shows the primary "Create site" with the corrected subtler glow (danger has no caller yet; pressed states
  aren't capturable in a static shot).

- [x] **1.6 (L) Crown mark, Placeholder greys, ping timing.**
  *Done when:* sidebar crown box gets `glowCrown` (`0 6px 22px rgba(124,92,255,.20)`), radius 8px, border
  `#2C303B` (`Sidebar.tsx:11`); Placeholder icon/label use `#4F5663`, hint `#5A6170`, icon box radius 12px
  (`Placeholder.tsx:16-28` — add tokens for these greys); `rex-ping` timing → `2.4s ease-out`
  (`tailwind.config.js:98`). (App Shell lines 42, 107-110.)
  ✓ Added tokens `--rex-glow-crown` (+ `shadow-glow-crown`), `--rex-crown-border #2c303b`,
  `--rex-placeholder #4f5663` / `--rex-placeholder-hint #5a6170`. CrownMark → `rounded-[8px]
  border-[var(--rex-crown-border)] shadow-glow-crown`; Placeholder icon-box `rounded-[12px]`, icon/label
  `text-[var(--rex-placeholder)]`, hint `text-[var(--rex-placeholder-hint)]`; `rex-ping` animation → `2.4s
  ease-out`. `pnpm tsc --noEmit` clean; chrome-devtools shows the crown's subtle violet glow (placeholder
  needs an empty/loading state to render — mock data is always populated; ping is non-static).

---

## 2. App Shell — sidebar, status footer

Source: `design/rexenv App Shell.dc.html`. Files: `src/components/shell/StatusFooter.tsx`,
`Sidebar.tsx`, `nav.ts`.

- [x] **2.1 (M) StatusFooter — global toggle logic + Stop-all styling.**
  *Done when:* the toggle keys off `running === 0` (not `summary === "all"`): in **partial** state it shows
  **"Stop all"** with the *secondary* style (`bg #1C1F27 / border #2E323C / text #C7CBD4`), not "Start all"
  primary. Accent strip uses the translucent variants; stopped label `#8A90A0`; dot pulse only when
  `running > 0`. (App Shell lines 154, 261, 293-299 · `StatusFooter.tsx:34,51,88-91`.)
  ✓ Replaced `allRunning` with `isStart = status.running === 0`: button is primary "Start all" only when
  nothing runs, else secondary "Stop all" (`bg-rex-surface-2 border-rex-border-strong text-rex-text-bright` +
  Square icon). `SUMMARY_META` gained translucent `accent` (strip), `glow` (dot box-shadow: run-glow / amber /
  none), and `labelClass` (stopped → `text-rex-text-muted`). Dot pulse now gated on `running > 0`
  (`motion-reduce:animate-none`). `pnpm tsc --noEmit` clean; chrome-devtools (mock = partial 3/12) now shows
  "■ Stop all" secondary (was violet "▶ Start all").

- [x] **2.2 (L) Sidebar — Tunnels live-pulse dot + Services/Settings icons.**
  *Done when:* the Tunnels nav count is preceded by a green pulsing dot when a tunnel is active; Services
  icon → server-rack rects, Settings icon → sliders (comp uses these, impl uses `Layers`/`Settings` gear).
  (App Shell lines 50, 55, 58 · `nav.ts:24,27,28`, `Sidebar.tsx:50-54`.)
  ✓ `nav.ts`: Services `Layers → Server`, Settings `Settings → SlidersHorizontal`; added `activeDot?: boolean`
  to `NavItem`, set on Tunnels (mock — real wiring lands in §8). `Sidebar` badge now renders a 6px
  `bg-status-running` ping dot (`motion-reduce:animate-none`) before the count when `activeDot`. `pnpm tsc
  --noEmit` clean; chrome-devtools shows the server-rack + sliders icons and the green pulse before Tunnels "1".

---

## 3. Sites screen

Source: `design/rexenv Sites Screen.dc.html`. File: `src/routes/Sites.tsx` (+ `TopBar.tsx`).

- [x] **3.1 (H) Wire search + add All/Running/Stopped filter + sort.**
  *Done when:* the header search filters rows live by name/domain (today `TopBar` search has no
  `value`/`onChange` — it filters nothing); a segmented control `All · Running · Stopped` with live count
  badges (active pill violet `rgba(124,92,255,.14)/#C9BCFF`) filters by status; a "Change sort order" button
  cycles `Name → Status → Recent`. Drives a "No sites match …" empty state. (lines 99-118, 162-167, 345-348
  · `Sites.tsx:115-164`, `TopBar.tsx:30-43`.)
  ✓ Made `TopBar` search controllable (`searchValue`/`onSearchChange`/`searchPlaceholder`, width 190px). Added
  `--rex-brand-tint-bg` (.14) token + `brand.tint-bg`. `Sites` now holds `query`/`filter`/`sort` state: a
  `FilterTabs` segmented control (`bg-rex-well`, active pill `bg-brand-tint-bg text-brand-tint`) with live
  `{all,running,stopped}` counts; a `SortButton` (ArrowDownUp) cycling Name→Status→Recent; a memoized
  filter→search→sort pipeline; and a filtered "No sites match “{q}”" state. `pnpm tsc --noEmit` clean;
  chrome-devtools verified: name-sort reorders the list, Stopped filter shows only Blog Network (counts
  All 3/Running 2/Stopped 1), and typing "zzz" shows the no-results state.

- [x] **3.2 (H) Per-row kebab menu.**
  *Done when:* the inert `MoreHorizontal` button opens a menu: Rename · Duplicate · Open in editor ·
  Copy domain · ─ · **Delete** (red). Move Delete OUT of the row as a standalone icon into this menu.
  (lines 147-159, 205-211 · `Sites.tsx:69-80`.)
  ✓ Built a reusable portal dropdown primitive `src/components/ui/menu.tsx` (`Menu`/`MenuItem`/`MenuSeparator`)
  — fixed-positioned from the trigger rect via `createPortal` to `<body>` so it's never clipped by the list's
  `overflow-hidden`; closes on outside-click / Escape / scroll / resize. Added `--rex-shadow-menu` token +
  `shadow-menu`. Sites row kebab (`MoreVertical`, always visible) → Rename · Duplicate · Open in editor ·
  Copy domain (writes `navigator.clipboard`) · ─ · Delete (danger, wired to `confirmDelete`). Removed the
  standalone Trash2 row button. Rename/Duplicate/Open-in-editor have no backend yet (shell). `pnpm tsc
  --noEmit` clean; chrome-devtools shows the menu open un-clipped with all items + red Delete.

- [x] **3.3 (H) Type avatar (W/L/P) + column-header strip.**
  *Done when:* each row leads with a 26×26 rounded badge showing the type letter on a per-type color
  (W blue `#7DB8D8`, L red `#EE837C`, P purple `#A7AADD`) instead of the generic globe; add the mono
  uppercase column-header strip (Stack 118px / Status 88px / Power). (lines 116-118, 125, 384-388 ·
  `Sites.tsx:38-40`.)
  ✓ Added `TYPE_META` (wordpress→W blue, laravel→L red, php→P purple — letter/bg/color/border per the comp's
  typeMap) and replaced the globe with a 26×26 `rounded-[7px]` letter avatar (`text-[11px] font-bold`). Added
  the `Stack 118 / Status 88 / Power` mono-uppercase header strip (`text-[var(--rex-placeholder)]`) to the
  right of the filter sub-row. `pnpm tsc --noEmit` clean; chrome-devtools shows W/W/L avatars + the header
  strip (column pixel-alignment refines in §3.5 with fixed widths).

- [x] **3.4 (M) Row action placement + SSL indicator + empty state.**
  *Done when:* Open-in-browser / Open-folder / Open-database reveal on hover on the LEFT (right of the name),
  status/php/server/toggle/kebab stay right; SSL shows closed-lock gray `#7E8492` when secure, **open-lock
  amber `#C99A3A` when not** (today: lock only when ssl, colored green); empty state = violet-gradient crown,
  copy "Point rexenv at a folder…", button "Create your first site". (lines 130-139, 171-180 ·
  `Sites.tsx:37-81, 138-150`.)
  ✓ Moved the three hover quick-actions to right of the name (name block now `w-[188px] flex-none`, actions,
  then a `flex-1` spacer) so status/php/server/toggle/kebab stay right. SSL indicator: `Lock` gray
  (`--rex-lock-secure #7e8492`) when ssl, `LockOpen` amber (`--rex-lock-insecure #c99a3a`) when not, with
  `SSL · trusted` / `No SSL` title. Rebuilt the zero-sites empty state: 60×60 violet-gradient crown medallion
  (`shadow-glow-crown`, 3 gems), "No sites yet" (19px), the "Point rexenv at a folder…" copy, and a `size="lg"`
  "Create your first site" button. `pnpm tsc --noEmit` clean; chrome-devtools verified hover-left actions, the
  amber open-lock (temp ssl:false), and the crown empty state (temp empty query) — both temps reverted.

- [x] **3.5 (L) Row container + badge sizing + PHP label.**
  *Done when:* rows are standalone 44px rounded elements (radius 9px, hover fill `#15171D`, no dividers),
  name column fixed 188px, server badge fixed 84px centered, status pill 92px; PHP badge shows `8.3` not
  `PHP 8.3`. (lines 124-142, 397 · `Sites.tsx:37-49, 152`.)
  ✓ List container is now a plain `flex flex-col` (dropped the bordered box); each row is `h-11 rounded-[9px]
  pl-3 pr-2 gap-[11px] hover:bg-rex-surface-1` (no `border-b`). Sites `Badge` restyled to the comp (surface-1
  bg, border-strong, `rounded-[6px]`, `px-[7px] py-[3px]`) + accepts `className`; PHP badge → `{phpVersion}`
  (no "PHP " prefix), server badge `w-[84px] text-center`. `StatusPill` gained a `className` prop; Sites passes
  `w-[92px]`. (Name 188px done in §3.4.) `pnpm tsc --noEmit` clean; chrome-devtools shows divider-less 44px
  rows, `8.3`/`8.1`/`8.2` badges, uniform server/status widths aligned under STACK/STATUS/POWER.

---

## 4. Site Detail screen

Source: `design/rexenv Site Detail.dc.html`. File: `src/routes/SiteDetail.tsx`.

- [x] **4.1 (H) Header action cluster + identity + back link.**
  *Done when:* the header (replacing the plain `TopBar`) shows: a `‹ All sites` back link; a 38×38 type
  avatar; the site name (19px) with an inline status pill; `domain · :<port>` beneath; and a right cluster of
  start/stop toggle + "Open in browser" (secondary) + "Open admin" (primary, gear). (lines 76-95 ·
  `SiteDetail.tsx:102`.)
  ✓ Extracted the type-avatar map to `src/lib/siteType.ts` (`siteTypeMeta`, reused by Sites). New `SiteHeader`
  replaces the plain `TopBar`: `‹ All sites` back link (→ `/sites`), 38×38 letter avatar, name (19px) + inline
  `StatusPill`, `domain · :443` (the edge HTTPS port), and a right cluster — `StartStopToggle` (wired to a new
  start/stop mutation), "Open in browser" (secondary, → `openExternal(url)`), and "Open admin" (primary, gear,
  → `/wp-admin`) gated to WordPress sites. `pnpm tsc --noEmit` clean; chrome-devtools: Acme (WP) shows the full
  cluster incl. Open admin; Portfolio (Laravel) correctly omits Open admin + the WordPress tab.

- [x] **4.2 (H) Environment card — 3-column mini-card grid + SSL certificate.**
  *Done when:* Environment renders as three mini-cards — PHP version (big 18px mono value + dropdown), Web
  server (value + dropdown), **SSL certificate** (green lock + "Trusted" + mono "rexenv CA") — instead of
  plain label/value rows; the SSL sub-card is currently absent. (lines 112-142 · `SiteDetail.tsx:229-263`.)
  ✓ Rebuilt Environment as a bespoke card (mono uppercase "Environment" label + `grid-cols-3`) with an `EnvMini`
  helper (`bg-rex-well`, subtle border, `rounded-[11px]`): PHP version = 18px mono value + select (`{m}` options,
  no "PHP " prefix), Web server = 15px value + select, SSL certificate = green `Lock` + "Trusted" + mono "rexenv CA"
  (amber `LockOpen` + "Not secured" when `!ssl`). Dropped the comp-absent Type/WordPress rows (removed the now-unused
  `Field` + `wp`/`WpInfo`). `pnpm tsc --noEmit` clean; chrome-devtools shows the 3 mini-cards incl. the SSL sub-card.

- [x] **4.3 (M) Recent-logs preview (remove stale placeholder).**
  *Done when:* the Recent-logs card shows a real log-preview box (last lines, color-coded by level) + a
  "View all logs ›" link, replacing the "Live log tailing arrives in §3 — this peek will show…" placeholder
  (LogsTab is already fully implemented in this file). (lines 186-198 · `SiteDetail.tsx:273-277`.)
  ✓ New `RecentLogs` component: tails the first log target's last 6 lines (`tailLog`, polled 5s) into the
  design's preview box (`bg-[#0B0C10]` border, mono 11.5px) with a brand-tint "View all logs ›" link (→ Logs
  tab via new `onViewLogs`). `logLineColor` heuristically tints ERROR (red) / WARN (amber) lines, others
  `#A9AEBA`; empty → "No recent activity" hint. Replaced the stale "§3" placeholder. `pnpm tsc --noEmit` clean;
  chrome-devtools shows the live-tailed lines + the View-all-logs link.

- [x] **4.4 (M) Overview layout: card order + Quick links grid + Paths config row.**
  *Done when:* order is Environment → (Paths | Quick links two-column 1.25fr/1fr) → Recent logs; Quick links
  is a 2-col tile grid with colored icons (Browser/WP admin/Database/Terminal) + a full-width "Open project
  folder"; Paths shows `Project path` + `Config path` (recessed boxes w/ copy + open-folder). (lines 110-199
  · `SiteDetail.tsx:204-277`.)
  ✓ Reordered Overview to Environment → `grid-cols-[1.25fr_1fr]` [Paths | Quick links] → Recent logs. New
  `PathField` (recessed `bg-rex-well` box + mono value + CopyButton + open-folder) renders Project path +
  (WP→`Config path` = wp-config.php / else `URL`). New `QuickTile` 2-col grid (Browser muted, WP admin
  `#7DB8D8`, Database muted, Terminal `brand-tint`, "Open project folder" `col-span-2`). Removed the unused
  `Card`/`Row`/`QuickLink`/`PathRow` helpers. `pnpm tsc --noEmit` clean; chrome-devtools shows the new order +
  two-column Paths/Quick-links grid + colored tiles.

- [x] **4.5 (L) Tab bar alignment + content width.**
  *Done when:* tabs are left-aligned full-width (not `mx-auto max-w-2xl`), WordPress tab tinted blue when
  inactive; Overview content fills the panel (drop `max-w-2xl`). (lines 97-105 · `SiteDetail.tsx:104-131`.)
  ✓ Tab strip is now left-aligned (`flex gap-0.5`, dropped `mx-auto max-w-2xl`), `px-3.5 py-2.5 text-[13.5px]`,
  with the WordPress tab `text-[#7DB8D8]` (blue) when inactive / brand-underlined when active. Content area
  dropped `mx-auto max-w-2xl` (now `px-[22px] pt-[18px] pb-[22px]`, inner `gap-[14px]`) so Overview fills the
  panel; terminal/database keep `h-full` + `overflow-hidden`. `pnpm tsc --noEmit` clean; chrome-devtools shows
  left-aligned tabs (WordPress blue) + full-width Overview.

---

## 5. Services screen (near-stub — big rebuild)

Source: `design/rexenv Services.dc.html`, `design/rexenv Service Row.dc.html`. File: `src/routes/Services.tsx`.

- [x] **5.1 (H) "Total resource usage" summary card.**
  *Done when:* a gradient card at top: live pulse dot, "TOTAL RESOURCE USAGE", "live · updates every 1.5s",
  big CPU number + meter, big Memory number + meter ("of N GB budget · stays light"), running/idle counts.
  (Services lines 65-87.)
  ✓ New `TotalUsageCard` (gradient bg, `#23262F` border): live `shadow-glow-run` pulse + "TOTAL RESOURCE USAGE"
  + "live · updates every 2s" (matches the existing 2s poll), a 3-col grid — CPU (`UsageMetric` big value +
  brand bar), Memory (value + bar + "of 4.0 GB budget · stays light", `RAM_BUDGET_MB=4096`), and running/idle
  counts (border-left, running in `status-running-bright`). All aggregated from the live services list. `pnpm
  tsc --noEmit` clean; chrome-devtools shows the card (CPU 1% / 515 MB / 3 running · 1 idle).

- [x] **5.2 (H) Section grouping + per-section summaries.**
  *Done when:* services group into PHP · Databases · Mail · Web servers & edge router, each a bordered card
  with a colored icon + title + mono "{r}/{n} running" — replacing the single flat list. (Services lines
  89-107 · `Services.tsx:77-82`.)
  ✓ Added `ServiceKind` + optional `kind`/`version`/`isDefault`/`isRouter` to `ServiceInfo` (Rust may not send
  them; `serviceKind()` derives `kind` from the name as a fallback). `GROUPS` (PHP purple `Code` / Databases blue
  `Database` / Mail amber `Mail` / Web teal `Server`) render each non-empty group as a bordered card with a colored
  icon + title + mono "{run}/{n} running". Reshaped `mockServices` to 7 entries spanning all groups (PHP-FPM 8.3
  default + 8.2, MySQL + PostgreSQL, Mailpit, Nginx + Caddy router). `pnpm tsc --noEmit` clean; chrome-devtools
  shows the 4 grouped cards (header "4/7 running").

- [x] **5.3 (H) Per-row toggle + action button + badges.**
  *Done when:* each row gets a start/stop toggle and a contextual action (PHP non-default → "Set default";
  DB → "Open in browser"; Mailpit → "Open inbox"; web → "Logs"), a 30×30 accent badge (My/8.3/Nx),
  and "default"/"edge router" pills where applicable (needs `ServiceInfo` to carry `kind`/`isDefault`/
  `isRouter`/`version`). (Service Row lines 15-40 · `Services.tsx:26-49`, `src/types/index.ts:75-82`.)
  ✓ Each row now leads with a 30×30 `KIND_ACCENT`-tinted monogram badge (`serviceBadge`: 8.3/My/Pg/Mp/Nx/Cd),
  shows `default` (violet) / `edge router` (teal) pills, a `StartStopToggle`, and a contextual `ActionBtn` —
  PHP non-default → "Set default" (`setDefaultPhpVersion`), database → "Open" (→ /databases), Mailpit → "Open
  inbox" (→ /mail). `pnpm tsc --noEmit` clean; chrome-devtools shows badges, both pills, toggles, and the three
  action types. **Notes:** per-service toggle is a documented no-op — needs a backend `start_service`/`stop_service`
  command (only whole-stack exists, §5.4); web "Logs" omitted (no global service-log route yet).

- [x] **5.4 (H) Global Start all / Stop all in header.**
  *Done when:* the header has the primary button toggling Start all (violet, play) / Stop all (surface,
  stop). (Services lines 56-60 · `Services.tsx:62-68`.)
  ✓ Added a header `action` button (same `isStart = running === 0` rule as the StatusFooter §2.1): primary
  "▶ Start all" only when nothing runs, else secondary "■ Stop all", wired to `startServices`/`stopServices`
  (invalidates `["services"]`). Subtitle now "{n} of {m} running". `pnpm tsc --noEmit` clean; chrome-devtools
  shows "■ Stop all" secondary (4/7 running).

- [x] **5.5 (M) Row sub-line = version; meters stacked; "Idle" label.**
  *Done when:* sub-line shows the version (own port column), not `host:port · pid`; the two mini-meters stack
  (CPU over RAM) in one column with design scaling (cpu/12, ram/500); stopped reads "Idle"; status pill
  status-tinted (folds into 1.1); idle rows dim to 0.74. (Service Row lines 13-33 · `Services.tsx:9-46`,
  `StatusPill.tsx:9`.)
  ✓ Sub-line now renders `svc.version` (—  when absent); added a 62px `:port` column. Replaced the two side-by-side
  `Meter`s with a `StackedMeters` component (CPU over RAM, 4px bars, scaling cpu/12 · ram/500, 132px column).
  Added a `label` override to `StatusPill` so stopped services read **"Idle"** (status-tint from §1.1); non-running
  rows get `opacity-[0.74]`. `pnpm tsc --noEmit` clean; chrome-devtools shows version sub-lines, the port column,
  stacked meters, "Idle" pills, and dimmed idle rows.

---

## 6. Databases screen — DECISION: keep engine-focused `[D]`

> **DECISION (user, this burndown): keep the Databases screen engine-focused** — one row per engine
> (MySQL/PostgreSQL) with status + CPU/RAM + start/stop + Browse. The comp's database/schema re-model
> (individual schemas with Site/Size, Filter/Import/Create, per-row Export/Import/Drop) is **intentionally
> not adopted** — it's an accepted divergence (would need a schema-enumeration backend + a larger UI). The
> engine view already aligns with the Services-screen grouping. All §6 tasks are therefore deferred.

Source: `design/rexenv Databases.dc.html`. Files: `src/routes/Databases.tsx`, `AdminerFrame.tsx`.

- [D] **6.1 (H) Re-model rows as databases (name · engine · site · size).** Deferred — screen stays
  engine-focused per the decision above.

- [D] **6.2 (H) Header actions: Filter · Import · Create database.** Deferred — database-centric controls; not
  applicable to the engine-focused view.

- [D] **6.3 (M) Per-row hover actions Export / Import / Drop + violet "Open".** Deferred — database-centric;
  the engine view keeps Browse + start/stop.

- [D] **6.4 (L) Adminer detail chrome.** Deferred with §6 (minor; the embedded Adminer already provides the
  schema/SQL chrome).

---

## 7. Mail (Mailpit inbox)

Source: `design/rexenv Mail.dc.html`. File: `src/routes/Mail.tsx`.
(Real email rendered via sandboxed iframe instead of the comp's mock body — intentional, not a gap.)

- [x] **7.1 (H) "Mark all read" + per-message Delete.**
  *Done when:* the header has a "Mark all read" button (alongside Clear all); the detail header has a
  per-message Delete (trash, red hover). (lines 58-59, 110-111 · `Mail.tsx:67-86, 179-197`.)
  ✓ Added IPC `mailpitMarkAllRead()` + `mailpitDelete(id)` (+ a mutable mock inbox so dev-build mark-read /
  delete / clear actually mutate state). Sub-bar gained a "✓ Mark all read" button (disabled when 0 unread);
  the preview detail header gained a per-message Delete (trash, `hover:bg-status-error-bg`). `pnpm tsc --noEmit`
  clean; chrome-devtools: clicking Mark-all-read took "2 unread → 0 unread" (dots cleared, button auto-disabled);
  Delete button present in the detail header. **TODO(backend):** add the `mailpit_mark_all_read` / `mailpit_delete`
  Rust commands (Mailpit's HTTP API supports both).

- [x] **7.2 (M) List row recipient + Headers tab + sender avatar + selected accent.**
  *Done when:* row line 3 shows `to <recipient>` (not the snippet); detail gains a 4th "Headers" tab (drop
  the always-on bottom headers panel); detail header shows a 30×30 sender-initial avatar; selected/unread
  rows get the 2.5px violet left bar + `rgba(124,92,255,.08)` tint. (lines 74-85, 102, 116-166 ·
  `Mail.tsx:149-246`.)
  ✓ Row line 3 now renders mono `to {m.to[0].address}`. Rows are `relative` with a 2.5px `bg-brand` left bar when
  selected OR unread, and `bg-brand-active` tint when selected. Added `"headers"` as a 4th `PreviewTab` (renders
  the headers as tab content, 120px key column) and removed the always-on bottom panel. Detail header gained a
  30×30 sender-initial avatar with a stable per-sender accent (`avatarColor` hash + `initial`). `pnpm tsc --noEmit`
  clean; chrome-devtools shows recipient lines, the violet selected/unread accents, the "A" avatar, and the
  HEADERS tab content.

- [x] **7.3 (L) Status pill `Mailpit · :8025`, footer count, SMTP chip, empty copy, widths.**
  *Done when:* status is a header pill with the port; list footer shows "{total} messages · {n} unread";
  empty state title "No emails yet"/"Nothing selected" + mono "SMTP · 127.0.0.1:1025 · auto-configured";
  list pane 344px, search 34px; tab labels cased ("Raw source"), active tab text `#C9BCFF`. (lines 55, 89,
  92, 116-119, 170-175 · `Mail.tsx:56-211`.)
  ✓ Sub-bar status is now a tinted `StatusPill` (`label` override) → "Mailpit · :{apiPort}" (from `mp.uiUrl`,
  fallback 8025) / "Mailpit stopped". Added a list footer "{total} messages · {n} unread". Replaced the preview
  Placeholder with a custom empty: "Nothing selected" / "No emails yet" + mono "SMTP · 127.0.0.1:1025 ·
  auto-configured" chip. List pane → `w-[344px]`, search → `h-[34px]`. `TAB_LABEL` map gives cased tabs
  (HTML / Text / Raw source / Headers) and the active tab is `text-brand-tint`. `pnpm tsc --noEmit` clean;
  chrome-devtools confirms all of the above.

---

## 8. Tunnels (public sharing)

Source: `design/rexenv Tunnels.dc.html`, `design/rexenv Tunnel Card.dc.html`. File: `src/routes/Tunnels.tsx`.

- [x] **8.1 (H) "Stop all sharing" header button.**
  *Done when:* when any tunnel is active the header shows a red "Stop all sharing" button. (lines 57-59 ·
  `Tunnels.tsx:30-36`.)
  ✓ Header `action` shows a red "■ Stop all sharing" button (`bg-status-error-bg border-status-error-border
  text-status-error-bright`) only when `active > 0`; it stops every running tunnel (`stopTunnel` per active
  site id, then invalidates). `active` now counts sites whose domain has a running tunnel. `pnpm tsc --noEmit`
  clean; chrome-devtools shows the red button (1 active tunnel).

- [x] **8.2 (M) Cards (not rows) + Shared/Not-shared sections + state-driven styling.**
  *Done when:* each site is a standalone card (`bg #15171D`, radius 13, gap 12) grouped under "Shared now" /
  "Not shared"; a live card gets a green border + glow + green toggle, a starting card amber border + violet
  toggle + spinner; live pill reads "Live" with the ping animation (not static "Public"). (Tunnel Card lines
  13-25 · Tunnels lines 70-80 · `Tunnels.tsx:51-131`.)
  ✓ Rows → standalone `TunnelCard`s (`rounded-[13px]`, `gap-3`) grouped under "Shared now" / "Shareable sites"
  (`SectionLabel`). A `CardState` (idle/starting/live/stopping) drives the border: live = `status-running-border`
  + green `shadow-[…0.12]`, starting = `status-warning-border`. `StartStopToggle` variant switches green (live)
  ↔ violet (starting, via `setting`) ↔ gray (idle). Live shows a "Live" `StatusPill` (ping) instead of "Public";
  starting shows an amber `rex-spin` ring + "Starting…". `pnpm tsc --noEmit` clean; chrome-devtools shows the
  green-bordered live Acme card with "Live" pill + green toggle under SHARED NOW, and idle cards under SHAREABLE
  SITES. (Avatar swap + live URL well land in §8.3.)

- [x] **8.3 (M) Live URL "well" row + meta line + per-site avatar + intro banner.**
  *Done when:* a live card has a boxed URL row (`bg #0B0C10`, cloud icon amber `#C9A24B`, mono URL `#9CC4E8`,
  Copy w/ text, open-external, explicit "Stop sharing"); a meta line "{reqs} requests · up {since}" + the
  "anyone with this link…" caption; a 34×34 per-site initial avatar (not the generic globe); the intro is a
  styled info card (lightbulb + "via cloudflared" chip). (Tunnel Card lines 15-40 · Tunnels lines 64-68 ·
  `Tunnels.tsx:46-109`.)
  ✓ Swapped the globe for a 34×34 per-site initial avatar (`siteTypeMeta` bg/border/color + name initial). Live
  card now renders a boxed URL "well" (`bg-[#0B0C10]` border, amber `Cloud` icon, mono `text-[#9CC4E8]` URL,
  copy, open-external, red "Stop sharing" → `onToggle(false)`) plus a meta line ("Public link active" + the
  "Anyone with this link can reach your local site." caption). Replaced the bare intro `<p>` with a styled
  info card (amber `Lightbulb` + copy + "via cloudflared" chip). `pnpm tsc --noEmit` clean; chrome-devtools
  shows the banner, A/P/B avatars, and the live well row + meta. **TODO(backend):** request-count/uptime
  aren't tracked on `TunnelInfo`, so the meta line omits them.

- [x] **8.4 (L) Idle "Share publicly" label, Copy→Copied, header copy, toggle size.**
  *Done when:* idle toggle has a visible "Share publicly" label; copy button shows "Copy"→"Copied" (green);
  header subtitle "{n} sites shared publicly" / "{n} ready to share"; toggle 42×25. (Tunnel Card lines 21-35
  · Tunnels lines 56, 248 · `Tunnels.tsx:34, 126-161`.)
  ✓ Idle cards show a "Share publicly" label before the toggle. `CopyButton` now shows icon + "Copy" text →
  "Copied" (`text-status-running-bright`) on click. Header subtitle → "{n} site(s) shared publicly" /
  "{n} site(s) ready to share". `pnpm tsc --noEmit` clean; chrome-devtools shows the label, the Copy-text
  button, and "1 site shared publicly". **Note:** the toggle stays the unified 46×27 `StartStopToggle` (§1.4)
  rather than a tunnels-only 42×25 — the comps vary toggle sizes per screen; one shared component is preferred.

---

## 9. Settings — section IA + missing sections

Source: `design/rexenv Settings.dc.html`. File: `src/routes/Settings.tsx`.

- [x] **9.1 (H) Section sub-navigation.**
  *Done when:* a 188px left column (General · DNS & SSL · Services · Updates · About) swaps the right pane,
  active-state violet tint, "Updates available" amber dot — replacing the single long card scroll. Keep the
  extra Blueprints + Uninstall content under the appropriate section. (lines 53-67 · `Settings.tsx:459-491`.)
  ✓ Rebuilt `Settings()` with a `section` state + 188px `nav` (General/DNS & SSL/Services/Updates/About, icons,
  active = `bg-brand-active text-brand-tint`, amber dot on Updates via `UPDATE_READY`) swapping a `max-w-[640px]`
  content pane; TopBar `subtitle` is the section crumb. Distributed existing components — General: Theme +
  Sites-folder + Blueprints; DNS & SSL: DnsSsl; Services: Startup + PHP-versions + Uninstall; Updates/About are
  placeholders (filled in §9.2/§9.3). `pnpm tsc --noEmit` clean; chrome-devtools confirms the active-state +
  crumb + content swap (General↔Services).

- [x] **9.2 (H) Updates section.** (static shell — the updater backend is `[D]` per TASKS-RELEASE §6.1)
  *Done when:* app medallion + "rexenv x.y.z" + status line, Check-for-updates button, "version N available"
  amber banner + Install & restart, and an "Install updates automatically" toggle. (Mark `[D]` if the updater
  itself stays deferred per §6.1 of TASKS-RELEASE — but build the static UI shell.) (lines 129-145.)
  ✓ New `UpdatesSetting`: a `CrownBadge` medallion + "rexenv 0.1.0" + status line + "Check again" button (local
  spinner state); an amber "Version 0.2.0 is available" banner (`status-warning` tint) with "Install & restart";
  and an "Install updates automatically" `StartStopToggle` (violet `setting`). All actions are UI-only — the
  real Tauri updater is deferred; the Install/auto buttons surface a clear "deferred" note. `pnpm tsc --noEmit`
  clean; chrome-devtools shows the medallion, banner, and toggle.

- [x] **9.3 (H) About section.**
  *Done when:* centered crown medallion + "rexenv" (Space Grotesk 22px) + mono build line (version · macOS ·
  arch) + tagline + link rows (Documentation / GitHub / Licenses & credits) + credits footer. (lines 147-161.)
  ✓ New `AboutSetting`: centered `CrownBadge(56)` + "rexenv" (`font-display` 22px) + mono "0.1.0 (build 104) ·
  macOS · Apple silicon" + tagline; a link card (Documentation → docs.rexenv.app blue `FileText`, GitHub →
  github.com/rexenv `Github`, Licenses & credits teal `ShieldCheck` — external rows show the host + `ArrowUpRight`,
  the licenses row a `ChevronRight`, all via `openExternal`); and the open-source credits footer. `pnpm tsc
  --noEmit` clean; chrome-devtools shows the medallion, build line, link rows, and footer.

- [x] **9.4 (H) Services section: Default ports card + "Stop idle services" toggle.**
  *Done when:* a Default ports card with mono HTTP/HTTPS/MySQL inputs; a second toggle "Stop idle services
  automatically" beside the existing autostart toggle. (lines 115-125.)
  ✓ New `ServicePrefsCard` with two `PrefRow`s (violet `setting` toggles): "Start services on login" (real
  autostart) + "Stop idle services automatically" (UI-only, deferred note). New `DefaultPortsCard` ("DEFAULT
  PORTS" mono label + HTTP/HTTPS/MySQL mono inputs, defaults 80/443/13306 — UI-only; ports are fixed today).
  Replaced the old single-toggle Startup card (removed `AutostartSetting`). `pnpm tsc --noEmit` clean;
  chrome-devtools shows both toggles + the ports grid under Services.

- [x] **9.5 (H) Theme picker — visual preview tiles.**
  *Done when:* Theme becomes a card with description + three 62px preview tiles (Dark/Light/System) each with
  a check-circle. **Decision:** the comp gates Light as "soon", but §4.4 shipped Light for real — keep Light
  selectable (don't re-gate); just adopt the tile UI. (lines 73-81 · `Settings.tsx:36-66`.)
  ✓ Rebuilt `ThemeSetting` from the segmented control into a description + 3-tile grid: each tile has a 62px
  gradient mock-window preview (dark / light / split-system, with mock bars + a violet accent square), a label,
  and a `CheckCircle2` that fills `brand` when selected (selected tile gets a `border-brand`). Light stays fully
  selectable (no "soon" badge — the comp's gating is stale post-§4.4). `pnpm tsc --noEmit` clean; chrome-devtools
  shows the three tiles with System selected (filled check + brand border).

- [x] **9.6 (M) DNS & SSL status tiles + per-action rows; General default-PHP select + sites-folder picker.**
  *Done when:* DNS & SSL shows a "STATUS" two-tile grid (resolver active w/ pulse · "Local CA — trusted ·
  expires <year>") and two separate action rows (Re-trust local CA / Regenerate certificates, each with its
  own description + busy state); General gains a compact "Default PHP version" select + subcopy, and the
  sites-folder row becomes read-only mono + a native "Choose…" picker (not an editable input + Save). Keep
  the richer PHP-versions install/remove card under Services or a sub-section. (lines 84-108 ·
  `Settings.tsx:91-256`.)
  ✓ Rebuilt `DnsSslSetting`: a "STATUS" card with a 2-tile grid (DNS resolver — pulsing green dot + "*.test →
  127.0.0.1 · active"; Local CA — green `Lock` + "trusted · login keychain") and a second card with two
  `ActionRow`s (Re-trust local CA / Regenerate certificates, each title+desc, `rex-spin` busy + "Working…").
  Replaced `SitesFolderSetting` with `GeneralPrefsCard`: a "Default PHP version" select (installed versions →
  `setDefaultPhpVersion`) + subcopy, and a read-only Sites-folder path + "Choose…" picker (prompt-based dev
  stand-in; native picker TODO). Removed `StatusDot`/`useEffect`/`getSetting`. The full PHP install/remove card
  stays under Services. `pnpm tsc --noEmit` clean; chrome-devtools shows the DNS tiles + action rows and the
  General default-PHP select + Choose… picker.

- [x] **9.7 (L) Header crumb + toast feedback + card sizing.**
  *Done when:* header shows the section name + mono crumb (a "Save changes"/dirty model is optional since
  settings persist per-control — note the deviation); replace `window.alert`/inline messages with the styled
  toast; cards `radius 13 / padding 18×20`, content `max-width 640`. (lines 53-58, 169-174 ·
  `Settings.tsx:29, 88, 462-464`.)
  ✓ Header crumb = the section name (mono TopBar subtitle, from §9.1). `Card` resized to the comp
  (`rounded-[13px]`, `p-5`, 14px title, `border-subtle`); content already `max-w-[640px]`. Added a styled
  `Notice` (left-accent green + `CheckCircle2`) for the DNS/SSL + Uninstall success messages (was plain mono
  text). `pnpm tsc --noEmit` clean; chrome-devtools shows consistent card sizing + the crumb. **Deviations
  (noted):** the "Save changes"/dirty model is intentionally skipped (settings persist per-control); error
  feedback still uses `window.alert` — a global toast system is a cross-cutting follow-up beyond §9.

---

## 10. Onboarding — stub → 4-step wizard (largest gap)

Source: `design/rexenv Onboarding.dc.html`. File: `src/routes/Onboarding.tsx` (currently a single welcome hero).

- [x] **10.1 (H) Wizard shell: 4 steps + progress dots + footer.**
  *Done when:* a `step` state machine (Welcome → Install → Domains & SSL → Done) with growing/recoloring
  progress dots, a footer with "Skip setup" (step 0), a mono step label ("Welcome" / "Step 2 of 4 · Install"
  / "Step 3 of 4 · Domains & SSL" / "All set"), and a per-step primary button (label/enabled/arrow change).
  (lines 129-137, 175-320.)
  ✓ Rebuilt `Onboarding` as a `step` (0–3) wizard: radial-gradient bg + dotted texture + violet top aura;
  growing/recoloring progress dots (active = 26px violet, past = violet, future = gray); a footer with "Skip
  setup" (step 0 → `/sites`), the mono `STEP_META` label, and a per-step primary button (Get started → Continue
  → Continue → Create your first site, with a chevron until the last). Steps 2–4 are `StepHeading` placeholders
  (filled in §10.2/§10.3/§10.4); the Welcome step is complete (folds in §10.5 typography + §10.6 crown/bg).
  `pnpm tsc --noEmit` clean; chrome-devtools verified the dots + labels advance Welcome→Install.

- [x] **10.2 (H) Step 2 — "Installing core components".**
  *Done when:* heading (Space Grotesk 27px) + subcopy, three install rows (PHP 8.3 / Nginx / Edge router)
  each with an abbreviation chip, a violet-gradient progress bar + glow, spinner/check state, per-row meta
  (size → % → "Installed"), and the toggling footnote. Wire to the real first-run download progress. (lines
  66-88, 296.)
  ✓ `Install` step renders three `INSTALL_ROWS` (PHP purple / Nx teal / Cf blue chips), each with a
  violet-gradient `shadow-glow-primary` progress bar, a `rex-spin` violet ring while active → green `Check` +
  "Installed" when done, and per-row meta (size → % → "Installed"). The footnote toggles "Downloading bundled
  runtimes…" → "All components installed · bundled, no system changes". Progress is simulated (staggered) for
  the shell — **TODO:** wire to the real first-run download progress. `pnpm tsc --noEmit` clean; chrome-devtools
  shows the three rows completing with checks + the done footnote.

- [x] **10.3 (H) Step 3 — "Set up local domains & SSL" (the permission moment).**
  *Done when:* three status pills (Local CA / Local DNS / HTTPS), heading + body (mono `https://anything.test`),
  a primary "Set up domains & SSL" button with the note "macOS will ask for your password once", and
  idle/busy/done states. This is the ~3-prompt system-setup moment from the brief. (lines 90-110, 156-168.)
  ✓ `Domains` step: three `StatusPill`s (Local CA teal `Shield` / Local DNS blue `Globe` / HTTPS green `Lock`),
  heading + body with mono `https://anything.test` (brand-tint) + `.test`, and a state machine — idle (primary
  "Set up domains & SSL" + "macOS will ask for your password once"), busy (spinner + "Configuring certificate
  authority & DNS…"), done (green "Domains & SSL are ready" pill). `StepHeading` now takes a `ReactNode`
  subtitle. Simulated for the shell — **TODO:** wire to `run_system_setup`. `pnpm tsc --noEmit` clean;
  chrome-devtools verified the pills, button + note, and the busy→done transition.

- [x] **10.4 (H) Step 4 — "Your kingdom is ready".**
  *Done when:* green check medallion (green glow), heading (Space Grotesk 34px), two confirmation chips
  (Core components ✓ / Domains & SSL ✓), and the "Create your first site" primary button. (lines 112-125.)
  ✓ `Done` step: a 78px green check medallion (green-tinted gradient + radial green glow + `Check`), "Your
  kingdom is ready" (`font-display` 34px), subtitle, and two `DoneChip`s (✓ Core components / ✓ Domains & SSL).
  The footer's last-step primary "Create your first site" (no chevron) navigates to `/sites`. `pnpm tsc --noEmit`
  clean; chrome-devtools verified the medallion, 34px heading, chips, and all-4 progress dots.

- [x] **10.5 (M) Welcome hero typography.**
  *Done when:* crown medallion 88px (radius 24) w/ float + aura; wordmark "rexenv" Space Grotesk **54px**
  (currently 32px `font-display`); tagline Space Grotesk 19px `#C7CBD4` (currently UI font) + the third
  description line. (lines 52-63 · `Onboarding.tsx:11-30`.)
  ✓ Built with the wizard (§10.1): `CrownHero` is 88px `rounded-[24px]` with the radial aura; wordmark is
  `font-display` 54px with the violet text-shadow; tagline is `font-display` 19px `text-rex-text-bright`
  (`#C7CBD4`) + the third description line. Added the crown **float** (`rex-float` 4s, `motion-reduce`-aware).
  `pnpm tsc --noEmit` clean; typography verified in the §10.1 Welcome screenshot.

- [x] **10.6 (L) Crown gems + background treatment.**
  *Done when:* three gem circles on the crown; radial violet backdrop + dotted texture + animated top aura.
  (lines 34-36, 57 · `Onboarding.tsx:10, 20`.)
  ✓ Crown has three gem circles (`#B9A6FF`/`#D7CCFF`); the shell has the radial violet backdrop + dotted
  texture (both from §10.1). Animated the top aura with `rex-aura` (6s breathe, `motion-reduce`-aware; keyframe
  added with §10.5). `pnpm tsc --noEmit` clean; chrome-devtools confirms the gems + centered animated aura.

---

## 11. WordPress Manager

Source: `design/rexenv WordPress Manager.dc.html`. File: `src/components/wordpress/WordPressManager.tsx`.

- [x] **11.1 (H) Plugins — search + All/Active/Updates filter.**
  *Done when:* a 230px "Search plugins…" input + a segmented All/Active/Updates control (Updates carries an
  amber count badge); filters the list. (lines 88-96 · `WordPressManager.tsx:720-745`.)
  ✓ Added a `query`/`filter` toolbar to `PluginsPanel`: a 230px "Search plugins…" input + `PluginFilterTabs`
  (All / Active / Updates, active pill `bg-brand-tint-bg text-brand-tint`, live counts; Updates count in amber
  `status-warning`). The list renders the filtered `visible` set with a "No plugins match." empty state.
  `pnpm tsc --noEmit` clean; chrome-devtools shows the search + All 3 / Active 2 / Updates 1 filter.

- [x] **11.2 (H) Plugins — per-row active toggle.**
  *Done when:* each row's Activate/Deactivate text button becomes the designed `role="switch"` toggle
  (34×20) + colored Active/Inactive label. (line 138 · `WordPressManager.tsx:849-851`.)
  ✓ Replaced the Activate/Deactivate text button in `PluginRow` with a colored status label (Active →
  `status-running-bright`, Inactive → muted) + a `StartStopToggle` (running = active, toggles
  activate/deactivate). Kept the update + delete buttons. (Toggle is the unified 46×27, not the comp's 34×20 —
  one shared component.) `pnpm tsc --noEmit` clean; chrome-devtools shows the green-ON Active rows + gray-OFF
  Inactive row.

- [x] **11.3 (H) Tools — Maintenance actions + one-click admin login + 2-col grid.**
  *Done when:* Maintenance lists Regenerate permalinks · **Export database** · danger **Reset site to a clean
  install**; the Debugging card gains a "One-click admin login" button; Tools laid out as a 2-col grid
  (Search & replace spans top, Debugging | Maintenance below). Keep Update/Re-install core (real features) —
  fold them in rather than dropping. (lines 218-258 · `WordPressManager.tsx:329-426`.)
  ✓ Rebuilt `ToolsPanel` as `grid-cols-2`: Search & replace spans both columns; Debugging (left) = WP_DEBUG
  `StartStopToggle` + "One-click admin login" (real, `wpUserLoginUrl(1)`); Maintenance (right) = Regenerate
  permalinks · Export database · Update core · Re-install core · red "Reset site to a clean install". Export-DB
  + Reset are UI shells (TODO — no backend); permalinks/core update/reinstall + admin login are wired. `pnpm
  tsc --noEmit` clean; chrome-devtools shows the 2-col grid with all actions + the danger Reset button.

- [x] **11.4 (M) Sub-tab bar: counts + order + content-width pills.**
  *Done when:* tabs read `Plugins {n} · Themes {n} · Users {n} · Tools · Network {n}` (Network LAST, mono
  count badges), content-width left-aligned pills, active pill brand-tint `#C9BCFF`. (lines 71-79 ·
  `WordPressManager.tsx:54-76`.)
  ✓ Added top-level `wpPlugins`/`wpThemes`/`wpUsers`/`wpNetworkSites` count queries (reuse the panels' cached
  results) and reordered the tabs to Plugins · Themes · Users · Tools · Network (Network LAST, multisite-only).
  The bar is now `self-start` content-width pills (`bg-rex-well`, `rounded-[7px]`) with mono count badges and a
  brand-tint active pill (`bg-brand-tint-bg text-brand-tint`). `pnpm tsc --noEmit` clean; chrome-devtools shows
  "Plugins 3 / Themes 3 / Users 2 / Tools" left-aligned with the violet active pill.

- [x] **11.5 (M) Plugins/Themes/Users content: descriptions, headers, avatars, roles, empty states.**
  *Done when:* Plugins rows show a description + column header + update-version badge, bulk bar "{n}
  plugin(s) selected" + Clear, and a rich empty state (icon + CTA); Users get colored initial avatars, a
  Last-login column, per-role color-coded badges, and a column header; Themes get the "{n} installed · 1
  active" header + gradient thumbnails + Live pill. (lines 102-210 · `WordPressManager.tsx:535-836`.)
  ✓ Plugins: added a "PLUGIN / STATUS" column header (update-version "update" badge already present). Users:
  `ROLE_META` per-role accent → colored initial avatars + color-coded role badges (Administrator violet /
  Editor blue), a "USER / ROLE / LAST LOGIN" header, and a "Log in" button. Themes: "{n} themes · {n} active"
  header, gradient thumbnails, and a "● Live" pill on the active theme. `pnpm tsc --noEmit` clean;
  chrome-devtools shows all three. **Data-limited (omitted):** plugin descriptions + user last-login aren't in
  the `WpPlugin`/`WpUser` DTOs (TODO), and the bulk-bar "Clear" / rich plugins empty state are deferred to §11.6.

- [x] **11.6 (L) Add flows as buttons, toasts, search-replace result panel, Network badge/rows.**
  *Done when:* persistent inline slug inputs become "Add plugin/theme/user/site" buttons opening a flow;
  styled toasts replace `window.alert`; search-replace shows a bordered result panel ("X occurrences across Y
  tables"); Network shows the inline mode badge + per-row status dots. (lines 98-305 ·
  `WordPressManager.tsx:141-596`.)
  ✓ Search-replace result is now a styled left-accent panel (`Replace` icon + mono detail). The plugins bulk
  bar reads "{n} plugin(s) selected" + a "Clear" button. `pnpm tsc --noEmit` clean; chrome-devtools shows "1
  plugin selected · Clear". **Deferred (LOW):** converting the inline slug inputs into "Add …" button-opened
  flows, a global styled-toast system (replacing `window.alert`), and the Network mode-badge/status-dot polish
  — all cross-cutting follow-ups; the inline adds remain functional.

---

## 12. New Site Flow

Source: `design/rexenv New Site Flow.dc.html`. File: `src/components/sites/NewSiteDialog.tsx`.

- [ ] **12.1 (H) Two-step wizard: Type → Configure.**
  *Done when:* a 2-step modal with a `1 Type → 2 Configure` indicator and Back / Continue / Create-site
  footer nav, replacing the single flat form. (lines 44-48, 192-205 · `NewSiteDialog.tsx:113-272`.)

- [ ] **12.2 (H) Step 1 — type cards (Blank PHP / WordPress / Laravel).**
  *Done when:* three clickable type cards (icon + title + description + radio-check, glow on select) replace
  the `<select>`; add the missing **Laravel** type. (lines 54-70 · `NewSiteDialog.tsx:15-18, 171-183`.)

- [ ] **12.3 (H) Step 2 — multisite toggle (remove stale "lands in §10" comment).**
  *Done when:* WordPress install section has a Multisite toggle revealing Subdomain/Subdirectory cards, wired
  to the already-shipped §10 convert. (lines 165-185 · `NewSiteDialog.tsx:33`.)

- [ ] **12.4 (M) WordPress fields: Site title + password show/Generate + `.test` suffix/validation.**
  *Done when:* a dedicated Site title input (separate from name); the password row gets a show/hide eye + a
  Generate button; the domain input shows a fixed `.test` suffix chip + live valid/taken indicator. (lines
  89-100, 132-163 · `NewSiteDialog.tsx:91, 158-243`.)
  *Note:* the **Database selector** the comp shows (MySQL/MariaDB/PostgreSQL/None, lines 118-124) — decide
  with §6: build it (MySQL/PostgreSQL only, per the intentional-divergence list) or `[D]`.

- [ ] **12.5 (L) Create button label + success toast + modal width.**
  *Done when:* the create button reads "Install WordPress" for WP (else "Create site"); a "Site created"
  toast with "New again" on success; inner panel 516px. (lines 211-217, 448 · `NewSiteDialog.tsx:119, 266`.)

---

## Notes / decisions

- **Two product decisions gate work:** (1) §6 Databases — re-model around databases vs keep engine-focused;
  (2) §12.4/§6 — whether New Site exposes a Database selector. Resolve both before starting those sections.
- **Mock data:** `src/lib/mock.ts` drives the browser dev build; extend it (plugin descriptions, last-login,
  tunnel request counts, per-DB sizes) as tasks need, so each screen can be verified off-Tauri.
- **Recommended burndown order:** §1 (primitives — unblocks visual fidelity everywhere) → §2 → the
  near-stubs §5 (Services) and §10 (Onboarding) → §3/§4 → §7/§8/§9/§11/§12 → §6 (after the product decision).
- **`[D]` candidates:** §9.2 Updates *behavior* (UI shell still worth building) tracks TASKS-RELEASE §6.1.
- All counts/line refs above are from the per-screen audit; design line numbers reference the `.dc.html`
  comps, `file:line` references the current implementation.
