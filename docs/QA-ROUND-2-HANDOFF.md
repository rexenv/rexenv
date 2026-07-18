# QA handoff — round 2 deep test (build of 18 Jul 2026)

**Install:** unsigned `.dmg` — right-click the app → Open → Open (same flow
as round 1). First launch may re-run the setup prompts if this is a fresh
account.

**Scope:** everything below landed AFTER the build you green-lit in round 1.
It's a lot — the app's WordPress tab, delete flows, and all three web-server
configs changed, so this round pairs a deep test of the NEW surface with a
regression sweep of the old one.

Where to look when something breaks: `~/Library/Application Support/
dev.rexenv.rexenv/logs/` — `health.log` (services), `repo-<domain>-<dir>.log`
(git jobs), `repo-<domain>-<dir>-watch.log` (watchers). Attach the matching
file to any report.

---

## A. Your round-2 fixes — re-verify (they shipped after your last build)

1. **Adminer**: Browse from a site → auto-login works; row EDIT and DELETE
   inside Adminer work (the confirm dialogs are ours now — native JS
   alert/confirm/prompt panels); redirects inside Adminer don't blank the
   pane. The new "open in browser" button: copies a first-party URL +
   opens in your default browser, logged in.
   Two sites on DIFFERENT engines (MySQL + MariaDB): auto-login lands on
   the RIGHT server for each (the guard is per-server now).
2. **Mailpit**: per-row delete, bulk selection delete, and clear-all
   (confirmed) — inbox updates without a manual refresh.
3. **Plugin/theme add (wp.org)**: live search → picked results stack as
   tags → ONE Install installs the whole batch; plugin list shows real
   titles + slugs + wp.org icons; checkboxes are big/pointer; select-all
   only selects the visible (filtered) rows.
4. **Type scale**: global 1.1× text — sweep Services, Databases, and the
   footer at the 980px minimum window width for overlap/clipped pills.

## B. rex CLI (new since your build)

Settings → General → Command-line tool → Install (one admin prompt), then
in a fresh terminal:

```
rex status            rex site list         rex site create qa1.rex
rex site logs qa1.rex --follow              rex wp qa1.rex plugin list
rex db export qa1.rex                       rex doctor
rex site delete qa1.rex --yes
```

New in this build — the repo group (drives the same git/asset machinery as
the UI; `--theme` switches from plugins to themes, `--json` everywhere):

```
rex repo <domain> list --status     rex repo <domain> status <dir>
rex repo <domain> branches <dir>    rex repo <domain> adopt <dir>
rex repo <domain> link <path> --name my-plugin
rex repo <domain> watch start <dir> <script> · watch list · watch stop <dir>
rex repo tools
```

Expect: `adopt`/`link`/`status` mirror exactly what the app panel shows for
the same asset (one code path); `watch start` says the watcher runs inside
the app and dies with it; a watcher started in the CLI shows up in the
app's footer chip and vice versa. Deleting git assets from the CLI is the
existing `rex wp <domain> plugin|theme delete` — it carries the SAME
unlink-only symlink guard as the UI (worth one CLI delete of a linked
plugin to confirm: link survives nowhere, folder survives everywhere).

Expect: every command talks to the RUNNING app (quit the app → `rex status`
must say "open the app first", exit 2 — never start anything itself).
Destructive commands ask for confirmation; `db reset` makes you TYPE the
domain. `rex help` lists the full surface; tab-completion works after
`rex completions zsh` setup.

## C. NEW — Git for plugins & themes (the headline feature)

A WP site's **WordPress tab → Plugins/Themes** add bar now has three
sources: **WordPress.org | From Git | Link folder**. Detailed test matrix:
`docs/GIT-FEATURE-TEST.md` (§1–12) — below is what to stress at user level.

### C1. Add from a URL
- Public repo: paste `https://github.com/WordPress/theme-check` → Fetch →
  branch dropdown (default marked) → Add. Steps stream LIVE (clone →
  detect); the plugin appears in the list with a sky **git** badge.
- A build-needing repo (`https://github.com/10up/insert-special-characters`):
  after clone, explicit `npm install` / `npm run build` buttons appear
  under a consent line ("runs the repo's own scripts…"). Nothing runs
  without a click. Activate is offered only after every step is green.
- **Your own PRIVATE repo over SSH** (`git@github.com:you/repo.git`):
  Fetch alone proves access (uses YOUR ssh-agent/keys — rexenv never
  prompts for or stores credentials). The same repo's https URL must fail
  FAST with a message pointing at the SSH form — never hang.
- Paste garbage / an archive link / a nonexistent repo: every failure is a
  plain-language message, often ending in a copy-paste `$ fix` — never a
  stack trace, never a frozen spinner.
