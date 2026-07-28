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

## Mail (Mailpit)
- [ ] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments).
- [ ] Opening the message shows its HTML/text body.

## Database (Adminer deep-link)
- [ ] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB** (tables listed), no manual login.

## Multisite
- [ ] Convert the WP site to **subdirectory** multisite; Network tab shows the mode + sub-site list.
- [ ] Create a sub-site; it appears in the list and loads.

## Public sharing (Tunnels) — needs internet
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

## Robustness (spot-check) — §2
- [ ] Quit with another app on :443, relaunch → a clear "port in use" message (no crash).
- [ ] Cancel an admin prompt once → a clear "permission cancelled, try again" state; retry works.

## Scale
- [ ] With ~15+ sites the Sites list, search, and status footer stay responsive.

## Clean uninstall — §3
- [ ] Settings → Uninstall → **Remove rexenv's system changes**; confirm.
- [ ] After: `ping foo.rex` no longer resolves; the local CA is no longer trusted (no cert warning is moot — it's gone); no rexenv services running.
- [ ] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted).

---
Result: ____ / all pass.  Issues found: ________________________________________
