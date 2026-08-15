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
