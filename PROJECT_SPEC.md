# rexenv — Project Specification

> **Goal:** A native, lightweight, limitless local development environment. No Docker. macOS first, then Windows and Linux. Built with Claude Code.

**Name:** `rexenv` — *a fast, native local development environment*

**CLI command:** `rexenv` (e.g. `rexenv start`, `rexenv create mysite`) — **not** `rex`, to avoid a PATH collision with the Perl Rex tool's `rex` command. (Full rationale and verification in the appendix.)

---

## 1. Foundational Architectural Decisions

Lock these in early — changing them later means rewriting large parts of the codebase.

| Area | Decision | Why |
|---|---|---|
| App shell | **Tauri 2.0** (Rust backend + web frontend) | Uses the OS-native webview instead of bundling Chromium → small installer, low RAM. This is the foundation of the "lightweight" goal. |
| Service model | **Native static binaries**, downloaded on demand | No Docker. To keep the installer small, binaries are downloaded on first use rather than bundled. |
| Edge layer | A single **internal edge router** runs at all times (holds :80/:443, terminates SSL, routes by Host header) | Multiple web servers (Nginx/Apache/OpenLiteSpeed) can't bind the same port, so an invisible front router is required to send each request to the right backend. |
| Default site serving | One shared **Nginx** process, a separate server block per site | N sites in a single process → very lightweight. |
| PHP model | **One PHP-FPM pool per PHP version** (not per site) | All sites on the same version share one FPM master → lower memory. |
| Per-site server override | A site can run on Apache / OpenLiteSpeed / FrankenPHP; those run as separate processes on internal loopback ports, with the edge router routing to them | Enables the "multiple servers" feature while keeping the default lightweight. |
| DNS | A single **embedded DNS resolver** (Rust, `hickory-dns`) resolves `*.test → 127.0.0.1` uniformly on every OS. **WP Multisite (subdomain) works automatically**, because `*.mysite.test` also resolves. | One uniform solution instead of three per-OS hacks (dnsmasq/hosts) → eliminates the "domains don't work" problem on Windows from the start. |
| Local SSL | A local **CA** issues a cert per domain (`rcgen` crate) and is added to the system/browser trust store. Must support **wildcard certs** (`*.mysite.test`) for multisite. | Auto-HTTPS on every site with no external dependency. |
| App state | **SQLite** (site list, settings, per-site config) | Lightweight, embedded, no separate server. |

