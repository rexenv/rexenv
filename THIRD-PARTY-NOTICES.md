# Third-party notices — what ships in the rexenv app

This file covers the software DISTRIBUTED with rexenv: the app bundle
(statically linked Rust crates, the bundled SQLite, the compiled frontend with
its npm dependencies and fonts, and **two vendored PHP packages** — see below)
and the `rex` CLI sidecar. For MOST of the server binaries rexenv downloads onto
your machine at runtime (MySQL, MariaDB, PostgreSQL, Redis, nginx, Caddy,
FrankenPHP, Apache httpd, Mailpit, Adminer, cloudflared, WP-CLI, Composer,
Xdebug, and PHP **8.0–8.5**) rexenv is not the distributor: those are fetched
from their own distributors, checksum-pinned, and carry their own licences; the
pinned versions and sources are listed in `docs/PORTS.md`.

**Windows (port in progress, 13 Sep 2026 — no Windows build has shipped).** The Windows
build downloads from distributors too, never from us: PHP from the PHP project's own
Windows builds (`downloads.php.net/~windows`), nginx from nginx.org's Windows zip, MySQL
from Oracle's `winx64` zips, PostgreSQL from theseus-rs's `x86_64-pc-windows-msvc`
builds, and Caddy, Mailpit and cloudflared from their GitHub releases. So the three
"rexenv's own build" sections below describe **macOS artifacts only** — none of them is
downloaded on Windows. Versions, sources and digests: `docs/PORTS.md`, Windows table.
The Windows app binary links a different Rust graph (the `windows-*` and `webview2-com*`
families in place of `objc2`), so this file needs a Windows crate table before any
Windows release — `docs/TODO.md`, "Third-party notices".

**Two exceptions, and each is a real change to the sentence above.** They are
listed separately because they arrived for different reasons and carry different
obligations.

**1. The vendored `wp dist-archive` package tree (4 Aug 2026).** The command is
not part of WP-CLI, so rexenv carries it rather than resolving it from a machine
it does not control — the tree is compiled into the app binary and written out on
first use. It is listed in its own section below.

**2. PHP 7.4.33 (15 Aug 2026), PHP 8.1-8.5 (10 Sep 2026) and nginx 1.30.4
(31 Aug 2026) — rexenv builds them, hosts them, and is therefore their
distributor.** static-php.dev publishes no PHP 7.4 and never did, so rexenv
builds it (`rexenv/runtimes`, static-php-cli, from
`shivammathur/php-src-backports`) and hosts the four artifacts as immutable
GitHub Release assets that the app then downloads. **8.1 through 8.5 joined for
a different reason: upstream's builds ship `pgsql` and no `pdo_pgsql`**, while
their PDO advertises `pgsql` — so a PDO connection is accepted and then stalls
until PostgreSQL closes it, which is every Laravel `pgsql` app. Ours add the
driver and prove it in CI against a real cluster; 8.0 remains upstream's, since
it aborts on x86_64 in a way nobody has explained and is EOL. nginx joined for
yet another reason — upstream's darwin builds moved to a macOS 26
deployment target, which would have locked out every Intel user below macOS 26 —
and it carries the same obligations. Every other row in
`docs/PORTS.md` names somebody else's build; this one names ours. The PHP
License 3.01 §2/§6 obligation attaches to us for it, and so do the licences of
every dependency statically linked into the binary — they travel inside the
Mach-O whether or not anyone names them. See **"PHP 7.4.33 — rexenv's own
build"** below for what is shipped and where — including onto your machine,
beside the interpreter.

rexenv itself is licensed under the Apache License 2.0 (see `LICENSE`).

Where a dependency is dual- or multi-licensed (e.g. "MIT OR Apache-2.0"),
rexenv uses it under the first permissive option compatible with this
distribution. Generated from the real dependency graphs on 2026-07-28 and
re-verified on 2026-08-03, 2026-08-04 and 2026-08-05. The Rust closure is
unchanged at 389 crates.
**12 Sep 2026: +2 Rust crates for the Windows port** (`zip` 8.6.0 MIT, `typed-path`
0.12.3 MIT OR Apache-2.0 — zip extraction for the Windows artifacts). Added as rows,
NOT regenerated: this header already read 393 against the 389 above, so the two-way
reconcile below is due before the next release, not assumed done.
**Measured 13 Sep 2026, both directions, with the command below:** every one of the table's
395 rows is in the graph (table − graph = 0), but the macOS arm64 graph links **409** crates
— **14 are missing**: `mysql_async` 0.37.0 and `mysql_common` 0.37.3 with what they pull in
(`allocator-api2`, `btoi`, `bytemuck`, `chacha20`, `crossbeam-queue`,
`keyed_priority_queue`, `lru`, `rand` / `rand_core` 0.10, `saturating`, `sha1`,
`twox-hash`); the Intel graph adds `cpufeatures` (15). The Windows graph (410) is 50 crates
beyond the table and 35 short of it. Tracked in `docs/TODO.md` until the rows land.

