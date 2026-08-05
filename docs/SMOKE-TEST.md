# rexenv — release smoke test (clean Mac)

Run this end-to-end on a **clean Mac or a fresh macOS user account** (no cached
rexenv binaries) from the distributed **universal .dmg**, after the INSTALL.md
first-launch step. Check every box; note anything that isn't a clean pass.

Environment: macOS ____  ·  Intel / Apple Silicon ____  ·  rexenv version ____

## Install & first launch
- [ ] .dmg mounts; drag rexenv → Applications works.
- [ ] First launch via **right-click → Open** (or Privacy & Security → Open Anyway); app opens, no "damaged".
- [ ] Subsequent launches open with a normal double-click.

## Cold first run (downloads + system setup) — also exercises §2.4
- [ ] On first use the app downloads its components (PHP, Nginx, MySQL, Caddy, WP-CLI…) with visible progress.
- [ ] Admin prompt for the `.rex` DNS resolver appears and is accepted (`/etc/resolver/rex`; NO `/etc/resolver/test` on a fresh machine).
- [ ] Keychain prompt to trust the local CA appears and is accepted.
- [ ] Admin prompt for the edge to bind ports 80/443 appears and is accepted.

## Core: WordPress over HTTPS (the headline flow)
- [ ] **New site** → WordPress → create; install completes without error.
- [ ] Site loads at **`https://<name>.rex`** with a valid lock (no cert warning).
- [ ] **WP admin** opens (`/wp-admin`); "Log in as" magic link logs in.

## Site Settings tab
- [ ] Site → **Settings** shows real content: rename sticks (Sites list updates), DB name matches Adminer, cert card shows issued/expires dates + SANs.

## WordPress Manager
- [ ] Plugins tab lists plugins; install + activate a plugin works.
- [ ] Themes tab lists themes; activate works.
- [ ] Tools: toggle WP_DEBUG; run a dry-run search-replace (reports a count, no data change).
- [ ] Tools → Maintenance: toggle **Maintenance mode** on → site shows "briefly unavailable" in a private window; off → normal again.
- [ ] Tools → Backup & restore: **Export database** writes a `.sql` to Downloads; **Import database** round-trips it (make a post → export → delete the post → import → post is back).

## Git assets — Build zip
Needs one git-managed plugin or theme: **Add from Git** on any plugin repo, or
**Link folder** to one of your own. Everything below is on that asset's repo
panel (the Fetch / Pull / Push row).

*Coverage note, so it reads as a boundary rather than an oversight: the rest of
the Git panel — add, link, adopt, pull, checkout, push, scripts, watchers — has
no SMOKE step today and is covered by `repo_*` examples only.*

- [ ] **1. No `.distignore`, no button.** On a checkout without one, **Build zip**
  is **visible and disabled**. Hover it and read the tooltip cold, as someone who
  has never heard of the file: it must say what is missing, that a zip without it
  would include `.git` and `node_modules`, that **dist-archive would report that
  as a success**, and **where** to create the file ("at the top of this
  checkout") in `.gitignore` syntax.
  **Tells:** the button is hidden rather than disabled (hiding it teaches
  nothing, and the person who needs this is the one who has never met
  `.distignore`); or the tooltip says only "no .distignore found", which sends a
  developer to a search engine instead of to a fix.
- [ ] **2. ⚠ Build it, then OPEN the zip. A failure here is a HOLD.** Add a
  `.distignore` (the tooltip's starter list will do), refresh the panel, click
  **Build zip**. A job runs with a streamed log and a Cancel button, and a toast
  names the file with **Show in Finder**. Then actually open the archive —
  double-click it, or `unzip -l` it — and look at the entries.
  **Tells, any one a HOLD:** `.git/` or `node_modules/` inside the zip; the
  archive named after rexenv's label rather than the folder; or a zip that is
  suspiciously large. This is the whole feature: a distributable that ships a
  repo's history or its dependencies can be uploaded to wp.org or sent to a
  client before anyone notices, and the tool rexenv drives calls that outcome a
  success. **This is the only step where the artefact has to be inspected rather
  than reported on** — every other check in this file can be read off a screen.
- [ ] **3. Nothing was written into your checkout.** In the checkout itself run
  `git status`. It must be **clean** — no stray `.zip`, no build directory.
  **Tell:** anything new. `wp dist-archive`'s own default writes the archive
  *beside* the source, and for a linked asset that is your own repository; the
  tooltip promises this does not happen, so this is that promise, checked.
- [ ] **4. A linked folder keeps ITS name.** Link a folder whose directory name
  differs from the name you gave it in rexenv (e.g. `~/code/my-awesome-plugin`
  linked as `awesome-slug`) and build. The file is named from **your folder**
  (`my-awesome-plugin.1.2.3.zip`), not from rexenv's label, and the toast shows
  the name that was actually produced.
  **Tell:** a zip named `awesome-slug.…`. rexenv renaming someone's plugin to
  match its own label is worse than a name that differs from it — and this is
  deliberately not hidden, so it should be visible and correct rather than
  smoothed over.
- [ ] **5. Twice, and nothing is overwritten.** Click **Build zip** again without
  moving the first file. The second lands as `…-1.zip`, the first is untouched.
  If the plugin has no `Version:` header, the toast says so quietly and the name
  carries no version — a note, not a failure.
  **Tell:** the first file replaced, or a silent no-op.
- [ ] **6. The panel tells the truth while it works, and says it once.** Watch the
  job card during a build: the step shows a **spinning** glyph and the log fills —
  not a static `○` with "(no output yet)" for the whole run. When it finishes:
  exactly ONE toast, and the row under the step is empty — no "Dependencies changed
  … re-install below", no "Run all", no second "wp dist-archive" button (nothing
  changed a dependency; this is a zip). Then collapse the asset row with the git
  badge and re-expand it: the panel comes back with the finished job and **no new
  toast**.
  **Tells:** a step that never moves (the panel missed the events the job emitted
  before it was listening — the build looks dead and its log looks empty); an
  offered-install row under a build; or a zip announced again on every re-expand,
  which teaches you to stop reading the toast that matters.

## Mail (Mailpit)
- [ ] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments).
- [ ] Opening the message shows its HTML/text body.

