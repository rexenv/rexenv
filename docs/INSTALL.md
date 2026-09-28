# Installing rexenv

**macOS is below; Windows and Linux are at the end of this file** — the installs share
almost nothing mechanically (a `.dmg` and Gatekeeper against an installer and
SmartScreen; `/etc/resolver` against NRPT; a keychain against the CurrentUser Root
store), so each is written out rather than cross-referenced.

## macOS

rexenv is a local development environment — it runs your web/WordPress stack
(web servers, PHP, databases, one-click WordPress, `.rex` domains with HTTPS)
natively on your Mac, no Docker.

This is a **limited build shared directly** (not from the App Store and not yet
notarized by Apple), so the **first launch needs one extra click** — see below.

### Requirements

- **macOS 15 (Sequoia) or later for everything; macOS 13 (Ventura) or later to run.**
  On 15+ every feature and the newest build of every component. On **13 and 14** rexenv
  runs with the last build of each component that loads there (older MySQL, Redis,
  MariaDB, Apache, Xdebug and tunnel client — `docs/PORTS.md` §"Legacy tiers" has the
  versions) and **without PostgreSQL and PHP 8.0 on 13** (PostgreSQL 16.4 only on 14):
  the app says so once at first launch, and each missing item stays listed with the
  reason. Nothing older than 13 is offered a download. (History: the stated floor was
  "11" until 15 Aug 2026 — never true; 15 from then to 23 Sep 2026, when the tiers
  landed: `docs/PLAN-macos-13-floor.md`.)
- **Intel or Apple Silicon** — this is a **universal** build, it runs natively on both.
- An internet connection on **first run** (rexenv downloads its components — PHP,
  Nginx, MySQL, Caddy, etc. — the first time; after that it works offline).
- **No Xcode Command Line Tools, no Homebrew.** The core stack (Caddy, Nginx, PHP,
  MySQL, Mailpit, Adminer, WP-CLI) installs on a Mac that has never seen a compiler. (This
  was false in 0.7.0–0.7.2: preparing a download asked `otool`, a CLT shim, and the first
  run failed every component on a clean Mac — caught by the first clean-VM smoke test,
  18 Sep 2026.) The one exception is the Homebrew-bottle bundles — Redis, MariaDB, Apache
  and Xdebug — whose dylibs must be relinked with `install_name_tool`; on a Mac without the
  tools those fail with a message naming `xcode-select --install`, and nothing pops a
  dialog. Tracked in `docs/TODO.md`.
- After first run, rexenv makes a few small read-only requests of its own at launch,
  and again every six hours if it is left running. None of them downloads or installs
  anything, and if any of them fails the screen says so rather than guessing:
  - `www.php.net`, so Settings can say whether a newer PHP patch exists;
  - two files on `raw.githubusercontent.com/rexenv/runtimes`, the signed list of PHP and
    Adminer versions rexenv can install;
  - two more from the same place: the signed record of rexenv's own latest release, so
    Settings → About can tell you a new version was published.

  You can turn the last one off — **Settings → About**, and "Check now" still works when
  you ask for it. Everything already cached keeps working with no network at all.

#### "8.4.24 exists" but rexenv is still on 8.4.23 — why that is normal

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
The row is there to answer "am I on something stale?", not to offer an install — and
when rexenv CAN install a patch (a signed, checksum-verified build it knows about), the
row grows an Update button beside it. The "exists" line stays for the ones it cannot.

If a patch matters to you urgently — a CVE you are exposed to — that is worth raising as
an issue rather than waiting: the pin can be moved in a release once a portable build of
it exists.

### Install

1. Open `rexenv_<version>_universal.dmg`.
2. Drag **rexenv** into the **Applications** folder.
3. Eject the disk image.

### First launch (important — one-time step)

