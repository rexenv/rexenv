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

- [ ] **2.1 (M) StatusFooter — global toggle logic + Stop-all styling.**
  *Done when:* the toggle keys off `running === 0` (not `summary === "all"`): in **partial** state it shows
  **"Stop all"** with the *secondary* style (`bg #1C1F27 / border #2E323C / text #C7CBD4`), not "Start all"
  primary. Accent strip uses the translucent variants; stopped label `#8A90A0`; dot pulse only when
  `running > 0`. (App Shell lines 154, 261, 293-299 · `StatusFooter.tsx:34,51,88-91`.)

- [ ] **2.2 (L) Sidebar — Tunnels live-pulse dot + Services/Settings icons.**
  *Done when:* the Tunnels nav count is preceded by a green pulsing dot when a tunnel is active; Services
  icon → server-rack rects, Settings icon → sliders (comp uses these, impl uses `Layers`/`Settings` gear).
  (App Shell lines 50, 55, 58 · `nav.ts:24,27,28`, `Sidebar.tsx:50-54`.)

---

## 3. Sites screen

Source: `design/rexenv Sites Screen.dc.html`. File: `src/routes/Sites.tsx` (+ `TopBar.tsx`).

- [ ] **3.1 (H) Wire search + add All/Running/Stopped filter + sort.**
  *Done when:* the header search filters rows live by name/domain (today `TopBar` search has no
  `value`/`onChange` — it filters nothing); a segmented control `All · Running · Stopped` with live count
  badges (active pill violet `rgba(124,92,255,.14)/#C9BCFF`) filters by status; a "Change sort order" button
  cycles `Name → Status → Recent`. Drives a "No sites match …" empty state. (lines 99-118, 162-167, 345-348
  · `Sites.tsx:115-164`, `TopBar.tsx:30-43`.)

- [ ] **3.2 (H) Per-row kebab menu.**
  *Done when:* the inert `MoreHorizontal` button opens a menu: Rename · Duplicate · Open in editor ·
  Copy domain · ─ · **Delete** (red). Move Delete OUT of the row as a standalone icon into this menu.
  (lines 147-159, 205-211 · `Sites.tsx:69-80`.)

- [ ] **3.3 (H) Type avatar (W/L/P) + column-header strip.**
  *Done when:* each row leads with a 26×26 rounded badge showing the type letter on a per-type color
  (W blue `#7DB8D8`, L red `#EE837C`, P purple `#A7AADD`) instead of the generic globe; add the mono
  uppercase column-header strip (Stack 118px / Status 88px / Power). (lines 116-118, 125, 384-388 ·
  `Sites.tsx:38-40`.)

- [ ] **3.4 (M) Row action placement + SSL indicator + empty state.**
  *Done when:* Open-in-browser / Open-folder / Open-database reveal on hover on the LEFT (right of the name),
  status/php/server/toggle/kebab stay right; SSL shows closed-lock gray `#7E8492` when secure, **open-lock
  amber `#C99A3A` when not** (today: lock only when ssl, colored green); empty state = violet-gradient crown,
  copy "Point rexenv at a folder…", button "Create your first site". (lines 130-139, 171-180 ·
  `Sites.tsx:37-81, 138-150`.)

- [ ] **3.5 (L) Row container + badge sizing + PHP label.**
  *Done when:* rows are standalone 44px rounded elements (radius 9px, hover fill `#15171D`, no dividers),
  name column fixed 188px, server badge fixed 84px centered, status pill 92px; PHP badge shows `8.3` not
  `PHP 8.3`. (lines 124-142, 397 · `Sites.tsx:37-49, 152`.)

---

## 4. Site Detail screen

Source: `design/rexenv Site Detail.dc.html`. File: `src/routes/SiteDetail.tsx`.

- [ ] **4.1 (H) Header action cluster + identity + back link.**
  *Done when:* the header (replacing the plain `TopBar`) shows: a `‹ All sites` back link; a 38×38 type
  avatar; the site name (19px) with an inline status pill; `domain · :<port>` beneath; and a right cluster of
  start/stop toggle + "Open in browser" (secondary) + "Open admin" (primary, gear). (lines 76-95 ·
  `SiteDetail.tsx:102`.)

