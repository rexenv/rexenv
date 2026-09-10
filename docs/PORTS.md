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
| php-fpm pools | **9774**, **9780–9785** | TCP | `core/php.rs` — `9700 + major*10 + minor` (7.4 and 8.0–8.5, one pool per installed minor). **Not contiguous**: 7.4 → 9774 sits below the 8.x block, which is the formula being honest rather than a range being tidy. **Ten slots per major, and the eleventh is REFUSED** — `8.10` would alias `9.0` on 9790, so `port_offset` returns `None` for `minor >= 10` and an x.10 minor fails `cargo test` rather than colliding in the field. Widening the formula is not the cheap fix: adoption, the managed-port set and the orphan sweep all enumerate by CALLING `fpm_port`, so moving a port strands a running master rexenv can no longer see. Ledger #346 |
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
| PHP | **7.4.33** / 8.0.30 / **8.1.34 / 8.2.32 / 8.3.32 / 8.4.23 / 8.5.8** | **Only 8.0.30 is somebody else's build now** (static-php **"bulk"**; "common" lacks `mysqli`). **8.1-8.5 became OURS on 10 Sep 2026** — one immutable release per version (`php-8x-3` … `php-8x-7`; `-1`/`-2` are superseded, see below) — and the reason is a hole rather than an absence: the bulk builds ship `pgsql` and **no `pdo_pgsql`** while their PDO advertises `pgsql`, so a PDO connection is accepted and then stalls until PostgreSQL closes it (`SQLSTATE[08006]`, ~60s). Laravel's `pgsql` driver IS that call. Ours add the driver and the CI gate is a REAL CONNECTION — `php -m` and `PDO::getAvailableDrivers()` both reported support the upstream artifact did not have, so only a socket could tell. **Parity is gated three ways, and each layer exists because the one before it missed something** (ledger #553): MODULE names against `docs/bulk-modules-<minor>.txt`; FUNCTION names via `get_defined_functions()` against upstream's artifact of the same version — our `mbstring` and theirs were the same NAME and 16 functions apart, and every Laravel `artisan` command died on `mb_split` behind a perfect module diff; and CONFIGURE FLAGS read out of both binaries, which caught `qdbm` (a dba handler) and Redis's ZSTD/LZ4 compressors — neither a module nor a function. **The one difference that remains is deliberate**: static-php-cli's swoole PostgreSQL hook is MUTUALLY EXCLUSIVE with the real `pdo_pgsql` ("swoole-hook-pgsql provides pdo_pgsql… you must remove pdo_pgsql extension"), so upstream ships a driver that only works inside a Swoole coroutine — which is exactly why theirs advertises `pgsql` and then hangs — and rexenv ships the real one, losing `SWOOLE_HOOK_PDO_PGSQL`/`_SQLITE`. **8.0.30 stays upstream's**: it builds and passes every gate on arm64 and aborts on x86_64 inside static-php-cli's own sanity check, reproducibly and unexplained, and it is EOL — so **8.0 has no PostgreSQL driver, and `php::pdo_pgsql_supported` answers per PATCH** rather than per minor or app-wide — because rexenv also RUNS patches it did not build (the signed update manifest offers upstream's), which is how a machine on 8.3.32 was told PostgreSQL was fine and hung on its first migration (ledger #550). **`php-8x-2` exists for that**: 8.2 and 8.3 pin `.32` — the patches upstream had already moved to — so an updated machine gets our build of the same version number rather than upstream's (`docs/PLAN-postgres-sites.md`). **7.4.33 is OURS** — static-php.dev publishes no 7.4, so `rexenv/runtimes` builds it (`docs/archive/PLAN-php-74-support.md`); pinned to release `php-7.4.33-6`, immutable, so that pin can 404 but never drift. **Which 7.4 that is, is answerable without leaving the repo**: `core/binaries.rs`' `PHP_7_4_33_SOURCE_COMMIT` names the `shivammathur/php-src-backports` commit it was built from — the branch NAME cannot, being rebased rather than appended. The hash is deliberately not repeated here; it lives in the code and in `THIRD-PARTY-NOTICES.md` for the licence audience, and those two are held equal by `the_pinned_74_names_the_source_it_was_built_from`. A third copy would be a third thing to drift. **Being the builder makes rexenv the DISTRIBUTOR**, so every self-built row carries a second pinned artifact — `licenses-<arch>.tar.gz` for 7.4, `licenses-php-<version>-<arch>.tar.gz` for the 8.x release (one release, five versions, so the name carries the version — a detail that would 404 half the tree if it were assumed rather than pinned), unpacked into the same cache dir as the interpreter (`bin/php-7.4.33/licenses/`) inside the same atomic publish. Keyed on `is_self_distributed`, so any future self-built runtime inherits it (ledger #336). 7.4 and 8.0 are upstream-EOL and the UI says so (`php::eol_since`). **7.4's extension set is at parity with the 8.x rows except five, each for a reason and none of them an omission:** `random` (a PHP 8.2 CORE extension — it does not exist in 7.4), `opcache` (static-php-cli's static-opcache patch series starts at 8.0), `opentelemetry` + `protobuf` (spc guards both on PHP >= 8.0), `swoole` (modern releases dropped 7.4). **`PCRE JIT` is compiled OUT of 7.4** — it bundles PCRE2 10.35 (May 2020), too old for Apple Silicon, and Composer died on `Allocation of JIT memory failed` until `--without-pcre-jit`; regex throughput on 7.4 is therefore lower than the 8.x rows. **No Xdebug on 7.4** (its build exports no `_OnUpdateBool`, the same wall 8.0 hits). **And its libcurl uses the THREADED resolver, not c-ares** (measured 23 Aug 2026): being our own build against our own curl 8.21.0, it never received static-php.dev's `--enable-cares`, so the `core::wp_dns` loopback bug class does not exist on 7.4 and the mu-plugin no-ops there. `core::wp_dns::resolver_for` records this per minor |
| Xdebug | 3.5.3 for PHP 8.1–8.5 — **pinned PER MINOR, not app-wide** (`XdebugBottle` rows; `XDEBUG_VERSION` is the default these five reference). Xdebug's support windows close, so a minor past its window is frozen at its last release (7.4 → 3.1.6, ledger #320). **7.4 has NO row and gets no Xdebug, and the code now says which KIND of "no" that is** (ledger #378 — `XdebugStatus::CannotLoadExtensions`, distinct from a minor whose bottle is merely unpinned): our own build exports ~22,400 symbols but not `_OnUpdateBool`, so no external `.so` can dlopen into it — measured 14 Aug 2026, the same wall 8.0 hits | bottle BUNDLE (one part) from `shivammathur/extensions` ghcr (the setup-php tap; arm64_sonoma + sonoma digests, all 10 downloaded + load-tested at pin time). Loads into the EXISTING static php-fpm (`-d zend_extension`) — the per-site toggle's debug pools; every spawn is gated on a real load probe. **PHP 8.0 excluded** (its static build exports no Zend symbols — dlopen impossible; the legacy `php-debug` self-build wiring in `core/binaries.rs` + `docs/xdebug-debug-build.md` stays as the fallback recipe, still unresolvable). **The fallback recipe carries its own pin, `PHP_DEBUG_XDEBUG_VERSION = 3.4.5`** against `PHP_DEBUG_VERSION = 8.3.31` — added here 21 Aug 2026, when it turned out to be the only pinned constant in `core/binaries.rs` with no row in this file. It is deliberately NOT 3.5.3: the debug build is a recorded recipe, not a shipped artifact, and bumping it would imply a rebuild nobody has done |
| Nginx | **1.30.4 — OURS** | `rexenv/runtimes` release `nginx-1.30.4-2`, immutable. Built for `minos 12.0` on BOTH slices (asserted per artifact) because upstream's darwin builds went to 26.0 — see the floor note below. PCRE2 compiled IN from source, no ssl module (the edge owns TLS), no gzip ⇒ the dylib closure is `libSystem` alone, so nothing to relink. Being the builder makes rexenv the DISTRIBUTOR: `licenses-<arch>.tar.gz` rides the binary from the same release, and a resolve REFUSES without it |
| MySQL | 8.4.6 (default) / 8.0.44 | dir tree, Oracle-signed (no re-sign), CDN URL + browser UA. **Per-engine version switch**: each SERIES keeps its own datadir (default series on the legacy `mysql/data`; others under `mysql/<series>/data`) — never an in-place up/downgrade. 8.0.44 hashed from real downloads (both arches), arm64 run-verified |
| WP-CLI | 2.12.0 | `.phar`, `resolve_file`, no chmod/codesign |
| Composer | 2.10.2 | `.phar`, `resolve_file` — ALWAYS run via the SITE's bundled PHP (add-from-Git composer step; platform checks match the PHP the plugin runs on; a system composer is never executed — non-phar wrappers exist, e.g. Herd's). Sha verified against getcomposer.org's published `.sha256sum`, run-tested on static PHP 8.3.31 at pin time |
| FrankenPHP | 1.12.4 | embeds its OWN PHP (not the pools) |
| PostgreSQL | 18.6.0 (default) / 17.11.0 / 16.15.0 | theseus-rs portable, TCP-only; project-published `.sha256` pins, each cross-checked against a fresh download. **Re-pinned 30 Aug 2026 for the FLOOR, not the patch level**: the June builds carried `minos 26.0` (macOS-26 runners), the 15 Aug ones carry `minos 15.0` — measured across every Mach-O in all six tarballs. PG major datadirs are mutually incompatible — the per-series datadir rule is load-bearing (`postgres/<major>/data`) |
| Redis | 8.8.0 | **bottle BUNDLE** (`resolve_bundle`): Homebrew redis + openssl@3 3.6.3 bottles (arm64_sonoma / sonoma), merged + relinked to `@loader_path` + re-signed by `prepare_binary_tree`. ghcr blobs are content-addressed — the URL embeds the pinned digest, so pins can 404 (formula GC) but never drift |
| MariaDB | 12.3.2 (default) / 11.4.12 LTS (`mariadb@11.4` bottle, identical layout+closure) | bottle BUNDLE: mariadb (server + client + dump + bootstrap SQL/errmsg/charsets ONLY — plugins/scripts excluded) + openssl@3 3.6.3 + pcre2 10.47. groonga/lz4/lzo/xz/zstd are plugin-only deps, not bundled. Init = `mariadbd --bootstrap` fed the bundled SQL over stdin (`core/mariadb.rs`) |
| Apache httpd | 2.4.68 | bottle BUNDLE: httpd (`bin/httpd` + ONLY the 10 modules the generated conf loads + `.bottle/etc/httpd/mime.types`) + apr 1.7.6 + apr-util 1.6.3 + pcre2 10.47. mod_ssl/mod_http2/mod_brotli excluded ⇒ openssl/nghttp2/brotli never bundled. Per-site loopback override backend; `.php` → the site's shared php-fpm pool via mod_proxy_fcgi (`core/apache.rs`) |
| Mailpit | 1.30.3 | |
| Adminer | 5.4.2 | single `.php`, OS-agnostic |
| cloudflared | 2026.6.1 | |