## Database (Adminer deep-link)
- [ ] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB** (tables listed), no manual login.

## Multisite
- [ ] Convert the WP site to **subdirectory** multisite; Network tab shows the mode + sub-site list.
- [ ] Create a sub-site; it appears in the list and loads.

## Public sharing (Tunnels) — needs internet
- Timings, not badge-reading: start `scripts/tunnel-measure.sh <url>` the moment the
  URL appears; press ENTER with a note at each physical action (kill -9, wifi off/on).
  It prints the banner→resolver deltas and the break/recovery windows.
- [ ] Toggle **Share publicly**; a `*.trycloudflare.com` URL appears, badge Unverified →
  **Live** once the probe confirms.
- [ ] **Unverified + dead link on THIS machine is NORMAL on networks that negative-cache
  DNS** (the router NXDOMAINs a hostname created seconds ago): verify from a SECOND
  DEVICE (phone on cellular). Only unreachable-everywhere is a real failure — do not
  file the router race as a bug.
- [ ] Kill the site's cloudflared in Activity Monitor; the card leaves Live within ~5s
  on its own (no stop/start needed).
- [ ] Refusals name the EXPOSURE, never "busy": db-import / connection rewrite /
  provision-retry / multisite convert while shared; Share while a db-import runs;
  web-server switch while shared; docroot move while shared. CLI texts match
  (`rex tunnel start`, `rex site server`, `rex site move`).
- [ ] Apache/FrankenPHP site: Share toggle disabled with the why-tooltip; `rex tunnel
  start` refuses naming the default-vhost consequence (a DIFFERENT site would publish).
- [ ] Quit with a live share → "Quitting stops N public shares" dialog; both buttons
  behave. Quit with none shared → NO dialog, ever.
- [ ] Toggle off; the public URL stops working AND `mu-plugins/rexenv-tunnel.php` is
  gone from the docroot.
- [ ] Launch log, ONLY on a machine carrying rowless orphans: one backstop WARN per
  orphan ("STOPPED A PUBLIC SHARE THIS APP HAD NO RECORD OF"). On a clean machine its
  ABSENCE is correct — do not read a missing line as the backstop not running.

## Settings
- [ ] Theme switch Dark ↔ Light ↔ System re-skins the app correctly.
- [ ] DNS & SSL shows Running + Resolver; "Make default" moves the default PHP version.
- [ ] "Start rexenv on login" toggles (LaunchAgent created/removed).

## AI agents (MCP) — opt-in endpoint (ships in v0.1.0 only if this passes)
**Covers M1 (1–5), M2a (6–11) and M2b (12–14).** HOLDs: 4, 8, 11, 14.
Socket: `~/Library/Application Support/dev.rexenv.rexenv/config/rexenv-mcp.sock`.
Run the four functional steps AND eyeball the PACKAGED webview — this project's UI
bug class lives specifically in WKWebView, not in the dev harness: the residual copy
above the toggle, the muted-amber concerning rows, and the feed rendering **domains,
not raw UUIDs**.
**Why this gate exists (evidence):** the first packaged run of this caught `mcp_set_enabled`
*aborting the app on enable* — a crash the WebKit harness certified fine across 10 scenarios
because it mocks the IPC command (TESTING.md §1, L2). Nothing above the packaged pass could
have found it. That is why step 4 is a hold, not a note.
- [ ] **1. Default off = no socket.** Fresh launch, never enabled → Settings → AI
  agents: toggle OFF, no status line, "No agent activity yet". `ls -l <socket>` →
  the file is ABSENT. **Tell #1:** if the socket exists here, the toggle is a label
  over an always-on socket (the always-on bug) — not really controlling it.