### Edge router tech choice
- **Phase 1:** use **Caddy** as the edge router (single binary, automatic HTTPS, simple config) for fast development. It is internal and invisible to the user.
- **Later (optional):** for maximum lightness/control, build a custom router in Rust with **Pingora** (Cloudflare's proxy framework) to replace Caddy.

---

## 2. Feature List (tiered)

### Tier 1 — MVP (without this the app is pointless)
- Site management: create / list / start / stop / delete
- Per-site PHP version select (multi-PHP, one-click switch)
- Per-site web server select (default Nginx)
- Local `.test` domains, wildcard support
- Auto-HTTPS (local CA, trusted cert per site)
- Built-in database services: **MySQL, MariaDB, PostgreSQL, Redis, SQLite** — start/stop/version switch (no separate DBngin needed)
- One-click WordPress install (download + create DB + wp-config + install)
- Resource monitor (per-service RAM/CPU — to back up the "lightweight" claim)

### Tier 2 — Differentiators (these set the product apart)
- **WordPress Manager** (WP-CLI-based, from the UI — the signature feature):
  - Plugins: list / activate / deactivate / update / install / delete
  - Themes: list / activate / update / install / delete
  - User management, one-click admin auto-login
  - Toggle debug mode (`WP_DEBUG`)
  - DB search-replace (to change URL/domain)
  - WordPress core update / re-install
  - **Multisite (Network):** one-click enable/convert; choose subdomain or subdirectory mode
  - **Network sites:** sub-site list / create / delete / archive; jump to each site's URL and admin
  - **Network plugins/themes:** network-wide activate/deactivate; see which plugin is active on which site
  - **Super admin** management
- Built-in **database browser** (Adminer — single PHP file, served via the stack)
- **Mailpit** email catching (captures all outgoing mail, shows it in the UI)
- Per-site **Xdebug** toggle (one click on/off)
- Real-time **log viewer** (nginx / php-fpm / mysql)
- Public sharing via **Cloudflare Tunnel** (free, unlimited — answer to Expose's limits)
- Built-in **terminal** (WP-CLI, Composer, npm in one place — xterm.js)

### Tier 3 — Polish (add later)
- Site templates / blueprints (like Local, with custom plugins/themes) — including multisite blueprints
- Backup / restore (snapshot files + DB together)
- Import from production (pull a live site down)
- Node.js version management (for modern builds)
- WP-Cron runner / scheduler
- One-click installers for other apps (Laravel, Drupal, Joomla)
- Auto-switch PHP/Node version on `cd` into a project folder (like FlyEnv)

### 2.1 WordPress Multisite — cross-cutting considerations
Multisite isn't just a UI feature; it touches DNS, SSL, and web-server config — three places. Keeping these in mind from Phase 1 avoids refactoring later.

- **DNS:** in subdomain multisite, every subdomain of `*.mysite.test` must reach the same site. Our embedded resolver already sends everything under `.test` to `127.0.0.1`, so **no extra DNS work is needed** — a real advantage of our architecture.
- **SSL:** subdomain mode over HTTPS needs a **wildcard cert** (`*.mysite.test`). So the cert-generation layer (`rcgen`) must support issuing certs with a wildcard SAN per multisite.
- **Web server rewrite rules:** subdomain and subdirectory modes need **different** Nginx/Apache rewrite rules. The config generator must know both modes (follow WordPress's official network rule templates).
- **wp-config constants:** enabling multisite requires adding constants like `MULTISITE`, `SUBDOMAIN_INSTALL`, `DOMAIN_CURRENT_SITE`, `PATH_CURRENT_SITE` — the app writes these automatically.
- **Implementation (WP-CLI):** `wp core multisite-install` / `wp core multisite-convert` (enable), `wp site list/create/delete`, `wp plugin activate --network`, `wp super-admin add` — all via the bundled WP-CLI.
- **Edge router routing:** in subdomain mode the edge router must send all Hosts under `*.mysite.test` to the same backend (a per-site wildcard route). Subdirectory mode is normal single-host; only the rewrite rules differ.

---

## 3. Tech Stack (by layer)

### App shell & backend
- **Tauri 2.0** — app framework
- **Rust** — all core logic (process management, DNS, SSL, service orchestration)
- **tokio** — async runtime
- IPC: Tauri commands (Rust ↔ frontend)

### Frontend (UI)
- **React + TypeScript + Vite**
- **Tailwind CSS** + **shadcn/ui** (Radix-based components)
- **TanStack Query** — backend/IPC state
- **Zustand** — UI state
- **lucide-react** — icons
- **xterm.js** — built-in terminal
- (optional) **Monaco editor** — config/file editing

### Core Rust crates
| Job | Crate |
|---|---|
| Child process spawn/supervise | `tokio::process`, `std::process` |
| Embedded DNS server | `hickory-dns` (formerly trust-dns) |
| Local cert generation (with wildcard SAN) | `rcgen` |
| Binary download | `reqwest` |
| Archive extraction | `tar` + `flate2`, `zip` |
| Config serialization | `serde`, `serde_json` |
| App state DB | `rusqlite` or `sqlx` (SQLite) |
| OS path resolution | `directories` / `dirs` |
| Find executables | `which` |
| Resource monitor | `sysinfo` |
| File watch (auto-switch) | `notify` |

### Bundled / downloaded external binaries
- **PHP (static):** `static-php-cli` (crazywhalecc/static-php-cli) — builds static PHP for all three OSes
- **Nginx:** prebuilt
- **Apache (httpd), OpenLiteSpeed:** official binaries
- **MySQL / MariaDB / PostgreSQL:** official binary tarball/zip
- **Redis:** official on Linux/macOS; ⚠️ **no official Redis on Windows** — use Memurai or a port (keep as a caveat)
- **Caddy** — edge router (single binary)
- **Mailpit** — single binary (GitHub releases)
- **cloudflared** — single binary (tunnel)
- **Adminer** — single PHP file (DB browser)
- **WP-CLI** — `wp-cli.phar`, run via the bundled PHP (handles both single and multisite)

### Build / packaging
- Tauri bundler → **.dmg** (mac), **.msi/.exe** (Windows, NSIS), **.deb + .AppImage** (Linux)
- Auto-update: **Tauri updater**

---

## 4. Platform-Specific — what must be handled separately (to avoid restructuring)

> **Golden rule:** keep all OS-dependent code in a `platform/` module, define each as a **Rust trait**, and keep per-OS implementations separate via `#[cfg(target_os = "...")]`. UI and core orchestration logic must **never** touch OS-specific code directly — always behind a trait.

Each item below = a trait. Building these as abstractions from the start means no code-breaking when adding Windows/Linux.

### 4.1 `DnsManager` — the biggest divergence
- **macOS:** `/etc/resolver/test` file + embedded DNS (or dnsmasq)
- **Linux:** systemd-resolved config or dnsmasq + NetworkManager — varies by distro
- **Windows:** no dnsmasq; run the embedded DNS server on loopback and point the OS's DNS at it (wildcards don't work in the hosts file)
- **Solution:** we run an embedded resolver via `hickory-dns` — but the part that trusts/points the OS at it differs per OS. (Subdomain multisite works for free from this uniform resolver.)

### 4.2 `CertTrustManager` — installing the CA into the system trust store
- **macOS:** `security add-trusted-cert` (Keychain)
- **Windows:** `certutil` (Windows cert store)
- **Linux:** `update-ca-certificates` — path varies by distro; **Firefox's NSS store must be handled separately**
- Note: once the CA is trusted, all certs it signs (including wildcard multisite certs) are trusted — so multisite needs nothing extra here; the work is in the cert-generation (`rcgen`) layer.

### 4.3 `PrivilegeManager` — binding :80/:443 and privileges
- **macOS/Linux:** binding ports 80/443 may need elevated privileges; sudo prompt or `setcap`
- **Windows:** Admin/UAC elevation
- How elevation is requested differs per OS

### 4.4 `ProcessSupervisor` & `AutostartManager` — services and start-on-boot
- **macOS:** `launchd` (.plist)
- **Linux:** `systemd` user unit
- **Windows:** Windows Service or Task Scheduler
- Spawning processes is similar, but "auto-start on boot" is completely different

### 4.5 `Paths` module — file locations and paths
- where binaries, app-data, config, logs live — different convention per OS
- ⚠️ **macOS is case-insensitive by default, Linux is case-sensitive** — be careful with filenames
- path separator (`/` vs `\`) — never hardcode; all paths via the `dirs` crate
- hosts file: `/etc/hosts` vs `C:\Windows\System32\drivers\etc\hosts`

### 4.6 `BinaryProvider` — resolving the right binary
- needed for: macOS (**arm64 + x86_64**), Windows (**x86_64**), Linux (**x86_64 + arm64**)
- source, format (tar.gz / zip), and extraction differ per OS/arch
- keep a manifest: "this OS + this arch + this service version → this URL + this checksum"

### 4.7 `PermissionManager` — file permissions
- **Unix:** chmod/chown (POSIX permissions)
- **Windows:** ACLs — chmod/chown are meaningless; a different API

### 4.8 `ShellRunner` — command execution and the built-in terminal
- **macOS/Linux:** bash/zsh
- **Windows:** PowerShell/cmd
- shell invocation differs when running WP-CLI, Composer, npm

### 4.9 Build pipeline (not code, but plan ahead)
- **macOS:** Apple **notarization** + signing required, or Gatekeeper blocks it
- **Windows:** without a code-signing cert, SmartScreen warns; Defender Firewall prompts; antivirus may interfere
- **Linux:** comparatively easy (AppImage/deb)

---

## 5. Build Order (phases)

1. **Phase 1 (macOS MVP):** Tauri scaffold → embedded DNS + local CA → Caddy edge router → one Nginx + one PHP version → site create/list → MySQL service → one-click WordPress. *Goal: a WP site running over HTTPS.*
2. **Phase 2:** multi-PHP, Apache + OpenLiteSpeed override, MariaDB + PostgreSQL + Redis, resource monitor.
3. **Phase 3 (differentiators):** WordPress Manager (**including Multisite/Network management**), Adminer, Mailpit, Xdebug toggle, log viewer, Cloudflare Tunnel, built-in terminal.
4. **Phase 4:** Windows port (Windows impl of each `platform/` trait).
5. **Phase 5:** Linux port.

---

## 6. Tips for working with Claude Code
- Keep this file at the repo root; ask Claude Code to reference it.
- Break each phase into small, verifiable tasks across separate sessions (not all at once).
- Build empty (stub) versions of the `platform/` traits first — fill in the macOS impl, leave the others as `todo!()`. This keeps the architecture right from the start.
- Leave room in the config generator from the start for three rewrite templates: single / subdomain-multisite / subdirectory-multisite — easier to add later.
- Reference reading: **FlyEnv** (open source, almost the same thing) and **Laravel Valet** (open source) — to see how they handle DNS/SSL/vhosts.

---

## Appendix: Name — final decision and verification

**Final name: `rexenv`** — "Rex" (king) + "env" (environment). Pronunciation: **rex-env** (in the familiar dotenv/direnv pattern).

Why rexenv:
- **Semantic fit:** "env" names the product category directly — a development **environment**. Other tested suffixes (stack/dev/kit/host/lde) didn't fit as precisely.
- **Availability (verified):** free on npm, crates.io, and as a GitHub org.
- **No collision:** no software product named "rexenv" (what exists is an environmental testing company and a razor — irrelevant).

⚠️ **Domain:** `rexenv.com` is taken (by that environmental company). A dev tool doesn't need `.com` — check/register `rexenv.dev`, `rexenv.app`, or `getrexenv.com` / `tryrexenv.com`. Quick check: `whois rexenv.dev`.

⚠️ **One caveat:** bare "Rex" is a known devops tool (RexOps/Rex, Perl + SSH). So keep public branding as `rexenv`, and the **CLI command as `rexenv`** (not `rex`) to avoid a PATH collision with Perl Rex's `rex` command.

### Considered alternatives (record)
| Name | Why rejected |
|---|---|
| Wisp / wispstack / trywisp | handles were free, but the "Wisp" brand is crowded in dev/hosting (wisp.gg, wispcms, wispbyte) |
| **stackcove** | strongest alternative — free across all three registries, no dev collision. Kept as a **backup name** since we chose rexenv |
| rexdev / rexhost / rexkit | "RexDev" is crowded as a generic dev handle and .com/.dev are taken; "host" = hosting (wrong meaning + saturated namespace); "kit" is vague + handles taken |
| RexLDE | unpronounceable; in speech it collapses to "Rex" and returns to the devops-Rex collision |

**Name-verification method (for the future):** GitHub → `github.com/<name>` (404 = free), npm → `npmjs.com/package/<name>`, crates → `crates.io/crates/<name>`, domain → registrar or `whois`, plus a quick trademark Google search.