### Measured macOS floors (`minos`, per binary)

Measured 15 Aug 2026 on the real cache (`otool -l | grep minos`) — recorded because
this drifted silently: the app claimed macOS 11 while shipping binaries below, and a
number nobody records drifts again. **The app's stated floor
(`tauri.conf.json` `minimumSystemVersion`, INSTALL.md) must equal the MAX across the
binaries the default stack requires** — today that is cloudflared at 15.0.
**PostgreSQL sat ABOVE that floor at 26.0 from 15 Aug to 30 Aug 2026** and the stated
floor did not move with it, because the rule was maintained by hand and nothing was
comparing. **It is now asserted rather than remembered** (30 Aug 2026,
`examples/macos_floor_check.rs`, network tier, ledger #433): the check fetches every
`binaries::DEFAULT_STACK` artifact for BOTH arches, verifies each against its pin before
believing anything it reads, and fails unless `max(minos)` per arch equals
`tauri.conf.json`'s `minimumSystemVersion` — in either direction, since a floor set too
high refuses users the app would have run for. It also reports any binary whose two slices
declare DIFFERENT floors, which is the case this table's arch caveat exists for and the one
a host-arch-only measurement can never see. The table below stays as the human-readable
record; the check is what keeps it from being the only one.
Re-measure and update this table whenever a pin changes.

**Re-measured 15 Aug 2026 after PHP 7.4.33 landed** (every Mach-O in the cache, not
just the headline binary per entry). 7.4 is `minos 12.0`, so **it does not move the
floor** — which is the answer the rule exists to produce rather than assume. The
sweep also convicted this table's own PHP row: it said "all minors", and 8.0.30 is
**14.0**, not 12.0. That row had been wrong since the table was written; nothing
depended on it because the floor is set by nginx either way, which is exactly how a
wrong number survives — it is only load-bearing on the day something else moves.

| Binary | minos |
|---|---|
| Caddy 2.11.4, Mailpit 1.30.3, FrankenPHP 1.12.4, **PHP 7.4.33 / 8.1.34 / 8.2.32 / 8.3.32 / 8.4.23 / 8.5.8** | 12.0 |
| **PHP 8.0.30** (cli + fpm — the one PHP row that is NOT 12.0), MySQL 8.0.44 / 8.4.6, MariaDB 11.4.12 / 12.3.2, Redis 8.8.0, Apache httpd 2.4.68, Xdebug 3.5.3 bottles | 14.0 |
| **cloudflared 2026.6.1** | **15.0** — the default stack's floor, and since 31 Aug 2026 the ONLY binary at it. **nginx 1.30.4 (ours) is 12.0 on both slices**, where the third-party 1.30.3 was 15.0 arm64 / **26.0 x86_64** |
| **PostgreSQL 16.15.0 / 17.11.0 / 18.6.0** | **15.0** — measured 30 Aug 2026 on EVERY executable and dylib in all six shipped tarballs (`vtool -show-build`), so PostgreSQL no longer sets the app floor. The previous pins (16.14.0 / 17.10.0 / 18.4.0, June builds off macOS-26 runners) carried **26.0**, presumed-but-untestable death below macOS 26; re-pinning answered that question instead of waiting for a VM to ask it |

**Arch caveat: every number above is the arm64 slice — EXCEPT PostgreSQL.** The cache
holds only what this machine downloaded, so the rest of the x86_64 artifacts' `minos`
has never been measured — the same standing gap as the un-run Intel digests at the foot
of this file. An Intel pass should re-run the sweep, not just the smoke test.
PostgreSQL's row covers both slices because the 30 Aug re-pin downloaded them: the
x86_64 tarballs were fetched and swept here, and the old x86_64 18.4.0 measured **26.0**
just like its arm64 twin, so the two slices moved together in both directions.

~~jirutka publishes NO darwin nginx below minos 14 (checked 1.24.0→1.31.3, 15 Aug
2026: only the stale 1.24.0/1.26.1/1.26.2 are 14.0; everything current is 15.0)~~ —
**both halves FALSE, and measured on one arch only** (30 Aug 2026, `macos_floor_check`).
The 15 Aug sweep read this machine's cache, which only ever holds arm64. Across both
slices now: **x86_64 is `minos 26.0` for every version from 1.26.3 up**, including the
pinned 1.30.3, whose arm64 slice is 15.0 — so the stated floor is a fiction for Intel
users today. 1.28.3 / 1.30.4 / 1.31.4 are 26.0 on **arm64 too**, so the next routine bump
takes Apple Silicon with it. Only ≤1.25.5 x86_64 is still 12.0. Lowering the floor — or
now merely KEEPING it — points at a self-built nginx; tracked as a 🔴 row in
`docs/TODO.md`.

**What `minos` does and does not prove (measured 15 Aug 2026).** On THIS machine
(macOS 26), dyld enforces minos for neither main executables nor dylibs: a copy of
postgres patched to `minos 99.0` (binary AND libssl, re-signed) runs clean. Yet the
hard refusal is real on older hosts — deterministic dyld crashes of minos-15
binaries on macOS 14 are documented in the wild — so enforcement is a property of
the HOST's dyld, and a machine on the newest macOS can never observe it. The floor
above therefore rests on measured metadata + the documented enforcement class, not
on a refusal reproduced here; confirming what actually happens on macOS 14/15 needs
an older-macOS VM (PUBLISH-TESTING's clean-VM shape).

**PostgreSQL's 26.0 pins are gone, and the VM question with them** (30 Aug 2026). The
open item was "does dyld actually refuse these below macOS 26, and do we re-pin?" — two
questions where the second dissolves the first. Upstream's 15 Aug builds are `minos 15.0`,
so re-pinning removes the presumed-dead artifact rather than testing it: an untestable
prediction stops mattering when the thing it predicts about is no longer shipped. The
prediction itself stays UNSETTLED and that is fine — it now applies to nothing rexenv
ships, and the `core::macho` diagnosis (#383) still fires for anything that regresses.

**If it IS real, the user is told which fact explains it** (23 Aug 2026, ledger #383).
A readiness timeout now reads the binary's own `minos` (`core::macho`, cross-checked
against `otool -l` on the real cache) and, when it outranks the running macOS, says so
in the top-level line instead of only pointing at a log. Diagnosis, never a gate:
refusing to spawn on `minos` would block builds that may run perfectly, on a prediction
this tree cannot test. **The table above is therefore no longer the only record** — the
number that matters at runtime is read from the artifact, which is what stops the
PHP-row mistake from recurring where it counts.

All checksum-locked (SHA-256/512 by source). macOS `prepare_binary` order is non-negotiable:
**de-quarantine → relink Homebrew dylibs → codesign LAST** (relinking invalidates the
signature; Apple Silicon kills unsigned binaries). The bundle counterpart
(`prepare_binary_tree`, for the bottle bundles) follows the same rule per tree:
**relink every Mach-O's non-system load command to `@loader_path` (erroring on any
dep not bundled), verify, then ad-hoc sign each Mach-O LAST.**

Standing caveat: the bottle bundles' x86_64 digests are Homebrew-published and MySQL
8.0.44's x86_64 tarball was downloaded + hashed but not run — run-verify all of them
on the next Intel smoke pass (arm64 artifacts were all extracted and RUN at pin time).