- [ ] **4.2 (H) Environment card — 3-column mini-card grid + SSL certificate.**
  *Done when:* Environment renders as three mini-cards — PHP version (big 18px mono value + dropdown), Web
  server (value + dropdown), **SSL certificate** (green lock + "Trusted" + mono "rexenv CA") — instead of
  plain label/value rows; the SSL sub-card is currently absent. (lines 112-142 · `SiteDetail.tsx:229-263`.)

- [ ] **4.3 (M) Recent-logs preview (remove stale placeholder).**
  *Done when:* the Recent-logs card shows a real log-preview box (last lines, color-coded by level) + a
  "View all logs ›" link, replacing the "Live log tailing arrives in §3 — this peek will show…" placeholder
  (LogsTab is already fully implemented in this file). (lines 186-198 · `SiteDetail.tsx:273-277`.)

- [ ] **4.4 (M) Overview layout: card order + Quick links grid + Paths config row.**
  *Done when:* order is Environment → (Paths | Quick links two-column 1.25fr/1fr) → Recent logs; Quick links
  is a 2-col tile grid with colored icons (Browser/WP admin/Database/Terminal) + a full-width "Open project
  folder"; Paths shows `Project path` + `Config path` (recessed boxes w/ copy + open-folder). (lines 110-199
  · `SiteDetail.tsx:204-277`.)

- [ ] **4.5 (L) Tab bar alignment + content width.**
  *Done when:* tabs are left-aligned full-width (not `mx-auto max-w-2xl`), WordPress tab tinted blue when
  inactive; Overview content fills the panel (drop `max-w-2xl`). (lines 97-105 · `SiteDetail.tsx:104-131`.)

---

## 5. Services screen (near-stub — big rebuild)

Source: `design/rexenv Services.dc.html`, `design/rexenv Service Row.dc.html`. File: `src/routes/Services.tsx`.

- [ ] **5.1 (H) "Total resource usage" summary card.**
  *Done when:* a gradient card at top: live pulse dot, "TOTAL RESOURCE USAGE", "live · updates every 1.5s",
  big CPU number + meter, big Memory number + meter ("of N GB budget · stays light"), running/idle counts.
  (Services lines 65-87.)

- [ ] **5.2 (H) Section grouping + per-section summaries.**
  *Done when:* services group into PHP · Databases · Mail · Web servers & edge router, each a bordered card
  with a colored icon + title + mono "{r}/{n} running" — replacing the single flat list. (Services lines
  89-107 · `Services.tsx:77-82`.)

- [ ] **5.3 (H) Per-row toggle + action button + badges.**
  *Done when:* each row gets a start/stop toggle and a contextual action (PHP non-default → "Set default";
  DB → "Open in browser"; Mailpit → "Open inbox"; web → "Logs"), a 30×30 accent badge (My/8.3/Nx),
  and "default"/"edge router" pills where applicable (needs `ServiceInfo` to carry `kind`/`isDefault`/
  `isRouter`/`version`). (Service Row lines 15-40 · `Services.tsx:26-49`, `src/types/index.ts:75-82`.)

- [ ] **5.4 (H) Global Start all / Stop all in header.**
  *Done when:* the header has the primary button toggling Start all (violet, play) / Stop all (surface,
  stop). (Services lines 56-60 · `Services.tsx:62-68`.)

- [ ] **5.5 (M) Row sub-line = version; meters stacked; "Idle" label.**
  *Done when:* sub-line shows the version (own port column), not `host:port · pid`; the two mini-meters stack
  (CPU over RAM) in one column with design scaling (cpu/12, ram/500); stopped reads "Idle"; status pill
  status-tinted (folds into 1.1); idle rows dim to 0.74. (Service Row lines 13-33 · `Services.tsx:9-46`,
  `StatusPill.tsx:9`.)

---

## 6. Databases screen — product decision needed

> **Conceptual mismatch:** the comp lists individual **databases/schemas** (name · engine · owning site ·
> size, with Filter/Import/Create + per-row Export/Import/Drop and a themed schema browser). The impl lists
> **engine processes** (MySQL/PostgreSQL) with status + CPU/RAM + start/stop. The engine start/stop +
> meters belong on the **Services** screen in the comps, not here. **Decide before building:** (a) re-model
> this screen around databases (matches comp, large), or (b) keep it engine-focused and accept the
> divergence. The tasks below assume (a).

Source: `design/rexenv Databases.dc.html`. Files: `src/routes/Databases.tsx`, `AdminerFrame.tsx`.