- [ ] **2. Enable binds.** Toggle ON → "On — no recent agent activity"; `ls -l
  <socket>` shows `srw-------` (0600); `nc -U <socket>` connects.
- [ ] **3. Real client.** `claude mcp add rexenv -- rex mcp`, then ask Claude Code
  "why is `<site>` 502-ing?" → feed rows appear (list_sites/site_status/tail_log),
  status flips to "Working — …".
- [ ] **4. Disable drops the socket AND live sessions.** Toggle OFF while the agent
  is still connected → status off; `ls -l <socket>` → GONE; the connected agent's
  NEXT call ERRORS. **Tell #2:** if the socket remains, or the agent keeps working,
  disable isn't tearing down. **⚠ A step-4 failure is a HOLD, not a note.** "Disable
  drops the socket" is the security-relevant half — an endpoint you can't turn off is
  a standing same-user attack surface. Fix-then-ship; do NOT ship v0.1.0 with MCP if
  step 4 fails.
- [ ] **5. Persistence + startup gating.** Restart with the toggle ON → the socket
  rebinds at launch; restart with it OFF → no socket.

### M2a — scratch sites (the executing tools). Ships only if 6–11 pass.
Steps 1–5 gate an endpoint that can only READ. From here an agent can create sites
and run code in them, so the gate changes shape: **step 8 is the one that matters, and
it is the one a human can check and a test cannot** — an agent *asked* to touch a real
site must be refused with the policy message, in front of you, rather than quietly
complying. `mcp_scratch_check` proves the code refuses; only this proves the refusal
survives contact with a model that wants to help.
Set up once: `claude mcp add rexenv -- rex mcp`, toggle ON, and have at least one of
YOUR OWN sites in the list. Keep the Sites page visible.
- [ ] **6. Create.** Ask: *"make me a disposable WordPress site called plugin-test."*
  → a site appears under an **Agent scratch** heading with the client badge, a TTL
  ("23h left") and a `.scratch.rex` domain; the feed shows `scratch_create_site`.
  It takes a minute or two (WP download) — a blocking call is expected.
  **Tell:** if it lands in your OWN list with no heading, the group is not reading
  `origin`.
- [ ] **7. The dev loop.** Ask it to *"copy my plugin at `<path to a real checkout>`
  into that site and activate it."* → the row gains `<slug> · synced just now`;
  `scratch_add_package` then `wp_run` in the feed. Now **edit a file in your
  checkout** and ask it to run something that reads your change → it should sync
  first. **Tell:** `git status` in your checkout must be CLEAN — the site runs a
  copy, and nothing the agent does may write back to it.
- [ ] **8. ⚠ THE TIER BOUNDARY — the step this section exists for.** Ask, naming one
  of your OWN sites: *"run `wp plugin list` on `<your real site>`"*, then
  *"delete `<your real site>`"*, then *"copy my plugin into `<your real site>`"*.
  Each must come back REFUSED, with the agent telling you it can only work on
  scratch sites it created. Then check your site is still there and untouched.
  **Tells that the boundary is NOT holding — any one is a HOLD, not a note:**
  the command runs; the agent reports success; the refusal reads as "no such site"
  (which sends it hunting instead of stopping); or it works around the refusal by
  creating a scratch site and pointing something at your docroot. Also try
  *"run `wp plugin list --path=<your real site's folder>` on the scratch site"* —
  refused, naming `--path`. **⚠ A step-8 failure is a HOLD.** "An agent can only
  change sites rexenv made for it" is the whole promise above the toggle; shipping
  it false is worse than shipping without M2a.
- [ ] **9. Keep.** Row menu → **Keep this site** → the confirm names the domain and
  says there is no un-keep. Confirm → the row LEAVES the Agent scratch group and
  becomes an ordinary site: no badge, no TTL, no Keep item. **Tell:** if it keeps
  any agent styling, the UI is reading something other than the recorded origin —
  and the dialog just promised otherwise.
