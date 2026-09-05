# Xdebug-enabled PHP "debug" build — the FALLBACK recipe, for PHP 8.0 only

> ## Read this first — this file described a path that is no longer the live one
>
> **The per-site Xdebug toggle SHIPS, and it does not use this recipe.** Xdebug
> works on PHP **8.1–8.5** via per-minor `xdebug.so` bottles from the
> `shivammathur/extensions` tap, loaded into the EXISTING static php-fpm with
> `-d zend_extension` (`docs/PORTS.md`, the Xdebug row). The premise this file
> opened with — "a *static* PHP cannot `dlopen` an external `xdebug.so`" — is
> false as a general claim: it is true of **PHP 8.0's** build specifically, which
> exports 98 symbols and no `_OnUpdateBool`, and false of 8.1+, which export
> ~39,000 (measured on the real cache, 14 Aug 2026).
>
> So what remains here is a **fallback recipe for the ONE minor still excluded,
> 8.0** — not the way §8.2 works. Anyone who picked this up believing otherwise
> would have built an artifact rexenv has no use for.

This file is the reproducible recipe for producing an Xdebug-compiled-in build,
hosting it, and pinning it so `BinaryProvider` can fetch it on demand like every
other binary.

> Status: the in-repo wiring is done (`core::binaries` `php-debug` /
> `php-fpm-debug` variants, gated on a pinned checksum) and stays **unresolvable**
> until someone runs this recipe and fills the four `PHP_DEBUG_*_SHA256` consts.
> §8.2 itself is NOT blocked on that — only 8.0's row is. PHP 8.0 is upstream-EOL,
> so leaving it excluded is an accepted outcome (`docs/TODO.md`, "Blocked on
> external work").

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

1. Upload the four `.tar.gz` as a release in **`rexenv/runtimes`** — the public
   build/host repo, created 14 Aug 2026. **This settles B33**, which had been open
   since the Xdebug work: the host is GitHub Releases, and `dl.rexenv.dev` is not
   used.
   ✓ **The code caught up 23 Aug 2026.** It had not, for two days: `binaries.rs`
   went on building every php-debug URL from a `PHP_DEBUG_BASE_URL` naming
   `dl.rexenv.dev` — the host this step says is not used — and the only reason
   nothing broke is that the artifacts do not exist. That scheduled the
   contradiction to be found at upload time, by whoever is reading this line.
   The host is no longer a string that can be wrong: php-debug URLs are built
   from `RUNTIMES_RELEASE_BASE`, the same const the 7.4 artifacts use.
   The reasons are in `docs/archive/PLAN-php-74-support.md` §6, and the one that
   decided it is that a release there is IMMUTABLE and its tag is never reused, so
   a pinned URL can 404 but can never resolve to different bytes — which is
   exactly what static-php.dev and FrankenPHP cannot promise.
   Prefer adding a job to that repo's workflow over building by hand: what
   produced an artifact should be a public log with an attestation, not a laptop.
   Keep the exact file names `php-{ver}-{cli|fpm}-xdebug-macos-{aarch64|x86_64}.tar.gz`.
   rexenv becomes a DISTRIBUTOR of PHP at that moment: ship the PHP licence, the
   Xdebug licence, and the statically linked deps' licences alongside (the
   `rexenv/runtimes` build script already collects the last set automatically).
2. Fill **two** things in `core/binaries.rs`, and note that filling either alone
   leaves php-debug unresolvable **by design**:
   - `PHP_DEBUG_TAG` — the FULL release tag you just created
     (`php-8.3.31-xdebug-1`; a rebuild is `-2`, never a re-upload), like
     `php_self_hosted_tag` does for 7.4. Not a stable base.
   - the four `PHP_DEBUG_{CLI,FPM}_MAC_{ARM64,AMD64}_SHA256` consts, from the
     `shasum` output above.

   The natural order here is upload → hash → pin, so the digests get filled first
   and the tag is the edit that is easy to forget — which is why
   `php_debug_spec` refuses on a missing tag before it looks at a digest, and
   `a_debug_build_with_digests_but_no_release_tag_does_not_resolve` holds it.
   With both filled, `manifest("php-debug", …)` and `manifest("php-fpm-debug", …)`
   resolve and `prepare_binary` (de-quarantine → relink → ad-hoc codesign)
   applies as usual.
3. Unblock §8.2: route the debug pool's fpm to the `php-fpm-debug` binary with
   `zend_extension=xdebug` + `xdebug.mode=debug,develop`.

## Verify after pinning

`cargo test --lib binaries` flips two tests, and both are deliberate tripwires
rather than chores: `php_debug_variant_is_wired_but_unresolvable_until_hosted`
(update it to assert the variant now **resolves** with a 64-char SHA-256,
mirroring `manifest_pins_php_cli_and_fpm`) and
`a_debug_build_with_digests_but_no_release_tag_does_not_resolve`, whose first
line asserts `PHP_DEBUG_TAG` is still empty and fails with "the tag is filled —
update this test with the pin". Then a live check should show
`php-fpm-debug` downloading, verifying, and `phpinfo()` reporting Xdebug.
