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
- [ ] Admin prompt for the `.test` DNS resolver appears and is accepted.
- [ ] Keychain prompt to trust the local CA appears and is accepted.
- [ ] Admin prompt for the edge to bind ports 80/443 appears and is accepted.

## Core: WordPress over HTTPS (the headline flow)
- [ ] **New site** → WordPress → create; install completes without error.
- [ ] Site loads at **`https://<name>.test`** with a valid lock (no cert warning).
- [ ] **WP admin** opens (`/wp-admin`); "Log in as" magic link logs in.

## WordPress Manager
- [ ] Plugins tab lists plugins; install + activate a plugin works.
- [ ] Themes tab lists themes; activate works.
- [ ] Tools: toggle WP_DEBUG; run a dry-run search-replace (reports a count, no data change).

## Mail (Mailpit)
- [ ] Trigger a WP email (e.g. password reset); it appears in **Mail** (inbox count increments).
- [ ] Opening the message shows its HTML/text body.

## Database (Adminer deep-link)
- [ ] Site → **Database** tab (or Sites row → Open database) lands **inside the site's DB** (tables listed), no manual login.

## Multisite
- [ ] Convert the WP site to **subdirectory** multisite; Network tab shows the mode + sub-site list.
- [ ] Create a sub-site; it appears in the list and loads.

## Public sharing (Tunnels) — needs internet
- [ ] Toggle **Share publicly** for a site; a `*.trycloudflare.com` URL appears and loads the site.
- [ ] Toggle off; the public URL stops working.

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
- [ ] After: `ping foo.test` no longer resolves; the local CA is no longer trusted (no cert warning is moot — it's gone); no rexenv services running.
- [ ] Site files remain under `~/Library/Application Support/dev.rexenv.rexenv/` (not deleted).

---
Result: ____ / all pass.  Issues found: ________________________________________
