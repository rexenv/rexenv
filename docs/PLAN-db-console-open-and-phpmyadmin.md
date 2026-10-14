# PLAN — "Open in browser" on the Databases rows, and phpMyAdmin as a second console

**Status: PLANNED 14 Oct 2026** — designed against `1269fb8f` (v0.8.13 + 77 commits), nothing
built. The task list is §10; the open rows live in `docs/TODO.md` ("Database console: open in
browser + phpMyAdmin"). This file is the reasoning, so the build starts from facts rather than
from memory.

Two owner asks (14 Oct 2026, with a screenshot of the Databases page):

1. The **Browse** button beside MySQL / MariaDB / PostgreSQL opens the console INSIDE the app.
   Beside it the owner wants a second control that opens the same console in a **real browser**,
   and it must look and behave like every other "open in browser" control in rexenv: the icon of
   the browser the click will use, and a chevron whose menu lists every installed browser, each
   with its private/incognito second target.
2. **phpMyAdmin.** "Onekei chachchhe" — users ask for it. Can rexenv support it, and how?

Short answers: (1) is a frontend-only change — every backend piece already exists and is already
OS-neutral; (2) yes, as a second internal vhost on the shared stack, **for MySQL and MariaDB
only** (phpMyAdmin has no PostgreSQL driver; Postgres stays on Adminer), chosen by ONE setting
rather than by a second button on every row.

---

## 1. What already exists (reuse, do not rebuild)

Measured in the tree, 14 Oct 2026:

- **The browser chooser is one component, used everywhere.** `SplitButton` (`src/components/ui/
  split-button.tsx`) + `useBrowserMenu(url)` / `PreferredBrowserIcon` / `BROWSER_MENU_WIDTH`
  (`src/components/ui/open-in.tsx`). `SiteDetail.tsx:498` is the reference use: the primary half
  calls `openExternal(url)`, the chevron menu lists `useBrowsers()`, each row carrying the private
  target when `supportsPrivate`. The menu is `undefined` with fewer than two browsers and the
  chevron is then not rendered at all.
- **The preference is applied in the backend** (`commands/system.rs::open_external`): a URL goes
  to the stored `preferred_browser` through `platform.shell().open_in_browser`, else the OS
  handler. `open_in_browser(browser, url, private)` refuses anything that is not `http(s)`
  (ledger #301) and errors rather than falling back to a normal window when a private one was
  asked for and the browser has no flag (#309).
- **All three OSes already answer "which browsers, which private flag".** macOS: the bundle
  table in `platform/macos/mod.rs`; Windows: `platform/windows/app_catalog.rs` (`--incognito`,
  `--inprivate`, `-private-window`); Linux: `platform/linux/desktop.rs` (seven browsers with a
  `private_flag`). The TSX reads `supportsPrivate` off the IPC and never spells a flag.
- **Adminer's external URL is already minted and already opened from a browser.** `adminerUrl({
  engine, db })` (`src/lib/adminer.ts`) is the first-party `https://adminer.rexenv.rex/?…&
  rexenv_auto=1` link; `AdminerFrame`'s slim bar shows it with Copy and a plain "Open in browser"
  (`openExternal`, no chevron). The passwordless auto-login is the wrapper's (`core/adminer.rs`
  `WRAPPER_INDEX_PHP`): accepted for the loopback engines rexenv manages only, submitted through
  Adminer's own CSRF-tokened form. Opening that URL in a private window is a fresh
  `sessionStorage`, so the auto-submit guard starts clean — nothing new is exposed by (1).
- **The in-app console goes through a custom scheme with a Rust cookie jar** (`core/adminer.rs::
  forward`, registered in `lib.rs:80`): `rexdb://localhost` on macOS and Linux, `http://rexdb.
  localhost` on Windows (`PlatformWords::db_browser_origin`, all three filled). The proxy pins
  the upstream host to loopback, strips hop-by-hop and cookie headers, rewrites the CSP's
  `frame-ancestors` to the app's origins (`PROXY_FRAME_ANCESTORS`) and rewrites absolute
  `Location`s back onto the proxy origin. The app's own CSP (`tauri.conf.json` `frame-src`)
  names the scheme — BOTH ends must agree or the panel is empty (#699, #702, #703).
- **An internal vhost is one `NginxSite` pushed in `sites::rebuild`** (`core/sites.rs:2911`):
  `RewriteMode::Single`, the default pool, a body cap driving nginx AND PHP from one number
  (`import_cap` / `import_php_value` + the `.user.ini` copy that reaches a Windows php-cgi group,
  #788), a 24 h read timeout, a cert from `ssl::ensure_site_cert`. It is not a `Site`, so it can
  never be a tunnel origin (`commands/tunnels.rs:1015` asserts `ADMINER_HOST` never appears in a
  tunnel start).
- **The catalog has a `File` shape (Adminer, wp-cli) and a `Dir` shape (mysql, postgres,
  openlitespeed; php/nginx on Windows)** — `binaries::Shape`, `resolve_dir`. A pinned artifact is
  URL + sha256 in `binaries.rs`, OS-agnostic when the arm says so (`manifest_pins_adminer_os_
  agnostic`). The signed update manifest's `updates::Family` is a separate, later concern.
- **Our PHP builds ship `mysqli` and `mbstring`** (PORTS: the "bulk" parity set; "common" lacks
  `mysqli` and is exactly why rexenv does not ship it). phpMyAdmin's hard requirements are
  `mysqli`, `session`, `ctype`, `json`, `hash`, `SPL`; `mbstring`, `openssl`, `zip`, `gd`,
  `zlib`/`bz2` are recommended — all present in the parity set.

## 2. Decision 1 — the row's second control (recommendation)

**Keep Browse as it is, add an "Open in browser" `SplitButton` beside it.** Primary half:
`PreferredBrowserIcon` + the label "Open in browser", `onClick → openExternal(consoleUrl)`.
Chevron: `useBrowserMenu(consoleUrl)` at `BROWSER_MENU_WIDTH`, `chevronLabel="Choose a browser"`.
`consoleUrl` is `adminerUrl({ engine })` today and the console's URL (§3) once phpMyAdmin
exists. The Databases row has no fixed-width quick-action column (the reason the Sites rows carry
no chevron, §C1.2), so the chevron fits.

Rules the control inherits, none of them new:
- a one-time detour: the menu never changes the preference (ARCHITECTURE §8.2);
- the private target appears only where `supportsPrivate` is true; Safari's row has none;
- the row is rendered only while the engine is running (the stopped row already links to
  Services), so the button can never open a console that 502s.

**The same split button replaces the plain "Open in browser" in `AdminerFrame`'s bar** — the site
Database tab and the Databases browse view both render it, and the owner's standing rule is "sob
jaigai consistency" (13 Sep 2026: the globe/arrow drift was the bug). One component, two call
sites, no third spelling.

Not chosen: a second icon-only button (no chevron) like the Sites rows — the ask names the
chevron and the private window explicitly.

## 3. Decision 2 — phpMyAdmin (recommendation)

### 3.1 Scope: MySQL and MariaDB, pinned, chosen once

- **Engines:** phpMyAdmin speaks MySQL protocol only. A PostgreSQL row keeps Adminer whatever
  the setting says, and its tooltip says so; Redis keeps its `redis-cli` line.
- **One setting, not two buttons per row:** `db_console` ∈ {`adminer`, `phpmyadmin`}, default
  `adminer`, a segmented control in the console card below the engine table (the card that today
  carries only the Adminer version row). Browse, Open in browser and the site Database tab all
  follow it for MySQL/MariaDB. The owner's ruling on consent UX (3 Sep 2026) was ONE dial over
  per-site knobs; a per-row pair of consoles is the same mistake with different icons. Switching
  is one click, so a user who wants both loses nothing.
- **Pinned version, no update family in v1.** phpMyAdmin 5.2.3 (8 Oct 2025, the newest release
  at planning time — MEASURE before pinning; 6.0 was announced as PHP ≥ 8.2 and is not out).
  Adminer's second-family machinery (`updates::Family`) exists and a third family is mechanical,
  but it is a separate row (§9): the v1 question is whether phpMyAdmin runs on our pinned PHP at
  all.
- **Downloaded on demand, never at stack start.** The ~15 MB zip is fetched when the setting is
  flipped to `phpmyadmin` (the same shape as `adminer_update_apply`: download → verify → stage →
  rebuild → reload, with the button holding the sentence while it runs). A fresh install that
  never flips the setting never downloads it, and the login-start offline planner (#175) has
  nothing new to know.

### 3.2 Serving: a second internal vhost, the same shape as Adminer's

- Host `phpmyadmin.rexenv.rex` on the `.rex` backbone; `NginxSite` with `RewriteMode::Single`,
  the default pool, `body_limit`/`read_timeout`/`php_value` from the SAME `adminer::import_cap`
  number (one cap for both consoles; importing dumps is both consoles' job), the `.user.ini`
  copy (#788), a cert from `ensure_site_cert`. Emitted by `sites::rebuild` **only when the docroot
  is staged** — an nginx block over an empty docroot is a 404 with rexenv's name on it. Added to
  the tunnel-origin assertion beside `ADMINER_HOST`.
- Docroot `app_data/phpmyadmin/` = the extracted release tree, published by rename (`phpmyadmin.
  new` → `phpmyadmin`), with a `.phpmyadmin-version` marker written LAST, as `adminer::ensure`
  does. Dotfiles stay unservable through `NGINX_DOTFILE_DENY`. The tree is ~40 MB extracted; one
  copy, no sweep (Adminer's no-sweep ruling, scaled: still one directory).
- `config.inc.php` is GENERATED by `core/phpmyadmin.rs` from a pure function, rewritten on every
  rebuild (edits lost, said in its header, like the `.user.ini`):
  - `$cfg['Servers'][1]` = MySQL `127.0.0.1:13306`, `[2]` = MariaDB `127.0.0.1:13307` — the
    fixed loopback ports from `core::db`, `auth_type = 'config'`, `user = 'root'`, `password =
    ''`, `AllowNoPassword = true`. **This is the passwordless gate**: with `auth_type config`
    there is no login form and no host field, so the only servers reachable are the two rexenv
    wrote, both loopback — the same promise Adminer's wrapper keeps in `login()`.
  - `$cfg['blowfish_secret']` = 32 random bytes, generated once into `app_data/phpmyadmin.secret`
    (owner-only through `platform.permissions()`), read back on every regeneration so sessions
    survive a rebuild.
  - `$cfg['TempDir']` = `app_data/phpmyadmin-tmp/` (Twig cache; phpMyAdmin is slow and noisy
    without it). Paths are written with forward slashes inside single-quoted PHP strings —
    app-data paths contain spaces on every OS and backslashes on Windows.
  - `$cfg['AllowThirdPartyFraming'] = false` — the DEFAULT, kept on purpose: the direct https
    vhost sends `X-Frame-Options: DENY` + `frame-ancestors 'self'`, so no page in the user's
    browser can frame a passwordless console (the clickjacking case Adminer's wrapper guards).
    The in-app frame gets its headers from the proxy (§3.3), not from this file.
  - `$cfg['SendErrorReports'] = 'never'`, `$cfg['ShowPhpInfo'] = false`, `$cfg['CheckConfigurationPermissions'] = false`
    (the file is rexenv's and world-readable permissions on Windows are not a defect).
  - `$cfg['PmaAbsoluteUri']` is NOT set: both origins (the https vhost and the proxy) must
    resolve relative, exactly as Adminer's links do.
- **Deep links:** `https://phpmyadmin.rexenv.rex/index.php?server=<1|2>` for an engine,
  `…?route=/database/structure&server=<n>&db=<name>` for a site's database. `src/lib/phpmyadmin.ts`
  mirrors `src/lib/adminer.ts`; one `consoleUrl(engine, db?)` + `consoleFrameSrc(...)` in a new
  `src/lib/dbconsole.ts` picks by the setting so no screen switches on the console name itself.

### 3.3 The in-app frame: a second proxy origin, both CSP ends

phpMyAdmin's pages use absolute paths (`/index.php`, `/js/…`, `/themes/…`), so it cannot live
under a path prefix of the `rexdb:` origin. It gets its own origin:

| | macOS | Windows | Linux |
|---|---|---|---|
| proxy origin | `rexpma://localhost` | `http://rexpma.localhost` | `rexpma://localhost` |
| `PlatformWords` field | `db_console_origins.phpmyadmin` (beside `db_browser_origin`, all three filled) | | |
| app CSP `frame-src` (`tauri.conf.json`) | `rexpma:` added beside `rexdb:` | `http://rexpma.localhost` added | as macOS |

`core/adminer.rs::forward` becomes `core/dbconsole_proxy.rs::forward(console, …)` — the same
client shape, parameterised on the upstream host and with its OWN cookie jar per console (two
`PHPSESSID`s on two hosts must never share a jar). It additionally **strips `X-Frame-Options`**
on the replayed copy — Adminer's wrapper removes that header at the source, phpMyAdmin sends
`DENY` and has no wrapper, and the proxy copy is unreachable from any browser (an origin only
rexenv's scheme handler answers), so widening it there costs nothing (`PROXY_FRAME_ANCESTORS`'s
own argument). `rewrite_frame_ancestors` and the `Location` rewrite apply unchanged.

Theme: phpMyAdmin 5.2's shipped themes have no app-driven dark switch; the frame keeps its own
look in v1. The `adminer::set_theme` dotfile trick has no phpMyAdmin equivalent — out of scope,
noted in §9.

### 3.4 Does it RUN on our PHP? — the question v1 actually answers

The pinned default pool is PHP 8.5.8. phpMyAdmin 5.2.3 declares PHP ≥ 7.2 with no ceiling, and
its vendored Twig/Symfony may deprecate-warn on 8.5. Two guards:

1. A **staging probe**, `phpmyadmin::verify_tree(php_bin, docroot)`: runs the bundled PHP CLI
   over `libraries/vendor/autoload.php` + `libraries/classes/Config.php` with our generated
   config and expects a clean exit (the Adminer `verify_pair` shape — a release zip is the only
   artifact, so only running it can answer). It runs BEFORE the rename publish; a tree that
   fails never becomes the docroot.
2. `.user.ini` carries `error_reporting = E_ALL & ~E_DEPRECATED & ~E_STRICT` and
   `display_errors = Off` for this docroot only — a deprecation in a vendored library is not a
   page the user should read.

The live example (`phpmyadmin_check`, stack tier) stages, hits the vhost through the loopback
edge and expects 200 + `<title>` containing "phpMyAdmin", on every PHP minor the stack can run
as the default pool. Run on macOS, the Dell (Windows) and the Ubuntu VM before the pin lands
(§10 T7); the PORTS row names the PHP version it was measured on.

### 3.5 What the MCP server and `rex` see

Nothing new in v1. `open` already takes a URL; `rex`'s roadmap has no console verb. A `db console`
row in `docs/CLI-ROADMAP.md` only if the owner asks.

## 4. Three platforms (`docs/PLATFORMS.md` §4)

**Common, written once:** the `SplitButton` + menu (TSX); `core/phpmyadmin.rs` (config
generation, staging, probe, docroot, marker); the vhost in `sites::rebuild`; the `db_console`
setting + IPC; `core/dbconsole_proxy.rs`; the catalog arm (OS-agnostic zip, `ANY_ARCH`).

**Per OS, all three filled:**

| Difference | macOS | Windows | Linux | Home |
|---|---|---|---|---|
| proxy origin for the phpMyAdmin frame | `rexpma://localhost` | `http://rexpma.localhost` | `rexpma://localhost` | `platform/words.rs` |
| app CSP `frame-src` entry | `rexpma:` | `http://rexpma.localhost` | `rexpma:` | `tauri.conf.json` (both `csp` and `devCsp`) |
| scheme registration | `register_asynchronous_uri_scheme_protocol` (as `rexdb`) | same call; WebView2 maps it to `http://rexpma.localhost` | as macOS (webkitgtk) | `lib.rs` (app-shell wiring, a §1.1 row in `PLAN-linux-port.md`) |
| secret file owner-only | `0600` | owner ACL via `permissions()` | `0600` | existing `PrivilegeManager`/`permissions` |
| the per-request ini cap reaching PHP | `PHP_VALUE` + `.user.ini` | `.user.ini` only (php-cgi group, #788) | as macOS | already written for all |
| paths in `config.inc.php` | `/Users/… /Library/Application Support/…` | `C:/Users/…/AppData/…` (forward slashes) | `/home/…/.local/share/…` | the generator, tested with a space AND a backslash fixture |
| private-window browsers in the chevron | bundle table | `app_catalog.rs` | `desktop.rs` | already filled |

**Proof per OS:** macOS GUI (dev build + packaged), Windows = `tauri build` on the Dell (a bare
cargo build's webview loads `localhost:1420`), Linux = the deb on the Ubuntu VM. `windows-check`
/ `linux-check` are compile gates only and the report says so. Each OS gets its SMOKE row for
both features; the ledger verdicts name the OS the proof came from.

## 5. Invariants (each a ledger row in the commit that writes its comment)

1. **phpMyAdmin's passwordless servers are the two loopback engines rexenv writes, and nothing
   the page can be told** — `auth_type = config`, no host field exists; the generator's test
   asserts no server is anything but `127.0.0.1:<fixed port>`.
2. **The direct https vhost is never frameable by a browser** — `AllowThirdPartyFraming` stays
   false; the frame-ancestor widening happens ONLY on the proxy copy (`dbconsole_proxy`), the
   same argument as `PROXY_FRAME_ANCESTORS`. A test runs the generator and asserts the key.
3. **Neither console host is ever a tunnel origin** — the existing `tunnels.rs:1015` assertion
   widened to both hosts.
4. **The two proxies never share a cookie jar** — one jar per console; a test asserts two
   distinct `Arc`s.
5. **A phpMyAdmin tree that fails the probe never becomes the docroot** — probe before rename;
   plant: a tree whose `autoload.php` is a fatal, expect the old docroot untouched.
6. **The vhost is emitted only for a staged docroot** — `rebuild` with no marker emits no
   `phpmyadmin.rexenv.rex` block (test on the generated nginx text).

## 6. Out of scope (v1)

- phpMyAdmin updates through the signed manifest (a third `Family`) — §9.
- phpMyAdmin's palette following the app — §9.
- PostgreSQL in phpMyAdmin — impossible; Postgres keeps Adminer, by design, forever.
- A per-site console choice — the owner's one-dial ruling.
- phpMyAdmin's configuration storage (`pmadb`, the control user) — the "advanced features"
  banner stays; wiring a `phpmyadmin` schema into every MySQL/MariaDB data dir is a day of its
  own and nobody asked for bookmarks.

## 7. Risks

- **PHP 8.5 vs phpMyAdmin 5.2.x.** The one unknown that can kill v1; T7 measures it before T2
  pins anything. If 5.2.3 fatals on 8.5, the options are (a) pin a `php` minor for the vhost's
  pool — rexenv runs one pool per PHP VERSION already, so the vhost can name 8.4's port instead
  of the default's; (b) wait for 6.0. Decide on the measurement, not now.
- **`notices-check.py`** walks THIRD-PARTY-NOTICES against what ships. phpMyAdmin's zip
  vendors ~60 composer packages; the row is phpMyAdmin (GPL-2.0-or-later) as ONE distributed
  artifact with its own `LICENSE` + `vendor/` notices intact in the tree, the way wp-cli's phar is
  one row. If the script's vendored-composer table wants each package, T2 says so and the table
  grows from the zip's `composer.lock`.
- **The second custom scheme on WebView2.** `rexdb` → `http://rexdb.localhost` is measured;
  `rexpma` follows the same mapping, but the Windows row is a RUN on the Dell, not an inference.
- **Session cookies through the proxy.** phpMyAdmin sets `pma_lang`, `pmaUser-*`, `phpMyAdmin`
  (session id) with `SameSite=Strict`; the Rust jar is not a browser and stores them anyway — the
  Adminer wrapper's `SameSite=None; Partitioned` rewrite exists for the DIRECT iframe case that
  the proxy made unnecessary. Measure once in T5; if the jar drops a `Strict` cookie, the proxy
  rewrites `SameSite` on replay (never at the source).
- **Disk.** ~40 MB per staged tree + the zip in the cache. One copy each; `disk-guard.sh` is
  unaffected (app data, not the build tree).

## 8. Testing

- **L0** (`cargo test --lib`): the config generator (servers loopback-only, forward-slash paths
  with a space and a backslash, the framing key, the secret round-trip), `needs_restage`,
  `vhost emitted only when staged`, the tunnel-origin assertion, the jar-separation test, the
  TSX test that the Databases row renders the split button only for a running browsable engine.
- **L1** (`src-tauri/examples/phpmyadmin_check.rs`, stack tier, fixture-owned docroot under
  `common::sandbox()`): stage → GET through the loopback edge → 200 + title; the probe refuses
  a planted fatal tree. Declared in `scripts/live-checks.sh` with `all` (macOS, Windows, Linux).
- **wk-check** `scripts/wk-checks/openin.js` gains the Databases row: the chevron opens, the
  private rows appear for `supportsPrivate` browsers only.
- **SMOKE** rows (macOS main body + § Windows + § Linux): open MySQL in the default browser,
  in a second browser from the chevron, in a private window; flip the console to phpMyAdmin,
  Browse lands in phpMyAdmin logged in, Postgres still opens Adminer; flip back.

## 9. Follow-ups (their own TODO rows, not v1)

- phpMyAdmin as the THIRD `updates::Family` (`phpmyadmin` artifact, `ANY_ARCH`, the probe as
  the gate) — the Adminer plan's shape, one afternoon once v1 is measured.
- phpMyAdmin's theme following the app (5.2 ships `pmahomme`/`bootstrap`/`metro`/`original`;
  a dark variant needs a theme we would vendor — decide when someone asks).
- A `rex db console` verb and an MCP `open` shortcut, if the CLI roadmap wants them.

## 10. Task list (the TODO rows, in build order)

| # | Task | Common / per-OS | Proof |
|---|---|---|---|
| T1 | Databases row: "Open in browser" `SplitButton` (icon + chevron + private rows) beside Browse; the same split button in `AdminerFrame`'s bar | frontend only, backend already per-OS | wk-check `openin.js`; GUI on macOS + Dell + Ubuntu VM; SMOKE ×3 |
| T2 | Catalog arm `phpmyadmin` (`Shape::Dir`, OS-agnostic zip, sha256, version measured by T7), `docs/PORTS.md` (macOS + Windows + Linux tables), THIRD-PARTY-NOTICES row, `notices-check.py` green | common | `cargo test --lib` manifest tests; verify |
| T3 | `core/phpmyadmin.rs`: docroot, marker, rename-publish staging, `config.inc.php` generator (pure, tested), blowfish secret file (owner-only via `permissions()`), `.user.ini`, `verify_tree` probe | common; secret perms + path spelling per OS inside existing traits | L0 + plant (§5.5) |
| T4 | The vhost in `sites::rebuild` (staged-only), cert, tunnel-origin assertion widened | common | L0 (§5.3, §5.6) |
| T5 | `core/dbconsole_proxy.rs`: `forward(console, …)` with one jar per console, XFO strip, `rexpma` scheme in `lib.rs`, `PlatformWords` origin (3 filled), `tauri.conf.json` `frame-src` (csp + devCsp), §1.1 row in `PLAN-linux-port.md` | per OS: origin + CSP entry | L0 (§5.4); GUI on all three (the empty-panel class, #703) |
| T6 | Setting `db_console` + IPC (`db_console_get` / `db_console_set` = download → probe → stage → rebuild → reload), the console card's segmented control + phpMyAdmin version line, `src/lib/dbconsole.ts` choosing URLs; Browse / Open in browser / site Database tab follow it; Postgres rows pinned to Adminer with the tooltip | common | tsc + eslint; GUI ×3; SMOKE ×3 |
| T7 | `examples/phpmyadmin_check.rs` (stack tier, `all`), run on macOS, the Dell, the Ubuntu VM on the pinned PHP; the measurement that fixes T2's version (and §7's fallback if 8.5 fatals) | per OS run | L1 ×3, `scripts/live-checks.sh`, `docs/TESTING.md` |
| T8 | Docs in the same commits: ARCHITECTURE (§Adminer → "Database consoles"), MAP, PLATFORMS §3 (the second proxy origin), SMOKE (main + Windows + Linux), DESIGN (the console card), ledger rows §5.1–§5.6 with per-OS verdicts, `status.py --write`, this plan's header → SHIPPED and the archive move | — | `verify.sh` green; `doc-counts.sh` |

Order: T1 ships on its own (one commit, no backend). T7's measurement comes BEFORE T2's pin
(a pin nobody has run is superstition with a constant name — the Adminer plan's lesson). T3–T5
are one feature and can land as separate commits behind the setting default (`adminer`), so
nothing a user sees changes until T6.