- Mid-clone: switch tabs and come back — the SAME job is still there,
  streaming (not a blank panel). Cancel mid-clone → partial folder gone,
  `ps -ax | grep git` clean.

### C2. The repo panel (click a git badge)
- Header: branch, clean/dirty counts, ↑ahead ↓behind, remote, source.
- **Pull** on a behind branch fast-forwards; a DIVERGED branch gives an
  honest "resolve in your editor/terminal" (rexenv never merges);
  uncommitted conflicts give "commit or stash first".
- **Checkout** a remote branch from the dropdown → becomes a local
  tracking branch; if the branch changes lockfiles, an install offer
  appears right there.
- **Push** your commit → lands on the remote via your agent; a fresh
  branch auto-sets upstream; pushing when the remote is ahead says "pull
  first" (never force).
- **Scripts**: package.json scripts listed — `Run: build` streams and
  finishes; `Watch: start` runs persistently with live output, a footer
  chip shows it from any screen. Watchers STOP when you quit the app (by
  design), never auto-restart after a crash (shows "exited (code N)" +
  Restart).

### C3. Adopt + Link
- **Adopt**: `git clone` something into `wp-content/plugins/` yourself in
  a terminal → the row grows a dashed **git?** chip → Adopt → full badge +
  panel. Nothing on disk changes.
- **Link folder**: pick a checkout you keep elsewhere on disk → it's
  symlinked in, works in wp-admin, panel shows `→ /path/to/your/folder`.

### C4. DELETE SAFETY — the critical block, please hammer it
- **Linked plugin**: put an uncommitted file in your real folder → delete
  the plugin in rexenv → confirm says "removes only the link; your
  original folder stays untouched" → verify YOUR FOLDER IS FULLY INTACT,
  uncommitted file included. Repeat via bulk delete mixed with a normal
  plugin. Repeat with a manual `ln -s` you never adopted.
- **Cloned plugin with uncommitted work + unpushed commits**: the delete
  confirm must NAME the numbers ("3 changed files, 2 untracked files, and
  2 unpushed commits will be lost") — clean+pushed repos get a plain
  confirm (no cry-wolf).
- Active linked THEME: delete refused until another theme is active.

## D. Regression sweep — round-1 areas (a lot changed around them)

Highest value first, since these sit closest to what changed:

1. **Every site still serves** after Stop all → Start all — nginx site,
   Apache site, FrankenPHP site (all three server configs were
   regenerated with a new security rule — see E1).
2. **wp.org install/activate/update/delete flows** (same tab the git work
   rewired) + the WP manager sub-tabs (users, network, tools, cron).
3. **DNS survives quit**: quit the app → sites keep resolving + serving
   (agent mode in Settings); relaunch adopts everything, no prompts.
4. **Adminer** end-to-end (see A1) and **Firefox**: `https://<site>.rex`
   padlock OK in Firefox specifically (its own trust store).
5. **Mailpit**: send from a site (e.g. password-reset email) → arrives in
   Mail; deletion flows (A2).
6. **.rex everywhere**: new site defaults, Adminer host, wp-login magic
   link, tunnels on a `.rex` site.
7. Site lifecycle: create (both DB engines), rename, PHP switch, server
   switch, Xdebug toggle, multisite convert, domain change, move, delete.
8. Downloads panel on a cold-cache binary (e.g. install a PHP version you
   removed), tunnels start/stop, DB export/import, blueprints.

## E. By design — don't file these as bugs

1. **Dotfiles are now 404** on all three servers (`/.git/`, `/.env`,
   `.htaccess` contents…): a deliberate security fix — a served docroot
   (especially through a TUNNEL) must not leak repo internals. Exception:
   `/.well-known/` still serves. If a plugin legitimately serves a
   dotfile path, that's a finding — tell us.
2. **Watchers die with the app** and never auto-restart; install/build
   jobs are cancelled on quit. `kill -9` of the app is the known gap
   (children finish or fail harmlessly).
3. **Composer runs our pinned composer.phar on the site's PHP version** —
   your system composer is never used (platform checks match the site).
4. **Node/npm/git are YOUR system tools**, resolved through your login
   shell (nvm/fnm work). No node → honest error with install guidance,
   nothing bundled.
5. One git/install job at a time per plugin folder; a second attempt is
   refused with "already running — reconnect".
6. Private repos: SSH form only; https-with-credentials deliberately
   fails fast (rexenv never prompts for or stores secrets).

Report format as usual — what you did, what you saw, the log file from the
top of this doc, and a screenshot for anything visual (WKWebView rendering
issues are exactly the class we chase).