- [ ] **6.1 (H) Re-model rows as databases (name · engine · site · size).**
  *Done when:* one row per database with an engine-colored avatar + engine pill, owning-site chip, and a
  right-aligned size; header subtitle "{count} databases · {total} total"; a column-header row. (Needs an IPC
  to enumerate schemas per engine; size where the engine exposes it, else "—".) (lines 70-81, 203-212 ·
  `Databases.tsx:133-148`.)

- [ ] **6.2 (H) Header actions: Filter · Import · Create database.**
  *Done when:* the header has a "Filter…" search, an Import button, and a primary "Create database" button;
  filter drives a "No databases match …" empty state. (lines 61-64, 92-94 · `Databases.tsx:120-124`.)

- [ ] **6.3 (M) Per-row hover actions Export / Import / Drop + violet "Open".**
  *Done when:* rows reveal Export, Import, Drop (red) on hover; the open-Adminer button is the violet "Open"
  (monitor icon), not the neutral "Browse". Remove the per-row status pill / meters / toggle (those move to
  Services). (lines 84-88 · `Databases.tsx:58-74`.)

- [ ] **6.4 (L) Adminer detail chrome.**
  *Done when:* the detail bar shows engine badge + host + "Adminer x.y · themed" pill + open-external + close
  (X) around the iframe (the schema sidebar / SQL tabs stay delegated to embedded Adminer). (lines 101-163 ·
  `Databases.tsx:96-116`.)

---

## 7. Mail (Mailpit inbox)