Because this build isn't notarized by Apple yet, macOS Gatekeeper will refuse a
normal double-click the first time ("rexenv can't be opened…" / "unidentified
developer"). Do this **once**:

- **Right-click** (or Control-click) **rexenv** in Applications → **Open** →
  in the dialog, click **Open** again.

If you double-clicked first and got blocked, you can instead go to
**System Settings → Privacy & Security**, scroll down, and click **Open Anyway**
next to the rexenv message, then **Open**.

After you do this **once**, rexenv opens normally (double-click) from then on.

### Where rexenv lives once it's open

rexenv is a **menu-bar app**: the crowned-R icon in your menu bar is its home, and it
has **no dock icon**. Click the icon for the menu — the stack's status, Start/Stop all,
your recent sites, **About rexenv** for the version and licences, and **Open rexenv**
for the window.

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

### First-run setup prompts (expected)

The first time you use rexenv it sets up local networking + HTTPS, so macOS will
ask for permission a few times. These are expected and all stay on your machine:

1. **Admin password** — to add the `.rex` DNS resolver (`/etc/resolver/rex`) so
   `https://yoursite.rex` resolves locally. (Other TLDs, `.test` included, install
   the same way on first use if you pick one in Settings.)
2. **Keychain prompt** — to **trust rexenv's local Certificate Authority**, so your
   local sites get a valid green-lock HTTPS cert (it signs only your local sites).
   This dialog is titled **rexenv** too, and asks for your login password.
3. **Admin password** — to let the built-in edge server use ports **80/443** when
   you start your services.

The admin-password dialogs read **rexenv** in bold with rexenv's logo on the lock, and
say what they are for — e.g. "rexenv wants to add a DNS resolver so .rex sites open on
this Mac." or "rexenv wants to start its HTTPS server on ports 80 and 443."
If one ever reads "osascript wants to make changes." instead, it is still rexenv
asking — the plain fallback used when its branded prompt cannot start (the app log
says why: "branded password prompt unavailable").

You can grant these once and get on with it. (A future signed/notarized build will
reduce the first-launch friction; the local setup prompts are inherent to running a
real HTTPS dev stack.)

### Verify it works

1. Open rexenv → **New site** → choose **WordPress** → create.
2. Start services if prompted, then open the site — it should load at
   **`https://<name>.rex`** with a valid HTTPS lock.

### Troubleshooting "it won't open"

- **"can't be opened / unidentified developer"** → that's Gatekeeper; use the
  **right-click → Open** step above (only needed once).
- **"app is damaged and can't be opened"** → usually means the download was
  quarantined; right-click → Open as above. (The app is ad-hoc code-signed, which
  is what lets Apple Silicon run it at all.)
- **Nothing happens / very old Mac** → check you're on **macOS 13 (Ventura) or
  later**, the same floor as the Requirements section above — and on 13 or 14 expect
  the first-launch note about what that macOS gets. (This line said macOS 11 until
  15 Aug 2026 and 15 until 23 Sep 2026 — each time the number the Requirements
  section had already moved away from, left behind in the one place a person reads
  *because* something is wrong.)

### Updating

**rexenv updates itself.** When a new version is published, Settings → About offers it:
it downloads the new build, checks its signature and checksum, replaces `rexenv.app` in
one step, then quits and reopens on the new version. Your sites, databases and DNS keep
running throughout — services outlive the app — while open terminals and running jobs
close with it, exactly as they do when you quit. The menu-bar menu shows the same offer,
and there is a "Check for Updates…" item in the rexenv menu.

Two things worth knowing:

- **macOS may ask again for permissions it had already granted.** rexenv has no Apple
  developer signature yet, so every build is a new identity to the system.
- **The version that introduced this cannot update itself to it.** The first in-app
  update is from the release AFTER it; before that, use the manual path below.
- **You can turn the checking off.** Settings → About → Updates → untick *“Check for new
  releases automatically”*. rexenv then contacts nothing on its own; *Check now* still
  works, and nothing ever installs without your click either way.

If rexenv cannot replace itself it says why and does not offer the button — for example
when it is running from the disk image rather than from Applications, or when
`/Applications` belongs to another account. Each of those comes with the command that
fixes it.

**Updating by hand** still works and is the fallback: quit rexenv and replace
`rexenv.app` in Applications with the new `.dmg`'s copy (drag over, replace). Your sites,
settings, and downloaded components are kept either way (they live in
`~/Library/Application Support/dev.rexenv.rexenv/`).

**If rexenv will not open after an update**, the copy it replaced is still on disk:

```sh
mv /Applications/rexenv.app ~/Desktop/rexenv-broken.app
mv /Applications/.rexenv-update-*/rexenv.app /Applications/rexenv.app
```

**"This copy of rexenv is older than its data"** on launch means the data folder was last
opened by a newer rexenv — you installed an older copy over a newer one, or run two versions
side by side. The screen names both schema versions and nothing has been changed: quit this
copy and open the newer rexenv (reinstall it if it was replaced). rexenv 0.7.1 and older do
not have this check, so going back to one of those after a newer version has run is not
supported.

**If rexenv quits unexpectedly or never shows its window**, look for `crash.log` in
`~/Library/Application Support/dev.rexenv.rexenv/logs/` (or your temp folder, if rexenv could
not resolve its data folder). Every internal panic is appended there with the version, the
message, where it happened and a backtrace — attach it to a bug report. It rotates to
`crash.log.old` past 1 MB.

**Installed with Homebrew?** `brew upgrade --cask rexenv` still works, and brew reads the
app's own version afterwards, so a self-updated copy is not downgraded by a plain
`brew upgrade`. `brew upgrade --greedy` and `brew reinstall` DO reinstall the cask's
version over a newer one.

### Uninstalling — do the in-app step FIRST

rexenv installs privileged, system-level things that deleting the app cannot
remove: a **root LaunchDaemon** running the edge proxy on :443,
`/etc/resolver/*` files for your dev TLDs, and the **local-CA trust** in your
login keychain.

1. In the app: **Settings → "Remove system changes"** (one admin prompt —
   removes the edge daemon, resolver files, CA trust, and the DNS agent).
2. Then delete `rexenv.app` from Applications (and, if you want a full wipe,
   `~/Library/Application Support/dev.rexenv.rexenv/` — your site files live
   there unless you moved the Sites folder, so check before deleting).

---

## Windows

> **Not yet installable.** rexenv has no Windows installer today: `bundle.windows` in
> `src-tauri/tauri.conf.json` is empty, and the NSIS installer, the Windows release job and
> the winget manifest are all open work (W11 in `docs/TODO.md`). What exists is a Windows
> build you can compile and run — the whole Rust suite passes there (1313 tests, 19 Sep
> 2026). This page describes what that build does on your machine, so it is ready when the
> installer lands.
>
> **When it does land it will be unsigned**, like the macOS build: rexenv is open source
> and earns nothing, so it buys no code-signing certificate. What that costs you is below.

#### What Windows says about an unsigned installer

rexenv's installer carries no code-signing certificate, so Windows warns about it. The
first installer was built 19 Sep 2026 (`rexenv_0.7.0_x64-setup.exe`, 9.7 MB) and this is
what it actually showed, word for word, on a file marked as downloaded from the internet:

> **Open File - Security Warning**
> The publisher could not be verified. Are you sure you want to run this software?
> Name: `…\Downloads\rexenv_0.7.0_x64-setup.exe` · Publisher: **Unknown Publisher** ·
> Type: Application
> **[ Run ]  [ Cancel ]** · ☑ Always ask before opening this file
> *This file does not have a valid digital signature that verifies its publisher. You
> should only run software from publishers you trust.*

**Run is on that first screen.** There is no "More info" to find first — one click and the
installer starts. "Unknown Publisher" is accurate and is what an unsigned build says.

**On a Windows 11 with its defaults, SmartScreen speaks first, and it is a different dialog.**
Measured 19 Sep 2026 on a clean Windows 11 Pro 24H2 (build 26100) install with SmartScreen
on, downloading through Edge. Edge's download shelf first: *"rexenv_0.7.0_x64-setup.exe isn't
commonly downloaded. Make sure you trust rexenv_0.7.0_x64-setup.exe before you open it."* —
**Keep** is behind its "See more" menu. Opening the kept file:

> **Windows protected your PC**
> Microsoft Defender SmartScreen prevented an unrecognized app from starting. Running this
> app might put your PC at risk.
> More info · **[ Don't run ]**

**"Run anyway" is not on that screen.** Click **More info**, and the same dialog adds
*App: rexenv_0.7.0_x64-setup.exe · Publisher: Unknown publisher* and **[ Run anyway ] [ Don't
run ]**. Run anyway starts the installer directly — the "Open File - Security Warning" above
does NOT appear behind it. (That dialog is what a machine with SmartScreen turned off by
policy shows instead — the two are alternatives, not a sequence.) A second build is a new
hash and goes through all of it again; that is the cost D5 chose.

The installer itself asks nothing more: Welcome → Choose Install Location
(`C:\Users\<you>\AppData\Local\rexenv`, about 37 MB) → Install → Finish, with **Run rexenv** and
**Create desktop shortcut** ticked. **No UAC prompt** — it is a per-user install. Let the Finish
page start rexenv, or start it from the Start menu; both work. (The copy the installer starts
is confined by the installer's own job, which would stop rexenv's servers from starting — so
rexenv notices and reopens itself through Explorer once, before it shows anything. Measured on
the first installed copy, 19 Sep 2026.)

**Installing a newer build over an installed one** asks one more thing: *"rexenv is still
running: the app, or the small background resolver that keeps your .rex sites answering after
you quit. Click OK to stop it and continue…"* — it asks even with the window closed and rexenv
quit, because the `.rex` DNS answerer is the same `rexenv.exe`. OK is right: it comes back on
the new build by itself a few seconds later (its logon task restarts it), and `.rex` keeps
resolving. (Up to 0.8.5 this read Tauri's stock *"rexenv is running! Click OK to kill it"*,
which told a user who had quit that the app was still open; the words now live in
`src-tauri/nsis/English.nsh`.)

### Requirements

- **Windows 11 x64.** Windows 10 22H2 is best-effort — it left mainstream support on
  14 Oct 2025. **arm64 is unsupported**: the x64 build runs under emulation, but the
  official PHP Windows builds are x86/x64 only and PostgreSQL's portable build is x64
  only, so nothing native exists to ship (D6, ruled 13 Sep 2026).
- An internet connection on **first run** — rexenv downloads its components (PHP, Caddy,
  nginx, MySQL, PostgreSQL, Mailpit, cloudflared) the first time, then works offline.
  The Windows builds are the same versions as macOS from different publishers; every one
  is checksum-locked (`docs/PORTS.md` names each archive and digest).
- The same small read-only requests at launch as on macOS (php.net, and the signed
  version lists on `raw.githubusercontent.com/rexenv/runtimes`). You can turn the release
  check off in **Settings → About**.

#### What is not in the Windows build

Refused in core with a message that says why, rather than offered and then failing (D4):

- **Apache** — on Windows that means Apache Lounge, a third-party trust decision nobody
  has made yet.
- **FrankenPHP** — pending its own proof.
- **Xdebug** — its DLLs must match each PHP's NTS build and compiler exactly, so they are
  pinned per version, and none is pinned here yet.
- **Redis** — no official Windows build exists.
- **MariaDB** — no Windows pin; MySQL 8.4 and 8.0 both ship, so v1 has a database either
  way.

### Where rexenv puts things

| What | Where |
|---|---|
| App data (sites, database, certs) | `%LOCALAPPDATA%\rexenv\rexenv\data` |
| Config, logs, downloaded binaries | `config\`, `logs\`, `bin\` under that folder |
| The `rex` CLI | a **copy** at `%LOCALAPPDATA%\rexenv\bin`, added to your user `Path` |

The `rex` copy is deliberate rather than a symlink: creating a symlink on Windows needs a
privilege an ordinary account may not have, and a copy in rexenv's own folder is
removable by the uninstall step without touching anything else (ledger #634).

### Where rexenv lives once it's open

rexenv is a **taskbar tray** app. **Left-click the tray icon to open the window;
right-click for the menu** — the reverse of macOS, because that is what Windows users
expect (ledger #624). The icon is the colour one, so it stays legible on a dark taskbar.

**Closing the window does not quit rexenv.** The `rex` CLI and the AI-agent (MCP)
endpoint are remote controls for the running app, so closing a window would take them
down while your sites kept serving. Quit from the tray menu.

### First-run setup prompts (expected)

Windows asks **twice**, where macOS asks three times:

1. **One administrator (UAC) prompt** — to add the `.rex` DNS rule so
   `https://yoursite.rex` resolves to this computer. rexenv says so first, in its own
   dialog — *"rexenv wants to add a DNS resolver so .rex sites open on this PC. Windows will
   ask for your permission next."* — and the UAC prompt follows your OK. rexenv uses Windows' **NRPT**
   (`Add-DnsClientNrptRule -Namespace .rex -NameServers 127.0.0.1`) and runs its own DNS
   answerer on `127.0.0.1:53`. Other TLDs get their rule on first use.
   **rexenv never edits your `hosts` file** — it is shared by every tool on the machine,
   and rexenv's rule is never to overwrite a file somebody else owns (D2).
2. **Windows' own certificate dialog** — "Security Warning: You are about to install a
   certificate from a certification authority…", to trust rexenv's local CA so your sites
   get a valid HTTPS lock. It goes into **your** user's Root store, never the machine's,
   so it needs no elevation and affects no other account. Answering **No** cancels the
   step; rexenv offers it again rather than pretending it worked.

**There is no third prompt.** On macOS the edge server needs an admin password to bind
ports 80/443; on Windows it does not — measured under an ordinary unelevated account, the
bundled Caddy binds both. It binds **127.0.0.1 only**, which also means no Windows
Defender Firewall alert, and that your sites are reachable from this computer only.
Public sharing (Tunnels) is unaffected: cloudflared dials localhost.

The DNS answerer runs as a **scheduled task at logon**, `\rexenv\dns-agent`, so `.rex`
keeps resolving after you quit rexenv — the same promise as macOS, different machinery.

### Verify it works

1. Open rexenv → **New site** → **WordPress** → create.
2. Start services if prompted, then open the site — it should load at
   `https://<name>.rex` with a valid HTTPS lock.
3. `rex status` should read `DNS answering (agent, udp 53) · resolver installed · CA trusted`.

**Testing from a terminal with Windows' own `curl.exe`:** add `--ssl-no-revoke`
(`curl.exe --ssl-no-revoke https://<name>.rex/`). Without it, curl's schannel backend refuses
with `CRYPT_E_NO_REVOCATION_CHECK` — it insists on checking revocation, and a local-CA
certificate has no revocation server to ask. Browsers, PowerShell's `Invoke-WebRequest`, and
PHP's curl inside your sites don't insist, so this is curl-only. A `SEC_E_ILLEGAL_MESSAGE`
instead means the name is not one of your sites (or you asked for `https://127.0.0.1/`):
there is no certificate for it, so the edge ends the handshake — macOS's curl says
`tlsv1 alert internal error` for the same thing. (Measured on the Dell, 21 Sep 2026.)

### Updating

**rexenv cannot update itself on Windows yet** (W11). On macOS it replaces itself from
Settings → About; here the pieces that would do it — reading the app's own install facts,
staging a replacement, and swapping a running `.exe` — are unported, and the code says so
by name: `WindowsAppBundle::facts` returns `Unported("windows app bundle facts
(self-update, plan D5/W11)")`. Checking still works; applying does not. Until the updater
lands, updating means running a newer build.

### Uninstalling — do the in-app step FIRST

rexenv installs things outside its own folder that deleting the app cannot remove: the
`.rex` NRPT rule, the `\rexenv\dns-agent` scheduled task, the local CA in your Root
store, and the `rex` copy on your `Path`.

1. In the app: **Settings → "Remove system changes"** (one UAC prompt — removes the NRPT
   rules, the scheduled task, the CA trust and the `rex` copy with its `Path` entry).
2. Then remove the app itself, and — only if you want a full wipe —
   `%LOCALAPPDATA%\rexenv`. **Your site files live under
   `%LOCALAPPDATA%\rexenv\rexenv\data` unless you moved the Sites folder**, so check
   before deleting.

## Linux (Ubuntu)

> **Not yet installable, and not yet run anywhere.** The Linux port landed 24 Sep 2026
> (`docs/PLAN-linux-port.md`): the code compiles for Linux and every platform piece is written,
> but no `.deb` has been built and no Linux machine has run it. This section says what the
> build WILL do on an Ubuntu machine, so it is ready when the package lands — and so the first
> tester knows what to look for.

### Requirements

- **Ubuntu 22.04 LTS or newer** (the first LTS with `libwebkit2gtk-4.1`, which the app's
  webview needs), x86_64 or arm64. Other distros with systemd-resolved and polkit will
  probably work and are not tested.
- **systemd-resolved** answering `/etc/resolv.conf` (Ubuntu's default). rexenv routes `.rex`
  through a dummy network link (`rexenv0`) that resolved treats as a split-DNS link — only `.rex`
  names go to it; on a machine where resolved is not running the setup step refuses with a
  sentence saying so, and writes nothing.
- **polkit** with a desktop authentication agent (any Ubuntu desktop). System changes are made
  through `pkexec`, so you see the desktop's own password dialog. From the `.deb` it reads
  "rexenv needs administrator permission to change system settings…" (the action file the
  package installs); from an AppImage it is polkit's generic "run /bin/sh as the super user".
- `libnss3-tools` (`certutil`), `xdg-utils`, `libayatana-appindicator3-1` (the tray). The
  `.deb` will declare these; an AppImage will not, and the messages name the missing one.
- An internet connection on **first run** — the same downloads as macOS, from the same
  publishers where they publish Linux builds (`docs/PORTS.md`, Linux table). MySQL's generic
  Linux build additionally needs `libaio1` and `libnuma1`, and PostgreSQL's needs libxml2, OpenSSL 3,
  zstd, lz4, Kerberos and xxhash (the `.deb` declares all of them; where a distribution renamed a
  library — `libaio1t64` on 24.04+, `libxml2-16` on 26.04 — a private symlink rexenv keeps under its
  own data folder bridges the name, so MySQL and PostgreSQL start there too).

### What system setup changes (one polkit prompt, then one more for the certificate)

1. `/etc/rexenv/dns.d/rex` (a marker), `/usr/local/lib/rexenv/dns-route.sh` and the unit
   `rexenv-dns-route.service` — a dummy link `rexenv0` with `.rex` as its ONLY routing domain and
   no default route, so `*.rex` goes to rexenv's resolver on `127.0.0.1:15353` and nothing else
   does. The link carries one address, `192.0.2.53/32` — from TEST-NET-1, a range reserved for
   documentation that no real network uses — because systemd-resolved ignores a link with no
   real address. (A global resolved drop-in was tried first and sent EVERY name there — measured
   24 Sep 2026. And 0.8.8's link had no address, so `.rex` resolved only where another machine
   answered it; after updating from 0.8.8 rexenv offers its setup step once more to fix that.)
2. `/etc/systemd/system/rexenv-edge.service` and `/usr/local/lib/rexenv/` — the root Caddy edge
   on `:80`/`:443`, kept alive by systemd, its binary copied to a root-owned folder.
3. Your browsers' certificate stores (`~/.pki/nssdb`, and the snap Chromium's own database when
   that snap has run — a snap does not see your `~/.pki`; via `certutil`, no prompt) AND the system
   store (`/usr/local/share/ca-certificates/rexenv-local-ca.crt` + `update-ca-certificates`, the
   second prompt) — so both Chrome and `curl`/PHP accept `https://*.rex`. Every Firefox profile's
   own certificate database gets the CA as well (the snap Firefox cannot see the system store).
4. `~/.config/systemd/user/rexenv-dns.service` — the resolver, started at sign-in, restarted on
   any exit, outliving the app. No prompt.
5. `/usr/local/bin/rex` — the CLI symlink, when you click Install (one prompt).

### Updating

Settings → About → Check now, as on macOS. From the `.deb`, Update downloads the new package,
checks it (`dpkg-deb`: it must be `rexenv`, the version the signed release names, and your
architecture, carrying `rexenv` and `rex`), and installs it with ONE polkit prompt — the same
rexenv sentence as setup. From an AppImage, Update swaps the file beside itself with no prompt
(keep it in a folder you own; a read-only or root-owned folder is refused with the fix named).
An AppImage needs `libfuse2` to mount, or nothing at all — rexenv runs it extract-and-run when
checking its version. A `cargo` build reads no update descriptor.

### Not in Linux v1

Redis, MariaDB, Apache, Xdebug (Homebrew's Linux bottles are x86_64-only and unrelinked).
PHP 7.4 IS available — rexenv's own static build, as on macOS.

### Uninstalling — do the in-app step FIRST

1. In the app: **Settings → "Remove system changes"** (polkit prompts — removes the drop-ins,
   the edge unit and its root folder, both certificate trusts, the DNS agent unit and the
   `rex` symlink).
2. Then `sudo apt remove rexenv` (or delete the AppImage), and — only if you want a full wipe —
   `~/.local/share/rexenv`. **Your site files live there unless you moved the Sites folder.**
