# rexenv — Claude Design Brief

> A paste-ready brief. Copy the **prompt blocks below into Claude Design, in order**. Each block is in English (most reliable for a design tool). Run them in the **same Claude Design session/canvas** so all screens stay consistent. Start with Block 1 (design system), then the rest.

---

## Design DNA (summary — why this direction)

- **Grounded in the subject:** "rex" = king. So rexenv is the developer's **command room** for their local kingdom — a calm, fast control panel to run every server/site/database and see at a glance what's running.
- **Signature element (where the boldness is spent):** the live **status system** — status pills + a global resource meter in the sidebar — which makes the app feel like a living control panel. Everything else stays quiet and disciplined. Plus a restrained violet crown mark (brand).
- **Palette (dark-first):** a royal-violet brand accent (regal — fits "rex", and doesn't clash with the green/red of status).
  - `--bg #0D0E12` · `--surface-1 #15171D` · `--surface-2 #1C1F27` · `--border #262A33`
  - `--text #E7E9EE` · `--text-muted #8A90A0`
  - `--brand (royal violet) #7C5CFF`
  - status: running `#3FB950` · stopped/idle `#6E7681` · error `#F85149` · warning `#D29922`
- **Typography (3 deliberate roles):** display = **Space Grotesk** (hero/onboarding only, restrained); UI/body = **SF Pro Text** (native macOS feel; Inter fallback); mono = **JetBrains Mono** for all technical values (domain, path, PHP version, port, command, log).
- **What to avoid (AI-design clichés):** (1) cream + serif + terracotta, (2) near-black + a single acid-green/vermilion accent, (3) broadsheet hairline-column look. Our direction is different: dark + royal violet + a crafted status system.

---

## Paste-ready prompt blocks

### Block 1 — Design system & foundation (paste this first)

```
Create the design system foundation for "rexenv" — a native, lightweight local development environment for web/WordPress developers, macOS-first (later Windows/Linux). The product concept: a calm, fast "command room" for a developer's local kingdom ("rex" = king) where they run all their servers, sites, and databases and see at a glance what's running.

Style direction (follow exactly; do NOT default to generic AI looks like cream+serif+terracotta, near-black+acid-green, or broadsheet hairlines):
- Dark-first, dense but breathable, technical and precise.
- Palette: bg #0D0E12, surface-1 #15171D, surface-2 #1C1F27, border #262A33, text #E7E9EE, text-muted #8A90A0. Brand accent = royal violet #7C5CFF (used sparingly: primary actions, active nav, focus rings, logo). Status colors: running #3FB950, stopped/idle #6E7681, error #F85149, warning #D29922.
- Type roles: display = Space Grotesk (hero moments only, restrained); UI/body = SF Pro Text (Inter fallback); monospace = JetBrains Mono for ALL technical values (domains, paths, versions, ports, commands).
- Signature: a polished live "status" language — status pills and toggles that feel crafted and alive. A small, restrained violet crown mark for the brand.

Design a component sheet with all states:
- Buttons: primary (violet), secondary, ghost, danger; default/hover/pressed/disabled.
- Status pill: running / stopped / starting / error (dot + label).
- Toggle switch (on/off) used for start/stop.
- Text input, select dropdown, search field (with states + focus ring in violet).
- Table row (with hover + selected), badge (version, "update available", "network active"), tab bar, sidebar nav item (default/hover/active), card/panel, modal shell, toast (success/error), progress bar, and an empty-state block.
- rexenv wordmark + crown mark.

Quality floor: visible keyboard focus, reduced-motion friendly, plan for a light theme variant later. Keep everything quiet except the status system.
```

### Block 2 — App shell (sidebar + top bar + global status)

```
Design the main app shell for rexenv, matching the established design system (dark, royal-violet accent, JetBrains Mono for technical values).

Layout:
- Left sidebar (~220px): rexenv wordmark + crown at top; primary nav with icons — Sites, Services, Databases, Mail, Tunnels, Settings (Sites is active).
- Sidebar footer = the SIGNATURE live status block: an overall status indicator (All running / Partial / Stopped), a compact total RAM and CPU meter (small live bars), and a global "Stop all / Start all" control. This is what makes the app feel like a living control room — make it feel crafted.
- Main area: a top context bar (current section title on the left, a contextual primary action on the right e.g. "+ New site", optional global search), then the content region below (show a placeholder).
- macOS-native chrome: leave room for traffic-light window controls and a draggable title region; clean, minimal.

Show sidebar item states (default/hover/active) and the global status block in two states (all running vs partial).
```

### Block 3 — Sites list (home / dashboard)

```
Design the Sites screen for rexenv (the home screen), inside the app shell. Match the design system.

It's a list/table of local sites — the heart of the app. Each row shows:
- A small type icon (WordPress / Laravel / plain PHP).
- Site name (bold) and its domain in monospace (e.g. mysite.test).
- A status pill + a start/stop toggle (running = green).
- PHP version badge (mono, e.g. 8.3) and web server badge (Nginx / Apache / OpenLiteSpeed).
- An SSL lock indicator.
- On row hover: quick actions — Open in browser, Open folder, Open database, and a "..." menu (rename, duplicate, delete, etc.).

Top of screen: "+ New site" primary button, a search/filter field, and a sort control. Rows ~44px, dense but readable.

Include an empty state for when there are no sites: a short headline ("No sites yet"), one line of guidance, and a primary "Create your first site" button (active-voice copy, inviting not apologetic).
```

### Block 4 — New Site flow (wizard / modal)

```
Design the "New site" flow for rexenv as a focused modal (or right-side panel). Match the design system.

Step 1 — Choose a type: three large selectable cards — "Blank PHP", "WordPress", "Laravel" (each with icon + one-line description).

Step 2 — Configure:
- Site name -> auto-fills the domain shown in monospace as name.test (editable).
- PHP version (select), web server (select: Nginx default / Apache / OpenLiteSpeed), database (select: MySQL / MariaDB / PostgreSQL / none).
- If WordPress is chosen, also: site title, admin username, admin email, admin password, language. Plus a "Multisite" toggle — when ON, reveal a choice between "Subdomain" and "Subdirectory" with a one-line plain-language explanation of each (e.g. "Subdomain: site1.mysite.test" vs "Subdirectory: mysite.test/site1").

Footer: "Cancel" and a primary "Create site" button. Show inline validation (e.g. domain already in use). Keep copy in active voice and user-facing terms.
```

### Block 5 — Site detail: Overview tab

```
Design the Site Detail screen for rexenv (Overview tab). Match the design system.

Top: tab bar — Overview, WordPress, Database, Logs, Settings. (The WordPress tab only appears for WordPress sites.) Header row: site name + domain (mono) + status pill & start/stop toggle + buttons "Open in browser" and "Open admin".

Overview content as cards:
- Environment: PHP version with an inline switch, web server with an inline switch, SSL status (trusted/lock).
- Paths: project path and config path (monospace, each with a copy button + "open folder").
- Quick links: Browser, WP admin, Database, Terminal (open in app's built-in terminal), Folder.
- A small "Recent logs" peek (last few lines, mono) with a link to the full Logs tab.

Use monospace for all technical values. Keep it calm and scannable.
```

### Block 6 — WordPress Manager (the signature feature — make it polished)

```
Design the WordPress Manager for rexenv — this is the WordPress tab inside Site Detail and the product's signature feature, so make it feel especially crafted. Match the design system.

Sub-tabs: Plugins, Themes, Users, Tools (plus a "Network" sub-tab that appears only for multisite).

Plugins (default): a searchable table — bulk-select checkbox; plugin name + short description; version (mono); an "update available" badge when relevant; a status toggle (Active / Inactive); a "Network active" badge for multisite; row actions (Activate/Deactivate, Update, Delete). A bulk-action bar appears when rows are selected. An "Add plugin" action (by slug or upload .zip). Include an empty state.

Themes: a grid of theme cards — screenshot thumbnail, theme name, version, an "Active" badge on the live one, and Activate / Update / Delete actions. An "Add theme" action.

Users: a table — username, email, role, last login — with a prominent "Log in as" button per row (one-click admin login) and an "Add user" action.

Tools: a WP_DEBUG toggle; a Search-replace tool (old URL -> new URL, a "dry run" checkbox, and a Run button); one-click admin login; regenerate permalinks; export/reset.

Network (multisite only): a mode badge (Subdomain / Subdirectory); a list of network sites (each: URL in mono, status, Visit, Admin, Delete); an "Add site" action; and super-admin management.

Keep technical values in monospace. Make the table dense but easy to scan; status and "update available" states should read instantly.
```

### Block 7 — Services panel

```
Design the Services screen for rexenv. Match the design system. This reinforces the "control room" feel and the "lightweight" promise (show resource usage).

Group services into sections, each row showing: name, status pill + start/stop toggle, version (mono), port (mono), and a small RAM/CPU mini-meter.
- PHP: multiple installed versions, each with status and a "Set default" action.
- Databases: MySQL, MariaDB, PostgreSQL, Redis — each with toggle, version, port, and "Open in database browser".
- Mail: Mailpit — status + "Open inbox".
- Web servers / edge router: Nginx, Apache, OpenLiteSpeed availability and status.

Top: a global "Start all / Stop all" control and total resource usage. Make resource meters feel precise and alive (part of the signature).
```

### Block 8 — Database browser

```
Design the Databases screen for rexenv. Match the design system.

Show a list of databases — name, size, engine (MySQL/MariaDB/PostgreSQL), and which site it belongs to — with actions: Open, Create, Drop, Import, Export. Selecting "Open" reveals an embedded database admin panel (it wraps Adminer) themed to match rexenv's dark look (don't redesign Adminer's internals — just frame and theme the container). Keep it simple and utilitarian.
```

### Block 9 — Mail (Mailpit inbox)

```
Design the Mail screen for rexenv (catches all outgoing email from local sites, via Mailpit). Match the design system.

Two-pane layout: left = a list of captured emails (from, to, subject, time, unread dot); right = a preview pane with tabs for HTML / Text / Raw source and a headers section. Add a search field and a "Clear all" action. Empty state copy: invite, don't apologize — e.g. "No emails yet — anything your local sites send will show up here."
```

### Block 10 — Tunnels (public sharing)

```
Design the Tunnels screen for rexenv (share a local site publicly via a free Cloudflare tunnel). Match the design system.

For each shareable site, a row/card: site name + a "Share publicly" toggle. When active, show the generated public URL (monospace + copy button), a live status indicator, and a "Stop sharing" action. Include a one-line explanation that this creates a free, temporary public link. Empty state: explain what sharing does and invite the user to enable it on a site.
```

### Block 11 — Settings

```
Design the Settings screen for rexenv. Match the design system. Use a left sub-navigation with a right content panel (or grouped cards).

Sections:
- General: theme (Dark / Light / System), default PHP version, sites folder location.
- DNS & SSL: re-trust local CA, regenerate certificates, DNS status indicator.
- Services: "Start services on login" toggle, default ports.
- Updates: current version + "Check for updates".
- About: version, links (docs, GitHub), credits.

Keep labels in plain, user-facing language and active voice.
```

### Block 12 — First-run onboarding

```
Design the first-run onboarding for rexenv as a short multi-step flow (numbered steps are appropriate here because it's a real sequence). This is the ONE place to use the Space Grotesk display face and a slightly bolder brand moment with the violet crown.

Steps:
1) Welcome — brand hero (crown + wordmark in the display face) with a one-line value prop ("Your local development environment — fast, native, all in one").
2) Install core components — a progress view downloading PHP, Nginx, and the edge router, each with its own progress row.
3) Set up local domains & SSL — explain in plain language that rexenv will add a local certificate authority and configure local DNS so .test sites work over HTTPS; show the step where the OS asks for permission.
4) Done — a confirmation with a primary "Create your first site" button.

Keep onboarding calm and confident; explain what's happening and why, in the interface's own voice.
```

---

## Iteration tips (in Claude Design)

- **Don't ask for everything at once** — establish the design system with Block 1, then the shell, then one screen at a time. Ask it to carry the previous block's visual language forward.
- **Refine via chat:** "increase table density", "make the status toggle more prominent", "tighten the empty-state copy", "snap spacing to an 8px grid", etc.
- **Consistency:** when asking for a new screen, say "same shell, same components, same tokens".
- **Always ask for states** on each screen: hover, active, empty, loading, error — without these you'll get stuck during the build.
- **Copy:** buttons = verbs ("Start site", "Create site" — not "Submit"); errors say what happened and how to fix it; an empty screen is an invitation to act.
- When the design is final, **export the tokens and components** — they feed directly into the Tauri + React + Tailwind + shadcn/ui build.