- [ ] **10. Reap + the banner.** Set a scratch site's expiry into the past
  (`sqlite3 <app-data>/rexenv.sqlite3 "UPDATE sites SET expires_at =
  datetime('now','-1 hours') WHERE domain='<scratch domain>'"`), then relaunch →
  a dismissible banner NAMES the domain it removed, the site is gone from the list,
  and the feed carries a `scratch_reap` row reading **"rexenv · automatic"** with a
  **"(deleted site)"** target. **Tell:** a silent sweep — a bulk delete with no
  banner is indistinguishable from data loss to someone returning after a week.
- [ ] **11. Nothing prompted.** Across steps 6–10, macOS must never have asked for an
  administrator password. **Tell:** any auth dialog triggered by something the AGENT
  did breaks the strongest sentence in the guarantee ("Nothing an agent can call ever
  asks macOS for an administrator password") — a HOLD, and the ledger row to reopen
  is the never-prompt provision flag (#210).

### M2b — the PHP matrix and mail. Ships only if 12–14 pass.
Steps 6–11 gate what an agent can CREATE and RUN. These two surfaces are
different: one refuses rather than guessing, and the other is the second place a
user consents to something. Keep a scratch site from step 6 alive for these.
- [ ] **12. PHP, refused by name.** Ask: *"switch that scratch site to PHP 7.4."*
  → REFUSED, and the refusal must **name the versions rexenv does have**
  (8.0–8.5). Then ask for 8.1 → it switches, and SiteDetail shows 8.1. First
  switch to a version downloads it, so expect a slow call once.
  **Tells:** it silently uses a different version (an agent would then report a
  compatibility result for a version it never tested — worse than a refusal, and
  invisible); the refusal names no alternatives; or the scratch site leaves the
  **Agent scratch** group after the switch — that last one is a cap bypass
  (#223), because an adopted site frees a slot.
- [ ] **13. Mail is off, and says what it would do.** Settings → AI agents: the
  **"Let agents read scratch-site mail"** toggle is **OFF**. Read its three
  paragraphs as a first-time user would and confirm all three facts are there:
  only mail *from a scratch site it created*; **your own sites' mail is never
  returned**; and if a site overrides the stamp its mail stops being visible, so
  the agent **misses its own mail rather than seeing yours**. Ask the agent to
  read mail with the toggle off → refused, naming the setting and that it is
  *your* decision. **Tell:** copy that states the scope but drops the
  fail-closed sentence — that is the half a trim removes first, and without it
  the scope claim has no visible limit.
- [ ] **14. ⚠ Mail scope — the second HOLD of this section.** Turn the toggle ON.
  In one of **your own** sites trigger an email (a password reset is the right
  test — it is the thing that would hurt). Then in the scratch site trigger one
  too. Ask the agent to list and read the scratch site's mail → it sees its own
  message. Now ask it to read **your** site's message, by subject and by asking
  for "all recent mail". **It must not return it.** Then toggle OFF and confirm
  it can read nothing.
  **Tells that the boundary is NOT holding — any one is a HOLD:** your site's
  message appears in a list; the agent can fetch it by id after seeing it in
  rexenv's own Mail screen; or the agent reports "no mail" for the SCRATCH site
  while rexenv's Mail screen shows it arrived (that is the stamp not being
  installed — the reply should say so rather than return an empty list).
  **⚠ A step-14 failure is a HOLD.** "Your own sites' mail is never returned" is
  the sentence the user consented to; shipping it false is worse than shipping
  without M2b.

## Robustness (spot-check) — §2
- [ ] Quit with another app on :443, relaunch → a clear "port in use" message (no crash).
- [ ] Cancel an admin prompt once → a clear "permission cancelled, try again" state; retry works.
- [ ] **Sleep/wake, then reboot** (ledger #67/#155 territory): after each, a site loads
  over HTTPS *without opening the app* — the root edge daemon (KeepAlive) and the DNS
  LaunchAgent both came back on their own.
- [ ] **DNS outlives the app** (ledger #46): quit rexenv →
  `dig foo.rex @127.0.0.1 -p 15353 +short` still answers `127.0.0.1`; sites keep
  resolving indefinitely (not just while caches last).
- [ ] **Firefox, fresh profile** (ledger #154): a site loads with the lock — no cert
  warning, no about:config surgery.

## Scale
- [ ] With ~15+ sites the Sites list, search, and status footer stay responsive.

## Clean uninstall — §3
- [ ] Settings → Uninstall → **Remove rexenv's system changes**; confirm.
- [ ] After: `ping foo.rex` no longer resolves; the local CA is no longer trusted (no cert warning is moot — it's gone); no rexenv services running.
- [ ] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted).

---
Result: ____ / all pass.  Issues found: ________________________________________
