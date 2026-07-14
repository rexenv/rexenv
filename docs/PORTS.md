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
| php-fpm pools | **9780–9785** | TCP | `core/php.rs` — `9700 + major*10 + minor` (8.0–8.5, one pool per installed minor) |
| FrankenPHP override backends | **8200–8299** (per-site, FNV-1a of domain) | TCP | `core/frankenphp.rs` `FRANKENPHP_BASE_PORT` |
| MySQL | **13306** | TCP | `core/database.rs` `MYSQL_PORT` |
| MariaDB | **13307** | TCP | `core/db.rs` `MARIADB_PORT` |
| PostgreSQL | **15432** | TCP | `core/db.rs` `POSTGRES_PORT` |
| Redis | **16379** | TCP | `core/db.rs` `REDIS_PORT` |
| Mailpit SMTP | **11025** | TCP | `core/mail.rs` `MAILPIT_SMTP_PORT` |
| Mailpit HTTP API | **18025** | TCP | `core/mail.rs` `MAILPIT_HTTP_PORT` |
| Adminer | no port — internal vhost `adminer.rexenv.rex` via shared nginx | — | `core/adminer.rs` |
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
| PHP | 8.0.30 / 8.1.34 / 8.2.31 / 8.3.31 / 8.4.23 / 8.5.8 | static-php **"bulk"** build ("common" lacks `mysqli`). 8.0 = upstream-EOL, frozen at .30. NO 7.4 (never published — needs self-hosting, like the Xdebug build) |
| PHP debug (Xdebug 3.4.5) | 8.3.31 | wired but **unresolvable** — checksums empty until hosted (see `docs/xdebug-debug-build.md`) |
| Nginx | 1.30.3 | jirutka static; Homebrew `libpcre2` relinked to `/usr/lib` |
| MySQL | 8.4.6 | dir tree, Oracle-signed (no re-sign), CDN URL + browser UA |
| WP-CLI | 2.12.0 | `.phar`, `resolve_file`, no chmod/codesign |
| FrankenPHP | 1.12.4 | embeds its OWN PHP (not the pools) |
| PostgreSQL | 18.4.0 | theseus-rs portable, TCP-only |
| Redis | 8.8.0 | **bottle BUNDLE** (`resolve_bundle`): Homebrew redis + openssl@3 3.6.3 bottles (arm64_sonoma / sonoma), merged + relinked to `@loader_path` + re-signed by `prepare_binary_tree`. ghcr blobs are content-addressed — the URL embeds the pinned digest, so pins can 404 (formula GC) but never drift |
| MariaDB | 12.3.2 | bottle BUNDLE: mariadb (server + client + dump + bootstrap SQL/errmsg/charsets ONLY — plugins/scripts excluded) + openssl@3 3.6.3 + pcre2 10.47. groonga/lz4/lzo/xz/zstd are plugin-only deps, not bundled. Init = `mariadbd --bootstrap` fed the bundled SQL over stdin (`core/mariadb.rs`) |
| Mailpit | 1.30.3 | |
| Adminer | 5.4.2 | single `.php`, OS-agnostic |
| cloudflared | 2026.6.1 | |

All checksum-locked (SHA-256/512 by source). macOS `prepare_binary` order is non-negotiable:
**de-quarantine → relink Homebrew dylibs → codesign LAST** (relinking invalidates the
signature; Apple Silicon kills unsigned binaries).
