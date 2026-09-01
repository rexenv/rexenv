# Installing rexenv (macOS)

rexenv is a local development environment — it runs your web/WordPress stack
(web servers, PHP, databases, one-click WordPress, `.rex` domains with HTTPS)
natively on your Mac, no Docker.

This is a **limited build shared directly** (not from the App Store and not yet
notarized by Apple), so the **first launch needs one extra click** — see below.

## Requirements

- **macOS 15 (Sequoia) or later.** (Raised from the previously stated 11 on
  15 Aug 2026 — that number was never true: the pinned nginx and cloudflared
  builds require macOS 15, MySQL/MariaDB/Redis require 14, so on macOS 11–14
  the app installed and then could not run its own web server. The stated
  floor now matches the measured one — `docs/PORTS.md` carries the per-binary
  numbers.) **PostgreSQL is the one exception in the other direction: its
  pinned builds currently require macOS 26** — tracked in `docs/TODO.md`.
- **Intel or Apple Silicon** — this is a **universal** build, it runs natively on both.
- An internet connection on **first run** (rexenv downloads its components — PHP,
  Nginx, MySQL, Caddy, etc. — the first time; after that it works offline).
- After first run, rexenv makes ONE routine outbound request of its own: a small
  read-only GET to `www.php.net` at launch, so Settings can say whether a newer PHP
  patch exists. It downloads nothing and installs nothing; if it fails, the row simply
  says so. Everything else already cached keeps working with no network at all.

### "8.4.24 exists" but rexenv is still on 8.4.23 — why that is normal

Expect a gap, usually of some weeks, and it is not a bug in either place.

rexenv does not compile PHP. It installs **pinned, portable static builds**, and for
PHP 8.x those are made by [static-php.dev](https://dl.static-php.dev/static-php-cli/),
a separate project that rebuilds after each upstream release. php.net announces a patch
on the day it ships; the portable build of that patch appears later. Measured on
16 Aug 2026: php.net listed 8.4.24 and 8.5.9 (released 30 Jul), while the newest
portable builds were 8.4.23 and 8.5.8 — exactly what rexenv pinned, and it had been that
way for 17 days.

So the Settings row says a patch **exists**, which is true, rather than "update
available", which would promise something rexenv cannot deliver on the day you read it.
There is deliberately no update button: every binary rexenv runs is checksum-pinned into
the app, so a new patch reaches you through a rexenv update, where the pin can be
verified before it ships. The row is there to answer "am I on something stale?", not to
offer an install.

If a patch matters to you urgently — a CVE you are exposed to — that is worth raising as
an issue rather than waiting: the pin can be moved in a release once a portable build of
it exists.

## Install

1. Open `rexenv_<version>_universal.dmg`.
2. Drag **rexenv** into the **Applications** folder.
3. Eject the disk image.

## First launch (important — one-time step)

Because this build isn't notarized by Apple yet, macOS Gatekeeper will refuse a
normal double-click the first time ("rexenv can't be opened…" / "unidentified
developer"). Do this **once**:

- **Right-click** (or Control-click) **rexenv** in Applications → **Open** →
  in the dialog, click **Open** again.

If you double-clicked first and got blocked, you can instead go to
**System Settings → Privacy & Security**, scroll down, and click **Open Anyway**
next to the rexenv message, then **Open**.

After you do this **once**, rexenv opens normally (double-click) from then on.

## Where rexenv lives once it's open

rexenv is a **menu-bar app**: the crowned-R icon in your menu bar is its home, and it
has **no dock icon**. Click the icon for the menu — the stack's status, Start/Stop all,
your recent sites, and **Open rexenv** for the window.

**Closing the window does not quit rexenv**, and that is deliberate: `rex` on the
command line and the AI-agent (MCP) endpoint are remote controls for the running app, so
closing a window used to take them down while your sites kept serving. Quit with
**Quit rexenv** in the menu.

If you turn on **Start on login** (Settings), rexenv starts straight into the menu bar
with no window — except when first-run setup isn't finished, where it shows the window,
because there is nothing useful it could do quietly on a machine that can't resolve
`.rex` yet.

> **Keep exactly one copy of rexenv.app.** macOS finds an app by its bundle id, not by
> where it sits, so a second copy — an old one in Downloads, a build left in a project
> folder — means `open -a rexenv`, Spotlight and the login item can all start the copy
> you did not mean. Both copies share the same data folder and the same services, so the
> symptom is not two apps: it is one app that is the wrong version. Drag the old copy to
> the Trash after updating.

## First-run setup prompts (expected)

The first time you use rexenv it sets up local networking + HTTPS, so macOS will
ask for permission a few times. These are expected and all stay on your machine:

1. **Admin password** — to add the `.rex` DNS resolver (`/etc/resolver/rex`) so
   `https://yoursite.rex` resolves locally. (Other TLDs, `.test` included, install
   the same way on first use if you pick one in Settings.)
2. **Keychain prompt** — to **trust rexenv's local Certificate Authority**, so your
   local sites get a valid green-lock HTTPS cert (it signs only your local sites).
3. **Admin password** — to let the built-in edge server use ports **80/443** when
   you start your services.

You can grant these once and get on with it. (A future signed/notarized build will
reduce the first-launch friction; the local setup prompts are inherent to running a
real HTTPS dev stack.)

## Verify it works

1. Open rexenv → **New site** → choose **WordPress** → create.
2. Start services if prompted, then open the site — it should load at
   **`https://<name>.rex`** with a valid HTTPS lock.

## Troubleshooting "it won't open"

- **"can't be opened / unidentified developer"** → that's Gatekeeper; use the
  **right-click → Open** step above (only needed once).
- **"app is damaged and can't be opened"** → usually means the download was
  quarantined; right-click → Open as above. (The app is ad-hoc code-signed, which
  is what lets Apple Silicon run it at all.)
- **Nothing happens / very old Mac** → check you're on **macOS 15 (Sequoia) or
  later**, the same floor as the Requirements section above. (This line said
  macOS 11 until 15 Aug 2026 — the number the Requirements section had already
  been corrected away from, left behind in the one place a person reads *because*
  something is wrong.)

## Updating

To update, quit rexenv and replace `rexenv.app` in Applications with the new
`.dmg`'s copy (drag over, replace). Your sites, settings, and downloaded
components are kept (they live in
`~/Library/Application Support/dev.rexenv.rexenv/`).

## Uninstalling — do the in-app step FIRST

rexenv installs privileged, system-level things that deleting the app cannot
remove: a **root LaunchDaemon** running the edge proxy on :443,
`/etc/resolver/*` files for your dev TLDs, and the **local-CA trust** in your
login keychain.

1. In the app: **Settings → "Remove system changes"** (one admin prompt —
   removes the edge daemon, resolver files, CA trust, and the DNS agent).
2. Then delete `rexenv.app` from Applications (and, if you want a full wipe,
   `~/Library/Application Support/dev.rexenv.rexenv/` — your site files live
   there unless you moved the Sites folder, so check before deleting).