Source: `design/rexenv Mail.dc.html`. File: `src/routes/Mail.tsx`.
(Real email rendered via sandboxed iframe instead of the comp's mock body — intentional, not a gap.)

- [ ] **7.1 (H) "Mark all read" + per-message Delete.**
  *Done when:* the header has a "Mark all read" button (alongside Clear all); the detail header has a
  per-message Delete (trash, red hover). (lines 58-59, 110-111 · `Mail.tsx:67-86, 179-197`.)

- [ ] **7.2 (M) List row recipient + Headers tab + sender avatar + selected accent.**
  *Done when:* row line 3 shows `to <recipient>` (not the snippet); detail gains a 4th "Headers" tab (drop
  the always-on bottom headers panel); detail header shows a 30×30 sender-initial avatar; selected/unread
  rows get the 2.5px violet left bar + `rgba(124,92,255,.08)` tint. (lines 74-85, 102, 116-166 ·
  `Mail.tsx:149-246`.)

- [ ] **7.3 (L) Status pill `Mailpit · :8025`, footer count, SMTP chip, empty copy, widths.**
  *Done when:* status is a header pill with the port; list footer shows "{total} messages · {n} unread";
  empty state title "No emails yet"/"Nothing selected" + mono "SMTP · 127.0.0.1:1025 · auto-configured";
  list pane 344px, search 34px; tab labels cased ("Raw source"), active tab text `#C9BCFF`. (lines 55, 89,
  92, 116-119, 170-175 · `Mail.tsx:56-211`.)

---

## 8. Tunnels (public sharing)

Source: `design/rexenv Tunnels.dc.html`, `design/rexenv Tunnel Card.dc.html`. File: `src/routes/Tunnels.tsx`.

- [ ] **8.1 (H) "Stop all sharing" header button.**
  *Done when:* when any tunnel is active the header shows a red "Stop all sharing" button. (lines 57-59 ·
  `Tunnels.tsx:30-36`.)

- [ ] **8.2 (M) Cards (not rows) + Shared/Not-shared sections + state-driven styling.**
  *Done when:* each site is a standalone card (`bg #15171D`, radius 13, gap 12) grouped under "Shared now" /
  "Not shared"; a live card gets a green border + glow + green toggle, a starting card amber border + violet
  toggle + spinner; live pill reads "Live" with the ping animation (not static "Public"). (Tunnel Card lines
  13-25 · Tunnels lines 70-80 · `Tunnels.tsx:51-131`.)

- [ ] **8.3 (M) Live URL "well" row + meta line + per-site avatar + intro banner.**
  *Done when:* a live card has a boxed URL row (`bg #0B0C10`, cloud icon amber `#C9A24B`, mono URL `#9CC4E8`,
  Copy w/ text, open-external, explicit "Stop sharing"); a meta line "{reqs} requests · up {since}" + the
  "anyone with this link…" caption; a 34×34 per-site initial avatar (not the generic globe); the intro is a
  styled info card (lightbulb + "via cloudflared" chip). (Tunnel Card lines 15-40 · Tunnels lines 64-68 ·
  `Tunnels.tsx:46-109`.)

- [ ] **8.4 (L) Idle "Share publicly" label, Copy→Copied, header copy, toggle size.**
  *Done when:* idle toggle has a visible "Share publicly" label; copy button shows "Copy"→"Copied" (green);
  header subtitle "{n} sites shared publicly" / "{n} ready to share"; toggle 42×25. (Tunnel Card lines 21-35
  · Tunnels lines 56, 248 · `Tunnels.tsx:34, 126-161`.)

---

## 9. Settings — section IA + missing sections

Source: `design/rexenv Settings.dc.html`. File: `src/routes/Settings.tsx`.

- [ ] **9.1 (H) Section sub-navigation.**
  *Done when:* a 188px left column (General · DNS & SSL · Services · Updates · About) swaps the right pane,
  active-state violet tint, "Updates available" amber dot — replacing the single long card scroll. Keep the
  extra Blueprints + Uninstall content under the appropriate section. (lines 53-67 · `Settings.tsx:459-491`.)

- [ ] **9.2 (H) Updates section.**
  *Done when:* app medallion + "rexenv x.y.z" + status line, Check-for-updates button, "version N available"
  amber banner + Install & restart, and an "Install updates automatically" toggle. (Mark `[D]` if the updater
  itself stays deferred per §6.1 of TASKS-RELEASE — but build the static UI shell.) (lines 129-145.)

- [ ] **9.3 (H) About section.**
  *Done when:* centered crown medallion + "rexenv" (Space Grotesk 22px) + mono build line (version · macOS ·
  arch) + tagline + link rows (Documentation / GitHub / Licenses & credits) + credits footer. (lines 147-161.)

- [ ] **9.4 (H) Services section: Default ports card + "Stop idle services" toggle.**
  *Done when:* a Default ports card with mono HTTP/HTTPS/MySQL inputs; a second toggle "Stop idle services
  automatically" beside the existing autostart toggle. (lines 115-125.)

- [ ] **9.5 (H) Theme picker — visual preview tiles.**
  *Done when:* Theme becomes a card with description + three 62px preview tiles (Dark/Light/System) each with
  a check-circle. **Decision:** the comp gates Light as "soon", but §4.4 shipped Light for real — keep Light
  selectable (don't re-gate); just adopt the tile UI. (lines 73-81 · `Settings.tsx:36-66`.)

- [ ] **9.6 (M) DNS & SSL status tiles + per-action rows; General default-PHP select + sites-folder picker.**
  *Done when:* DNS & SSL shows a "STATUS" two-tile grid (resolver active w/ pulse · "Local CA — trusted ·
  expires <year>") and two separate action rows (Re-trust local CA / Regenerate certificates, each with its
  own description + busy state); General gains a compact "Default PHP version" select + subcopy, and the
  sites-folder row becomes read-only mono + a native "Choose…" picker (not an editable input + Save). Keep
  the richer PHP-versions install/remove card under Services or a sub-section. (lines 84-108 ·
  `Settings.tsx:91-256`.)

- [ ] **9.7 (L) Header crumb + toast feedback + card sizing.**
  *Done when:* header shows the section name + mono crumb (a "Save changes"/dirty model is optional since
  settings persist per-control — note the deviation); replace `window.alert`/inline messages with the styled
  toast; cards `radius 13 / padding 18×20`, content `max-width 640`. (lines 53-58, 169-174 ·
  `Settings.tsx:29, 88, 462-464`.)

---

## 10. Onboarding — stub → 4-step wizard (largest gap)

Source: `design/rexenv Onboarding.dc.html`. File: `src/routes/Onboarding.tsx` (currently a single welcome hero).

- [ ] **10.1 (H) Wizard shell: 4 steps + progress dots + footer.**
  *Done when:* a `step` state machine (Welcome → Install → Domains & SSL → Done) with growing/recoloring
  progress dots, a footer with "Skip setup" (step 0), a mono step label ("Welcome" / "Step 2 of 4 · Install"
  / "Step 3 of 4 · Domains & SSL" / "All set"), and a per-step primary button (label/enabled/arrow change).
  (lines 129-137, 175-320.)

- [ ] **10.2 (H) Step 2 — "Installing core components".**
  *Done when:* heading (Space Grotesk 27px) + subcopy, three install rows (PHP 8.3 / Nginx / Edge router)
  each with an abbreviation chip, a violet-gradient progress bar + glow, spinner/check state, per-row meta
  (size → % → "Installed"), and the toggling footnote. Wire to the real first-run download progress. (lines
  66-88, 296.)

- [ ] **10.3 (H) Step 3 — "Set up local domains & SSL" (the permission moment).**
  *Done when:* three status pills (Local CA / Local DNS / HTTPS), heading + body (mono `https://anything.test`),
  a primary "Set up domains & SSL" button with the note "macOS will ask for your password once", and
  idle/busy/done states. This is the ~3-prompt system-setup moment from the brief. (lines 90-110, 156-168.)

- [ ] **10.4 (H) Step 4 — "Your kingdom is ready".**
  *Done when:* green check medallion (green glow), heading (Space Grotesk 34px), two confirmation chips
  (Core components ✓ / Domains & SSL ✓), and the "Create your first site" primary button. (lines 112-125.)

- [ ] **10.5 (M) Welcome hero typography.**
  *Done when:* crown medallion 88px (radius 24) w/ float + aura; wordmark "rexenv" Space Grotesk **54px**
  (currently 32px `font-display`); tagline Space Grotesk 19px `#C7CBD4` (currently UI font) + the third
  description line. (lines 52-63 · `Onboarding.tsx:11-30`.)

- [ ] **10.6 (L) Crown gems + background treatment.**
  *Done when:* three gem circles on the crown; radial violet backdrop + dotted texture + animated top aura.
  (lines 34-36, 57 · `Onboarding.tsx:10, 20`.)

---

## 11. WordPress Manager

Source: `design/rexenv WordPress Manager.dc.html`. File: `src/components/wordpress/WordPressManager.tsx`.

- [ ] **11.1 (H) Plugins — search + All/Active/Updates filter.**
  *Done when:* a 230px "Search plugins…" input + a segmented All/Active/Updates control (Updates carries an
  amber count badge); filters the list. (lines 88-96 · `WordPressManager.tsx:720-745`.)

- [ ] **11.2 (H) Plugins — per-row active toggle.**
  *Done when:* each row's Activate/Deactivate text button becomes the designed `role="switch"` toggle
  (34×20) + colored Active/Inactive label. (line 138 · `WordPressManager.tsx:849-851`.)

- [ ] **11.3 (H) Tools — Maintenance actions + one-click admin login + 2-col grid.**
  *Done when:* Maintenance lists Regenerate permalinks · **Export database** · danger **Reset site to a clean
  install**; the Debugging card gains a "One-click admin login" button; Tools laid out as a 2-col grid
  (Search & replace spans top, Debugging | Maintenance below). Keep Update/Re-install core (real features) —
  fold them in rather than dropping. (lines 218-258 · `WordPressManager.tsx:329-426`.)

- [ ] **11.4 (M) Sub-tab bar: counts + order + content-width pills.**
  *Done when:* tabs read `Plugins {n} · Themes {n} · Users {n} · Tools · Network {n}` (Network LAST, mono
  count badges), content-width left-aligned pills, active pill brand-tint `#C9BCFF`. (lines 71-79 ·
  `WordPressManager.tsx:54-76`.)

- [ ] **11.5 (M) Plugins/Themes/Users content: descriptions, headers, avatars, roles, empty states.**
  *Done when:* Plugins rows show a description + column header + update-version badge, bulk bar "{n}
  plugin(s) selected" + Clear, and a rich empty state (icon + CTA); Users get colored initial avatars, a
  Last-login column, per-role color-coded badges, and a column header; Themes get the "{n} installed · 1
  active" header + gradient thumbnails + Live pill. (lines 102-210 · `WordPressManager.tsx:535-836`.)

- [ ] **11.6 (L) Add flows as buttons, toasts, search-replace result panel, Network badge/rows.**
  *Done when:* persistent inline slug inputs become "Add plugin/theme/user/site" buttons opening a flow;
  styled toasts replace `window.alert`; search-replace shows a bordered result panel ("X occurrences across Y
  tables"); Network shows the inline mode badge + per-row status dots. (lines 98-305 ·
  `WordPressManager.tsx:141-596`.)

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