**The npm table was INCOMPLETE from the day it was generated, and the 5 Aug
pass found it: 112 rows against a 127-package production closure.** Fifteen
packages were missing, all MIT, several of them genuinely bundled — `scheduler`
(React's) is in `dist/assets/*.js`, and so are `loose-envify`, `js-tokens` and
`util-deprecate`. They are added above.

The earlier passes did not catch it because they only checked one direction:
every documented row was verified to exist in the graph, which was true, and
nothing asked whether the graph held rows the table did not. "112/112" was a
real comparison of the wrong set. A reconciliation has to run BOTH ways —
`graph − table` is the direction that finds an omission, and it is the direction
that matters, because a notice you never wrote is exactly the one nobody misses.

Two npm rows were repaired in the 4 Aug pass: `@tauri-apps/api` and
`@tauri-apps/plugin-dialog` had name, version and licence collapsed into one
cell by the original generation. The packages and their licences were right; the
table rendered them wrong. Regenerate before each release with:

```sh
# Rust (the app + statically linked deps, macOS graph):
cd src-tauri && cargo metadata --format-version 1 --filter-platform aarch64-apple-darwin
# npm (production closure that Vite bundles):
pnpm list --prod --depth Infinity --json
```

## Fonts (bundled woff2, via @fontsource)

| Font | Copyright | Licence |
|---|---|---|
| Inter | Copyright 2016 The Inter Project Authors (https://github.com/rsms/inter) | SIL OFL 1.1 |
| Space Grotesk | Copyright 2020 The Space Grotesk Project Authors (https://github.com/floriankarsten/space-grotesk) | SIL OFL 1.1 |
| JetBrains Mono | Copyright 2020 The JetBrains Mono Project Authors (https://github.com/JetBrains/JetBrainsMono) | SIL OFL 1.1 |

The full SIL Open Font License 1.1 text is in the licence-texts section below.

## Vendored PHP (compiled into the app binary; 2 packages)

`wp dist-archive` is not core WP-CLI — it is a separate composer package. rexenv
vendors it rather than installing it at runtime, so that what the app runs is
what the app shipped (`src-tauri/src/core/wp_packages.rs`). Both packages are
MIT; the full text is in the licence-texts section below.

| Package | Version | Source | Licence |
|---|---|---|---|
| wp-cli/dist-archive-command | 3.1.0 | https://github.com/wp-cli/dist-archive-command (dist ref `e91730cddd4b`) | MIT |
| inmarelibero/gitignore-checker | 1.0.4 | https://github.com/inmarelibero/gitignore-checker (dist ref `57cdaa05ceaa`) | MIT |

The tree is committed at `src-tauri/resources/wp-dist-archive/`, and
`composer.lock` beside it is the authoritative provenance record (exact
versions, dist references, content hash). Regenerate — and bump — with:

```sh
./scripts/build-wp-dist-archive.sh                    # at the pinned version
DIST_ARCHIVE_VERSION=3.2.0 ./scripts/build-wp-dist-archive.sh   # to bump
```

The versions above, `core::wp_packages::DIST_ARCHIVE_VERSION`, and the vendored
tree are pinned together: `the_vendored_tree_is_the_version_we_pinned` reads the
version out of the tree's own `composer/installed.json` and fails the build if
they disagree, so these rows cannot silently describe a different version than
the one that ships. Note that dist-archive's version is coupled to the pinned
WP-CLI: v3.1.0 needs `wp-cli/wp-cli ^2` (our 2.12.0), while v3.2.0 requires
`^2.13` and will not resolve until WP-CLI is bumped first.

## nginx 1.30.4 — rexenv's own build (downloaded at runtime, distributed BY US)

The SECOND artifact rexenv builds and hosts, since 31 Aug 2026. Same standing as
PHP 7.4 below, and it arrived for a concrete reason rather than a preference:
upstream's darwin binaries declare `minos 26.0` on x86_64 (and, from 1.28.3, on
arm64), eleven majors above the floor this app states, so an Intel user below
macOS 26 was installing an app whose shared web server may refuse to load.

| | |
|---|---|
| Artifacts | `nginx-1.30.4-macos-{aarch64,x86_64}` (raw per-arch binaries) |
| Built by | `rexenv/runtimes` (public), `scripts/build-nginx.sh`, GitHub Actions |
| Source | nginx 1.30.4 (nginx.org) + PCRE2 10.47 — both tarballs mirrored as assets in the same release, so the build is reproducible from URLs alone |
| Release | `nginx-1.30.4-2` — immutable, never re-uploaded; a rebuild is the next build number |
| Pinned in | `core/binaries.rs` (`NGINX_1_30_4_*_SHA256`, `NGINX_VERSION`) |
| Licence | **BSD-2-Clause** (`licenses/nginx.LICENSE`) |

**PCRE2 is compiled INTO the binary**, so its licence travels with it:
`licenses-<arch>.tar.gz` carries `pcre2.LICENCE.md` and `pcre2.COPYING`
(**BSD-3-Clause**) beside nginx's own. There is nothing else in the closure —
this build asks for no ssl module and no gzip, so neither OpenSSL nor zlib is
present, and `otool -L` shows `/usr/lib/libSystem.B.dylib` alone. That is
asserted in the build, not assumed.

**The licence archive is load-bearing, not paperwork.** `core::binaries`
refuses to resolve an artifact served from rexenv's own infrastructure unless a
licence archive is pinned beside it — build 1 of this release shipped without
one and the app would not load it, which is how the rule proved itself.

## PHP 8.1-8.5 — rexenv's own builds (downloaded at runtime, distributed BY US)

Same standing as the 7.4 row below, for a different reason: these versions ARE
published upstream, and the upstream build has a hole. `pdo_pgsql` is absent
while `PDO::getAvailableDrivers()` lists `pgsql`, so a Laravel app on PostgreSQL
connects to nothing and stalls until the server times it out.

| | |
|---|---|
| Artifacts | `php-{8.1.34,8.2.32,8.3.32,8.4.23,8.5.8}-{cli,fpm}-macos-{aarch64,x86_64}.tar.gz` |
| Built by | `rexenv/runtimes` (public), static-php-cli 2.8.5, GitHub Actions |
| Source | php.net release tarballs, fetched and pinned by static-php-cli |
| Release | One per version — `php-8x-3` (8.1.34), `php-8x-4` (8.2.32), `php-8x-5` (8.3.32), `php-8x-6` (8.4.23), `php-8x-7` (8.5.8) — immutable, never re-uploaded; a rebuild is the next build number. `php-8x-1` and `php-8x-2` are superseded and still published: those builds lacked mbstring's regex half and gd's avif, which a module list cannot see |
| Pinned in | `core/binaries.rs` (`PHP_8_*_SHA256`, `php_self_hosted_tag`) |
| Licence | **PHP License 3.01** (`licenses/PHP-3.01.txt`) |

Four build gates decide whether these ship, and each exists because the one
before it missed something:

1. **module parity** with the upstream build each replaces, per minor, from a
   list generated off the shipped artifact;
2. **function parity** — `get_defined_functions()` diffed against upstream's
   build of the same version. A module list is a list of NAMES: our `mbstring`
   and theirs were the same name and 16 functions apart, and every Laravel
   `artisan` command died on `mb_split` behind a perfect module diff;
3. **configure-flag parity** — read out of both binaries' own `Configure
   Command`. It found `qdbm` (a dba handler) and Redis's ZSTD/LZ4 compressors,
   neither of which is a module name or a function name;
4. a **real PostgreSQL connection through PDO**, because `php -m` and
   `PDO::getAvailableDrivers()` both reported PostgreSQL support in the artifact
   that had none.

**Why upstream's PDO says `pgsql` and then hangs**, since it is the reason this
build exists: static-php-cli offers swoole's coroutine HOOK for PostgreSQL, and
it is mutually exclusive with the real extension — *"swoole-hook-pgsql provides
pdo_pgsql, if you enable pgsql hook for swoole, you must remove pdo_pgsql
extension"*. Upstream takes the hook, which only works inside a Swoole
coroutine; ordinary PHP connects to nothing and waits. rexenv takes the real
extension and gives up two coroutine constants (`SWOOLE_HOOK_PDO_PGSQL`,
`SWOOLE_HOOK_PDO_SQLITE`) — the only difference between these builds and
upstream's, and a deliberate one.

**PHP 8.0.30 is NOT ours** and still comes from static-php.dev, so it has no
working `pdo_pgsql`. It builds and passes every gate on arm64 and aborts on
x86_64 inside static-php-cli's own sanity check, reproducibly and unexplained;
it has been end-of-life since Nov 2023. Statically linked dependencies' licences
travel the same way as for 7.4 below, as `licenses-php-<version>-<arch>.tar.gz`
in the same release.

## PHP 7.4.33 — rexenv's own build (downloaded at runtime, distributed BY US)

Not compiled into the app: downloaded on demand like every other server binary.
It is in this file anyway because **the distributor is rexenv**, which is not
true of any other row in `docs/PORTS.md`.

| | |
|---|---|
| Artifacts | `php-7.4.33-{cli,fpm}-macos-{aarch64,x86_64}.tar.gz` |
| Built by | `rexenv/runtimes` (public), static-php-cli, GitHub Actions |
| Source | `shivammathur/php-src-backports` @ `5a576d8eb53e` — mirrored as an asset in the same release, so the build is reproducible from URLs alone rather than from a branch that is rebased, not appended |
| Release | `php-7.4.33-6` — immutable, never re-uploaded; a rebuild is the next build number |
| Pinned in | `core/binaries.rs` (`PHP_7_4_33_*_SHA256`, `php_self_hosted_tag`) |
| Licence | **PHP License 3.01** (`licenses/PHP-3.01.txt`) |

**Statically linked dependencies travel inside the binary, so their licences
travel with it.** They are collected at build time and published beside every
artifact as `licenses-<arch>.tar.gz`, which carries `PHP-3.01.txt`,
`php-src.LICENSE`, and the licence text of each linked dependency (curl,
freetype, libedit, libjpeg, libjxl, libtiff, libxml2, libxslt, libzip,
imagemagick, imap, postgresql, ext-zip). A dependency with no findable licence
**fails the build** in `rexenv/runtimes` rather than warning — the libxml2 case,
whose file is named `Copyright`, is why: a warning inside a green build is one
nobody reads.

**They also ship onto your machine, beside the bytes** (16 Aug 2026). rexenv
downloads `licenses-<arch>.tar.gz` as part of resolving 7.4 and unpacks it into
the same cache directory as the interpreter, inside the same atomic publish — so
a published 7.4 either carries its licence texts or does not exist:

```
~/Library/Application Support/dev.rexenv.rexenv/bin/php-7.4.33/
├── php
└── licenses/          PHP-3.01.txt, php-src.LICENSE, curl.COPYING, …
```

(and the same beside `php-fpm-7.4.33/php-fpm`, which is a separate artifact with
its own resolve). A fetch failure fails the resolve rather than falling back to
shipping the interpreter alone — the fallback would be the exact outcome this
prevents. A cache from before this existed is treated as stale and repaired on
next resolve, at the cost of one re-download.

**Why it was not left at "reproduced in this file".** That reading is
defensible under §2's "documentation and/or other materials provided with the
distribution" — and it is an ARGUMENT. A licence obligation is the last place to
hold a position that needs defending, and a second manifest arm is a small price
for not having to make one.

**Naming.** PHP License 3.01 §4/§6 restrict use of the name "PHP" in derived
products. rexenv's build is unmodified upstream PHP (plus the backport branch's
own patches) rather than a derived product, is not named "PHP-something", and the
build repo is `rexenv/runtimes` rather than `rexenv/php-builds` — a neutral name
chosen for exactly this reason.

## SQLite

`rusqlite` is built with the `bundled` feature, so SQLite itself is compiled
into the app. SQLite is in the public domain (https://sqlite.org/copyright.html).

## Rust crates (statically linked; 395 rows — the macOS arm64 graph links 409, see the 13 Sep 2026 note)

| Crate | Version | Licence |
|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| ahash | 0.7.8 | MIT OR Apache-2.0 |
| ahash | 0.8.12 | MIT OR Apache-2.0 |
| aho-corasick | 1.1.4 | Unlicense OR MIT |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.4 | BSD-3-Clause |
| anyhow | 1.0.103 | MIT OR Apache-2.0 |
| arrayvec | 0.7.7 | MIT OR Apache-2.0 |
| asn1-rs | 0.6.2 | MIT OR Apache-2.0 |
| asn1-rs-derive | 0.5.1 | MIT OR Apache-2.0 |
| asn1-rs-impl | 0.2.0 | MIT/Apache-2.0 |
| async-trait | 0.1.89 | MIT OR Apache-2.0 |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| base64 | 0.21.7 | MIT OR Apache-2.0 |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| bit-set | 0.8.0 | Apache-2.0 OR MIT |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| bitflags | 2.13.0 | MIT OR Apache-2.0 |
| bitvec | 1.1.1 | MIT |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| block2 | 0.6.2 | MIT |
| borsh | 1.7.0 | MIT OR Apache-2.0 |
| borsh-derive | 1.7.0 | Apache-2.0 |
| brotli | 8.0.4 | BSD-3-Clause AND MIT |
| brotli-decompressor | 5.0.3 | BSD-3-Clause/MIT |
| bs58 | 0.5.1 | MIT/Apache-2.0 |
| byte-unit | 5.2.3 | MIT |
| bytecheck | 0.6.12 | MIT |
| bytecheck_derive | 0.6.12 | MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| bytes | 1.12.0 | MIT |
| camino | 1.2.3 | MIT OR Apache-2.0 |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 |
| cargo_metadata | 0.19.2 | MIT |
| cargo_toml | 0.22.3 | Apache-2.0 OR MIT |
| cc | 1.2.65 | MIT OR Apache-2.0 |
| cfb | 0.7.3 | MIT |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 |
| cfg_aliases | 0.1.1 | MIT |
| cfg_aliases | 0.2.1 | MIT |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| cookie | 0.18.1 | MIT OR Apache-2.0 |
| cookie_store | 0.22.1 | MIT OR Apache-2.0 |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 |
| core-graphics | 0.25.0 | MIT OR Apache-2.0 |
| core-graphics-types | 0.2.0 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crc32fast | 1.5.0 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.15 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.21 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| cssparser | 0.36.0 | MPL-2.0 |
| cssparser-macros | 0.6.1 | MPL-2.0 |
| ctor | 0.8.0 | Apache-2.0 OR MIT |
| ctor-proc-macro | 0.0.7 | Apache-2.0 OR MIT |
| darling | 0.23.0 | MIT |
| darling_core | 0.23.0 | MIT |
| darling_macro | 0.23.0 | MIT |
| data-encoding | 2.11.0 | MIT |
| der-parser | 9.0.0 | MIT/Apache-2.0 |
| deranged | 0.5.8 | MIT OR Apache-2.0 |
| derive_more | 2.1.1 | MIT |
| derive_more-impl | 2.1.1 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| directories | 5.0.1 | MIT OR Apache-2.0 |
| dirs | 6.0.0 | MIT OR Apache-2.0 |
| dirs-sys | 0.4.1 | MIT OR Apache-2.0 |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| displaydoc | 0.2.6 | MIT OR Apache-2.0 |
| document-features | 0.2.12 | MIT OR Apache-2.0 |
| dom_query | 0.27.0 | MIT |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dtoa | 1.0.11 | MIT OR Apache-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| dtor | 0.3.0 | Apache-2.0 OR MIT |
| dtor-proc-macro | 0.0.6 | Apache-2.0 OR MIT |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 |
| embed-resource | 3.0.9 | MIT |
| embed_plist | 1.2.2 | MIT OR Apache-2.0 |
| enum-as-inner | 0.6.1 | MIT/Apache-2.0 |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| erased-serde | 0.4.10 | MIT OR Apache-2.0 |
| errno | 0.3.14 | MIT OR Apache-2.0 |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |
| fastrand | 2.4.1 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| fern | 0.7.1 | MIT |
| filedescriptor | 0.8.3 | MIT |
| filetime | 0.2.29 | MIT/Apache-2.0 |
| find-msvc-tools | 0.1.9 | MIT OR Apache-2.0 |
| flate2 | 1.1.9 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foldhash | 0.2.0 | Zlib |
| foreign-types | 0.5.0 | MIT/Apache-2.0 |
| foreign-types-macros | 0.2.3 | MIT/Apache-2.0 |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| funty | 2.0.0 | MIT |
| futures-channel | 0.3.32 | MIT OR Apache-2.0 |
| futures-core | 0.3.32 | MIT OR Apache-2.0 |
| futures-io | 0.3.32 | MIT OR Apache-2.0 |
| futures-macro | 0.3.32 | MIT OR Apache-2.0 |
| futures-sink | 0.3.32 | MIT OR Apache-2.0 |
| futures-task | 0.3.32 | MIT OR Apache-2.0 |
| futures-util | 0.3.32 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| getrandom | 0.3.4 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| glob | 0.3.3 | MIT OR Apache-2.0 |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| hashlink | 0.9.1 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| hex | 0.4.3 | MIT OR Apache-2.0 |
| hickory-proto | 0.24.4 | MIT OR Apache-2.0 |
| hickory-server | 0.24.4 | MIT OR Apache-2.0 |
| html5ever | 0.38.0 | MIT OR Apache-2.0 |
| http | 1.4.2 | MIT OR Apache-2.0 |
| http-body | 1.0.1 | MIT |
| http-body-util | 0.1.3 | MIT |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| hyper | 1.10.1 | MIT |
| hyper-rustls | 0.27.9 | Apache-2.0 OR ISC OR MIT |
| hyper-util | 0.1.20 | MIT |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 |
| ico | 0.5.0 | MIT |
| icu_collections | 2.2.0 | Unicode-3.0 |
| icu_locale_core | 2.2.0 | Unicode-3.0 |
| icu_normalizer | 2.2.0 | Unicode-3.0 |
| icu_normalizer_data | 2.2.0 | Unicode-3.0 |
| icu_properties | 2.2.0 | Unicode-3.0 |
| icu_properties_data | 2.2.0 | Unicode-3.0 |
| icu_provider | 2.2.0 | Unicode-3.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |
| image | 0.25.10 | MIT OR Apache-2.0 |
| indexmap | 1.9.3 | Apache-2.0 OR MIT |
| indexmap | 2.14.0 | Apache-2.0 OR MIT |
| infer | 0.19.0 | MIT |
| ipnet | 2.12.0 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| json-patch | 3.0.1 | MIT/Apache-2.0 |
| jsonptr | 0.6.3 | MIT OR Apache-2.0 |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 |
| lazy_static | 1.5.0 | MIT OR Apache-2.0 |
| libc | 0.2.186 | MIT OR Apache-2.0 |
| libsqlite3-sys | 0.30.1 | MIT |
| litemap | 0.8.2 | Unicode-3.0 |
| litrs | 1.0.0 | MIT OR Apache-2.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.33 | MIT OR Apache-2.0 |
| lru-slab | 0.1.2 | MIT OR Apache-2.0 OR Zlib |
| markup5ever | 0.38.0 | MIT OR Apache-2.0 |
| memchr | 2.8.2 | Unlicense OR MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.2.1 | MIT |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| muda | 0.19.3 | Apache-2.0 OR MIT |
| new_debug_unreachable | 1.0.6 | MIT |
| nix | 0.28.0 | MIT |
| nom | 7.1.3 | MIT |
| num-bigint | 0.4.6 | MIT OR Apache-2.0 |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| num-integer | 0.1.46 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| num_threads | 0.1.7 | MIT OR Apache-2.0 |
| objc2 | 0.6.4 | MIT |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-cloud-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-data | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-image | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-text | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-video | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-encode | 4.1.0 | MIT |
| objc2-exception-helper | 0.1.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-foundation | 0.3.2 | MIT |
| objc2-io-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-javascript-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-security | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| objc2-web-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| oid-registry | 0.7.1 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| option-ext | 0.2.0 | MPL-2.0 |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| pem | 3.0.6 | MIT |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_codegen | 0.13.1 | MIT |
| phf_generator | 0.13.1 | MIT |
| phf_macros | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| pkg-config | 0.3.33 | MIT OR Apache-2.0 |
| plist | 1.9.0 | MIT |
| png | 0.17.16 | MIT OR Apache-2.0 |
| png | 0.18.1 | MIT OR Apache-2.0 |
| portable-pty | 0.9.0 | MIT |
| potential_utf | 0.1.5 | Unicode-3.0 |
| powerfmt | 0.2.0 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 |
| psl-types | 2.0.11 | MIT/Apache-2.0 |
| ptr_meta | 0.1.4 | MIT |
| ptr_meta_derive | 0.1.4 | MIT |
| publicsuffix | 2.3.0 | MIT/Apache-2.0 |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| quick-xml | 0.39.4 | MIT |
| quinn | 0.11.11 | MIT OR Apache-2.0 |
| quinn-proto | 0.11.15 | MIT OR Apache-2.0 |
| quinn-udp | 0.5.14 | MIT OR Apache-2.0 |
| quote | 1.0.46 | MIT OR Apache-2.0 |
| radium | 0.7.0 | MIT |
| rand | 0.8.6 | MIT OR Apache-2.0 |
| rand | 0.9.4 | MIT OR Apache-2.0 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| rand_core | 0.9.5 | MIT OR Apache-2.0 |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| rcgen | 0.13.2 | MIT OR Apache-2.0 |
| ref-cast | 1.0.25 | MIT OR Apache-2.0 |
| ref-cast-impl | 1.0.25 | MIT OR Apache-2.0 |
| regex | 1.12.4 | MIT OR Apache-2.0 |
| regex-automata | 0.4.14 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rend | 0.4.2 | MIT |
| reqwest | 0.12.28 | MIT OR Apache-2.0 |
| rfd | 0.16.0 | MIT |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rkyv | 0.7.46 | MIT |
| rkyv_derive | 0.7.46 | MIT |
| rusqlite | 0.32.1 | MIT |
| rust_decimal | 1.42.1 | MIT |
| rustc-hash | 2.1.2 | Apache-2.0 OR MIT |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 |
| rusticata-macros | 4.1.0 | MIT/Apache-2.0 |
| rustix | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustls | 0.23.41 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.14.1 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.13 | ISC |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| schemars | 0.8.22 | MIT |
| schemars | 0.9.0 | MIT |
| schemars | 1.2.1 | MIT |
| schemars_derive | 0.8.22 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| seahash | 4.1.0 | MIT |
| selectors | 0.36.1 | MPL-2.0 |
| semver | 1.0.28 | MIT OR Apache-2.0 |
| serde | 1.0.228 | MIT OR Apache-2.0 |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 |
| serde_core | 1.0.228 | MIT OR Apache-2.0 |
| serde_derive | 1.0.228 | MIT OR Apache-2.0 |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 |
| serde_json | 1.0.150 | MIT OR Apache-2.0 |
| serde_repr | 0.1.20 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.1 | MIT OR Apache-2.0 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 |
| serde_with | 3.21.0 | MIT OR Apache-2.0 |
| serde_with_macros | 3.21.0 | MIT OR Apache-2.0 |
| serial2 | 0.2.37 | BSD-2-Clause OR Apache-2.0 |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| shell-words | 1.1.1 | MIT/Apache-2.0 |
| shlex | 2.0.1 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.9 | MIT |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |
| siphasher | 1.0.3 | MIT/Apache-2.0 |
| slab | 0.4.12 | MIT |
| smallvec | 1.15.2 | MIT OR Apache-2.0 |
| socket2 | 0.6.4 | MIT OR Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| string_cache | 0.9.0 | MIT OR Apache-2.0 |
| string_cache_codegen | 0.6.1 | MIT OR Apache-2.0 |
| strsim | 0.11.1 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| swift-rs | 1.0.7 | MIT OR Apache-2.0 |
| syn | 1.0.109 | MIT OR Apache-2.0 |
| syn | 2.0.118 | MIT OR Apache-2.0 |
| sync_wrapper | 1.0.2 | Apache-2.0 |
| synstructure | 0.13.2 | MIT |
| sysinfo | 0.36.1 | MIT |
| tao | 0.35.3 | Apache-2.0 |
| tap | 1.0.1 | MIT |
| tar | 0.4.46 | MIT OR Apache-2.0 |
| tauri | 2.11.3 | Apache-2.0 OR MIT |
| tauri-build | 2.6.3 | Apache-2.0 OR MIT |
| tauri-codegen | 2.6.3 | Apache-2.0 OR MIT |
| tauri-macros | 2.6.3 | Apache-2.0 OR MIT |
| tauri-plugin | 2.6.3 | Apache-2.0 OR MIT |
| tauri-plugin-dialog | 2.7.1 | Apache-2.0 OR MIT |
| tauri-plugin-fs | 2.5.1 | Apache-2.0 OR MIT |
| tauri-plugin-log | 2.8.0 | Apache-2.0 OR MIT |
| tauri-runtime | 2.11.3 | Apache-2.0 OR MIT |
| tauri-runtime-wry | 2.11.3 | Apache-2.0 OR MIT |
| tauri-utils | 2.9.3 | Apache-2.0 OR MIT |
| tauri-winres | 0.3.6 | MIT |
| tendril | 0.5.0 | MIT OR Apache-2.0 |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror | 2.0.18 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.18 | MIT OR Apache-2.0 |
| time | 0.3.51 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.30 | MIT OR Apache-2.0 |
| tinystr | 0.8.3 | Unicode-3.0 |
| tinyvec | 1.11.0 | Zlib OR Apache-2.0 OR MIT |
| tinyvec_macros | 0.1.1 | MIT OR Apache-2.0 OR Zlib |
| tokio | 1.52.3 | MIT |
| tokio-macros | 2.7.0 | MIT |
| tokio-rustls | 0.26.4 | MIT OR Apache-2.0 |
| tokio-util | 0.7.18 | MIT |
| toml | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 |
| toml | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_edit | 0.25.12+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_writer | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| tower | 0.5.3 | MIT |
| tower-http | 0.6.11 | MIT |
| tower-layer | 0.3.3 | MIT |
| tower-service | 0.3.3 | MIT |
| tracing | 0.1.44 | MIT |
| tracing-attributes | 0.1.31 | MIT |
| tracing-core | 0.1.36 | MIT |
| tray-icon | 0.24.1 | MIT OR Apache-2.0 |
| try-lock | 0.2.5 | MIT |
| typed-path | 0.12.3 | MIT OR Apache-2.0 |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unic-char-property | 0.9.0 | MIT/Apache-2.0 |
| unic-char-range | 0.9.0 | MIT/Apache-2.0 |
| unic-common | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-ident | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-version | 0.9.0 | MIT/Apache-2.0 |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 |
| untrusted | 0.9.0 | ISC |
| url | 2.5.8 | MIT OR Apache-2.0 |
| urlpattern | 0.3.0 | MIT |
| utf-8 | 0.7.6 | MIT OR Apache-2.0 |
| utf8-width | 0.1.8 | MIT |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT |
| uuid | 1.23.4 | Apache-2.0 OR MIT |
| value-bag | 1.12.0 | Apache-2.0 OR MIT |
| vcpkg | 0.2.15 | MIT/Apache-2.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| walkdir | 2.5.0 | Unlicense/MIT |
| want | 0.3.1 | MIT |
| web_atoms | 0.2.5 | MIT OR Apache-2.0 |
| webpki-roots | 1.0.8 | CDLA-Permissive-2.0 |
| window-vibrancy | 0.6.0 | Apache-2.0 OR MIT |
| winnow | 0.7.15 | MIT |
| winnow | 1.0.3 | MIT |
| writeable | 0.6.3 | Unicode-3.0 |
| wry | 0.55.1 | Apache-2.0 OR MIT |
| wyz | 0.5.1 | MIT |
| x509-parser | 0.16.0 | MIT OR Apache-2.0 |
| xattr | 1.6.1 | MIT OR Apache-2.0 |
| yasna | 0.5.2 | MIT OR Apache-2.0 |
| yoke | 0.8.3 | Unicode-3.0 |
| yoke-derive | 0.8.2 | Unicode-3.0 |
| zerocopy | 0.8.52 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.7 | Unicode-3.0 |
| zeroize | 1.9.0 | Apache-2.0 OR MIT |
| zerotrie | 0.2.4 | Unicode-3.0 |
| zerovec | 0.11.6 | Unicode-3.0 |
| zerovec-derive | 0.11.3 | Unicode-3.0 |
| zip | 8.6.0 | MIT |
| zmij | 1.0.21 | MIT |

Notes on the non-MIT/Apache families above: the five MPL-2.0 crates
(`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`) are
used unmodified; their source is available from crates.io at the exact versions
listed, which satisfies MPL-2.0 §3.2 for unmodified library use.
`webpki-roots` (CDLA-Permissive-2.0) packages Mozilla's CA trust data.

## npm packages (production closure bundled by Vite; 127 packages)

| Package | Version | Licence |
|---|---|---|
| @alloc/quick-lru | 5.2.0 | MIT |
| @fontsource/inter | 5.2.8 | OFL-1.1 |
| @fontsource/jetbrains-mono | 5.2.8 | OFL-1.1 |
| @fontsource/space-grotesk | 5.2.10 | OFL-1.1 |
| @jridgewell/gen-mapping | 0.3.13 | MIT |
| @jridgewell/resolve-uri | 3.1.2 | MIT |
| @jridgewell/sourcemap-codec | 1.5.5 | MIT |
| @jridgewell/trace-mapping | 0.3.31 | MIT |
| @nodelib/fs.scandir | 2.1.5 | MIT |
| @nodelib/fs.stat | 2.0.5 | MIT |
| @nodelib/fs.walk | 1.2.8 | MIT |
| @radix-ui/primitive | 1.1.6 | MIT |
| @radix-ui/react-compose-refs | 1.1.3 | MIT |
| @radix-ui/react-context | 1.2.0 | MIT |
| @radix-ui/react-dialog | 1.1.20 | MIT |
| @radix-ui/react-dismissable-layer | 1.1.16 | MIT |
| @radix-ui/react-focus-guards | 1.1.4 | MIT |
| @radix-ui/react-focus-scope | 1.1.13 | MIT |
| @radix-ui/react-id | 1.1.2 | MIT |
| @radix-ui/react-portal | 1.1.14 | MIT |
| @radix-ui/react-presence | 1.1.8 | MIT |
| @radix-ui/react-primitive | 2.1.7 | MIT |
| @radix-ui/react-slot | 1.3.0 | MIT |
| @radix-ui/react-use-callback-ref | 1.1.2 | MIT |
| @radix-ui/react-use-controllable-state | 1.2.4 | MIT |
| @radix-ui/react-use-effect-event | 0.0.3 | MIT |
| @radix-ui/react-use-layout-effect | 1.1.2 | MIT |
| @tanstack/query-core | 5.101.1 | MIT |
| @tanstack/react-query | 5.101.1 | MIT |
| @tauri-apps/api | 2.11.1 | Apache-2.0 OR MIT |
| @tauri-apps/plugin-dialog | 2.7.1 | MIT OR Apache-2.0 |
| @types/prop-types | 15.7.15 | MIT |
| @types/react | 18.3.31 | MIT |
| @types/react-dom | 18.3.7 | MIT |
| @xterm/addon-fit | 0.11.0 | MIT |
| @xterm/xterm | 6.0.0 | MIT |
| any-promise | 1.3.0 | MIT |
| anymatch | 3.1.3 | ISC |
| arg | 5.0.2 | MIT |
| aria-hidden | 1.2.6 | MIT |
| binary-extensions | 2.3.0 | MIT |
| braces | 3.0.3 | MIT |
| camelcase-css | 2.0.1 | MIT |
| chokidar | 3.6.0 | MIT |
| class-variance-authority | 0.7.1 | Apache-2.0 |
| clsx | 2.1.1 | MIT |
| cmdk | 1.1.1 | MIT |
| commander | 4.1.1 | MIT |
| cookie | 1.1.1 | MIT |
| cssesc | 3.0.0 | MIT |
| csstype | 3.2.3 | MIT |
| detect-node-es | 1.1.0 | MIT |
| didyoumean | 1.2.2 | Apache-2.0 |
| dlv | 1.1.3 | MIT |
| es-errors | 1.3.0 | MIT |
| fast-glob | 3.3.3 | MIT |
| fastq | 1.20.1 | ISC |
| fdir | 6.5.0 | MIT |
| fill-range | 7.1.1 | MIT |
| fsevents | 2.3.3 | MIT |
| function-bind | 1.1.2 | MIT |
| get-nonce | 1.0.1 | MIT |
| glob-parent | 6.0.2 | ISC |
| glob-parent | 5.1.2 | ISC |
| hasown | 2.0.4 | MIT |
| is-binary-path | 2.1.0 | MIT |
| is-core-module | 2.16.2 | MIT |
| is-extglob | 2.1.1 | MIT |
| is-glob | 4.0.3 | MIT |
| is-number | 7.0.0 | MIT |
| jiti | 1.21.7 | MIT |
| js-tokens | 4.0.0 | MIT |
| lilconfig | 3.1.3 | MIT |
| lines-and-columns | 1.2.4 | MIT |
| loose-envify | 1.4.0 | MIT |
| lucide-react | 0.469.0 | ISC |
| merge2 | 1.4.1 | MIT |
| micromatch | 4.0.8 | MIT |
| mz | 2.7.0 | MIT |
| nanoid | 3.3.15 | MIT |
| normalize-path | 3.0.0 | MIT |
| object-assign | 4.1.1 | MIT |
| object-hash | 3.0.0 | MIT |
| path-parse | 1.0.7 | MIT |
| picocolors | 1.1.1 | ISC |
| picomatch | 2.3.2 | MIT |
| picomatch | 4.0.4 | MIT |
| pify | 2.3.0 | MIT |
| pirates | 4.0.7 | MIT |
| postcss | 8.5.15 | MIT |
| postcss-import | 15.1.0 | MIT |
| postcss-js | 4.1.0 | MIT |
| postcss-load-config | 6.0.1 | MIT |
| postcss-nested | 6.2.0 | MIT |
| postcss-selector-parser | 6.1.4 | MIT |
| postcss-value-parser | 4.2.0 | MIT |
| queue-microtask | 1.2.3 | MIT |
| react | 18.3.1 | MIT |
| react-dom | 18.3.1 | MIT |
| react-remove-scroll | 2.7.2 | MIT |
| react-remove-scroll-bar | 2.3.8 | MIT |
| react-router | 7.18.0 | MIT |
| react-router-dom | 7.18.0 | MIT |
| react-style-singleton | 2.2.3 | MIT |
| read-cache | 1.0.0 | MIT |
| readdirp | 3.6.0 | MIT |
| resolve | 1.22.12 | MIT |
| reusify | 1.1.0 | MIT |
| run-parallel | 1.2.0 | MIT |
| scheduler | 0.23.2 | MIT |
| set-cookie-parser | 2.7.2 | MIT |
| source-map-js | 1.2.1 | BSD-3-Clause |
| sucrase | 3.35.1 | MIT |
| supports-preserve-symlinks-flag | 1.0.0 | MIT |
| tailwind-merge | 2.6.1 | MIT |
| tailwindcss | 3.4.19 | MIT |
| tailwindcss-animate | 1.0.7 | MIT |
| thenify | 3.3.1 | MIT |
| thenify-all | 1.6.0 | MIT |
| tinyglobby | 0.2.17 | MIT |
| to-regex-range | 5.0.1 | MIT |
| ts-interface-checker | 0.1.13 | Apache-2.0 |
| tslib | 2.8.1 | 0BSD |
| use-callback-ref | 1.3.3 | MIT |
| use-sidecar | 1.1.3 | MIT |
| util-deprecate | 1.0.2 | MIT |
| zustand | 5.0.14 | MIT |

## Licence texts

**Apache License 2.0** — the full text is this repository's `LICENSE` file.

**MIT License** (each MIT package above, under its own copyright holder):

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions: The above copyright
> notice and this permission notice shall be included in all copies or
> substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
> WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
> TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
> FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR
> THE USE OR OTHER DEALINGS IN THE SOFTWARE.

**ISC License**:

> Permission to use, copy, modify, and/or distribute this software for any
> purpose with or without fee is hereby granted, provided that the above
> copyright notice and this permission notice appear in all copies. THE
> SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
> REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
> AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
> INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
> LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
> OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
> PERFORMANCE OF THIS SOFTWARE.

**BSD 2-Clause / BSD 3-Clause**: redistribution and use in source and binary
forms, with or without modification, are permitted provided that the copyright
notice, the conditions list and the disclaimer are retained (and, for
3-Clause, that neither the copyright holder's nor contributors' names are used
to endorse derived products without permission). Full texts:
https://opensource.org/license/bsd-2-clause / https://opensource.org/license/bsd-3-clause

**Zlib, 0BSD, Unlicense, CC0-1.0, MIT-0, BSL-1.0, Unicode-3.0,
CDLA-Permissive-2.0**: permissive licences whose canonical texts are at
https://spdx.org/licenses/ under the identifiers used in the tables above.

**MPL-2.0**: https://www.mozilla.org/en-US/MPL/2.0/ — the covered crates are
used unmodified; source for the exact versions is available from crates.io.

**SIL Open Font License 1.1** (applies to the bundled Inter, Space Grotesk and
JetBrains Mono fonts):

> This Font Software is licensed under the SIL Open Font License, Version 1.1.
> PREAMBLE: The goals of the Open Font License (OFL) are to stimulate worldwide
> development of collaborative font projects, to support the font creation
> efforts of academic and linguistic communities, and to provide a free and
> open framework in which fonts may be shared and improved in partnership with
> others. The OFL allows the licensed fonts to be used, studied, modified and
> redistributed freely as long as they are not sold by themselves. The fonts,
> including any derivative works, can be bundled, embedded, redistributed
> and/or sold with any software provided that any reserved names are not used
> by derivative works. PERMISSION & CONDITIONS: Permission is hereby granted,
> free of charge, to any person obtaining a copy of the Font Software, to use,
> study, copy, merge, embed, modify, redistribute, and sell modified and
> unmodified copies of the Font Software, subject to the following conditions:
> (1) Neither the Font Software nor any of its individual components, in
> Original or Modified Versions, may be sold by itself. (2) Original or
> Modified Versions of the Font Software may be bundled, redistributed and/or
> sold with any software, provided that each copy contains the above copyright
> notice and this license. (3) No Modified Version of the Font Software may use
> the Reserved Font Name(s) unless explicit written permission is granted by
> the corresponding Copyright Holder. (4) The name(s) of the Copyright Holder(s)
> or the Author(s) of the Font Software shall not be used to promote, endorse
> or advertise any Modified Version or any related software, except to
> acknowledge the contribution(s) of the Copyright Holder(s) and the Author(s)
> or with their explicit written permission. (5) The Font Software, modified or
> unmodified, in part or in whole, must be distributed entirely under this
> license, and must not be distributed under any other license. TERMINATION:
> This license becomes null and void if any of the above conditions are not
> met. DISCLAIMER: THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
> ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
> COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE COPYRIGHT
> HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY
> GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN
> AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR
> INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT
> SOFTWARE.
