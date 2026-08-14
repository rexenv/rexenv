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
| php-fpm DEBUG (Xdebug) pools | **9981–9985** | TCP | `core/php.rs` — `9900 + major*10 + minor` (8.1–8.5 only; started on demand for Xdebug-toggled sites) |
| Xdebug DBGp (IDE listens) | **9003** | TCP | Xdebug default — outbound from PHP to the IDE, rexenv binds nothing |
| FrankenPHP override backends | **8200–8299** (per-site, RECORDED — allocated lowest-free, collision-free; never re-derived) | TCP | `core/frankenphp.rs` `FRANKENPHP_BASE_PORT`, `sites.override_port` |
| Apache override backends | **8300–8399** (per-site, RECORDED — disjoint range) | TCP | `core/apache.rs` `APACHE_BASE_PORT`, `sites.override_port` |
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
| Xdebug | 3.5.3 for PHP 8.1–8.5 — **pinned PER MINOR, not app-wide** (`XdebugBottle` rows; `XDEBUG_VERSION` is the default these five reference). Xdebug's support windows close, so a minor past its window is frozen at its last release (7.4 → 3.1.6, ledger #320) | bottle BUNDLE (one part) from `shivammathur/extensions` ghcr (the setup-php tap; arm64_sonoma + sonoma digests, all 10 downloaded + load-tested at pin time). Loads into the EXISTING static php-fpm (`-d zend_extension`) — the per-site toggle's debug pools; every spawn is gated on a real load probe. **PHP 8.0 excluded** (its static build exports no Zend symbols — dlopen impossible; the legacy `php-debug` self-build wiring in `core/binaries.rs` + `docs/xdebug-debug-build.md` stays as the fallback recipe, still unresolvable) |
| Nginx | 1.30.3 | jirutka static; Homebrew `libpcre2` relinked to `/usr/lib` |
| MySQL | 8.4.6 (default) / 8.0.44 | dir tree, Oracle-signed (no re-sign), CDN URL + browser UA. **Per-engine version switch**: each SERIES keeps its own datadir (default series on the legacy `mysql/data`; others under `mysql/<series>/data`) — never an in-place up/downgrade. 8.0.44 hashed from real downloads (both arches), arm64 run-verified |
| WP-CLI | 2.12.0 | `.phar`, `resolve_file`, no chmod/codesign |
| Composer | 2.10.2 | `.phar`, `resolve_file` — ALWAYS run via the SITE's bundled PHP (add-from-Git composer step; platform checks match the PHP the plugin runs on; a system composer is never executed — non-phar wrappers exist, e.g. Herd's). Sha verified against getcomposer.org's published `.sha256sum`, run-tested on static PHP 8.3.31 at pin time |
| FrankenPHP | 1.12.4 | embeds its OWN PHP (not the pools) |
| PostgreSQL | 18.4.0 (default) / 17.10.0 / 16.14.0 | theseus-rs portable, TCP-only; project-published `.sha256` pins. PG major datadirs are mutually incompatible — the per-series datadir rule is load-bearing (`postgres/<major>/data`) |
| Redis | 8.8.0 | **bottle BUNDLE** (`resolve_bundle`): Homebrew redis + openssl@3 3.6.3 bottles (arm64_sonoma / sonoma), merged + relinked to `@loader_path` + re-signed by `prepare_binary_tree`. ghcr blobs are content-addressed — the URL embeds the pinned digest, so pins can 404 (formula GC) but never drift |
| MariaDB | 12.3.2 (default) / 11.4.12 LTS (`mariadb@11.4` bottle, identical layout+closure) | bottle BUNDLE: mariadb (server + client + dump + bootstrap SQL/errmsg/charsets ONLY — plugins/scripts excluded) + openssl@3 3.6.3 + pcre2 10.47. groonga/lz4/lzo/xz/zstd are plugin-only deps, not bundled. Init = `mariadbd --bootstrap` fed the bundled SQL over stdin (`core/mariadb.rs`) |
| Apache httpd | 2.4.68 | bottle BUNDLE: httpd (`bin/httpd` + ONLY the 10 modules the generated conf loads + `.bottle/etc/httpd/mime.types`) + apr 1.7.6 + apr-util 1.6.3 + pcre2 10.47. mod_ssl/mod_http2/mod_brotli excluded ⇒ openssl/nghttp2/brotli never bundled. Per-site loopback override backend; `.php` → the site's shared php-fpm pool via mod_proxy_fcgi (`core/apache.rs`) |
| Mailpit | 1.30.3 | |
| Adminer | 5.4.2 | single `.php`, OS-agnostic |
| cloudflared | 2026.6.1 | |

All checksum-locked (SHA-256/512 by source). macOS `prepare_binary` order is non-negotiable:
**de-quarantine → relink Homebrew dylibs → codesign LAST** (relinking invalidates the
signature; Apple Silicon kills unsigned binaries). The bundle counterpart
(`prepare_binary_tree`, for the bottle bundles) follows the same rule per tree:
**relink every Mach-O's non-system load command to `@loader_path` (erroring on any
dep not bundled), verify, then ad-hoc sign each Mach-O LAST.**

Standing caveat: the bottle bundles' x86_64 digests are Homebrew-published and MySQL
8.0.44's x86_64 tarball was downloaded + hashed but not run — run-verify all of them
on the next Intel smoke pass (arm64 artifacts were all extracted and RUN at pin time).
