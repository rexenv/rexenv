# CLAUDE.md — rexenv

rexenv is a native, lightweight, NO-Docker local development environment for web &
WordPress developers. Tauri 2 desktop app (Rust backend + React/TS frontend). It runs
a developer's whole local stack — web servers, multiple PHP versions, databases,
one-click WordPress, local `.test` domains with auto-HTTPS, mail catching, public
sharing — from one UI. **macOS first**, then Windows, then Linux.

## Architecture rule (non-negotiable)
- `commands/` are **thin** Tauri IPC handlers — they only translate calls and invoke `core/`.
- `core/` is **platform-agnostic** ("the what") — domain logic, never imports OS-specific code.
- ALL OS-specific code lives ONLY in `src-tauri/src/platform/`, behind **Rust traits**
  (`traits.rs`), with impls selected via `#[cfg(target_os = "...")]`.
- Build the **macOS** impls now. Leave `platform/windows/` and `platform/linux/` as
  `todo!()` stubs — adding those OSes later means filling stubs, NOT restructuring.
- Platform traits: `DnsManager`, `CertTrustManager`, `PrivilegeManager`,
  `ProcessSupervisor`, `AutostartManager`, `PermissionManager`, `ShellRunner`,
  `Paths`, `BinaryProvider`.

## Non-negotiables (foundational decisions — don't relitigate)
- **No Docker.** Services are **native static binaries**, downloaded on demand (small installer).
- All binaries go through **`BinaryProvider`** (manifest: os+arch+version → url+checksum;
  checksum is SHA-256 or SHA-512 depending on source). **macOS `prepare_binary`:**
  de-quarantine (`xattr -d com.apple.quarantine`), **relink any Homebrew dylib deps to macOS
  system libs** (`install_name_tool -change … /usr/lib/…`, so the binary is self-contained — no
  Homebrew at runtime), then ad-hoc code-sign LAST (`codesign --force --sign -`; relinking
  invalidates the signature, and Apple Silicon kills unsigned binaries).
- **Request topology (default sites):** browser → **Caddy** (:443, TLS termination with
  local-CA certs; auto-HTTPS/internal issuer DISABLED) → one shared **Nginx** on an internal
  HTTP port (vhost by `server_name`) → **php-fpm** (FastCGI). Caddy proxies all `*.test` to
  the single Nginx port. **No direct Caddy→php-fpm path for default sites.**
- One shared **Nginx** process (a server block per site) is the default site server.
- **One PHP-FPM pool per PHP version** (not per site) — shared master, low memory.
- Caddy is the Phase 1 edge router (invisible to the user; may become custom Rust/Pingora later).
- Embedded DNS via **hickory-dns** resolves `*.test → 127.0.0.1` (wildcard ⇒ multisite works
  free). Runs as a managed task on a fixed loopback port (`DEFAULT_DNS_PORT`).
- Local **CA via rcgen** issues a trusted cert per domain; must support wildcard SAN (`*.site.test`).
- **System changes go through platform traits, never direct.** The `/etc/resolver/test` write
  is a root op via **`PrivilegeManager`** (macOS osascript admin prompt). The CA **trust** is a
  USER op via **`CertTrustManager`** (macOS: login keychain — `security add-trusted-cert` shows
  its own native dialog, no root; System-keychain trust can't be set from a detached-root
  osascript session). So system setup is ~2 prompts; a true single prompt needs a privileged
  helper (SMAppService) — deferred.
- **SQLite** for all app state (site list, settings, per-site config).
- Leave room in the config generator for THREE rewrite templates from the start:
  single / subdomain-multisite / subdirectory-multisite.

## Tech stack
- Backend: Tauri 2, Rust, tokio, hickory-dns, rcgen, reqwest, rusqlite/sqlx, sysinfo, which, notify.
- Frontend: React + TypeScript + Vite, Tailwind + shadcn/ui (Radix), TanStack Query
  (IPC/server state), Zustand (UI state), lucide-react, xterm.js.
- **Full folder tree: see README.md** ("Project structure"). Don't duplicate it here.

## Conventions
- **TypeScript strict** mode.
- All Tauri IPC goes through **typed wrappers in `src/lib/ipc/`** — UI never calls raw `invoke`.
- Design tokens come from **DESIGN_BRIEF.md** (palette + 3 type roles); surfaced as
  CSS vars in `src/styles/tokens.css` and the Tailwind theme. Don't hardcode hex values.
- `src/routes/` map **1:1** to the screens in DESIGN_BRIEF.md
  (Sites, SiteDetail, Services, Databases, Mail, Tunnels, Settings, Onboarding).
- JetBrains Mono for ALL technical values (domains, paths, versions, ports, commands).
  Space Grotesk for hero/onboarding only. Inter (SF Pro Text) for UI/body.

## Pointers
- Full detail in **PROJECT_SPEC.md**. Screen designs in **DESIGN_BRIEF.md** and **design/** (.dc.html).
- Phase plan: PROJECT_SPEC.md §5. Current: **Phase 1 (macOS MVP)**. Task list in **TASKS.md**.

## Working rule
- Work in **small, verifiable steps. One task at a time. Verify before moving on.**
- Surface assumptions before non-trivial work; keep changes surgical (scope discipline).
