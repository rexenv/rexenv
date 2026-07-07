# Ports & pinned binaries (as built — macOS)

The single place for the two fact sets that drift most. If you change a port or bump a
binary, update THIS file in the same commit.

## Port map

| Service | Port | Proto | Constant |
|---|---|---|---|
| Caddy edge HTTP | **80** | TCP | `core/proxy.rs` `DEFAULT_HTTP_PORT` |
| Caddy edge HTTPS | **443** | TCP | `core/proxy.rs` `DEFAULT_HTTPS_PORT` |
| Caddy admin | **unix socket** (`<config>/caddy-admin.sock`, `0600`) — never TCP `:2019` | — | `core/proxy.rs` `ADMIN_SOCKET_FILE` |
| Embedded DNS | **15353** | UDP | `core/dns.rs` `DEFAULT_DNS_PORT` |
| Shared Nginx | **18088** | TCP | `core/services.rs` `NGINX_HTTP_PORT` |
| php-fpm pools | **9781 / 9782 / 9783** | TCP | `core/php.rs` — `9700 + major*10 + minor` (8.1/8.2/8.3) |
| FrankenPHP override backends | **8200–8299** (per-site, FNV-1a of domain) | TCP | `core/frankenphp.rs` `FRANKENPHP_BASE_PORT` |
| MySQL | **13306** | TCP | `core/database.rs` `MYSQL_PORT` |
| MariaDB (stub) | **13307** | TCP | `core/db.rs` `MARIADB_PORT` |
| PostgreSQL | **15432** | TCP | `core/db.rs` `POSTGRES_PORT` |
| Redis (stub) | **16379** | TCP | `core/db.rs` `REDIS_PORT` |
| Mailpit SMTP | **11025** | TCP | `core/mail.rs` `MAILPIT_SMTP_PORT` |
| Mailpit HTTP API | **18025** | TCP | `core/mail.rs` `MAILPIT_HTTP_PORT` |
| Adminer | no port — internal vhost `adminer.rexenv.test` via shared nginx | — | `core/adminer.rs` |
| cloudflared | outbound-only, no inbound port | — | `core/tunnels.rs` |

- Canonical startup-order registry: `core/ports.rs` `default_ports()`.
- Every start is gated by `core/ports::ensure_free` — probe rules: privileged TCP (<1024)
  = connect-probe, high TCP = bind-test, UDP = bind-test. `is_listening()` = connect probe.
- Mailpit is offset from stock 1025/8025 so a standalone Mailpit/MailHog or Herd Pro's
  bundled one never clashes.

## Pinned binaries (`core/binaries.rs`)

| Binary | Version | Notes |
|---|---|---|
| Caddy | 2.11.4 | edge only |
| PHP | 8.1.34 / 8.2.31 / 8.3.31 | static-php **"bulk"** build ("common" lacks `mysqli`) |
| PHP debug (Xdebug 3.4.5) | 8.3.31 | wired but **unresolvable** — checksums empty until hosted (see `docs/xdebug-debug-build.md`) |
| Nginx | 1.30.3 | jirutka static; Homebrew `libpcre2` relinked to `/usr/lib` |
| MySQL | 8.4.6 | dir tree, Oracle-signed (no re-sign), CDN URL + browser UA |
| WP-CLI | 2.12.0 | `.phar`, `resolve_file`, no chmod/codesign |
| FrankenPHP | 1.12.4 | embeds its OWN PHP (not the pools) |
| PostgreSQL | 18.4.0 | theseus-rs portable, TCP-only |
| Mailpit | 1.30.3 | |
| Adminer | 5.4.2 | single `.php`, OS-agnostic |
| cloudflared | 2026.6.1 | |

All checksum-locked (SHA-256/512 by source). macOS `prepare_binary` order is non-negotiable:
**de-quarantine → relink Homebrew dylibs → codesign LAST** (relinking invalidates the
signature; Apple Silicon kills unsigned binaries).
