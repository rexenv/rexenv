# Xdebug-enabled PHP "debug" build — recipe + hosting/pinning (§11.2)

The stock static-php.dev **bulk** builds ship **no Xdebug**, and a *static* PHP
cannot `dlopen` an external `xdebug.so` (no matching loadable artifact is published —
see TASKS-PHASE3.md §8.1). So the per-site Xdebug toggle (§8.2) needs a **separate
custom static-php compile** of the same PHP minor with **Xdebug compiled in**.

This file is the reproducible recipe for producing that artifact, hosting it, and
pinning it so `BinaryProvider` can fetch it on demand like every other binary.

> Status: the in-repo wiring is done (`core::binaries` `php-debug` / `php-fpm-debug`
> variants, gated on a pinned checksum). The variant stays **unresolvable** until a
> maintainer runs this recipe, uploads the artifacts, and fills the four
> `PHP_DEBUG_*_SHA256` consts. Until then §8.2 remains blocked.

## What to build

- **PHP minor:** `PHP_DEBUG_VERSION` in `src-tauri/src/core/binaries.rs` (currently `8.3.31`).
- **Xdebug:** `PHP_DEBUG_XDEBUG_VERSION` (currently `3.4.5`).
- **Targets:** `php` (cli) **and** `php-fpm`, **NTS**, static, for **both** macOS arches
  (`aarch64` + `x86_64`) — four artifacts total.
- **Extension parity:** match the bulk build's extension set so debug sites behave
  identically (only Xdebug differs). The bulk `php -m` set we target is:

  ```
  apcu bcmath bz2 calendar ctype curl dba dom event exif fileinfo filter ftp gd gmp
  iconv imagick imap intl mbstring mysqli opcache openssl opentelemetry pcntl pdo
  pdo_mysql pgsql posix protobuf readline redis session shmop simplexml soap sockets
  sodium sqlite3 swoole sysvmsg sysvsem sysvshm tokenizer xml xmlreader xmlwriter xsl
  zip zlib
  ```

  (core/always-on modules — Core, standard, SPL, date, hash, json, libxml, pcre,
  Phar, random, Reflection, mysqlnd, Zend OPcache — are implicit.)

## Build with static-php-cli (`spc`)

`spc` and a full toolchain (Xcode CLT/clang, make, autoconf) are required. Run once
**per arch** on a matching macOS host (or cross-build per spc docs).

```sh
# 1) Get the spc builder (per arch).
curl -fSL -o spc https://dl.static-php.dev/static-php-cli/spc-bin/nightly/spc-macos-aarch64
chmod +x spc

# 2) Extension list = bulk parity set + xdebug. Verify slugs against `./spc list-ext`.
EXTS="apcu,bcmath,bz2,calendar,ctype,curl,dba,dom,event,exif,fileinfo,filter,ftp,gd,gmp,iconv,imagick,imap,intl,mbstring,mysqli,opcache,openssl,opentelemetry,pcntl,pdo_mysql,pgsql,posix,protobuf,readline,redis,session,shmop,simplexml,soap,sockets,sodium,sqlite3,swoole,sysvmsg,sysvsem,sysvshm,tokenizer,xml,xmlreader,xmlwriter,xsl,zip,zlib,xdebug"

# 3) Download sources (pin the PHP minor; prefer pre-built deps to speed it up).
./spc download --with-php=8.3.31 --for-extensions="$EXTS" --prefer-pre-built

# 4) Build BOTH the cli and fpm SAPIs, statically, with the extensions above.
./spc build "$EXTS" --build-cli --build-fpm

# Artifacts land in ./buildroot/bin/php and ./buildroot/bin/php-fpm
./buildroot/bin/php -m | grep -i xdebug   # sanity: Xdebug present
```

## Package, host, pin

Package each SAPI as a single-member `.tar.gz` whose member name matches the
manifest (`php` for cli, `php-fpm` for fpm) — same shape as the bulk artifacts.

```sh
# Per arch, per SAPI:
tar -C buildroot/bin -czf php-8.3.31-cli-xdebug-macos-aarch64.tar.gz php
tar -C buildroot/bin -czf php-8.3.31-fpm-xdebug-macos-aarch64.tar.gz php-fpm
shasum -a 256 php-8.3.31-*-xdebug-macos-*.tar.gz   # → the four SHA-256s
```

1. Upload the four `.tar.gz` to the host behind `PHP_DEBUG_BASE_URL`.
   **The host itself is an OPEN DECISION (review item B33, tracked in
   `docs/TODO.md`)** — a `dl.` host is wired in `core/binaries.rs:40` but was
   never ratified vs the canonical project domain. Note: whichever host serves
   these, rexenv becomes a DISTRIBUTOR of PHP at that moment — ship the PHP
   licence + Xdebug licence texts alongside the artifacts.
   (`src-tauri/src/core/binaries.rs`), keeping the exact file names
   `php-{ver}-{cli|fpm}-xdebug-macos-{aarch64|x86_64}.tar.gz`.
2. Fill the four consts `PHP_DEBUG_{CLI,FPM}_MAC_{ARM64,AMD64}_SHA256` with the
   `shasum` output. The moment they're non-empty, `manifest("php-debug", …)` and
   `manifest("php-fpm-debug", …)` resolve and `prepare_binary` (de-quarantine →
   relink → ad-hoc codesign) applies as usual.
3. Unblock §8.2: route the debug pool's fpm to the `php-fpm-debug` binary with
   `zend_extension=xdebug` + `xdebug.mode=debug,develop`.

## Verify after pinning

`cargo test --lib binaries` flips `php_debug_variant_is_wired_but_unresolvable_until_hosted`
expectations — update that test to assert the variant now **resolves** with a 64-char
SHA-256, mirroring `manifest_pins_php_cli_and_fpm`. Then a live check should show
`php-fpm-debug` downloading, verifying, and `phpinfo()` reporting Xdebug.
