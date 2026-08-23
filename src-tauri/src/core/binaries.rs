//! core::binaries — BinaryProvider orchestration (Phase 1 task 4.1).
//!
//! Platform-agnostic: a manifest (os+arch+version → url + checksum) plus the
//! download → verify → extract → cache flow. OS-specific steps (arch detection,
//! make-executable, ad-hoc codesign/de-quarantine) are delegated to the
//! `BinaryProvider` / `PermissionManager` platform traits. No binaries are
//! bundled; everything is fetched on demand and checksum-verified.

use crate::core::downloads;
use crate::error::{Error, Result};
use crate::platform::traits::{Arch, Platform};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Pinned Caddy version (edge router).
pub const CADDY_VERSION: &str = "2.11.4";
/// Default PHP version (static-php build; provides `php` cli and `php-fpm`).
/// Used where a single version is implied (Phase 1 paths). Must be in [`PHP_VERSIONS`].
pub const PHP_VERSION: &str = "8.3.31";
/// All PHP versions with pinned static-php "bulk" builds (one minor each, newest
/// last). The per-version FPM pool manager + UI (Phase 2 §1.2/§1.5) install from
/// this set; each caches independently under `bin_dir/php-<version>/`.
/// **Not all from one source.** 8.x are static-php.dev's bulk builds; **7.4.33 is
/// OURS** — static-php.dev publishes no 7.4 and never did, so rexenv builds it
/// (`rexenv/runtimes`) and hosts it as an immutable release asset. `php_url`
/// picks the source; this list only says which versions exist.
/// 7.4 and 8.0 are both upstream-EOL and say so in the UI (`php::eol_since`).
pub const PHP_VERSIONS: &[&str] =
    &["7.4.33", "8.0.30", "8.1.34", "8.2.31", "8.3.31", "8.4.23", "8.5.8"];
/// PHP minor used for the **debug build** (Xdebug compiled in) that backs the §8.2
/// per-site Xdebug debug pool. The stock static-php "bulk" builds ship NO Xdebug
/// and a static PHP can't `dlopen` an external `xdebug.so` (§8.1), so this is a
/// SEPARATE custom static-php compile — see `docs/xdebug-debug-build.md` for the
/// reproducible `spc` recipe. The artifact is self-hosted; until it's uploaded and
/// its SHA-256 pinned below, `php-debug`/`php-fpm-debug` stay UNRESOLVABLE (§11.2).
pub const PHP_DEBUG_VERSION: &str = "8.3.31";
/// Xdebug version compiled into the debug build (recorded for the recipe + UI).
pub const PHP_DEBUG_XDEBUG_VERSION: &str = "3.4.5";
/// The `rexenv/runtimes` release TAG serving the self-built debug artifacts —
/// EMPTY until the first upload, exactly like the four digests below.
///
/// **This const replaced a base URL naming `dl.rexenv.dev`, and the replacement
/// is the point.** B33 moved php-debug hosting to GitHub Releases in
/// `rexenv/runtimes` (`docs/PLAN-php-74-support.md` §6/§9) and
/// `docs/xdebug-debug-build.md` recorded that `dl.rexenv.dev` "is not used" —
/// while the code went on building every php-debug URL from that dead host. A
/// doc asserting a state the code contradicts is a shape this project has
/// already paid for twice, and it survived because nothing FETCHES these URLs
/// yet: the contradiction was scheduled to be discovered at upload time, by the
/// person following the recipe, which is the worst possible reader.
///
/// So the host stopped being a string that can be wrong. It is now derived from
/// [`RUNTIMES_RELEASE_BASE`] — the same construction the 7.4 artifacts use — and
/// the only thing left to fill is what genuinely cannot be known until a release
/// exists: its tag. Like [`php_self_hosted_tag`], this is the FULL immutable tag
/// (`php-8.3.31-xdebug-1`, then `-2` for a rebuild), never a stable base, so a
/// pin here can 404 but can never resolve to different bytes.
const PHP_DEBUG_TAG: &str = "";
/// Pinned nginx version (jirutka/nginx-binaries static build).
pub const NGINX_VERSION: &str = "1.30.3";
/// Default MySQL version (official macOS tarball — a full bin/lib/share tree).
/// Must be in [`MYSQL_VERSIONS`].
pub const MYSQL_VERSION: &str = "8.4.6";
/// All MySQL versions with pinned tarballs (per-engine version switch — the
/// Databases page picker). Each SERIES keeps its own datadir; the default
/// series stays on the legacy `mysql/data` path.
pub const MYSQL_VERSIONS: &[&str] = &["8.4.6", "8.0.44"];
/// Pinned WP-CLI version (a .phar run via the bundled PHP; OS-agnostic).
pub const WP_CLI_VERSION: &str = "2.12.0";
/// Pinned Composer version (a .phar, ALWAYS run via the SITE's bundled PHP so
/// `composer install` platform checks match the PHP the plugin runs on; a
/// system composer is never executed — it can be a non-phar wrapper, e.g.
/// Herd's). Downloaded + sha-verified against getcomposer.org's published
/// .sha256sum + run-tested on the static PHP at pin time.
pub const COMPOSER_VERSION: &str = "2.10.2";
/// Pinned FrankenPHP version (one static binary: embedded PHP + Caddy). Used as a
/// per-site override server on an internal loopback port — Phase 2 §2.
pub const FRANKENPHP_VERSION: &str = "1.12.4";
/// The PHP compiled INTO the pinned FrankenPHP binary. FrankenPHP does not use
/// rexenv's php-fpm pools — a FrankenPHP site is served by THIS PHP whatever its
/// `php_version` says. Recorded as data (it was only ever a sentence in the
/// re-pin comment below) so `core::sites` can refuse a combination FrankenPHP
/// cannot honour, instead of the site quietly running something else.
/// **Moves with `FRANKENPHP_VERSION`** — the re-pin procedure prints it
/// (`frankenphp version` → "FrankenPHP v1.12.4 PHP 8.5.8 Caddy v2.11.4").
pub const FRANKENPHP_EMBEDDED_PHP: &str = "8.5.8";
/// Default PostgreSQL version (theseus-rs portable build — a full bin/lib/share
/// tree, like MySQL). Phase 2 §5.3. Must be in [`POSTGRES_VERSIONS`].
pub const POSTGRES_VERSION: &str = "18.4.0";
/// All PostgreSQL versions with pinned builds (per-engine version switch).
/// PG major datadirs are mutually INCOMPATIBLE — per-series datadirs are load-
/// bearing here, not just tidy.
pub const POSTGRES_VERSIONS: &[&str] = &["18.4.0", "17.10.0", "16.14.0"];
/// Pinned Mailpit version (one static Go binary: SMTP sink + web UI/API). Phase 3 §2.1.
pub const MAILPIT_VERSION: &str = "1.30.3";
/// Pinned Adminer version (a single `adminer.php`, all drivers, run via the bundled
/// PHP — OS-agnostic, like WP-CLI). Phase 3 §5.1.
pub const ADMINER_VERSION: &str = "5.4.2";
/// Pinned cloudflared version (one static Go binary; quick-tunnel public sharing). Phase 3 §9.1.
pub const CLOUDFLARED_VERSION: &str = "2026.6.1";
/// Pinned Redis version — the FIRST Homebrew-bottle BUNDLE (no portable static
/// build exists): the redis bottle's `bin/` merged with the openssl@3 bottle's
/// two dylibs, relinked to `@loader_path` by `prepare_binary_tree` (the shipped
/// "Deferred services" plan — docs/archive/SHIPPED-2026-07.md). Resolved via
/// [`resolve_bundle`].
pub const REDIS_VERSION: &str = "8.8.0";
/// Offered Redis versions (single — homebrew-core keeps no versioned redis
/// formula worth pinning; the picker hides for a one-entry set).
pub const REDIS_VERSIONS: &[&str] = &["8.8.0"];
/// Pinned MariaDB version (bottle bundle: server/client/dump + bootstrap SQL/
/// errmsg/charsets from the mariadb bottle, plus openssl@3 + pcre2 dylibs —
/// the ONLY libs `mariadbd`/clients actually link. groonga/lz4/lzo/xz/zstd are
/// PLUGIN-only deps (mroonga/connect); those plugins are excluded, so their
/// libs aren't bundled).
pub const MARIADB_VERSION: &str = "12.3.2";
/// All MariaDB versions with pinned bottle bundles (per-engine version switch).
/// 11.4 is the long-term-support series many hosts run (versioned formula
/// `mariadb@11.4` — same bottle layout, same runtime closure, verified).
pub const MARIADB_VERSIONS: &[&str] = &["12.3.2", "11.4.12"];
/// openssl@3 version bundled INTO dylib bundles (redis, mariadb).
/// Not a standalone binary — only ever a [`BundlePart`].
pub const BUNDLED_OPENSSL_VERSION: &str = "3.6.3";
/// pcre2 version bundled into the mariadb + httpd bundles (both link libpcre2-8).
pub const BUNDLED_PCRE2_VERSION: &str = "10.47";
/// Pinned Apache httpd version (bottle bundle: httpd + apr + apr-util + pcre2).
/// The `bin/httpd` core links ONLY apr/apr-util/pcre2 (+ system expat/iconv);
/// openssl/brotli/nghttp2 are deps of mod_ssl/mod_brotli/mod_http2 — those
/// modules are excluded (TLS/H2 are the edge's job), so their libs never enter
/// the bundle. Runs per-site as a loopback OVERRIDE backend (`core/apache.rs`).
pub const HTTPD_VERSION: &str = "2.4.68";
/// apr / apr-util versions bundled into the httpd bundle.
pub const BUNDLED_APR_VERSION: &str = "1.7.6";
pub const BUNDLED_APR_UTIL_VERSION: &str = "1.6.3";
/// Xdebug version pinned for the CURRENT PHP minors (per-site toggle, §8.2). ONE
/// `xdebug.so` per PHP minor from shivammathur/homebrew-extensions bottles (the
/// tap GitHub Actions setup-php uses on macOS) — they dlopen straight into our
/// EXISTING static-php binaries (ABI = Zend API nr + NTS + non-debug, all
/// matching; live-proven cli+fpm on 8.1–8.5, full DBGp handshake). No debug PHP
/// build needed; the debug pool is the same fpm binary + `-d zend_extension`.
/// PHP 8.0 is EXCLUDED: the Nov 2024 static 8.0.30 build exports no Zend symbols,
/// so any external .so fails to dlopen (`_OnUpdateBool` unresolved).
///
/// **This is a DEFAULT, not the law.** Xdebug's own support windows close: 3.1.6
/// is the last release for PHP 7.4 and no later one will ever exist, so a single
/// app-wide pin would make `xdebug-7.4` unresolvable by construction the day 7.4
/// ships (`docs/PLAN-php-74-support.md` §4.6). The per-minor version lives in
/// [`xdebug_bottle`]'s table beside the digests it must agree with — one row, one
/// version, one pair of hashes, so a minor cannot end up asking for a `.so` that
/// was never pinned for it.
pub const XDEBUG_VERSION: &str = "3.5.3";

/// How a downloaded artifact is packaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    /// gzip-compressed tar; extract `member` from it (single binary).
    TarGz,
    /// the download is the binary itself.
    Raw,
    /// gzip-compressed tar of a full directory tree; extracted whole (the
    /// single top-level dir is stripped). Used for MySQL (bin/lib/share).
    TarGzTree,
}

/// Pinned content hash of a downloaded artifact (digest varies by source:
/// Caddy publishes SHA-512; static-php has none, so we pin a SHA-256 we computed).
#[derive(Debug, Clone)]
pub enum Checksum {
    Sha256(String),
    Sha512(String),
}

/// A resolved download target: where to get it, how to verify and unpack it.
#[derive(Debug, Clone)]
pub struct BinarySpec {
    pub url: String,
    pub checksum: Checksum,
    pub archive: Archive,
    /// File name to extract from a `TarGz` archive (ignored for `Raw`).
    pub member: &'static str,
}

/// One Homebrew bottle contributing files to a dylib bundle. Bottles are OCI
/// blobs on ghcr.io — content-addressed (the URL embeds the SHA-256 we also pin
/// independently), so the bytes can never change under a URL, unlike the
/// rebuild-in-place sources (static-php/FrankenPHP). Entries are laid out
/// `<formula>/<version>/…` inside the tarball (TWO components stripped).
#[derive(Debug, Clone)]
pub struct BundlePart {
    /// Formula name, for logs/errors (e.g. `openssl@3`).
    pub formula: &'static str,
    pub url: String,
    pub checksum: Checksum,
    /// Post-strip tree paths to keep (component-wise prefix match — `"bin"`
    /// keeps the whole dir, `"lib/libssl.3.dylib"` exactly that file). Bottles
    /// carry docs/static-libs/receipts we never want in the cache.
    pub include: &'static [&'static str],
}

/// A multi-bottle dylib bundle: parts merged into ONE cached tree, then
/// relinked + re-signed by `BinaryProvider::prepare_binary_tree`.
#[derive(Debug, Clone)]
pub struct BundleSpec {
    pub parts: Vec<BundlePart>,
    /// Primary binary within the merged tree (the cache marker, like
    /// `BinarySpec::member` for `TarGzTree`).
    pub member: &'static str,
}

/// ghcr.io blob URL for a Homebrew-core bottle. The registry path maps a
/// versioned formula's `@` to `/` (`openssl@3` → `homebrew/core/openssl/3`).
fn bottle_url(formula: &str, digest: &str) -> String {
    tap_bottle_url("homebrew/core", formula, digest)
}

/// ghcr.io blob URL for a bottle under an arbitrary tap root (same `@` → `/`
/// mapping; e.g. `shivammathur/extensions` + `xdebug@8.4` →
/// `shivammathur/extensions/xdebug/8.4`).
fn tap_bottle_url(root: &str, formula: &str, digest: &str) -> String {
    format!(
        "https://ghcr.io/v2/{root}/{}/blobs/sha256:{digest}",
        formula.replace('@', "/")
    )
}

/// ghcr.io requires a bearer token even for public blobs; `QQ==` (base64 of the
/// empty string) is the documented anonymous token Homebrew itself uses.
const GHCR_ANON_AUTH: &[(&str, &str)] = &[("Authorization", "Bearer QQ==")];

// Official Caddy SHA-512 checksums (from caddy_<ver>_checksums.txt).
const CADDY_2_11_4_MAC_ARM64_SHA512: &str = "3190ae0df98b59ab4b6021556fa35adc3c526a4f3e138776b0eaec8a037cc26121cbbb1ad53453f565551b47d37d5ba4755e2c2c3652256737fe2ce9e53c8ec0";
const CADDY_2_11_4_MAC_AMD64_SHA512: &str = "e04eb10f9ce7e2e079bc9bff1bd5d3a3164888d1edbb1a49e5d15be4eab691b57e89ed36bb29c65ba43f1ba8d9279e0967b1003991c13fe4cb78384c3caf25de";

// static-php.dev "bulk" build SHA-256 (computed at pin time — source publishes
// no checksums; downloaded and hashed each artifact). The bulk build includes
// mysqli (required by WordPress) + a wide extension set, unlike "common".
// One digest per version × {cli,fpm} × {arm64,amd64}; both arches pinned together.
// NOTE: upstream REBUILDS these artifacts in place (same URL, new bytes) — a
// sudden checksum mismatch in the wild usually means a rebuild, not tampering.
// On mismatch: download, verify (`php -v` version + `php -m` has mysqli, Mach-O
// arch), then re-pin. 8.2/8.3 re-pinned 2026-07-05 after the 2026-07-01 rebuild.
// 8.0.30 / 8.4.23 / 8.5.8 pinned 2026-07-11: all 12 artifacts downloaded, hashed,
// extracted, and RUN (arm64 native + x86_64 under Rosetta) — version + mysqli +
// Mach-O arch verified on every one before pinning.
const PHP_8_0_30_CLI_MAC_ARM64_SHA256: &str = "13c77c837cd50c027e1c614c192b25205123311a30d2335e9a3c8f82d23acb9a";
const PHP_8_0_30_CLI_MAC_AMD64_SHA256: &str = "b025f2c343916dd97d4cad543f0cc4c07ac882af10bc72773ad27ab8f6c9f59a";
const PHP_8_0_30_FPM_MAC_ARM64_SHA256: &str = "e91bc2624c4469ceb0d7f7d93d643e1aebadd451556b0b1cc8d5142531b14138";
const PHP_8_0_30_FPM_MAC_AMD64_SHA256: &str = "ec02cd54162c190c0029ddccbc382e21442b4941aedef9455b8d6fd32bc472d5";
const PHP_8_1_34_CLI_MAC_ARM64_SHA256: &str = "b721271659d6e3448c29c0dc5755ffc4b8a1498c4709e1aba6602cfb584a84e4";
const PHP_8_1_34_CLI_MAC_AMD64_SHA256: &str = "5fe69256365f96a270e34208ec574be7012c8c08a23bdf52948d0d16d4d8ec6a";
const PHP_8_1_34_FPM_MAC_ARM64_SHA256: &str = "c5faad9eac5ce9753c30a17fb2a2023dcf72e5b367e0ac76b81d006647ea0e52";
const PHP_8_1_34_FPM_MAC_AMD64_SHA256: &str = "eab87df298d83c8182f296e3f56ac4025cdb2a74baa4b6587d27ea39ac31b5e6";
const PHP_8_2_31_CLI_MAC_ARM64_SHA256: &str = "f4ed44af2ad24588ba2ac1934bfaf794a923dd629fbc8958e37caabef9a592aa";
const PHP_8_2_31_CLI_MAC_AMD64_SHA256: &str = "5e38df46d55b058765ea81c54b5368ce951cf2974591f42377d41b78426b75e0";
const PHP_8_2_31_FPM_MAC_ARM64_SHA256: &str = "d2041fbb23cdfcedd4561edc76be81e2d2ea07e4e0d9a2033f224ef5a30954f1";
const PHP_8_2_31_FPM_MAC_AMD64_SHA256: &str = "33984f891a586baa98ac847a224d050f294278dbc8953e84c2358aef1949c395";
const PHP_8_3_31_CLI_MAC_ARM64_SHA256: &str = "8dd2089ced9f07165fe7d8c1789810547e27936b9d3b3075f91cf608dbf65bb9";
const PHP_8_3_31_CLI_MAC_AMD64_SHA256: &str = "a3b39184563f7e53b7d53df94ec38ac02d69388aefec5bf7b5fd82d6061cc753";
const PHP_8_3_31_FPM_MAC_ARM64_SHA256: &str = "1995f59e7eecfd7897e837929bdeb45fc277bad3d0375a228eaa74c0862188cb";
const PHP_8_3_31_FPM_MAC_AMD64_SHA256: &str = "33e10b2eac7a478f913ed6ce6bfd7c10e0b8177ebf26747e9c8175408308d3eb";
const PHP_8_4_23_CLI_MAC_ARM64_SHA256: &str = "4a5dca6df0211f7fb21425cf1c867968b2dfb7c079f98cf37770c5728e0bf709";
const PHP_8_4_23_CLI_MAC_AMD64_SHA256: &str = "be88a71134d43e8800946f8372da931ee1796c400a1fb4cf2afd718644251ffc";
const PHP_8_4_23_FPM_MAC_ARM64_SHA256: &str = "1a417db44f0eb0b40a9f8cd862f24ecfabdea87f85863f30d218c4876bb688ef";
const PHP_8_4_23_FPM_MAC_AMD64_SHA256: &str = "67bbb7f2b2543d45c8a4ab0e4759fbd956530e87fba47a00505c06f09c9b9950";
const PHP_8_5_8_CLI_MAC_ARM64_SHA256: &str = "5e5032e8244a2367b1e8a9c70ff6f793dee7433966c9291da85fadf2167cd55f";
const PHP_8_5_8_CLI_MAC_AMD64_SHA256: &str = "d5a9a505ebce66c7f6b4f4e16629c36f385f2d360df915875722d67fe8bb2161";
const PHP_8_5_8_FPM_MAC_ARM64_SHA256: &str = "1d994fbc4e49015a7cd4ad4fcb7c03e7e219f65fdb14e5e33ef1c44d928368e9";
const PHP_8_5_8_FPM_MAC_AMD64_SHA256: &str = "57cdce953a8392e655a800908eb5e8fa61e2b7d795b8e7d3f354b3d81939563d";

// jirutka/nginx-binaries SHA-256 (computed at pin time; cross-checked vs the
// project's published SHA-1).
const NGINX_1_30_3_MAC_ARM64_SHA256: &str = "b6c4e80357977457b9395a43497f5709dc989ff8fce9102bb558ed5d2e066b15";
const NGINX_1_30_3_MAC_AMD64_SHA256: &str = "fe1df1fdf5de7c5b778a16b1c73aa73d0c22d48219bd712e51094c0e8655a641";

// Official MySQL macOS tarball SHA-256 (computed at pin time from dev.mysql.com).
const MYSQL_8_4_6_MAC_ARM64_SHA256: &str = "56ac9150b9d8fc757a36a2661a1214f5b09e5352d0a220e7a6c302685a5fca10";
const MYSQL_8_4_6_MAC_AMD64_SHA256: &str = "257d36d7ae26c4d1cc616dacf58cd1498c9b3b6dc592f90a63d7e7ecd83be844";
// 8.0 LTS series (per-engine version switch) — both arches downloaded + hashed
// 2026-07-15; arm64 extracted and RUN (`mysqld --version` = 8.0.44, Mach-O arm64).
const MYSQL_8_0_44_MAC_ARM64_SHA256: &str = "e0a9b7a04051c570706ca4c7b8a8d6749ac984aab9eecfa41c6ca395a75a0c91";
const MYSQL_8_0_44_MAC_AMD64_SHA256: &str = "71fda78dfb3479a5ab1dc3f1a86fc099b781ed93280435d5683b14e119f24add";

/// Pinned SHA-256 for a MySQL tarball, or `None` for an unpinned version.
fn mysql_sha256(version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match version {
        "8.4.6" => (MYSQL_8_4_6_MAC_ARM64_SHA256, MYSQL_8_4_6_MAC_AMD64_SHA256),
        "8.0.44" => (MYSQL_8_0_44_MAC_ARM64_SHA256, MYSQL_8_0_44_MAC_AMD64_SHA256),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
}

/// The CDN archive folder for a MySQL version (`mysql-8.0/`, `mysql-8.4/`).
fn mysql_series(version: &str) -> String {
    let mut it = version.split('.');
    format!(
        "{}.{}",
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default()
    )
}

/// Pinned SHA-256 for a theseus-rs PostgreSQL build, or `None` if unpinned.
fn postgres_sha256(version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match version {
        "18.4.0" => (POSTGRES_18_4_0_MAC_ARM64_SHA256, POSTGRES_18_4_0_MAC_AMD64_SHA256),
        "17.10.0" => (POSTGRES_17_10_0_MAC_ARM64_SHA256, POSTGRES_17_10_0_MAC_AMD64_SHA256),
        "16.14.0" => (POSTGRES_16_14_0_MAC_ARM64_SHA256, POSTGRES_16_14_0_MAC_AMD64_SHA256),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
}

// WP-CLI phar SHA-256 (GitHub release; same artifact on every OS/arch).
const WP_CLI_2_12_0_SHA256: &str = "ce34ddd838f7351d6759068d09793f26755463b4a4610a5a5c0a97b68220d85c";
// Verified 17 Jul 2026: download hashed == getcomposer.org's published
// composer.phar.sha256sum, phar run-tested on the static PHP 8.3.31.
const COMPOSER_2_10_2_SHA256: &str =
    "5ee7125f8a30a34d246cefdc0bc85b8a783b28f2aec968994118512350d28027";

// Adminer single-file SHA-256 (GitHub release `adminer-5.4.2-en.php`; same on every
// OS/arch — a PHP script). English UI, all DB drivers (MySQL + PostgreSQL).
const ADMINER_5_4_2_SHA256: &str = "f8b1cdc676d72e88d2d470dd05f2dcb7212bf6cdcf78f1eadb7fc292f4cefd39";

// cloudflared static Go binary SHA-256 (computed at pin time from the GitHub
// release `.tgz`). De-quarantined + ad-hoc signed by prepare_binary (no relink).
const CLOUDFLARED_2026_6_1_MAC_ARM64_SHA256: &str = "f6d4c439c6c782b83264951d327989ce5e23373acc5942b872411601fedb020d";
const CLOUDFLARED_2026_6_1_MAC_AMD64_SHA256: &str = "d7a66b525fe76820da6e5406611b61e48b40de682368ac00454d9158f085be4b";

// FrankenPHP static binary SHA-256 (computed at pin time from the GitHub release).
// A fully static Mach-O (embeds PHP + Caddy), so no Homebrew relink is needed.
//
// NOTE: FrankenPHP REBUILDS the latest release's assets in place on a DAILY cron
// (static.yaml, `gh release upload --clobber`) to embed new PHP patches — the tag
// URL is stable but the bytes are not, so these pins go stale whenever upstream
// rebuilds (same story as static-php; the checksum guard firing is the designed
// signal to re-verify + re-pin). Re-pin procedure: fetch the GitHub API asset
// `digest` for the release, confirm the downloaded bytes hash to it, decode the
// sigstore attestation (`repos/dunglas/frankenphp/attestations/sha256:<hash>` —
// uploads still target the pre-rename repo) to confirm the subject was built by
// php/frankenphp's static.yaml, and run the binary (`frankenphp version`).
//
// Re-pinned 2026-07-11: upstream's scheduled rebuild bumped the embedded PHP to
// 8.5.8 (all 9 release assets replaced). Both hashes verified against the GitHub
// API digest + SLSA attestation (workflow php/frankenphp static.yaml, commit
// bc9c6db8) and both binaries run: "FrankenPHP v1.12.4 PHP 8.5.8 Caddy v2.11.4".
const FRANKENPHP_1_12_4_MAC_ARM64_SHA256: &str = "44308eddac92d0207636b054ed66500f57b34b423f6df335073fd59007e78b0d";
const FRANKENPHP_1_12_4_MAC_AMD64_SHA256: &str = "9aa5ea729ec9aee6fda6facfb7f874555cda7c07ac0933d72fb1b7045d7cd363";

// PostgreSQL portable build SHA-256 (theseus-rs/postgresql-binaries — the project
// PUBLISHES these `.sha256` files; cross-checked against a fresh download). A
// relocatable bin/lib/share tree (unsigned Mach-O that runs as-is on Apple Silicon).
const POSTGRES_18_4_0_MAC_ARM64_SHA256: &str = "1b68828f524b638a24918e258b173d0f16773547a0d3b83d9ba74473b61649f2";
const POSTGRES_18_4_0_MAC_AMD64_SHA256: &str = "cbc38067a795d10bbddc730e61c835df0b351c36a7bd2544d388790fcf50aa4d";
// 17/16 series (per-engine version switch) — same published-.sha256 source,
// fetched 2026-07-15.
const POSTGRES_17_10_0_MAC_ARM64_SHA256: &str = "e15b5d3b86363d51fe06c9f26ee1d35d09b13951be82641b8f4b2d0e06e2c51e";
const POSTGRES_17_10_0_MAC_AMD64_SHA256: &str = "737c0e14bd2f1546aaf728153851cfee2d93e682520eb87ba0576a08ec6d9789";
const POSTGRES_16_14_0_MAC_ARM64_SHA256: &str = "a7a4846456df26d27f815267dfe725b4ad4f46312c032e7b5939468250a4891c";
const POSTGRES_16_14_0_MAC_AMD64_SHA256: &str = "c5ecdea2528e29503140e259c043002f6f8f2e9d1ee2f1decb44e8b394254820";

// Mailpit static binary SHA-256 (computed at pin time — the project publishes no
// checksums file; each darwin tarball downloaded and hashed). A static Go Mach-O
// (no Homebrew deps to relink), de-quarantined + ad-hoc signed by prepare_binary.
const MAILPIT_1_30_3_MAC_ARM64_SHA256: &str = "46b68e5701c32f2137e97d325605f7e8f0fbb6518e567b7589147c3534bd943e";
const MAILPIT_1_30_3_MAC_AMD64_SHA256: &str = "ea8c2f5ac717ece100b453de282474b46e8f4c327d3e61bee6348f60989eade3";

// Homebrew bottle digests (from formulae.brew.sh, pinned 2026-07-14; the ghcr
// blob URL embeds the same digest, so the pin is self-consistent — a changed
// upstream can only 404, never swap bytes silently. That is ghcr's contract,
// not something we can test (ledger #90) — which is fine, because the local
// checksum verify catches a swapped body anyway; the pin never TRUSTS the
// transport, it just makes drift loud as a 404 instead of a hash mismatch). arm64 = the arm64_sonoma
// bottle, x86_64 = the sonoma bottle: oldest supported build of each arch runs
// on every newer macOS. arm64 bottles downloaded + hashed + relinked + RUN at
// pin time; x86_64 digests are Homebrew-published (verify live on the next
// Intel smoke run).
const REDIS_8_8_0_BOTTLE_ARM64_SHA256: &str = "b483b7c9b4b107512ecb359d98e494cc224c0a14318a04ba97ce9223335e39a0";
const REDIS_8_8_0_BOTTLE_AMD64_SHA256: &str = "07d051d7a255d7d6a535b46387cc8977b4d3bf1798cb67b3a6d078ef5c4c3343";
const OPENSSL_3_6_3_BOTTLE_ARM64_SHA256: &str = "79774ba3c854f0a9f94d939c628414c9b3dd2ff5eeb1dc61743199c979dd3490";
const OPENSSL_3_6_3_BOTTLE_AMD64_SHA256: &str = "f641a0a3028a7ba2ab247767a6961226ba8c1777dac6e986e6fc62ec09e4a62a";
const MARIADB_12_3_2_BOTTLE_ARM64_SHA256: &str = "c27bbe91e87906b5f67d8828d061fc898f63ecf3520442bef5c55653c1a07dd2";
const MARIADB_12_3_2_BOTTLE_AMD64_SHA256: &str = "d25f40713fb7e44f2da1ad4117028cb992167c680fada0da27357cd5ca6d4d4f";
// mariadb@11.4 (LTS) versioned-formula bottle — identical layout + runtime
// closure to 12.x (mariadbd links openssl+pcre2 only; same bootstrap SQL
// names; verified from the downloaded bottle 2026-07-15).
const MARIADB_11_4_12_BOTTLE_ARM64_SHA256: &str = "4c59779a87d97f762b4a0f2b72f273e77b52461a5797338ecfc2456a962806b8";
const MARIADB_11_4_12_BOTTLE_AMD64_SHA256: &str = "bb9932c984c75e52372f3e0c15b53b6fa90bfa9d04d5a0b89eb059f8fb0a6327";
// pcre2 bottle (revision `10.47_1` inside the tarball — irrelevant post-strip).
const PCRE2_10_47_BOTTLE_ARM64_SHA256: &str = "f6d184fa59de4ca2f3115cb661f113c6c25ced2247b4e169dd99389c0d58be3f";
const PCRE2_10_47_BOTTLE_AMD64_SHA256: &str = "72691a0ed5b0ec4d21641ee33aa00fad05e6e8ddbfa417fe27f4cd26521ed24a";
const HTTPD_2_4_68_BOTTLE_ARM64_SHA256: &str = "5835c1181a511b8c0eb5729dc27734669387a1c2d0dd95322ec6ae6b2a3f0bfc";
const HTTPD_2_4_68_BOTTLE_AMD64_SHA256: &str = "50b619ada5467134fbd92ee54c1fed89f4f8c3b4598ae6c9c197993e40e447aa";
const APR_1_7_6_BOTTLE_ARM64_SHA256: &str = "d89324cbc51a250e109e00dc2e90ce77611058027060c39c83bb771118502332";
const APR_1_7_6_BOTTLE_AMD64_SHA256: &str = "fdf0f628598225db7ea43128abaf944011df61e2469811709c250607745b8570";
// apr-util bottle (revision `1.6.3_1` inside the tarball — irrelevant post-strip).
const APR_UTIL_1_6_3_BOTTLE_ARM64_SHA256: &str = "e21a775a4cd6e721ad4f09cd7ed0355b5a1181ca8ad6834911a045c8f076eb01";
const APR_UTIL_1_6_3_BOTTLE_AMD64_SHA256: &str = "a59301c0e98b321c57fc3c8fac679a1e1bcdd5bce470fef60adc240f9c575674";

// Xdebug 3.5.3 bottles from shivammathur/homebrew-extensions (ghcr root
// `shivammathur/extensions`, NOT homebrew/core) — one per PHP minor, the .so is
// ABI-bound to its minor. Same digest convention as above (arm64_sonoma +
// sonoma). Pinned 2026-07-16: all 10 blobs downloaded + hashed; every arm64
// .so LOADED into the cached static php/php-fpm of its minor (`with Xdebug
// v3.5.3` banner + module listed); x86_64 spot-verified under Rosetta (8.4
// static php x86_64 + sonoma .so → loads). Old tags on this repo stay
// pullable (verified back to 3.2.2), so these pins can 404 only on a
// registry-side prune — never drift.
const XDEBUG_PHP81_BOTTLE_ARM64_SHA256: &str = "78531e924acdca0f8b89c9f0059cf37aacd49e3d42e4daf3b78afa9da99d1507";
const XDEBUG_PHP81_BOTTLE_AMD64_SHA256: &str = "3995225ef848dc149b49be17c86c657e869f28ff4f64d222dabdd516fc022909";
const XDEBUG_PHP82_BOTTLE_ARM64_SHA256: &str = "9f7142dd46a12d0f5599c3268c47053f6fe82bceb1d266284f7d3f2b233cdf7b";
const XDEBUG_PHP82_BOTTLE_AMD64_SHA256: &str = "796a4c7d44eb00e2564d9858abf5f494b9324e4b884fa207484e5e7dd2facec3";
const XDEBUG_PHP83_BOTTLE_ARM64_SHA256: &str = "3f6a844f949e0574e9569e34eddedb7eef873f49d65d7488a0e710d83719a748";
const XDEBUG_PHP83_BOTTLE_AMD64_SHA256: &str = "cb8083ae1897da4ea92f473c3425926674fb0a841f58da21fe62044f0d983f2c";
const XDEBUG_PHP84_BOTTLE_ARM64_SHA256: &str = "6c7304aa2b45236f72ab3417d0ad6cc361888095c5ef34b770544e6af66f6968";
const XDEBUG_PHP84_BOTTLE_AMD64_SHA256: &str = "bf938d1b176343cd13be5ae1b786e1d029607f1a04b065fee6da0ac60d659af4";
const XDEBUG_PHP85_BOTTLE_ARM64_SHA256: &str = "c648f2a92e7f1995fb95b46981b16ffa9048c4507625f12999e247e7a3b58581";
const XDEBUG_PHP85_BOTTLE_AMD64_SHA256: &str = "068070ccb2080d8ce7a312fc934dfe3c6d5901c6527a20fcec783f78e619b53c";

/// One PHP minor's Xdebug row: the version pinned FOR THAT MINOR, the tap
/// formula, and the two arch digests. Kept as one struct so the four facts can
/// never be edited apart — a version bumped without its hashes is a 404, and
/// hashes bumped without their version silently resolve the old cache dir.
#[derive(Debug, Clone, Copy)]
struct XdebugBottle {
    /// Xdebug release for this minor. NOT necessarily [`XDEBUG_VERSION`]: a
    /// minor past Xdebug's support window is frozen at its last release (7.4 →
    /// 3.1.6, and there will never be a 3.2 for it).
    version: &'static str,
    formula: &'static str,
    arm64: &'static str,
    amd64: &'static str,
}

/// Why a PHP minor has no Xdebug — or that it does.
///
/// **This exists because `Option<XdebugBottle>` answered two different questions
/// with the same `None`,** and the difference is the whole user-facing message.
/// 7.4 and 8.0 physically cannot load ANY external `.so`: their static builds
/// export no Zend symbols (`_OnUpdateBool` unresolved), measured on 8.0 in Nov
/// 2024 and on our own 7.4 build on 14 Aug 2026. No pin can fix that. A minor
/// with no row for any OTHER reason is a gap in this table — somebody has not
/// pinned a bottle yet — and telling that user "its static build can't load
/// extensions" would be a confident falsehood about their PHP.
///
/// It cost nothing today only because both current absences happen to be the
/// physical kind, so the one message shipped was true of both. It goes wrong the
/// day a minor arrives before its bottle does, which is a normal Tuesday: PHP
/// 8.6 lands, `PHP_VERSIONS` grows, and the toggle explains itself with a reason
/// that is not the reason.
/// Private on purpose: the outside world gets [`xdebug_supported`] and
/// [`xdebug_unavailable_reason`], never the rows. A caller that could match on
/// this could write its own message, which is the drift this type exists to stop.
#[derive(Debug, Clone, Copy)]
enum XdebugStatus {
    /// A pinned bottle: the toggle works for this minor.
    Available(XdebugBottle),
    /// The PHP build cannot dlopen any extension. Carries where that was
    /// measured, because "we tried it" is the only thing that makes this
    /// different from "we did not get round to it".
    CannotLoadExtensions { measured: &'static str },
    /// No bottle pinned for this minor yet. A gap in the table, not a fact about
    /// PHP — and never reachable for a version in [`PHP_VERSIONS`], which
    /// `every_offered_php_minor_has_an_explicit_xdebug_verdict` enforces.
    NotPinned,
}

/// The Xdebug status for a PHP minor: a pinned row, or the REASON there is none.
/// The single source of which minors support the toggle, of which release each
/// one gets, and of what to tell a user who asks for one that has none.
fn xdebug_status(minor: &str) -> XdebugStatus {
    let row = |version, formula, arm64, amd64| {
        XdebugStatus::Available(XdebugBottle { version, formula, arm64, amd64 })
    };
    match minor {
        // Measured, not assumed. Both exports checked with `nm -gU`; 7.4's build
        // shows ~22,400 symbols and not the one that matters.
        "7.4" => XdebugStatus::CannotLoadExtensions {
            measured: "rexenv's own 7.4.33 build, 14 Aug 2026",
        },
        "8.0" => XdebugStatus::CannotLoadExtensions {
            measured: "static-php.dev's 8.0.30 build, Nov 2024",
        },
        "8.1" => row(XDEBUG_VERSION, "xdebug@8.1", XDEBUG_PHP81_BOTTLE_ARM64_SHA256, XDEBUG_PHP81_BOTTLE_AMD64_SHA256),
        "8.2" => row(XDEBUG_VERSION, "xdebug@8.2", XDEBUG_PHP82_BOTTLE_ARM64_SHA256, XDEBUG_PHP82_BOTTLE_AMD64_SHA256),
        "8.3" => row(XDEBUG_VERSION, "xdebug@8.3", XDEBUG_PHP83_BOTTLE_ARM64_SHA256, XDEBUG_PHP83_BOTTLE_AMD64_SHA256),
        "8.4" => row(XDEBUG_VERSION, "xdebug@8.4", XDEBUG_PHP84_BOTTLE_ARM64_SHA256, XDEBUG_PHP84_BOTTLE_AMD64_SHA256),
        "8.5" => row(XDEBUG_VERSION, "xdebug@8.5", XDEBUG_PHP85_BOTTLE_ARM64_SHA256, XDEBUG_PHP85_BOTTLE_AMD64_SHA256),
        _ => XdebugStatus::NotPinned,
    }
}

/// The pinned row for a minor, dropping the reason. For callers that only need
/// to know WHETHER, never why — everything user-facing goes through
/// [`xdebug_unavailable_reason`] instead.
fn xdebug_bottle(minor: &str) -> Option<XdebugBottle> {
    match xdebug_status(minor) {
        XdebugStatus::Available(b) => Some(b),
        _ => None,
    }
}

/// Whether the per-site Xdebug toggle is available for a PHP minor.
pub fn xdebug_supported(minor: &str) -> bool {
    xdebug_bottle(minor).is_some()
}

/// Why the toggle is unavailable, as a sentence for the user — or `None` when it
/// IS available.
///
/// Lives here rather than at the call site so the two refusal paths
/// (`core::sites`' toggle and anything that grows one later) cannot drift into
/// telling the same user two different stories. The advice differs with the
/// reason on purpose: "switch to a newer PHP" is right for a build that cannot
/// load extensions and actively wrong for a minor whose bottle is merely
/// missing — there may be nothing newer to switch to.
pub fn xdebug_unavailable_reason(minor: &str) -> Option<String> {
    match xdebug_status(minor) {
        XdebugStatus::Available(_) => None,
        XdebugStatus::CannotLoadExtensions { measured } => Some(format!(
            "Xdebug isn't available for PHP {minor} — its static build exports no Zend \
             symbols, so it can't load any extension ({measured}). No version of Xdebug \
             can change that. Switch the site to PHP 8.1 or newer first."
        )),
        XdebugStatus::NotPinned => Some(format!(
            "Xdebug isn't available for PHP {minor} yet — rexenv has no Xdebug build \
             pinned for this version. Nothing is wrong with your site; the toggle will \
             work once one ships."
        )),
    }
}

/// The Xdebug release pinned for a PHP minor, or `None` where the toggle isn't
/// offered. For the UI: the version a debug pool will actually load, which is
/// not app-wide (see [`XDEBUG_VERSION`]).
pub fn xdebug_version_for(minor: &str) -> Option<&'static str> {
    xdebug_bottle(minor).map(|b| b.version)
}

/// The one-part bundle for an Xdebug row: a bare `xdebug.so` from the
/// shivammathur/extensions tap (NOT homebrew/core), links only system
/// libSystem+libz so the relink pass is a no-op and re-sign applies as usual.
/// Loaded into the SAME static php-fpm by the debug pool (`core::php`), never a
/// separate PHP build.
///
/// Pure and taking the ROW rather than the minor, so a frozen-version row can be
/// exercised before one is in the table — which is the case this whole shape
/// exists for.
fn xdebug_spec(bottle: &XdebugBottle, arch: Arch) -> BundleSpec {
    let digest = pick(arch, bottle.arm64, bottle.amd64);
    BundleSpec {
        member: "xdebug.so",
        parts: vec![BundlePart {
            formula: bottle.formula,
            url: tap_bottle_url("shivammathur/extensions", bottle.formula, &digest),
            checksum: Checksum::Sha256(digest),
            include: &["xdebug.so"],
        }],
    }
}

/// The bundle (name, version) whose cached tree holds `xdebug.so` for a PHP
/// minor — pass to [`resolve_bundle`]. The minor is baked into the NAME (the
/// .so is ABI-bound to it); the VERSION is that minor's Xdebug release, so a pin
/// bump busts the cache dir like every other binary — and a minor frozen at an
/// older release keeps its own dir instead of colliding with the current one.
pub fn xdebug_bundle_id(minor: &str) -> Option<(String, &'static str)> {
    xdebug_bottle(minor).map(|b| (format!("xdebug-{minor}"), b.version))
}

/// Caddy uses `mac_arm64`/`mac_amd64`; static-php uses `macos-aarch64`/`macos-x86_64`.
fn caddy_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "amd64",
    }
}
fn php_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "aarch64",
        Arch::X86_64 => "x86_64",
    }
}
fn nginx_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "x86_64",
    }
}
fn mysql_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "x86_64",
    }
}
fn frankenphp_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "x86_64",
    }
}
fn postgres_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "aarch64",
        Arch::X86_64 => "x86_64",
    }
}
fn mailpit_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "amd64",
    }
}
fn cloudflared_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "amd64",
    }
}

fn pick(arch: Arch, arm: &str, amd: &str) -> String {
    match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    }
    .to_string()
}

/// Where every artifact rexenv BUILDS is published: releases in the public
/// `rexenv/runtimes` repo.
///
/// One const rather than two format strings, so the bulk builds and the Xdebug
/// debug build cannot drift onto different hosts — which is exactly what had
/// happened (`PHP_DEBUG_TAG`). It is a prefix of `SELF_DISTRIBUTED_HOSTS`'
/// `https://github.com/rexenv/` entry, so anything built from it carries the
/// licence obligation by construction rather than by remembering to.
const RUNTIMES_RELEASE_BASE: &str = "https://github.com/rexenv/runtimes/releases/download";

/// The rexenv-hosted release TAG serving a self-built PHP artifact, or `None`
/// for a version that comes from static-php.dev.
///
/// Some PHP versions have **no upstream portable build at all** — static-php.dev
/// publishes 8.x only, and 7.4 is not published anywhere (verified: every
/// `dl.static-php.dev/.../php-7.4.3*` URL 404s). Those are built by
/// `rexenv/runtimes` CI and hosted as GitHub Release assets
/// (`docs/PLAN-php-74-support.md` §2/§6).
///
/// **The FULL tag lives here and goes into the pinned URL**, rather than a stable
/// base + a separate version const. Both of rexenv's existing upstreams —
/// static-php.dev and FrankenPHP — REBUILD release assets in place, which is why
/// this file carries two long comments about pins going stale under a stable URL.
/// Our own repo is the chance to make a pin permanent, and it only works if the
/// URL names one immutable release: a rebuild is a NEW tag (`-2`), never a
/// re-upload, so a pin can 404 but can never silently change bytes.
fn php_self_hosted_tag(version: &str) -> Option<&'static str> {
    match version {
        // -6. Every earlier build is still published and still immutable; that
        // is the contract working, not a mess. -1 had no `phar` (WP-CLI and
        // Composer ARE phars, so no WordPress action could run), -2 had a PCRE
        // JIT that cannot allocate on Apple Silicon, and -4 was short 5 of the
        // extensions the 8.x rows carry.
        "7.4.33" => Some("php-7.4.33-6"),
        _ => None,
    }
}

/// The verified manifest catalog this process is running with.
///
/// A [`VersionCatalog`] can only be built by `updates::verify`, and its field is
/// private — so the TYPE is the proof that whatever is in here had a valid
/// signature over it. There is no way to install unverified entries, which is
/// what makes a process-global acceptable for something this load-bearing.
///
/// Empty until `install_catalog` runs, and empty is the whole install base: the
/// app resolves exactly what its compiled-in pins say.
static CATALOG: std::sync::RwLock<Option<crate::core::updates::VersionCatalog>> =
    std::sync::RwLock::new(None);

/// Publish a verified catalog to the resolve path. Called after a refresh and at
/// launch from the re-verified cache.
/// Serialises every test that INSTALLS a catalog.
///
/// `CATALOG` is process-global and `cargo test` runs in parallel, so two tests
/// that install different catalogs interleave — and the one asserting "no
/// catalog, so this version does not resolve" fails against the other's fixture.
/// It bit on the first Adminer test, and the only reason it had not bitten
/// before is that nothing asserted the EMPTY case for PHP.
///
/// Poison-tolerant on purpose: a panicking test must not turn every later
/// catalog test red for a reason that has nothing to do with them.
#[cfg(test)]
pub(crate) fn catalog_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let m = LOCK.get_or_init(|| std::sync::Mutex::new(()));
    m.lock().unwrap_or_else(|p| {
        m.clear_poison();
        p.into_inner()
    })
}

pub fn install_catalog(cat: crate::core::updates::VersionCatalog) {
    if let Ok(mut w) = CATALOG.write() {
        *w = Some(cat);
    }
}

/// The BinarySpec for a PHP artifact: the compiled-in pin FIRST, then the
/// verified catalog.
///
/// Order is load-bearing. A compiled-in pin can never be overridden by a
/// manifest — so a signed document, however valid, cannot move a version the app
/// already knows onto different bytes. The catalog may only ADD versions the app
/// was built before, which is the whole property `docs/PLAN-binary-updates.md`
/// §2 calls "the compiled-in pins remain the floor".
fn php_spec(kind: &str, version: &str, arch: Arch) -> Option<BinarySpec> {
    if let Some(hex) = php_sha256(kind, version, arch) {
        return Some(BinarySpec {
            url: php_url(kind, version, arch),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarGz,
            member: if kind == "fpm" { "php-fpm" } else { "php" },
        });
    }
    // Not pinned: ask the verified catalog. `name` is asserted against the
    // manifest's own allowlist HERE rather than trusting `updates` to have
    // filtered — the guard belongs where the value is used.
    let name = if kind == "fpm" { "php-fpm" } else { "php" };
    if !crate::core::updates::nameable(name) {
        return None;
    }
    let arch_s = crate::core::updates::catalog_arch(arch);
    let guard = CATALOG.read().ok()?;
    let a = guard.as_ref()?.artifact(name, version, arch_s)?;
    Some(BinarySpec {
        url: a.url.clone(),
        checksum: Checksum::Sha256(a.sha256.clone()),
        archive: Archive::TarGz,
        member: if kind == "fpm" { "php-fpm" } else { "php" },
    })
}

/// The pinned Adminer build, else the verified catalog — the same precedence as
/// [`php_spec`], for the same reason.
///
/// A compiled-in pin can never be overridden by a manifest: a signed document,
/// however valid, cannot move a version the app already knows onto different
/// bytes. The catalog may only ADD versions the app was built before.
///
/// **`archive` and `member` are derived from the NAME here, never carried by the
/// document.** A manifest that could choose them would be choosing WHICH
/// EXTRACTOR RUNS — a strictly larger grant than choosing bytes, and it would
/// reach the tar path with an attacker-chosen member name. `php_spec` hardcodes
/// its pair for the same reason.
fn adminer_spec(version: &str) -> Option<BinarySpec> {
    // One file, identical on every machine — see `updates::ANY_ARCH`.
    if version == ADMINER_VERSION {
        return Some(BinarySpec {
            url: format!(
                "https://github.com/vrana/adminer/releases/download/v{version}/adminer-{version}-en.php"
            ),
            checksum: Checksum::Sha256(ADMINER_5_4_2_SHA256.to_string()),
            archive: Archive::Raw,
            member: "adminer.php",
        });
    }
    // Not pinned: ask the verified catalog. The name is asserted against the
    // manifest's own allowlist HERE rather than trusting `updates` to have
    // filtered — the guard belongs where the value is used.
    if !crate::core::updates::nameable("adminer") {
        return None;
    }
    let guard = CATALOG.read().ok()?;
    let a = guard.as_ref()?.artifact("adminer", version, crate::core::updates::ANY_ARCH)?;
    Some(BinarySpec {
        url: a.url.clone(),
        checksum: Checksum::Sha256(a.sha256.clone()),
        archive: Archive::Raw,
        member: "adminer.php",
    })
}

/// The download URL for a pinned PHP build, from whichever source publishes it.
///
/// **This branch must exist BEFORE any checksum is pinned.** `manifest`'s PHP arms
/// gate only on `php_sha256(..).is_some()` and used to hardcode the static-php.dev
/// template, so filling in 7.4's four hashes first would have produced a manifest
/// that resolves to a permanent 404 — and `manifest_pins_every_pinned_php_version`
/// would have stayed green, because it asserts the URL's SHAPE. The failure would
/// have surfaced only on a user's machine, as "php-fpm 7.4 download failed".
fn php_url(kind: &str, version: &str, arch: Arch) -> String {
    let arch = php_arch(arch);
    match php_self_hosted_tag(version) {
        Some(tag) => format!(
            "{RUNTIMES_RELEASE_BASE}/{tag}/php-{version}-{kind}-macos-{arch}.tar.gz"
        ),
        None => format!(
            "https://dl.static-php.dev/static-php-cli/bulk/php-{version}-{kind}-macos-{arch}.tar.gz"
        ),
    }
}

// Self-built 7.4 artifacts — OURS (rexenv/runtimes). The live release tag is
// `php_self_hosted_tag` above, NOT this comment: it read `-4` while the code
// said `-6`, because a re-pin edits the consts and the tag and leaves the prose.
// Read the tag, never a sentence about it.
//
// **Re-pinned 15 Aug 2026 after -1 shipped broken.** It had no `phar`, and both
// WP-CLI and Composer ARE phars run through the SITE's PHP, so every WordPress
// action on a 7.4 site died with `Class 'Phar' not found` — while `php -v`,
// `php -m` and the whole build gate looked healthy. -2 fixed that and exposed
// the next one: PHP 7.4 bundles PCRE2 10.35 (May 2020), too old for Apple
// Silicon JIT, so Composer died on `Allocation of JIT memory failed`; -4 builds
// `--without-pcre-jit`. Both failures were invisible to CI — the runner's OS
// permits the allocation and a developer's Mac does not — which is why the
// pin ceremony below now RUNS the tools locally instead of trusting a green
// build. `examples/php_tools_check` makes that permanent.
//
// Pinned from the published release: each file downloaded over the
// real `releases/download` URL rexenv itself uses, hashed here, and
// cross-checked against the release's own SHA256SUMS. The arm64 cli was then
// EXTRACTED AND RUN — `PHP 7.4.33 (cli)`, `mysqli=1 gd=1 intl=1`, and an
// `otool -L` closure of /usr/lib + /System only, which is what
// `relink_to_system_libs` will accept on a user's machine.
//
// Unlike static-php.dev and FrankenPHP, these bytes CANNOT change under the
// URL: the release is immutable and the tag is never reused (a rebuild is
// `php-7.4.33-2`). A pin here can 404; it can never drift.
/// The upstream source commit the pinned 7.4 artifacts were built FROM.
///
/// PHP 7.4 is EOL, so there is no php.net release to name: the build takes
/// `shivammathur/php-src-backports` (vanilla 7.4.33 does not compile against
/// OpenSSL 3.6). `docs/PLAN-php-74-support.md` §11 states the risk this const
/// answers in its own words — **"the backports branch is one volunteer's rebased
/// branch; if it stops, the artifact quietly becomes a frozen, known-vulnerable
/// PHP"** — and a rebased branch is the case where a branch NAME is worth
/// nothing: it is rewritten, not appended, so `PHP-7.4-security-backports` today
/// and the same name in a year are different code with no record of the
/// difference. A commit is the only durable answer to "which 7.4 is this?".
///
/// It was not unrecorded — `THIRD-PARTY-NOTICES.md` has carried it since 7.4
/// shipped — but it was unrecorded HERE, next to the digests it explains, so the
/// answer required leaving the tree. The two copies now agree by test rather
/// than by memory (`the_pinned_74_names_the_source_it_was_built_from`), which
/// matters because the notices file is the one a licence auditor reads and this
/// file is the one that decides what gets downloaded.
///
/// Short hash as published in the release. The mirrored source tarball is an
/// asset of the SAME immutable release as the binaries, so the build is
/// reproducible from URLs rather than from a branch that may have moved.
pub const PHP_7_4_33_SOURCE_COMMIT: &str = "5a576d8eb53e";

const PHP_7_4_33_CLI_MAC_ARM64_SHA256: &str = "7fac111fda4e549b136da008fb8f7568c9ea32b96e67cc5fb18e1e431569ed22";
const PHP_7_4_33_CLI_MAC_AMD64_SHA256: &str = "f6878248da0b9e119d29ec21fbe73e8c6c1bfdc34b2d23250a3783b9c04b5598";
const PHP_7_4_33_FPM_MAC_ARM64_SHA256: &str = "3f32e75738c66642c64b8d817680a872519007cf3903ad5824fa01c04be3fec9";
const PHP_7_4_33_FPM_MAC_AMD64_SHA256: &str = "fc75852c08304d5c92ccb5c6f0e13f70719fda6262d6a2e798af8d4b86b2bfc4";

// ── The licence texts that travel with a PHP we BUILT ────────────────────────
//
// Pinned the same way and from the same release as the binaries above: each
// downloaded over the real `releases/download` URL rexenv itself uses, hashed
// locally, and cross-checked against the release's own SHA256SUMS (16 entries
// per arch, identical file lists, `PHP-3.01.txt` among them).
const PHP_7_4_33_LICENSES_MAC_ARM64_SHA256: &str = "d8fd80a258f1d8e6609d3e0e95a3b62e5c30820a3dba8e0390c78e4494491478";
const PHP_7_4_33_LICENSES_MAC_AMD64_SHA256: &str = "fa1ae808cb2febdb01e2df2975b0618dba4c0caff39f51161328ee97c2b60ba2";

/// Directory inside a published cache dir holding the artifact's licence texts.
/// Matches the tarball's own top-level dir, so extraction is `strip = 0`.
pub const LICENSES_DIR: &str = "licenses";

/// The licence-text download for an artifact **rexenv is the distributor of**,
/// or `None` when somebody else distributes it.
///
/// # Why this exists as a manifest arm rather than a line in a notices file
///
/// rexenv fetches every other server binary from the party that built it, and
/// carries no obligation for those. PHP 7.4 is the exception: nobody publishes a
/// portable 7.4, so `rexenv/runtimes` builds it and rexenv ships those bytes to
/// users. That makes rexenv a distributor, and PHP License 3.01 §2 asks for the
/// notice in "the documentation and/or other materials provided with the
/// distribution" — plus the licences of everything statically linked in, which
/// travel inside the Mach-O whether or not anybody names them.
///
/// `THIRD-PARTY-NOTICES.md` reproduces them, and that is defensible. It is also
/// an ARGUMENT, and a licence obligation is the last place to hold a position
/// that needs defending. The texts ship beside the bytes instead, which is not
/// arguable, and the cost is this function plus one fetch.
///
/// **Keyed on `php_self_hosted_tag`, never on the string "7.4"** — so a second
/// self-built runtime inherits the obligation by existing, and if the day comes
/// that 7.4 is published upstream and the tag goes away, this goes quiet on its
/// own. The rule is "we built it, so its licences travel with it".
/// The hosts that serve artifacts **rexenv itself built**. Bytes downloaded from
/// one of these are ours to distribute, and the licence obligation attaches.
///
/// Deliberately narrow prefixes, not a bare domain: `github.com` also serves
/// nginx (jirutka), Caddy, cloudflared, WP-CLI, Adminer and PHP upstream, and
/// none of those are ours. Only `github.com/rexenv/` is.
const SELF_DISTRIBUTED_HOSTS: &[&str] = &["https://github.com/rexenv/", "https://dl.rexenv.dev/"];

/// Whether rexenv is the DISTRIBUTOR of the artifact at `url` — it built the
/// bytes and hosts them — and therefore owes the licence texts beside them.
///
/// The one question that decides the obligation, in one place, so the answer
/// cannot be given differently by a manifest arm, a cache check and a doc.
///
/// # Why this reads the HOST and not the name or the version
///
/// It used to be `(name == "php" || name == "php-fpm") && php_self_hosted_tag(version)`,
/// carrying a comment that a second self-built runtime would "inherit the
/// obligation by existing". **It would not, and the counter-example was already
/// in this file**: `php-debug` / `php-fpm-debug` are self-built (a custom
/// static-php compile — `docs/xdebug-debug-build.md`) and self-hosted under
/// `RUNTIMES_RELEASE_BASE`, and the name check excluded both. The day their digests
/// are pinned they would have shipped with no licences and nothing would have
/// said so. Same family as the four guards in the ledger that claimed a whole
/// surface and checked one place inside it.
///
/// A host cannot be forgotten the way a list entry can: to escape this rule an
/// artifact has to stop being served from our own infrastructure, at which point
/// we are genuinely not the distributor. That is the property worth having, and
/// it is worth more than the shorter expression it replaced.
pub fn is_self_distributed(url: &str) -> bool {
    SELF_DISTRIBUTED_HOSTS.iter().any(|h| url.starts_with(h))
}

/// [`is_self_distributed`] for a NAMED artifact, resolving its pinned URL first.
///
/// The shape callers outside this module need (`examples/php_versions_check`),
/// so a live check cannot answer the ownership question differently from the
/// resolve that acts on it. An unresolvable artifact is not ours to distribute,
/// because it is not distributed at all.
pub fn artifact_is_self_distributed(name: &str, version: &str, arch: Arch) -> bool {
    manifest(name, version, "macos", arch).is_some_and(|s| is_self_distributed(&s.url))
}

/// A sibling of `url` in the same directory — same release, by construction.
fn sibling_url(url: &str, file: &str) -> Option<String> {
    let cut = url.rfind('/')?;
    Some(format!("{}/{file}", &url[..cut]))
}

/// The licence-text download that must ride alongside the artifact at `url`.
///
/// `Ok(None)` means nothing is owed — the ordinary case, every binary somebody
/// else distributes. `Err` means **we owe licences and cannot name them**, which
/// is a bug rather than a default: it means shipping somebody's code without its
/// licence, so it refuses to resolve instead of resolving without them.
///
/// The URL is derived as a SIBLING of the artifact's own URL, so the licences
/// necessarily come from the same immutable release as the bytes they cover —
/// previously a property asserted in prose beside a separately-formatted URL.
/// Only the digest stays a pin, because a digest cannot be derived.
fn licenses_spec(url: &str, name: &str, version: &str, arch: Arch) -> Result<Option<BinarySpec>> {
    if !is_self_distributed(url) {
        return Ok(None);
    }
    let (arm, amd) = match version {
        "7.4.33" => (
            PHP_7_4_33_LICENSES_MAC_ARM64_SHA256,
            PHP_7_4_33_LICENSES_MAC_AMD64_SHA256,
        ),
        _ => ("", ""),
    };
    let hex = match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    };
    let missing = || {
        Error::Other(format!(
            "{name} {version} is served from rexenv's own infrastructure ({url}) — rexenv is \
             its distributor and PHP License 3.01 §2 attaches — but no licence archive is \
             pinned for it. Pin `licenses-{}.tar.gz` from the same release, or serve the \
             artifact from whoever built it.",
            php_arch(arch)
        ))
    };
    if hex.is_empty() {
        return Err(missing());
    }
    Ok(Some(BinarySpec {
        url: sibling_url(url, &format!("licenses-{}.tar.gz", php_arch(arch)))
            .ok_or_else(missing)?,
        checksum: Checksum::Sha256(hex.to_string()),
        archive: Archive::TarGzTree,
        member: LICENSES_DIR,
    }))
}

/// Whether a published cache dir carries the licence texts it owes.
///
/// `true` when nothing is owed — the ordinary case, every binary somebody else
/// distributes. When something IS owed, an absent or empty `licenses/` makes the
/// dir stale, which costs one re-fetch on machines that cached 7.4 before this
/// shipped. That asymmetry is the `.rexenv-prepared` receipt's (S0.3, #331): a
/// pin marker cannot see a directory that was never fetched, and "the binary is
/// the right bytes" was true of those caches — they are missing something the
/// pin never described. One refetch is the price of repairing the field, and
/// the field here is a licence obligation rather than a broken dylib.
/// Keyed on the artifact's HOST, so an artifact we owe licences for but have no
/// licence pin for reads as UNSATISFIED rather than as nothing-owed. That is the
/// fail direction that matters: the old form asked `php_licenses_spec(..).is_none()`,
/// which answered "nothing owed" for both "somebody else built it" and "we built
/// it and forgot to pin its licences" — one of which is a licence violation.
fn licenses_satisfied(dir: &Path, url: &str) -> bool {
    if !is_self_distributed(url) {
        return true;
    }
    std::fs::read_dir(dir.join(LICENSES_DIR)).is_ok_and(|mut d| d.next().is_some())
}

/// Pinned SHA-256 for a PHP artifact, or `None` if the version isn't pinned.
/// `kind` is `"cli"` or `"fpm"`. Both arches are pinned together, so a `Some` for
/// one arch implies a `Some` for the other. An EMPTY const is treated as unpinned
/// — that is what keeps a self-built version wired but unresolvable until its
/// artifact actually exists.
fn php_sha256(kind: &str, version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match (kind, version) {
        ("cli", "7.4.33") => (PHP_7_4_33_CLI_MAC_ARM64_SHA256, PHP_7_4_33_CLI_MAC_AMD64_SHA256),
        ("fpm", "7.4.33") => (PHP_7_4_33_FPM_MAC_ARM64_SHA256, PHP_7_4_33_FPM_MAC_AMD64_SHA256),
        ("cli", "8.0.30") => (PHP_8_0_30_CLI_MAC_ARM64_SHA256, PHP_8_0_30_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.0.30") => (PHP_8_0_30_FPM_MAC_ARM64_SHA256, PHP_8_0_30_FPM_MAC_AMD64_SHA256),
        ("cli", "8.1.34") => (PHP_8_1_34_CLI_MAC_ARM64_SHA256, PHP_8_1_34_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.1.34") => (PHP_8_1_34_FPM_MAC_ARM64_SHA256, PHP_8_1_34_FPM_MAC_AMD64_SHA256),
        ("cli", "8.2.31") => (PHP_8_2_31_CLI_MAC_ARM64_SHA256, PHP_8_2_31_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.2.31") => (PHP_8_2_31_FPM_MAC_ARM64_SHA256, PHP_8_2_31_FPM_MAC_AMD64_SHA256),
        ("cli", "8.3.31") => (PHP_8_3_31_CLI_MAC_ARM64_SHA256, PHP_8_3_31_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.3.31") => (PHP_8_3_31_FPM_MAC_ARM64_SHA256, PHP_8_3_31_FPM_MAC_AMD64_SHA256),
        ("cli", "8.4.23") => (PHP_8_4_23_CLI_MAC_ARM64_SHA256, PHP_8_4_23_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.4.23") => (PHP_8_4_23_FPM_MAC_ARM64_SHA256, PHP_8_4_23_FPM_MAC_AMD64_SHA256),
        ("cli", "8.5.8") => (PHP_8_5_8_CLI_MAC_ARM64_SHA256, PHP_8_5_8_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.5.8") => (PHP_8_5_8_FPM_MAC_ARM64_SHA256, PHP_8_5_8_FPM_MAC_AMD64_SHA256),
        _ => return None,
    };
    let v = match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    };
    (!v.is_empty()).then_some(v)
}

// Debug build (Xdebug compiled in) SHA-256 — EMPTY until the artifact is built
// (docs/xdebug-debug-build.md) and self-hosted; an empty const keeps the variant
// unresolvable so we never try to fetch a non-existent file (§11.2).
const PHP_DEBUG_CLI_MAC_ARM64_SHA256: &str = "";
const PHP_DEBUG_CLI_MAC_AMD64_SHA256: &str = "";
const PHP_DEBUG_FPM_MAC_ARM64_SHA256: &str = "";
const PHP_DEBUG_FPM_MAC_AMD64_SHA256: &str = "";

/// Pinned SHA-256 for a debug (Xdebug) artifact, or `None` while unpinned (empty
/// const = not yet built+hosted). `kind` is `"cli"` or `"fpm"`.
fn php_debug_sha256(kind: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match kind {
        "cli" => (PHP_DEBUG_CLI_MAC_ARM64_SHA256, PHP_DEBUG_CLI_MAC_AMD64_SHA256),
        "fpm" => (PHP_DEBUG_FPM_MAC_ARM64_SHA256, PHP_DEBUG_FPM_MAC_AMD64_SHA256),
        _ => return None,
    };
    let v = match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    };
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// The URL a hosted debug artifact HAS, given the release tag serving it.
///
/// Separate from [`php_debug_spec`] because the shape is knowable now and the
/// tag is not: this is what lets the licence guards and the URL-shape test keep
/// a real subject while `PHP_DEBUG_TAG` is still empty. Before, they asserted
/// against a `dl.rexenv.dev` URL no artifact would ever be uploaded to.
fn php_debug_url(tag: &str, kind: &str, arch: Arch) -> String {
    format!(
        "{RUNTIMES_RELEASE_BASE}/{tag}/php-{PHP_DEBUG_VERSION}-{kind}-xdebug-macos-{}.tar.gz",
        php_arch(arch)
    )
}

/// The download spec for a debug (Xdebug) build artifact, or `None` while the
/// artifact is not hosted. Pure — same archive shape as a bulk build, tagged
/// `-xdebug`, from the same `rexenv/runtimes` releases as the 7.4 builds.
///
/// **Both halves gate, and that is the fix, not tidiness.** Hosting a debug
/// build takes two edits — the tag and the four digests — and the resolve path
/// used to read only the digests. Pin the digests first (which is step 2 of the
/// recipe, and the natural order: you hash the files you just uploaded) and
/// `manifest` would have started handing out URLs built from a host the ruling
/// deleted a week earlier. Requiring the tag makes the half-done state
/// unresolvable instead of wrong.
fn php_debug_spec(kind: &str, arch: Arch) -> Option<BinarySpec> {
    php_debug_spec_from(PHP_DEBUG_TAG, php_debug_sha256(kind, arch), kind, arch)
}

/// The gate itself, with both halves as PARAMETERS.
///
/// Split out so the half-done states are reachable from a test. They are not
/// reachable through [`php_debug_spec`]: both consts are empty today, so a test
/// calling it cannot tell "the tag gate works" from "the digest gate works" —
/// deleting the tag check would leave every assertion still passing. That is the
/// vacuous-green shape, and a guard against a two-edit mistake is worthless if
/// the only state it is ever exercised in is zero-edits.
fn php_debug_spec_from(
    tag: &str,
    sum: Option<&str>,
    kind: &str,
    arch: Arch,
) -> Option<BinarySpec> {
    if tag.is_empty() {
        return None;
    }
    Some(BinarySpec {
        url: php_debug_url(tag, kind, arch),
        checksum: Checksum::Sha256(sum?.to_string()),
        archive: Archive::TarGz,
        member: if kind == "fpm" { "php-fpm" } else { "php" },
    })
}

/// File recording WHICH pinned bytes a published cache dir was built from.
///
/// The cache is keyed `<name>-<version>`, and `resolve` returns early when the
/// binary simply EXISTS — so when a pin's BYTES change while its version string
/// does not, every machine that already downloaded the old artifact keeps it
/// forever. That is not hypothetical: rexenv's own PHP 7.4.33 shipped without
/// `phar`, was rebuilt at the same version, and the fixed artifact could not
/// reach anyone who had installed the broken one. Upstream has the same shape —
/// static-php.dev and FrankenPHP both rebuild release assets in place.
///
/// The extracted binary cannot be hashed to detect this: the pin is the
/// ARCHIVE's digest, not the member's. So the digest we verified at download
/// time is recorded beside the result, and compared as a string afterwards —
/// no re-hashing, no cost on the warm path.
const PIN_MARKER: &str = ".pinned-digest";

/// Whether a published cache dir holds the bytes `checksum` names.
///
/// A MISSING marker is only treated as stale for artifacts rexenv hosts itself
/// (`php_self_hosted_tag`), and that asymmetry is deliberate: those are exactly
/// the pins whose bytes can legitimately be re-issued at an unchanged version,
/// so they are the ones worth re-fetching once. Making an absent marker stale
/// for EVERYTHING would re-download every binary on every existing install to
/// catch a case upstream has not yet caused.
fn cache_matches_pin(dir: &Path, version: &str, checksum: &Checksum) -> bool {
    match std::fs::read_to_string(dir.join(PIN_MARKER)) {
        Ok(recorded) => recorded.trim() == checksum_hex(checksum),
        Err(_) => php_self_hosted_tag(version).is_none(),
    }
}

/// Record the digest a published dir was built from. Best-effort: a marker we
/// fail to write reads as "unknown", which is the same state as before it
/// existed — never a reason to fail a download that otherwise succeeded.
fn write_pin_marker(dir: &Path, checksum: &Checksum) {
    let _ = std::fs::write(dir.join(PIN_MARKER), checksum_hex(checksum));
}

/// The prepare receipt: a published BUNDLE tree records which revision of the
/// prepare logic (relink + verify + re-sign) built it (S0.3, neighbour of
/// #86/#328). `resolve_bundle`'s early return used to stat only the member
/// file — but "the file exists" cannot see "dyld refuses to load it", so a
/// tree published by a prepare whose VERIFY was wrong (#319: an escaping
/// `@loader_path` waved through over 49 Mach-Os) was cached forever: never
/// re-downloaded, never repaired, unrecoverable in the field short of
/// deleting the cache by hand.
const PREPARE_RECEIPT: &str = ".rexenv-prepared";

/// Bump this when a prepare-logic bug that could have PUBLISHED a broken tree
/// is fixed — the bump is what makes existing caches stale, so the fix
/// actually reaches machines that already hold the broken output. Rev 1 =
/// the post-#319 relink predicate (resolved `@loader_path`, escape-refusing).
const PREPARE_REV: &str = "1";

/// Whether a published bundle tree carries a CURRENT prepare receipt.
///
/// An ABSENT receipt is stale — deliberately the opposite asymmetry from
/// [`cache_matches_pin`]'s marker, and for the same kind of reason: every
/// pre-receipt tree was prepared by logic that includes the #319 predicate
/// era, and a broken tree is indistinguishable from a good one by any stat.
/// The one-time cost is re-downloading each cached bundle once; the
/// alternative is a field machine serving a tree dyld refuses, forever.
fn bundle_prepared_current(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(PREPARE_RECEIPT))
        .map(|s| s.trim() == PREPARE_REV)
        .unwrap_or(false)
}

/// Stamp the receipt into a STAGING tree that `prepare_binary_tree` just
/// finished (and whose member was verified present) — written before the
/// atomic publish, so a published tree either carries it or predates it.
fn write_prepare_receipt(staging: &Path) -> Result<()> {
    std::fs::write(staging.join(PREPARE_RECEIPT), PREPARE_REV)?;
    Ok(())
}

/// Look up the download spec for `name`@`version` on `os`+`arch`, or `None` if
/// unknown. `php` resolves the CLI build; `php-fpm` the FPM build.
pub fn manifest(name: &str, version: &str, os: &str, arch: Arch) -> Option<BinarySpec> {
    match (name, os, version) {
        ("caddy", "macos", "2.11.4") => Some(BinarySpec {
            url: format!(
                "https://github.com/caddyserver/caddy/releases/download/v{version}/caddy_{version}_mac_{}.tar.gz",
                caddy_arch(arch)
            ),
            checksum: Checksum::Sha512(pick(
                arch,
                CADDY_2_11_4_MAC_ARM64_SHA512,
                CADDY_2_11_4_MAC_AMD64_SHA512,
            )),
            archive: Archive::TarGz,
            member: "caddy",
        }),
        // PHP is version-driven: any version pinned in `php_sha256` resolves. The
        // SOURCE is `php_url`'s job — most versions come from static-php.dev, the
        // ones nobody publishes are built and hosted by us — so a version can
        // never resolve to a URL its artifact was never uploaded to.
        ("php", "macos", v) => php_spec("cli", v, arch),
        ("php-fpm", "macos", v) => php_spec("fpm", v, arch),
        // Debug builds (Xdebug compiled in) for the §8.2 debug pool. Only resolve
        // once the self-built artifact is hosted — BOTH its release tag and its
        // checksum pinned (§11.2). The gate lives in `php_debug_spec` rather than
        // here so the two conditions cannot be satisfied one at a time.
        ("php-debug", "macos", v) if v == PHP_DEBUG_VERSION => php_debug_spec("cli", arch),
        ("php-fpm-debug", "macos", v) if v == PHP_DEBUG_VERSION => php_debug_spec("fpm", arch),
        ("nginx", "macos", "1.30.3") => Some(BinarySpec {
            // jirutka/nginx-binaries ships a single static binary (not an archive).
            url: format!(
                "https://jirutka.github.io/nginx-binaries/nginx-{version}-{}-darwin",
                nginx_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                NGINX_1_30_3_MAC_ARM64_SHA256,
                NGINX_1_30_3_MAC_AMD64_SHA256,
            )),
            archive: Archive::Raw,
            member: "nginx",
        }),
        // MySQL is version-driven like PHP: any version pinned in `mysql_sha256`
        // resolves (the CDN URL is templated per series).
        ("mysql", "macos", v) if mysql_sha256(v, arch).is_some() => Some(BinarySpec {
            // Direct CDN URL (the dev.mysql.com/get redirector 403s non-curl clients).
            url: format!(
                "https://cdn.mysql.com/archives/mysql-{series}/mysql-{v}-macos15-{}.tar.gz",
                mysql_arch(arch),
                series = mysql_series(v),
            ),
            checksum: Checksum::Sha256(mysql_sha256(v, arch).unwrap().to_string()),
            archive: Archive::TarGzTree,
            member: "bin/mysqld", // primary binary within the extracted tree
        }),
        ("postgres", "macos", v) if postgres_sha256(v, arch).is_some() => Some(BinarySpec {
            // theseus-rs portable PostgreSQL — a bin/lib/share tree (one top dir).
            url: format!(
                "https://github.com/theseus-rs/postgresql-binaries/releases/download/{v}/postgresql-{v}-{}-apple-darwin.tar.gz",
                postgres_arch(arch)
            ),
            checksum: Checksum::Sha256(postgres_sha256(v, arch).unwrap().to_string()),
            archive: Archive::TarGzTree,
            member: "bin/postgres", // primary binary within the extracted tree
        }),
        ("frankenphp", "macos", "1.12.4") => Some(BinarySpec {
            // One static binary per arch (raw, not an archive).
            url: format!(
                "https://github.com/php/frankenphp/releases/download/v{version}/frankenphp-mac-{}",
                frankenphp_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                FRANKENPHP_1_12_4_MAC_ARM64_SHA256,
                FRANKENPHP_1_12_4_MAC_AMD64_SHA256,
            )),
            archive: Archive::Raw,
            member: "frankenphp",
        }),
        ("mailpit", "macos", "1.30.3") => Some(BinarySpec {
            // One static Go binary per arch, inside a tar.gz (member `mailpit`).
            url: format!(
                "https://github.com/axllent/mailpit/releases/download/v{version}/mailpit-darwin-{}.tar.gz",
                mailpit_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                MAILPIT_1_30_3_MAC_ARM64_SHA256,
                MAILPIT_1_30_3_MAC_AMD64_SHA256,
            )),
            archive: Archive::TarGz,
            member: "mailpit",
        }),
        ("cloudflared", "macos", "2026.6.1") => Some(BinarySpec {
            // One static Go binary per arch, inside a .tgz (member `cloudflared`).
            url: format!(
                "https://github.com/cloudflare/cloudflared/releases/download/{version}/cloudflared-darwin-{}.tgz",
                cloudflared_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                CLOUDFLARED_2026_6_1_MAC_ARM64_SHA256,
                CLOUDFLARED_2026_6_1_MAC_AMD64_SHA256,
            )),
            archive: Archive::TarGz,
            member: "cloudflared",
        }),
        // Adminer is a single PHP file (run via the bundled PHP), identical on every OS.
        ("adminer", _, v) => adminer_spec(v),
        // WP-CLI is a PHP .phar (run via the bundled PHP), identical on every OS.
        ("wp-cli", _, "2.12.0") => Some(BinarySpec {
            url: format!(
                "https://github.com/wp-cli/wp-cli/releases/download/v{version}/wp-cli-{version}.phar"
            ),
            checksum: Checksum::Sha256(WP_CLI_2_12_0_SHA256.to_string()),
            archive: Archive::Raw,
            member: "wp-cli.phar",
        }),
        // Composer, same model as WP-CLI: a .phar run via the SITE's bundled
        // PHP (core::repo composer step), identical on every OS.
        ("composer", _, "2.10.2") => Some(BinarySpec {
            url: format!("https://getcomposer.org/download/{version}/composer.phar"),
            checksum: Checksum::Sha256(COMPOSER_2_10_2_SHA256.to_string()),
            archive: Archive::Raw,
            member: "composer.phar",
        }),
        _ => None,
    }
}

/// Build one [`BundlePart`] from a formula + its per-arch bottle digests.
fn bottle_part(
    formula: &'static str,
    arch: Arch,
    arm_sha: &'static str,
    amd_sha: &'static str,
    include: &'static [&'static str],
) -> BundlePart {
    let digest = pick(arch, arm_sha, amd_sha);
    BundlePart {
        formula,
        url: bottle_url(formula, &digest),
        checksum: Checksum::Sha256(digest),
        include,
    }
}

/// The openssl@3 runtime dylibs part shared by every TLS-linking bundle —
/// never the static libs, headers, cmake/pkgconfig, or provider modules.
fn openssl_part(arch: Arch) -> BundlePart {
    bottle_part(
        "openssl@3",
        arch,
        OPENSSL_3_6_3_BOTTLE_ARM64_SHA256,
        OPENSSL_3_6_3_BOTTLE_AMD64_SHA256,
        &["lib/libssl.3.dylib", "lib/libcrypto.3.dylib"],
    )
}

/// Look up the BUNDLE spec for `name`@`version` on `os`+`arch` — services with
/// no portable static build, assembled from Homebrew bottles and relinked into
/// a self-contained tree (the shipped "Deferred services" plan —
/// docs/archive/SHIPPED-2026-07.md). Disjoint from
/// [`manifest`]: a name resolves through exactly one of the two.
pub fn bundle_manifest(name: &str, version: &str, os: &str, arch: Arch) -> Option<BundleSpec> {
    match (name, os, version) {
        ("redis", "macos", "8.8.0") => Some(BundleSpec {
            member: "bin/redis-server",
            parts: vec![
                bottle_part(
                    "redis",
                    arch,
                    REDIS_8_8_0_BOTTLE_ARM64_SHA256,
                    REDIS_8_8_0_BOTTLE_AMD64_SHA256,
                    &["bin"],
                ),
                openssl_part(arch),
            ],
        }),
        ("httpd", "macos", "2.4.68") => Some(BundleSpec {
            member: "bin/httpd",
            parts: vec![
                bottle_part(
                    "httpd",
                    arch,
                    HTTPD_2_4_68_BOTTLE_ARM64_SHA256,
                    HTTPD_2_4_68_BOTTLE_AMD64_SHA256,
                    // The server binary, ONLY the modules our generated conf
                    // loads (mod_ssl/mod_http2/mod_brotli would drag openssl/
                    // nghttp2/brotli into the closure), and the real mime map
                    // (bottles stage etc/ under `.bottle/`).
                    &[
                        "bin/httpd",
                        "lib/httpd/modules/mod_mpm_event.so",
                        "lib/httpd/modules/mod_unixd.so",
                        "lib/httpd/modules/mod_authz_core.so",
                        "lib/httpd/modules/mod_dir.so",
                        "lib/httpd/modules/mod_mime.so",
                        "lib/httpd/modules/mod_env.so",
                        "lib/httpd/modules/mod_rewrite.so",
                        "lib/httpd/modules/mod_proxy.so",
                        "lib/httpd/modules/mod_proxy_fcgi.so",
                        "lib/httpd/modules/mod_log_config.so",
                        ".bottle/etc/httpd/mime.types",
                    ],
                ),
                bottle_part(
                    "apr",
                    arch,
                    APR_1_7_6_BOTTLE_ARM64_SHA256,
                    APR_1_7_6_BOTTLE_AMD64_SHA256,
                    &["lib/libapr-1.0.dylib"],
                ),
                bottle_part(
                    "apr-util",
                    arch,
                    APR_UTIL_1_6_3_BOTTLE_ARM64_SHA256,
                    APR_UTIL_1_6_3_BOTTLE_AMD64_SHA256,
                    &["lib/libaprutil-1.0.dylib"],
                ),
                bottle_part(
                    "pcre2",
                    arch,
                    PCRE2_10_47_BOTTLE_ARM64_SHA256,
                    PCRE2_10_47_BOTTLE_AMD64_SHA256,
                    &["lib/libpcre2-8.0.dylib"],
                ),
            ],
        }),
        ("mariadb", "macos", "11.4.12") => Some(BundleSpec {
            member: "bin/mariadbd",
            parts: vec![
                bottle_part(
                    // Versioned LTS formula — ghcr path `mariadb/11.4`.
                    "mariadb@11.4",
                    arch,
                    MARIADB_11_4_12_BOTTLE_ARM64_SHA256,
                    MARIADB_11_4_12_BOTTLE_AMD64_SHA256,
                    // Identical layout to 12.x (verified from the bottle).
                    &[
                        "bin/mariadbd",
                        "bin/mariadb",
                        "bin/mariadb-dump",
                        "share/mysql/english",
                        "share/mysql/charsets",
                        "share/mysql/mariadb_system_tables.sql",
                        "share/mysql/mariadb_performance_tables.sql",
                        "share/mysql/mariadb_system_tables_data.sql",
                    ],
                ),
                openssl_part(arch),
                bottle_part(
                    "pcre2",
                    arch,
                    PCRE2_10_47_BOTTLE_ARM64_SHA256,
                    PCRE2_10_47_BOTTLE_AMD64_SHA256,
                    &["lib/libpcre2-8.0.dylib"],
                ),
            ],
        }),
        ("mariadb", "macos", "12.3.2") => Some(BundleSpec {
            member: "bin/mariadbd",
            parts: vec![
                bottle_part(
                    "mariadb",
                    arch,
                    MARIADB_12_3_2_BOTTLE_ARM64_SHA256,
                    MARIADB_12_3_2_BOTTLE_AMD64_SHA256,
                    // Server + the two clients the DB features need (bundled-
                    // client rule), the bootstrap SQL `core::mariadb::initialize`
                    // feeds over stdin, and the runtime share data (errmsg.sys,
                    // charsets). The 221MB bin/ full set, plugins (whose deps
                    // we don't bundle), scripts (baked brew paths), include/,
                    // and docs all stay out.
                    &[
                        "bin/mariadbd",
                        "bin/mariadb",
                        "bin/mariadb-dump",
                        "share/mysql/english",
                        "share/mysql/charsets",
                        "share/mysql/mariadb_system_tables.sql",
                        "share/mysql/mariadb_performance_tables.sql",
                        "share/mysql/mariadb_system_tables_data.sql",
                    ],
                ),
                openssl_part(arch),
                bottle_part(
                    "pcre2",
                    arch,
                    PCRE2_10_47_BOTTLE_ARM64_SHA256,
                    PCRE2_10_47_BOTTLE_AMD64_SHA256,
                    &["lib/libpcre2-8.0.dylib"],
                ),
            ],
        }),
        // Per-minor Xdebug .so (shivammathur/extensions tap, not homebrew/core):
        // a ONE-part bundle whose member is the bare `xdebug.so` — links only
        // system libSystem+libz, so the relink pass is a no-op and re-sign
        // applies as usual. Loaded into the SAME static php-fpm by the debug
        // pool (`core::php`), never a separate PHP build.
        // The version gate is THAT MINOR's pinned release, not one app-wide
        // constant: a minor past Xdebug's support window is frozen at its last
        // release, and gating on a single version would make its bundle
        // unresolvable rather than merely older.
        (n, "macos", v) if n.starts_with("xdebug-") => {
            let bottle = xdebug_bottle(n.strip_prefix("xdebug-")?)?;
            // Cache-dir identity is honest: only THIS minor's pinned release
            // resolves. Comparing against the row rather than one app-wide
            // constant is the whole point — see [`XdebugBottle::version`].
            (v == bottle.version).then(|| xdebug_spec(&bottle, arch))
        }
        _ => None,
    }
}

/// Resolve a multi-bottle dylib BUNDLE (see [`bundle_manifest`]) to its merged,
/// relinked, re-signed tree — the bundle counterpart of [`resolve_dir`], same
/// staging → prepare → atomic-publish shape (H4). Idempotent via the `member`
/// marker.
pub async fn resolve_bundle(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let _flight = in_flight(name, version).await;
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    let spec = bundle_manifest(name, version, os, arch)
        .ok_or_else(|| Error::Other(format!("no bundle manifest for {name} {version} on {os}")))?;

    let bin_dir = platform.paths().bin_dir()?;
    let dir = bin_dir.join(format!("{name}-{version}"));
    if dir.join(spec.member).exists() && bundle_prepared_current(&dir) {
        return Ok(dir);
    }
    // Present but without a current prepare receipt: the tree may be the
    // output of a prepare whose verify was wrong (#319 published trees dyld
    // refused, and a member-stat kept them forever). Drop and refetch — same
    // shape as `resolve`'s stale-pin path.
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }

    let id = downloads::item_id(name, version);
    downloads::hub().item_started(name, version);
    let staging = staging_path(&bin_dir, name, version);
    let staged: Result<()> = async {
        std::fs::create_dir_all(&staging)?;
        // Parts download sequentially under the one hub item (the bar restarts
        // per bottle — honest enough for a two-part bundle).
        for part in &spec.parts {
            let archive = staging.join(".bottle.tar.gz");
            download_with_headers(
                &part.url,
                &archive,
                Some(&part.checksum),
                Some(&id),
                GHCR_ANON_AUTH,
            )
            .await?;
            downloads::hub().item_preparing(&id);
            // Bottle layout is `<formula>/<version>/…` — strip both.
            extract_tar_gz_tree_filtered(
                open_buffered(&archive)?,
                &staging,
                2,
                Some(part.include),
            )
            .map_err(|e| Error::Other(format!("extract {} bottle: {e}", part.formula)))?;
            std::fs::remove_file(&archive)?;
        }
        // Relink every Mach-O to @loader_path + ad-hoc re-sign (LAST), so the
        // published tree is self-contained — no Homebrew install needed.
        platform.binaries().prepare_binary_tree(&staging)?;
        ensure_member_extracted(&staging, name, version, spec.member)?;
        refuse_self_distributed_bundle(&spec, name, version)?;
        write_prepare_receipt(&staging)?;
        publish(&staging, &dir, spec.member)
    }
    .await;
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    finish_item(&id, &staged);
    staged?;
    Ok(dir)
}

/// A bundle assembled from bytes rexenv itself hosts is REFUSED, loudly.
///
/// Every bundle today is Homebrew bottles from `ghcr.io` — somebody else's
/// build, so nothing is owed and this never fires. It is a tripwire for the day
/// that changes, and it refuses rather than fetching because **a `BundlePart`
/// carries no version**: there is nothing to key a licence digest on, so the
/// honest options are "refuse and say why" or "silently pin the wrong archive",
/// and the second is worse than the gap it papers over.
///
/// Whoever trips this is adding a self-hosted bundle part, and the message tells
/// them the actual work: give `BundlePart` a version, or publish the licences
/// under the merged tree's own name/version like the single-spec paths do.
fn refuse_self_distributed_bundle(spec: &BundleSpec, name: &str, version: &str) -> Result<()> {
    let Some(part) = spec.parts.iter().find(|p| is_self_distributed(&p.url)) else {
        return Ok(());
    };
    Err(Error::Other(format!(
        "bundle {name} {version} includes '{}', which rexenv builds and hosts ({}) — so rexenv \
         is its distributor and owes its licence texts. A BundlePart carries no version to pin a \
         licence archive against, so this refuses rather than publishing without them: give \
         BundlePart a version, or publish the licences under the merged tree's own name.",
        part.formula, part.url
    )))
}

/// A prepared bundle MUST contain its pinned `member` before we publish it — the
/// file the early-return marker (`dir.join(member)`) and the caller both depend
/// on. `extract_tar_gz_tree_filtered` silently skips anything outside its
/// `include` prefixes and never errors on an absent member, so a bad
/// `member`/`include` pin would otherwise publish an INCOMPLETE tree: the caller
/// spawns a missing binary (ENOENT), and because the marker is never satisfied
/// every future resolve re-downloads the whole bundle forever. Fail loud here
/// instead (B35) — mirrors `resolve`'s "member not found in archive".
fn ensure_member_extracted(staging: &Path, name: &str, version: &str, member: &str) -> Result<()> {
    if staging.join(member).exists() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "bundle {name} {version}: pinned member '{member}' is missing from the extracted \
             tree (bad member/include pin?)"
        )))
    }
}

/// Whether `name`@`version` is already fully published in the binary cache —
/// the same marker files the resolves' early-return checks use (a partially
/// published dir never has its marker: the staging→rename publish is atomic,
/// H4). Unknown manifest = not cached. Used by the download planner to split
/// an action's binary set into cached vs to-download without resolving.
/// Which resolver an artifact's distribution shape needs.
///
/// **The ONE place the name → resolver mapping lives.** It used to be a `match`
/// in `downloads::resolve_any` and, separately, a disjunction inside
/// `is_cached` — two answers to one question, and they disagreed: `composer` is
/// `Archive::Raw` with `member: "composer.phar"` and every consumer calls
/// [`resolve_file`], but `resolve_any` let it fall through to [`resolve`], which
/// publishes under `name`. So a prefetch cached `composer` and the consumer then
/// downloaded `composer.phar` into the same dir — twice the bytes, and
/// `prepare_binary` ad-hoc signing a PHP archive on the way past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A single executable, published at `dir/<name>` ([`resolve`]).
    Single,
    /// A plain file kept under its own name, no chmod/codesign ([`resolve_file`]).
    File,
    /// A directory distribution ([`resolve_dir`]).
    Dir,
    /// A merged bottle bundle ([`resolve_bundle`]).
    Bundle,
}

/// The distribution shape of `name` — see [`Shape`].
pub fn shape_of(name: &str) -> Shape {
    match name {
        "mysql" | "postgres" => Shape::Dir,
        "redis" | "mariadb" | "httpd" => Shape::Bundle,
        n if n.starts_with("xdebug-") => Shape::Bundle,
        "wp-cli" | "adminer" | "composer" => Shape::File,
        _ => Shape::Single,
    }
}

/// **The ONE cache predicate**: the path a resolve of `name`@`version` would
/// return *without downloading anything*, or `None` if it would fetch.
///
/// Every caller that wants to know "is this already here" asks this — the
/// planner ([`is_cached`]), the sync adoption path ([`cached_bin`]), and each
/// resolve's own fast path. They gave three different answers before:
///
/// - [`resolve`] required the pin marker AND the licence texts;
/// - [`is_cached`] tested existence only, so the planner reported "cached,
///   nothing to do" about a tree `resolve` was about to delete and re-fetch —
///   a silent ~100MB download with no hub row, which is exactly what an
///   upstream in-place rebuild plus a re-pin produces (it happened 5 Jul 2026);
/// - [`cached_bin`] tested existence only under a doc comment calling itself
///   "`resolve`'s cache-hit fast path", and fed mailpit/frankenphp adoption.
///
/// A predicate that three callers answer differently is not a predicate.
/// The path whose existence says "something was published here" — the member
/// each shape's resolve looks for, before any pin or licence question.
fn published_member(platform: &dyn Platform, name: &str, version: &str) -> Option<PathBuf> {
    let dir = platform.paths().bin_dir().ok()?.join(format!("{name}-{version}"));
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;
    Some(match shape_of(name) {
        Shape::Bundle => dir.join(bundle_manifest(name, version, os, arch)?.member),
        Shape::Single => dir.join(name),
        _ => dir.join(manifest(name, version, os, arch)?.member),
    })
}

/// Fetch the licence texts `spec`'s artifact owes into its STAGING dir, so they
/// ride the same atomic publish as the bytes they cover: a published artifact
/// either carries `licenses/` or does not exist.
///
/// A failure here fails the resolve. That is the point — shipping the artifact
/// and silently skipping its licence is the outcome being prevented, so it must
/// not be the fallback when the network is unkind.
///
/// Called by every single-spec resolve. It used to be inline in [`resolve`]
/// only, which meant the obligation applied to the shape 7.4 happens to have and
/// not to the artifact: an entry declaring itself a tree or a plain file took a
/// path with no licence step at all, and `php-debug` — self-hosted, and a tree —
/// was one pin away from taking exactly that path.
async fn stage_licenses(
    spec: &BinarySpec,
    name: &str,
    version: &str,
    arch: Arch,
    staging: &Path,
    id: &str,
) -> Result<()> {
    let Some(lic) = licenses_spec(&spec.url, name, version, arch)? else {
        return Ok(());
    };
    let archive = staging.join(".licenses.tar.gz");
    download(&lic.url, &archive, Some(&lic.checksum), Some(id)).await?;
    extract_tar_gz_tree_filtered(open_buffered(&archive)?, staging, 0, None)?;
    std::fs::remove_file(&archive)?;
    let dir = staging.join(LICENSES_DIR);
    if !std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some()) {
        return Err(Error::Other(format!(
            "{name} {version} is an artifact rexenv builds and distributes, but its licence \
             archive unpacked to nothing at {} — refusing to publish it without the licences \
             that must ship beside it",
            dir.display()
        )));
    }
    Ok(())
}

/// A cache that is PRESENT but would not satisfy its resolve: the bytes are on
/// disk and something beside them is not.
///
/// The distinction matters because "never downloaded" and "downloaded, now
/// incomplete" want different handling. The second is a REPAIR — the user
/// already has this artifact and something changed under it (a re-pinned digest,
/// or the licence texts an artifact we distribute must carry, which every cache
/// predating ledger #336 is missing). Repairing it at app launch keeps the
/// login-start path strictly offline (ledger #175: never download, never
/// prompt), instead of making that path fetch or making the predicate lie.
pub fn needs_repair(platform: &dyn Platform, name: &str, version: &str) -> bool {
    published_member(platform, name, version).is_some_and(|m| m.exists())
        && cached_path(platform, name, version).is_none()
}

pub fn cached_path(platform: &dyn Platform, name: &str, version: &str) -> Option<PathBuf> {
    let bin_dir = platform.paths().bin_dir().ok()?;
    let dir = bin_dir.join(format!("{name}-{version}"));
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    match shape_of(name) {
        Shape::Bundle => {
            // Bundles carry a prepare RECEIPT rather than a pin marker: they are
            // merged from many part digests, so there is no single checksum to
            // record (#331). The receipt is the equivalent gate.
            let bundle = bundle_manifest(name, version, os, arch)?;
            let member = dir.join(bundle.member);
            (member.exists() && bundle_prepared_current(&dir)).then_some(dir)
        }
        shape => {
            let spec = manifest(name, version, os, arch)?;
            let member = match shape {
                Shape::Single => dir.join(name),
                _ => dir.join(spec.member),
            };
            if !member.exists()
                || !cache_matches_pin(&dir, version, &spec.checksum)
                || !licenses_satisfied(&dir, &spec.url)
            {
                return None;
            }
            Some(match shape {
                Shape::Dir => dir,
                _ => member,
            })
        }
    }
}

/// Whether `name`@`version` resolves without a download. See [`cached_path`].
pub fn is_cached(platform: &dyn Platform, name: &str, version: &str) -> bool {
    cached_path(platform, name, version).is_some()
}

/// Path of an ALREADY-CACHED executable — [`resolve`]'s cache-hit fast path
/// without the download. `None` when absent OR when the cached copy is one
/// `resolve` would refetch. For deriving config values that only need the
/// binary's location (e.g. the Mailpit sendmail shim) in sync contexts like
/// startup adoption, where triggering a download is wrong.
///
/// Only ever answers for [`Shape::Single`]: its callers want an executable to
/// run, and handing back a tree or a `.phar` would be a different thing wearing
/// the same type.
pub fn cached_bin(platform: &dyn Platform, name: &str, version: &str) -> Option<PathBuf> {
    (shape_of(name) == Shape::Single)
        .then(|| cached_path(platform, name, version))
        .flatten()
}

/// The cached bundle tree dir for `name`/`version` iff it's actually present —
/// a bundle dir is valid ONLY when its `member` exists (the same gate
/// [`resolve_bundle`] uses), so a half-extracted `<name>-<version>/` without the
/// binary is correctly rejected. Lets `adopt_startup` wire `httpd_dir` from an
/// existence-checked path with no resolve/download (B28).
pub fn cached_bundle_dir(
    platform: &dyn Platform,
    name: &str,
    version: &str,
    member: &str,
) -> Option<PathBuf> {
    cached_bundle_dir_in(&platform.paths().bin_dir().ok()?, name, version, member)
}

/// Pure core of [`cached_bundle_dir`] (no `Platform`), so the "a bundle dir is
/// valid iff its member exists" rule is unit-testable directly.
fn cached_bundle_dir_in(bin_dir: &Path, name: &str, version: &str, member: &str) -> Option<PathBuf> {
    let dir = bin_dir.join(format!("{name}-{version}"));
    dir.join(member).exists().then_some(dir)
}

/// Every PHP patch whose cache tree must SURVIVE the launch GC — or `None` when
/// the question cannot be answered, in which case the caller must SKIP the sweep.
///
/// Two halves, and both are load-bearing:
///
/// - **Each minor's EFFECTIVE patch** — the user's in-app update choice floored
///   by the pin (`php::effective_patches`). This is the only thing standing
///   between the GC and the tree the user just selected and is serving from; the
///   8 Aug draft of `docs/PLAN-binary-updates.md` §6 promised "the new tree is
///   not deleted" while the GC was keyed on the pin alone, which is precisely
///   what would have deleted it.
/// - **Every patch a pool is LIVE on.** Not redundant with the above: this block
///   is reached after a restart that failed partway, with later minors still
///   serving from their old masters, and the process is the only thing that
///   cannot be wrong about which bytes it is running.
///
/// # What is deliberately NOT kept, and why the earlier answer was wrong
///
/// The compiled-in pins used to be a third, unconditional half, on the argument
/// that **"a floor whose bytes were deleted is not a floor"** — that "no network
/// → falls back to the pins" is a download unless the pinned tree is on disk.
/// That argument does not survive contact with `updates::floored`, which returns
/// the pin **only when the pin is what the minor will run** — in which case the
/// pin already IS that minor's effective patch and is kept by the first half.
/// When a higher selection exists, nothing resolves the pin: not the pool, not
/// the terminal, not WP-CLI, not the planner. The tree was pure weight, and a
/// user who updated 8.2 and 8.3 found ~358 MB of it (two trees per minor,
/// ~90 MB each, and the bin dir was 3.3 GB). It was reported by the person whose
/// disk it was on, which is where "we keep it just in case" arguments usually go
/// to die.
///
/// The revert path does not need it either: `php_update_apply` restores the
/// PREVIOUS SELECTION, not the pin, and that patch was the effective one at the
/// last sweep — so it is on disk. Only a first-ever update reverts to the pin,
/// and at that moment the pin is still the effective patch. The one thing this
/// trades away is a hypothetical future "go back to the version this app ships"
/// control, which would have to re-download. There is no such control.
///
/// # `None` is not an empty list
///
/// An empty `effective` means the registry could not be read (`lib.rs` builds it
/// with `unwrap_or_default`), and the pins half used to mask that. Without it, an
/// empty keep-set would delete **every** PHP tree on the machine. The sweep
/// deletes what is NOT in the set, so not-knowing must cost a skipped sweep and
/// never a live tree — the same rule the unidentifiable-pool guard already obeys.
///
/// Passing both sets in rather than reading them here keeps this pure and keeps
/// `core/binaries.rs` off the database.
pub fn php_caches_to_keep(effective: &[String], running: &[String]) -> Option<Vec<String>> {
    if effective.is_empty() {
        return None;
    }
    let mut keep: Vec<String> = Vec::with_capacity(effective.len() + running.len());
    for patch in effective.iter().chain(running) {
        if !keep.iter().any(|k| k == patch) {
            keep.push(patch.clone());
        }
    }
    Some(keep)
}

/// Whether a binary-cache dir name holds a PHP patch that nothing needs any
/// more — `php-8.3.30/` or `php-fpm-8.3.30/` once 8.3 moved to 8.3.31 and no
/// pool is on 8.3.30.
///
/// `keep` is [`php_caches_to_keep`]'s answer; a version in it is never
/// outdated, whatever the pin says. Pure (name + set) so the GC rule is
/// unit-testable without a `Platform` or a database.
///
/// Deliberately narrow otherwise: only `php-`/`php-fpm-` dirs, only a strict
/// `x.y.z` numeric version, and only minors that HAVE a pin — so the debug
/// builds (`php-debug-…`), other binaries, staging dirs, and versions from a
/// NEWER app (downgrade) are all left alone.
pub fn is_outdated_php_cache(dir_name: &str, keep: &[String]) -> bool {
    // Strip the longer prefix first — `php-` also matches `php-fpm-…`.
    let Some(version) = dir_name
        .strip_prefix("php-fpm-")
        .or_else(|| dir_name.strip_prefix("php-"))
    else {
        return false;
    };
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return false; // not a plain x.y.z (e.g. `php-debug-8.3.31`)
    }
    let minor = crate::core::php::minor_of(version);
    if crate::core::php::patch_for_minor(&minor).is_none() {
        return false; // unpinned minor (newer app's cache) — don't touch
    }
    !keep.iter().any(|k| k == version)
}

/// Remove cache dirs left behind by a PHP patch bump (Option A updates: pins
/// move with an app release; the old `php-<oldpatch>/` trees would otherwise
/// accumulate — measured ~136-208MB per minor, two trees of 68-104MB each).
/// Best-effort — a dir that can't be removed is skipped, never an error.
/// Returns the removed dir names.
///
/// `effective` is `php::effective_patches`' answer and `running` is what the live
/// masters are executing — see [`php_caches_to_keep`] for why both, and why an
/// EMPTY `effective` means "skip" rather than "keep nothing". They are PARAMETERS
/// rather than reads because the caller already knows and this module stays off
/// the database — but they must be passed honestly: this GC runs unconditionally
/// at launch, including on the path where `restart_pools_for` failed partway and
/// later minors are still serving from their old masters (`lib.rs`). Deleting a
/// running master's tree does not kill it — macOS keeps the process alive on the
/// unlinked inode — it just makes the pool unrestartable later, which is the
/// worst of both.
pub fn gc_outdated_php_caches(
    platform: &dyn Platform,
    effective: &[String],
    running: &[String],
) -> Vec<String> {
    let Some(keep) = php_caches_to_keep(effective, running) else {
        log::warn!(
            "php: skipped the outdated-cache sweep — the registry gave no effective patches, \
             and a sweep with an empty keep-set deletes every PHP tree on the machine"
        );
        return Vec::new();
    };
    let Ok(bin_dir) = platform.paths().bin_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&bin_dir) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_outdated_php_cache(&name, &keep) && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed.push(name);
        }
    }
    removed
}

/// Single-flight guard for concurrent resolves of the same `name`@`version`:
/// the second caller waits for the first download to publish, then hits the
/// resolve's cached-path early return instead of racing a duplicate download
/// (same bytes twice, hub progress jittering between the two streams). If the
/// first attempt FAILS, the waiter proceeds and downloads normally — a natural
/// retry, same semantics as the panel's retry button. One async mutex per
/// distinct pinned binary; the map only ever holds that small fixed set, so
/// entries are never evicted.
async fn in_flight(name: &str, version: &str) -> tokio::sync::OwnedMutexGuard<()> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static FLIGHTS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let flight = FLIGHTS
        .get_or_init(Default::default)
        .lock()
        .expect("in-flight download map lock poisoned")
        .entry(downloads::item_id(name, version))
        .or_default()
        .clone();
    flight.lock_owned().await
}

/// Resolve `name`@`version` to a ready-to-run cached binary path, downloading +
/// verifying + extracting + signing on first use. Idempotent: a cached binary is
/// returned without re-downloading.
pub async fn resolve(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let _flight = in_flight(name, version).await;
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    let bin_dir = platform.paths().bin_dir()?;
    let dir = bin_dir.join(format!("{name}-{version}"));
    let bin_path = dir.join(name);

    // The manifest is consulted BEFORE the cache check, because "is this cached"
    // now means "are these the bytes we pin" — not merely "is a file there".
    let spec = manifest(name, version, os, arch).ok_or_else(|| {
        Error::Other(format!(
            "no binary manifest for {name} {version} on {os}/{}",
            php_arch(arch)
        ))
    })?;
    if spec.archive == Archive::TarGzTree {
        return Err(Error::Other(format!(
            "{name} is a directory distribution — use resolve_dir"
        )));
    }
    if let Some(hit) = cached_path(platform, name, version) {
        return Ok(hit);
    }
    // Present but NOT the pinned bytes: a re-issued artifact at an unchanged
    // version. Drop it and fetch, rather than serving it forever. Also reached
    // when the bytes are right but the licence texts we OWE alongside them are
    // absent — a cache from before those shipped.
    if bin_path.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Stage in a temp dir on the same filesystem, prepare it there, then publish
    // atomically — so a failed `set_executable`/`prepare_binary` (an unrelinkable
    // dylib dep, a codesign error) NEVER leaves a poisoned (unsigned/unrelinked)
    // binary at the cached path that every later `resolve` returns via `exists()`
    // and Apple Silicon SIGKILLs. On any failure the staging dir is removed, so a
    // retry re-downloads and prepares cleanly (task 2.5 / H4). The download
    // streams straight into the staging dir (checksum hashed in-flight) and is
    // reported to the download hub as it goes.
    let id = downloads::item_id(name, version);
    downloads::hub().item_started(name, version);
    let staging = staging_path(&bin_dir, name, version);
    let staged_bin = staging.join(name);
    let staged: Result<()> = async {
        std::fs::create_dir_all(&staging)?;
        match spec.archive {
            Archive::TarGz => {
                let archive = staging.join(".archive.tar.gz");
                download(&spec.url, &archive, Some(&spec.checksum), Some(&id)).await?;
                downloads::hub().item_preparing(&id);
                extract_tar_gz_member(open_buffered(&archive)?, spec.member, &staged_bin)?;
                std::fs::remove_file(&archive)?;
            }
            Archive::Raw => {
                download(&spec.url, &staged_bin, Some(&spec.checksum), Some(&id)).await?;
                downloads::hub().item_preparing(&id);
            }
            Archive::TarGzTree => unreachable!("TarGzTree returned above"),
        }
        platform.permissions().set_executable(&staged_bin)?;
        platform.binaries().prepare_binary(&staged_bin)?;
        // The licences of an artifact WE distribute, fetched into the same
        // staging dir so they are part of the same atomic publish: a published
        // 7.4 either carries its licence texts or does not exist. Fetching
        // after the publish would create exactly the state this is meant to
        // rule out — the binary on disk, in use, with nothing beside it.
        //
        // A failure here fails the resolve. That is the point: shipping the
        // interpreter and silently skipping its licence is the outcome being
        // prevented, so it cannot be the fallback when the network is unkind.
        stage_licenses(&spec, name, version, arch, &staging, &id).await?;
        write_pin_marker(&staging, &spec.checksum);
        publish(&staging, &dir, name)
    }
    .await;
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    finish_item(&id, &staged);
    staged?;
    Ok(bin_path)
}

/// Report a resolve's outcome to the download hub (done, or failed with the
/// error the UI's retry row will show).
fn finish_item(id: &str, outcome: &Result<()>) {
    match outcome {
        Ok(()) => downloads::hub().item_done(id),
        Err(e) => downloads::hub().item_failed(id, &e.to_string()),
    }
}

fn open_buffered(path: &Path) -> Result<std::io::BufReader<std::fs::File>> {
    Ok(std::io::BufReader::new(std::fs::File::open(path)?))
}

/// Resolve a raw, non-executable artifact (a `Raw` archive that is NOT a native
/// binary — e.g. the WP-CLI `.phar`, run via the bundled PHP). Downloads +
/// verifies + writes it under `bin_dir/<name>-<version>/<member>`; does NOT
/// chmod +x or codesign (it's a script, not a Mach-O). Idempotent.
pub async fn resolve_file(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let _flight = in_flight(name, version).await;
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;
    let spec = manifest(name, version, os, arch)
        .ok_or_else(|| Error::Other(format!("no binary manifest for {name} {version}")))?;
    if spec.archive != Archive::Raw {
        return Err(Error::Other(format!("{name} is not a raw file artifact")));
    }
    let bin_dir = platform.paths().bin_dir()?;
    let dir = bin_dir.join(format!("{name}-{version}"));
    let path = dir.join(spec.member);
    if let Some(hit) = cached_path(platform, name, version) {
        return Ok(hit);
    }
    // Stage + publish atomically so an interrupted write never caches a truncated
    // script (task 2.5 / H4): the stream lands in the staging dir and is only
    // renamed into place after the checksum verifies. No chmod/codesign — it's a
    // script, not a Mach-O.
    let id = downloads::item_id(name, version);
    downloads::hub().item_started(name, version);
    let staging = staging_path(&bin_dir, name, version);
    let staged: Result<()> = async {
        std::fs::create_dir_all(&staging)?;
        download(&spec.url, &staging.join(spec.member), Some(&spec.checksum), Some(&id)).await?;
        downloads::hub().item_preparing(&id);
        stage_licenses(&spec, name, version, arch, &staging, &id).await?;
        write_pin_marker(&staging, &spec.checksum);
        publish(&staging, &dir, spec.member)
    }
    .await;
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    finish_item(&id, &staged);
    staged?;
    Ok(path)
}

/// Resolve a directory-distribution (`TarGzTree`, e.g. MySQL) to its extracted
/// base dir, downloading + verifying + extracting on first use. The single
/// top-level dir in the tarball is stripped, so the base dir directly contains
/// `bin/`, `lib/`, `share/`. Idempotent: a cached tree is returned as-is.
pub async fn resolve_dir(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let _flight = in_flight(name, version).await;
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    let spec = manifest(name, version, os, arch).ok_or_else(|| {
        Error::Other(format!(
            "no binary manifest for {name} {version} on {os}/{}",
            php_arch(arch)
        ))
    })?;

    let bin_dir = platform.paths().bin_dir()?;
    let dir = bin_dir.join(format!("{name}-{version}"));
    // Idempotent, but validate the PRIMARY binary — not just that `bin/` exists.
    // A tree whose `bin/` survived while its main binary was stripped (partial
    // extract, Gatekeeper/AV quarantine of a large Mach-O) must re-extract, else
    // every later resolve returns a broken tree that fails at spawn with an opaque
    // "No such file or directory" (task 2.5 / H4).
    if let Some(hit) = cached_path(platform, name, version) {
        return Ok(hit);
    }

    if spec.archive != Archive::TarGzTree {
        return Err(Error::Other(format!(
            "{name} is not a directory distribution — use resolve"
        )));
    }

    // Stream the archive into the staging dir (checksum hashed in-flight — the
    // ~600MB MySQL tarball never sits in RAM), extract there, then publish
    // atomically — a download/extract that fails partway never leaves a partial
    // tree that later resolves accept via the marker short-circuit (task 2.5 /
    // H4). MySQL's binaries are Oracle-signed + notarized (Postgres is
    // relocatable/unsigned-ok), and a reqwest download adds no quarantine
    // attribute, so there's no ad-hoc re-signing step here.
    let id = downloads::item_id(name, version);
    downloads::hub().item_started(name, version);
    let staging = staging_path(&bin_dir, name, version);
    let staged: Result<()> = async {
        std::fs::create_dir_all(&staging)?;
        let archive = staging.join(".archive.tar.gz");
        download(&spec.url, &archive, Some(&spec.checksum), Some(&id)).await?;
        downloads::hub().item_preparing(&id);
        extract_tar_gz_tree(open_buffered(&archive)?, &staging)?;
        // Drop the archive BEFORE publishing so the cached tree doesn't carry a
        // dead 600MB tarball into the final dir.
        std::fs::remove_file(&archive)?;
        stage_licenses(&spec, name, version, arch, &staging, &id).await?;
        write_pin_marker(&staging, &spec.checksum);
        publish(&staging, &dir, spec.member)
    }
    .await;
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    finish_item(&id, &staged);
    staged?;
    Ok(dir)
}

/// How many times a transient download failure (network drop, timeout, 5xx) is
/// retried before giving up. A 4xx (not-found / forbidden) is NOT retried. Each
/// retry RESUMES from the partial on disk (HTTP Range), so the count bounds how
/// many drops a single download tolerates, not how many times it re-fetches the
/// whole file.
const DOWNLOAD_ATTEMPTS: usize = 5;

/// Exponential backoff (ms) before retry `attempt` (1-based), capped at 8s, plus
/// a small jitter in `[0, base/4)` so repeated retries don't re-hit a
/// rate-limiting mirror on an identical cadence. Pure (jitter is injected) so the
/// growth/cap/bound are unit-testable; the caller passes a runtime seed.
fn backoff_delay_ms(attempt: usize, jitter_seed: u64) -> u64 {
    let shift = attempt.saturating_sub(1).min(4) as u32; // 0,1,2,3,4 → ×1,2,4,8,16
    let base = 500u64.saturating_mul(1u64 << shift).min(8000);
    base + jitter_seed % (base / 4 + 1)
}

/// The full size `Z` from a `Content-Range: bytes X-Y/Z` header (the response to
/// a resumed range request), or `None` when absent/`*`/malformed.
fn content_range_total(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Re-hash an existing partial file from disk so a resumed download's running
/// digest covers the bytes already written (sha2 mid-state isn't serializable).
/// Returns the seeded hasher (`None` when there's no checksum to verify) AND the
/// exact byte count read — the caller aligns the Range offset and `downloaded`
/// counter to this so the file content, offset, and hash input never drift.
/// Streamed in 64 KiB reads: re-hashing a 34 MB partial costs no extra RAM.
fn rehash_partial(
    dest: &Path,
    checksum: Option<&Checksum>,
) -> std::result::Result<(Option<StreamHasher>, u64), FetchError> {
    let permanent = |e: std::io::Error| {
        FetchError::Permanent(format!("can't read partial {}: {e}", dest.display()))
    };
    let mut file = std::fs::File::open(dest).map_err(permanent)?;
    let mut hasher = checksum.map(StreamHasher::new);
    let mut buf = [0u8; 64 * 1024];
    let mut count: u64 = 0;
    loop {
        let n = file.read(&mut buf).map_err(permanent)?;
        if n == 0 {
            break;
        }
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        count += n as u64;
    }
    Ok((hasher, count))
}

/// Append a plain-language hint when a download failure looks like a connectivity
/// problem, so the UI message is understandable on a machine with no internet.
fn download_error_message(url: &str, e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        format!("can't reach {url} — check your internet connection ({e})")
    } else {
        format!("download {url} failed: {e}")
    }
}

/// Whether an HTTP error status is worth retrying: 5xx is a server-side blip;
/// 4xx (not found / forbidden) won't change, so it's permanent.
fn status_is_transient(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
}

/// Per-chunk inactivity timeout while streaming a download. Replaces the old
/// whole-request timeout (120s), which silently capped how BIG a download could
/// be on a slow link (the 600MB MySQL tree at <5MB/s would have aborted); a
/// stall guard is what we actually want.
const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Absolute ceiling on a download whose length the server never declares — above
/// the largest real artifact (the ~600 MB DB/PHP trees), so a legit download
/// never reaches it.
const DOWNLOAD_UNKNOWN_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

/// Hard ceiling on bytes written for one download, so a mirror that lies about
/// (or omits) `Content-Length` can't fill the disk before the post-EOF checksum
/// runs. A declared length is allowed plus generous slack (+25% +8 MiB): the
/// client does no response decompression, so an honest transfer streams exactly
/// `Content-Length` bytes and never approaches the slack — while a length that
/// under-claims by a wide margin is still caught early. An undeclared length
/// falls back to [`DOWNLOAD_UNKNOWN_MAX_BYTES`]. This is a disk-safety valve
/// ONLY — the pinned checksum stays the authoritative integrity gate for any
/// download that completes within the ceiling (B36).
fn download_ceiling(total: Option<u64>) -> u64 {
    match total {
        Some(t) => t.saturating_add(t / 4).saturating_add(8 * 1024 * 1024),
        None => DOWNLOAD_UNKNOWN_MAX_BYTES,
    }
}

fn http_client() -> Result<reqwest::Client> {
    // Some CDNs (e.g. dev.mysql.com) reject the default reqwest User-Agent with
    // 403; present a browser-like UA so downloads are accepted everywhere. Bound
    // connects so "no internet" fails fast (not a hang); the body is guarded by
    // the per-chunk CHUNK_TIMEOUT instead of a total deadline.
    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| Error::Other(format!("http client: {e}")))
}

/// Download `url` into memory with bounded retries + a connect timeout. Public so
/// the §2 robustness checks can exercise the retry / connectivity-error behavior
/// directly; normal callers use `resolve`/`resolve_file`/`resolve_dir`, which
/// stream to disk (no whole-body buffering) via [`download`].
pub async fn http_get(url: &str) -> Result<Vec<u8>> {
    let tmp = std::env::temp_dir().join(format!(
        "rexenv-http-{}-{}",
        std::process::id(),
        STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let out = match download(url, &tmp, None, None).await {
        Ok(()) => std::fs::read(&tmp).map_err(Error::from),
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(&tmp);
    out
}

/// Stream `url` to `dest` with bounded retries, hashing incrementally when a
/// `checksum` is given (verification costs no extra read) and reporting byte
/// progress into the download hub under `item`. The whole body never sits in
/// memory — peak RAM is one chunk. On any failure `dest` is removed.
async fn download(
    url: &str,
    dest: &Path,
    checksum: Option<&Checksum>,
    item: Option<&str>,
) -> Result<()> {
    download_with_headers(url, dest, checksum, item, &[]).await
}

/// [`download`] with extra request headers — for registries that demand them
/// (ghcr.io bottle blobs need the anonymous bearer token).
async fn download_with_headers(
    url: &str,
    dest: &Path,
    checksum: Option<&Checksum>,
    item: Option<&str>,
    headers: &[(&str, &str)],
) -> Result<()> {
    let client = http_client()?;
    let mut last_err = String::new();
    // Resume state: on a transient failure we KEEP the partial and re-request
    // `Range: bytes=<resume_from>-` next attempt, so a link that drops mid-body
    // makes forward progress instead of re-fetching all 34 MB from byte 0. The
    // offset is re-derived from the file on disk (ground truth) each time.
    let mut resume_from: u64 = 0;
    // Give up early if the link can't move even one chunk two attempts running —
    // more waiting won't help. Bounded either way by DOWNLOAD_ATTEMPTS.
    let mut zero_progress_streak: u32 = 0;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let mut wrote: u64 = 0;
        match fetch_to_file(&client, url, dest, checksum, item, headers, resume_from, &mut wrote).await
        {
            Ok(()) => return Ok(()),
            // A permanent failure (4xx / checksum mismatch / local write error)
            // won't get better on retry — surface it immediately, partial removed
            // so the next run starts clean (a checksum mismatch fails closed).
            Err(FetchError::Permanent(msg)) => {
                let _ = std::fs::remove_file(dest);
                return Err(Error::Other(msg));
            }
            Err(FetchError::Transient(msg)) => {
                last_err = msg;
                // Keep the partial for resume; next offset = its real size.
                resume_from = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                zero_progress_streak = if wrote == 0 { zero_progress_streak + 1 } else { 0 };
                log::warn!(
                    "rexenv: download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed \
                     (resume from {resume_from} bytes): {last_err}"
                );
                if zero_progress_streak >= 2 {
                    last_err = format!("{last_err} — no data received across two attempts");
                    break;
                }
                if attempt < DOWNLOAD_ATTEMPTS {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64)
                        .unwrap_or(0);
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_delay_ms(
                        attempt, seed,
                    )))
                    .await;
                }
            }
        }
    }
    // Gave up: drop the partial so the next run (or the UI retry) starts fresh.
    let _ = std::fs::remove_file(dest);
    Err(Error::Other(format!(
        "{last_err} (gave up after {DOWNLOAD_ATTEMPTS} attempts)"
    )))
}

/// Outcome of a single download attempt: a permanent error short-circuits the
/// retry loop; a transient one is retried.
#[derive(Debug)]
enum FetchError {
    Permanent(String),
    Transient(String),
}

/// Await the response head with a size-independent stall guard. `connect_timeout`
/// covers only TCP+TLS, and the per-chunk `CHUNK_TIMEOUT` in [`fetch_to_file`]
/// only guards body reads AFTER `send()` resolves — so a server that completes
/// the handshake then never sends headers (captive portal / wedged mirror) would
/// otherwise hang `send()` forever, inside attempt 1, with no retry (B34). Mapped
/// to `Transient` so the retry loop handles it, exactly like a chunk stall.
async fn send_bounded(
    req: reqwest::RequestBuilder,
    url: &str,
    timeout: std::time::Duration,
) -> std::result::Result<reqwest::Response, FetchError> {
    match tokio::time::timeout(timeout, req.send()).await {
        Ok(Ok(r)) => Ok(r),
        // Connect/timeout/transport problems are transient (retry); they're also
        // what "no internet" looks like, so use the connectivity-aware message.
        Ok(Err(e)) => Err(FetchError::Transient(download_error_message(url, &e))),
        Err(_) => Err(FetchError::Transient(format!(
            "download {url} stalled (no response headers for {}s)",
            timeout.as_secs()
        ))),
    }
}

/// One download attempt. `resume_from > 0` requests `Range: bytes=<resume_from>-`
/// and, if granted (206), APPENDS to the existing partial while re-hashing its
/// prefix from disk so the running digest covers the whole assembled file. If the
/// server ignores the range (200) or the partial is stale (416), it truncates and
/// starts over. `*wrote` reports the bytes written THIS attempt (0 = the body
/// never started), for the caller's zero-progress guard.
///
/// The pinned checksum stays the ONE integrity gate: every byte of the final file
/// (re-hashed prefix + appended chunks) flows through a single hash compared to
/// the pin unconditionally at the end — so a truncation, a corrupted partial, or
/// a mirror swapping bytes across ranges all fail CLOSED, and no resume path can
/// bypass it.
#[allow(clippy::too_many_arguments)]
async fn fetch_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    checksum: Option<&Checksum>,
    item: Option<&str>,
    headers: &[(&str, &str)],
    resume_from: u64,
    wrote: &mut u64,
) -> std::result::Result<(), FetchError> {
    *wrote = 0;
    let mut req = client.get(url);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    if resume_from > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let mut resp = send_bounded(req, url, CHUNK_TIMEOUT).await?;

    // 206 → resume granted; 200 → full body (fresh, or the range was ignored);
    // 416 → our partial is stale/complete, restart fresh. Any other non-2xx uses
    // the retry-policy split (5xx transient, 4xx permanent).
    let status = resp.status();
    let resuming = resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !status.is_success() && status != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let msg = format!("download {url} failed: HTTP {status}");
        return Err(if status_is_transient(status) {
            FetchError::Transient(msg)
        } else {
            FetchError::Permanent(msg)
        });
    }

    // Set up the sink + hasher + byte counter for this attempt's mode.
    let (mut file, mut hasher, mut downloaded, total) = if resuming {
        // Full size from Content-Range (`bytes X-Y/Z`), else offset + remaining.
        let total = content_range_total(resp.headers())
            .or_else(|| resp.content_length().map(|remaining| resume_from + remaining));
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .map_err(|e| FetchError::Permanent(format!("can't open partial {}: {e}", dest.display())))?;
        // Seed the digest from the bytes already on disk; `count` is ground truth
        // for the offset (kept aligned with the file the append writes onto).
        let (hasher, count) = rehash_partial(dest, checksum)?;
        (file, hasher, count, total)
    } else {
        // Fresh: resume_from==0, or the server sent 200/416 — truncate & restart.
        let total = resp.content_length();
        let file = std::fs::File::create(dest)
            .map_err(|e| FetchError::Permanent(format!("can't write {}: {e}", dest.display())))?;
        (file, checksum.map(StreamHasher::new), 0u64, total)
    };

    let ceiling = download_ceiling(total);
    if let Some(id) = item {
        downloads::hub().item_progress(id, downloaded, total);
    }
    loop {
        let chunk = match tokio::time::timeout(CHUNK_TIMEOUT, resp.chunk()).await {
            // No bytes for the whole guard window: a stalled transfer, not a
            // slow one (slow links keep chunks trickling in) — retryable.
            Err(_) => {
                return Err(FetchError::Transient(format!(
                    "download {url} stalled (no data for {}s)",
                    CHUNK_TIMEOUT.as_secs()
                )))
            }
            // A body read cut off mid-stream (aborted download) is transient.
            Ok(Err(e)) => return Err(FetchError::Transient(download_error_message(url, &e))),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(c))) => c,
        };
        file.write_all(&chunk)
            .map_err(|e| FetchError::Permanent(format!("can't write {}: {e}", dest.display())))?;
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        downloaded += chunk.len() as u64;
        *wrote += chunk.len() as u64;
        // Disk-safety valve on the TOTAL assembled size (offset + this attempt):
        // a mirror that lies about (or omits) Content-Length can't stream past the
        // ceiling and fill the disk before the post-EOF checksum runs. The
        // checksum below stays the real integrity gate — this only kills a
        // runaway (B36).
        if downloaded > ceiling {
            return Err(FetchError::Transient(format!(
                "download {url} exceeded its expected size (>{ceiling} bytes) — \
                 aborting before the disk fills"
            )));
        }
        if let Some(id) = item {
            downloads::hub().item_progress(id, downloaded, total);
        }
    }
    // Integrity gate: the digest covers the WHOLE assembled file. A mismatch is
    // permanent (fail closed) — the caller removes the partial so the next run
    // starts clean.
    if let (Some(h), Some(c)) = (hasher, checksum) {
        let got = h.finish();
        let expected = checksum_hex(c);
        if !got.eq_ignore_ascii_case(expected) {
            return Err(FetchError::Permanent(checksum_mismatch_message(
                url, expected, &got,
            )));
        }
    }
    Ok(())
}

/// Incremental digest matching a pinned [`Checksum`]'s algorithm, fed chunk by
/// chunk while streaming — so verifying a 600MB tree costs no second read.
enum StreamHasher {
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}

impl StreamHasher {
    fn new(checksum: &Checksum) -> Self {
        use sha2::Digest;
        match checksum {
            Checksum::Sha256(_) => StreamHasher::Sha256(sha2::Sha256::new()),
            Checksum::Sha512(_) => StreamHasher::Sha512(sha2::Sha512::new()),
        }
    }
    fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest;
        match self {
            StreamHasher::Sha256(h) => h.update(bytes),
            StreamHasher::Sha512(h) => h.update(bytes),
        }
    }
    fn finish(self) -> String {
        use sha2::Digest;
        match self {
            StreamHasher::Sha256(h) => hex_lower(&h.finalize()),
            StreamHasher::Sha512(h) => hex_lower(&h.finalize()),
        }
    }
}

fn checksum_hex(c: &Checksum) -> &str {
    match c {
        Checksum::Sha256(h) | Checksum::Sha512(h) => h,
    }
}

fn checksum_mismatch_message(url: &str, expected: &str, got: &str) -> String {
    format!(
        "checksum mismatch for {url}: expected {expected}, got {got} — the \
         upstream file changed (rebuilt release or tampering); rexenv needs \
         an update with a re-verified pin"
    )
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn extract_tar_gz_member(reader: impl std::io::Read, member: &str, dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive as TarArchive;

    let mut archive = TarArchive::new(GzDecoder::new(reader));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let is_member = entry
            .path()?
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| n == member)
            .unwrap_or(false);
        if is_member {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(Error::Other(format!(
        "member '{member}' not found in archive"
    )))
}

/// Extract an entire tar.gz tree into `dest`, stripping the single top-level
/// directory (e.g. `mysql-8.4.6-macos15-arm64/bin/mysqld` → `<dest>/bin/mysqld`).
/// Preserves file modes (executables) and symlinks via tar's `unpack`.
///
/// We compute the output path ourselves (to strip the top dir), which bypasses the
/// `tar` crate's own extraction guards — so we re-add them (L2, defense-in-depth;
/// archives are already checksum-pinned): every entry must resolve inside `dest`,
/// both by path (no `..`/absolute components) and, for links, by target.
fn extract_tar_gz_tree(reader: impl std::io::Read, dest: &Path) -> Result<()> {
    extract_tar_gz_tree_filtered(reader, dest, 1, None)
}

/// [`extract_tar_gz_tree`] with a configurable strip depth and an optional
/// include filter (component-wise prefix match on the post-strip path).
/// Homebrew bottles nest `<formula>/<version>/…` (strip 2) and carry receipts/
/// docs/static-libs a bundle must not cache.
fn extract_tar_gz_tree_filtered(
    reader: impl std::io::Read,
    dest: &Path,
    strip: usize,
    include: Option<&[&str]>,
) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::path::PathBuf;
    use tar::Archive as TarArchive;

    let mut archive = TarArchive::new(GzDecoder::new(reader));
    for entry in archive.entries()? {
        let mut entry = entry?;
        // Drop the leading stripped components.
        let rel: PathBuf = entry.path()?.components().skip(strip).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Keep only included subtrees. A kept FILE's parent dirs are created on
        // unpack, so skipped standalone dir entries cost nothing.
        if let Some(prefixes) = include {
            if !prefixes.iter().any(|p| rel.starts_with(p)) {
                continue;
            }
        }
        // Reject an entry path that would escape `dest` (`..`/absolute/prefix).
        let out = safe_join(dest, &rel)?;
        // A link must also not TARGET a path outside `dest` (else a later entry could
        // be written through it). Symlink targets resolve relative to the link's dir;
        // hardlink targets relative to the extraction root. Real dylib symlinks use
        // in-tree `../lib/…`, which is allowed as long as it stays under `dest`.
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry.link_name()?.ok_or_else(|| {
                Error::Other(format!("archive link {} has no target", rel.display()))
            })?;
            let base = if kind.is_symlink() {
                out.parent().unwrap_or(dest)
            } else {
                dest
            };
            if !link_stays_within(dest, base, &target) {
                return Err(Error::Other(format!(
                    "unsafe link target in archive: {} -> {}",
                    rel.display(),
                    target.display()
                )));
            }
        }
        if kind.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&out)?;
        }
    }
    Ok(())
}

/// Join `rel` under `base`, rejecting any component that would escape it — `..`, an
/// absolute root, or a Windows drive prefix. Tar entry paths (file locations) should
/// only ever be plain (`Normal`) components; anything else is a traversal attempt.
fn safe_join(base: &Path, rel: &Path) -> Result<PathBuf> {
    use std::path::Component;
    let mut out = base.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Other(format!(
                    "unsafe path in archive: {}",
                    rel.display()
                )));
            }
        }
    }
    Ok(out)
}

/// Whether a link whose directory is `base` (already inside `dest`) with `target`
/// stays within `dest`. Absolute targets are rejected; relative ones are resolved
/// lexically (no filesystem access, so on-disk symlinks can't be abused mid-resolve)
/// and must not climb above `dest`. `..` is allowed as long as the result stays under
/// `dest` — real dylib symlinks use `../lib/…` within the tree.
fn link_stays_within(dest: &Path, base: &Path, target: &Path) -> bool {
    use std::path::Component;
    if target.is_absolute() {
        return false;
    }
    let mut resolved = base.to_path_buf();
    for comp in target.components() {
        match comp {
            Component::ParentDir => {
                if !resolved.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            Component::Normal(c) => resolved.push(c),
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved.starts_with(dest)
}

/// Monotonic counter so concurrent stagings in one process never collide.
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// A unique, hidden staging dir under `bin_dir` — on the SAME filesystem as the final
/// cache dir, so the publish rename is atomic. The leading dot + `-<pid>-<seq>` suffix
/// keep it from ever being mistaken for a resolved binary and let two resolves stage
/// independently.
fn staging_path(bin_dir: &Path, name: &str, version: &str) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
    bin_dir.join(format!(".staging-{name}-{version}-{}-{seq}", std::process::id()))
}

/// Atomically move a fully-prepared `staging` dir to its final `dir`. `marker` is the
/// entry that proves a dir is fully published (the binary file for `resolve`, `bin`
/// for a tree). Three cases:
///  - target absent → rename in place (atomic swap into the cache).
///  - target already has `marker` → a concurrent resolve won; keep theirs, drop ours.
///  - target exists WITHOUT `marker` → a stale/partial leftover (a prior crash mid-
///    prepare); remove it and publish ours. This is what un-poisons the cache (H4).
fn publish(staging: &Path, dir: &Path, marker: &str) -> Result<()> {
    if std::fs::rename(staging, dir).is_ok() {
        return Ok(());
    }
    if dir.join(marker).exists() {
        let _ = std::fs::remove_dir_all(staging);
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(staging, dir).map_err(|e| {
        Error::Other(format!(
            "failed to publish {} → {}: {e}",
            staging.display(),
            dir.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Platform` that answers only the two questions [`cached_path`] asks:
    /// where the bin dir is, and which arch this machine is. Everything else
    /// panics, so a future caller that starts needing more is loud rather than
    /// silently served a default.
    struct CachePlatform {
        paths: CachePaths,
        binaries: CacheBinaries,
    }
    struct CachePaths(PathBuf);
    struct CacheBinaries;

    impl crate::platform::traits::Paths for CachePaths {
        fn bin_dir(&self) -> Result<PathBuf> {
            Ok(self.0.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf> { unimplemented!() }
        fn config_dir(&self) -> Result<PathBuf> { unimplemented!() }
        fn log_dir(&self) -> Result<PathBuf> { unimplemented!() }
        fn hosts_file(&self) -> PathBuf { unimplemented!() }
    }
    impl crate::platform::traits::BinaryProvider for CacheBinaries {
        fn arch(&self) -> Arch {
            Arch::Arm64
        }
        fn prepare_binary(&self, _path: &Path) -> Result<()> { unimplemented!() }
        fn prepare_binary_tree(&self, _root: &Path) -> Result<()> { unimplemented!() }
    }
    impl Platform for CachePlatform {
        fn paths(&self) -> &dyn crate::platform::traits::Paths {
            &self.paths
        }
        fn binaries(&self) -> &dyn crate::platform::traits::BinaryProvider {
            &self.binaries
        }
        fn supervisor(&self) -> &dyn crate::platform::traits::ProcessSupervisor { unimplemented!() }
        fn dns(&self) -> &dyn crate::platform::traits::DnsManager { unimplemented!() }
        fn cert_trust(&self) -> &dyn crate::platform::traits::CertTrustManager { unimplemented!() }
        fn privileges(&self) -> &dyn crate::platform::traits::PrivilegeManager { unimplemented!() }
        fn autostart(&self) -> &dyn crate::platform::traits::AutostartManager { unimplemented!() }
        fn permissions(&self) -> &dyn crate::platform::traits::PermissionManager { unimplemented!() }
        fn shell(&self) -> &dyn crate::platform::traits::ShellRunner { unimplemented!() }
        fn edge(&self) -> &dyn crate::platform::traits::EdgeSupervisor { unimplemented!() }
        fn dns_agent(&self) -> &dyn crate::platform::traits::DnsAgentManager { unimplemented!() }
    }

    /// A throwaway bin dir. Named per-test AND per-pid so two tests in the same
    /// binary cannot share one — `cached_path` writes nothing, but the fixtures
    /// below plant and remove files, and a shared dir makes that order-dependent.
    fn cache_fixture(tag: &str) -> (CachePlatform, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("rexenv-cache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (
            CachePlatform { paths: CachePaths(root.clone()), binaries: CacheBinaries },
            root,
        )
    }

    /// **One question, one answer.** `is_cached` (the planner), `cached_bin`
    /// (sync adoption) and `resolve`'s own fast path all route through
    /// [`cached_path`], and the case that used to split them is a cache whose
    /// bytes are present but whose PIN MARKER names different bytes — an
    /// upstream in-place rebuild plus a re-pin, which happened 5 Jul 2026.
    /// The planner said "cached, nothing to do" and `resolve` then deleted the
    /// dir and re-downloaded with no hub row.
    #[test]
    fn the_planner_and_the_resolve_agree_about_what_is_cached() {
        let (platform, root) = cache_fixture("agree");
        let v = "7.4.33"; // self-hosted: an ABSENT marker is stale for it
        let dir = root.join(format!("php-{v}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("php"), "not really a binary").unwrap();

        // The binary exists — the OLD `is_cached` stopped here and said yes.
        assert!(dir.join("php").exists());
        // …but it is not resolvable: no pin marker, and 7.4 is ours, so an
        // absent marker is stale. Planner and resolve now agree it is not.
        assert!(!is_cached(&platform, "php", v));
        assert!(cached_path(&platform, "php", v).is_none());
        assert!(cached_bin(&platform, "php", v).is_none());
        // And it reads as a REPAIR, not as "never downloaded" — the distinction
        // that keeps login-start offline (ledger #175).
        assert!(needs_repair(&platform, "php", v));

        // Give it the right marker and the licences it owes, and all three flip.
        let spec = manifest("php", v, "macos", Arch::Arm64).unwrap();
        write_pin_marker(&dir, &spec.checksum);
        std::fs::create_dir_all(dir.join(LICENSES_DIR)).unwrap();
        std::fs::write(dir.join(LICENSES_DIR).join("PHP-3.01.txt"), "…").unwrap();
        assert!(is_cached(&platform, "php", v));
        assert_eq!(cached_path(&platform, "php", v).unwrap(), dir.join("php"));
        assert_eq!(cached_bin(&platform, "php", v).unwrap(), dir.join("php"));
        assert!(!needs_repair(&platform, "php", v), "a whole cache needs no repair");

        // Nothing on disk at all is neither cached nor a repair.
        assert!(!is_cached(&platform, "php", PHP_VERSION));
        assert!(!needs_repair(&platform, "php", PHP_VERSION));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **The name → resolver mapping is ONE fact.** It lived in
    /// `downloads::resolve_any` as a `match` and in `is_cached` as a
    /// `member || name` disjunction, and they disagreed about `composer`:
    /// `resolve_any` let it fall through to `resolve` (publishing `dir/composer`)
    /// while every consumer calls `resolve_file` (reading `dir/composer.phar`).
    /// So a prefetch cached one path and the consumer downloaded the other.
    ///
    /// Derived from the manifest rather than asserted as a list, so a new
    /// artifact joins by existing.
    #[test]
    fn the_shape_of_an_artifact_matches_where_its_resolve_publishes_it() {
        // composer is the regression: Raw archive, member != name, File shape.
        let composer = manifest("composer", COMPOSER_VERSION, "macos", Arch::Arm64).unwrap();
        assert_eq!(composer.member, "composer.phar");
        assert_ne!(composer.member, "composer");
        assert_eq!(shape_of("composer"), Shape::File, "composer publishes under its member");

        // The other two file artifacts, for the same reason.
        for n in ["wp-cli", "adminer"] {
            assert_eq!(shape_of(n), Shape::File);
        }
        // Tree distributions, and the bundles (which have no plain manifest).
        for n in ["mysql", "postgres"] {
            assert_eq!(shape_of(n), Shape::Dir);
            assert_eq!(
                manifest(n, MYSQL_VERSION, "macos", Arch::Arm64)
                    .map(|s| s.archive)
                    .unwrap_or(Archive::TarGzTree),
                Archive::TarGzTree
            );
        }
        for n in ["redis", "mariadb", "httpd", "xdebug-8.4"] {
            assert_eq!(shape_of(n), Shape::Bundle);
            assert!(manifest(n, "1", "macos", Arch::Arm64).is_none(), "{n} is not a plain spec");
        }
        // Everything else is a single executable published at `dir/<name>`.
        for n in ["php", "php-fpm", "caddy", "nginx", "mailpit", "frankenphp", "cloudflared"] {
            assert_eq!(shape_of(n), Shape::Single, "{n}");
        }
    }

    /// `cached_bin` hands back an executable or nothing — never a tree or a
    /// `.phar` wearing the same type. Its callers (mailpit/frankenphp adoption)
    /// go on to SPAWN what they are given.
    #[test]
    fn cached_bin_only_ever_answers_for_a_single_executable() {
        let (platform, root) = cache_fixture("bin-shape");
        // Plant a complete-looking cache for a File-shaped artifact.
        let dir = root.join(format!("composer-{COMPOSER_VERSION}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("composer.phar"), "#!/usr/bin/env php").unwrap();
        assert!(is_cached(&platform, "composer", COMPOSER_VERSION), "it IS cached…");
        assert!(cached_bin(&platform, "composer", COMPOSER_VERSION).is_none(), "…but not a binary");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn download_ceiling_allows_slack_and_caps_unknown_length() {
        let mb = 1024 * 1024;
        // Declared length: allowed up to +25% +8 MiB. The client does no
        // decompression, so an honest download arrives at ~total and never
        // reaches this — no false-trip.
        assert_eq!(download_ceiling(Some(100 * mb)), 100 * mb + 25 * mb + 8 * mb);
        // The largest real artifact (~600 MB bundle) fits comfortably.
        assert!(download_ceiling(Some(600 * mb)) > 600 * mb);
        // Undeclared length falls back to the absolute cap.
        assert_eq!(download_ceiling(None), DOWNLOAD_UNKNOWN_MAX_BYTES);
        // A huge declared length saturates instead of overflowing.
        assert_eq!(download_ceiling(Some(u64::MAX)), u64::MAX);
    }

    #[tokio::test]
    async fn send_bounded_times_out_when_response_headers_never_arrive() {
        // A server that accepts the TCP connection then sends NOTHING back — the
        // exact "handshake done, headers never arrive" stall (B34). send_bounded
        // must return a transient "stalled" error fast, not hang forever.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                // Hold the connection open, replying nothing.
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                drop(stream);
            }
        });

        let url = format!("http://{addr}/x");
        let req = reqwest::Client::new().get(&url);
        let start = std::time::Instant::now();
        let out = send_bounded(req, &url, std::time::Duration::from_millis(300)).await;

        assert!(
            matches!(out, Err(FetchError::Transient(_))),
            "a header stall is transient (retryable)"
        );
        if let Err(FetchError::Transient(msg)) = out {
            assert!(msg.contains("stalled"), "{msg}");
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "must not hang past the guard: {:?}",
            start.elapsed()
        );
    }

    // ---- A + B: resume + retry backoff ------------------------------------

    #[test]
    fn backoff_delay_ms_grows_exponentially_caps_and_bounds_jitter() {
        // Zero jitter → the base 0.5/1/2/4/8s sequence, capped at 8s.
        assert_eq!(backoff_delay_ms(1, 0), 500);
        assert_eq!(backoff_delay_ms(2, 0), 1000);
        assert_eq!(backoff_delay_ms(3, 0), 2000);
        assert_eq!(backoff_delay_ms(4, 0), 4000);
        assert_eq!(backoff_delay_ms(5, 0), 8000);
        assert_eq!(backoff_delay_ms(9, 0), 8000, "capped at 8s");
        // Jitter stays in [0, base/4] for any seed (base 2000 → [2000, 2500]).
        for seed in [1u64, 7, 123, 999_999, u64::MAX] {
            let d = backoff_delay_ms(3, seed);
            assert!((2000..=2500).contains(&d), "seed {seed} → {d}");
        }
    }

    #[test]
    fn content_range_total_parses_the_full_size() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_RANGE};
        let mut h = HeaderMap::new();
        h.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 500-999/2000"));
        assert_eq!(content_range_total(&h), Some(2000));
        // Unknown total (`*`) and a missing header → None (fall back to offset+len).
        h.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 0-99/*"));
        assert_eq!(content_range_total(&h), None);
        assert_eq!(content_range_total(&HeaderMap::new()), None);
    }

    #[test]
    fn rehash_partial_reproduces_the_whole_file_hash_and_count() {
        let dir = std::env::temp_dir().join(format!("rexenv-rehash-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("partial.bin");
        let bytes: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();

        // The re-hashed prefix must equal a fresh full hash of the same bytes, and
        // the count must equal the file length — so offset, content and hash stay
        // aligned on resume (the fail-closed guarantee rests on this).
        let ck = Checksum::Sha256(String::new()); // variant selects the algorithm
        let (hasher, count) = rehash_partial(&path, Some(&ck)).unwrap();
        assert_eq!(count, bytes.len() as u64);
        assert_eq!(hasher.unwrap().finish(), sha256_hex(&bytes));

        // No checksum → no hasher, count still correct.
        let (none_h, c2) = rehash_partial(&path, None).unwrap();
        assert!(none_h.is_none());
        assert_eq!(c2, bytes.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A scripted raw-HTTP server: each closure handles ONE connection (= one
    // download attempt), gets the request's Range offset (0 if none), and returns
    // the raw bytes to send before the socket closes — so a test can simulate a
    // mid-body drop, a 206 resume, a 200-ignoring-Range, or corruption.
    async fn scripted_server(responses: Vec<Box<dyn Fn(u64) -> Vec<u8> + Send + Sync>>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for build in responses {
                let Ok((mut stream, _)) = listener.accept().await else { break };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match stream.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_ascii_lowercase();
                let offset = req
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("range: bytes=")
                            .and_then(|r| r.split('-').next())
                            .and_then(|n| n.trim().parse::<u64>().ok())
                    })
                    .unwrap_or(0);
                let _ = stream.write_all(&build(offset)).await;
                let _ = stream.flush().await;
            }
        });
        format!("http://{addr}/file")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        hex_lower(&h.finalize())
    }

    /// `200 OK` claiming `total` bytes but sending only `body` (truncate `body`
    /// short of `total` to simulate a mid-body drop).
    fn http_200(total: usize, body: &[u8]) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n")
                .into_bytes();
        out.extend_from_slice(body);
        out
    }

    /// `206 Partial Content` from `offset` of a `total`-byte file, sending `body`.
    fn http_206(offset: u64, total: usize, body: &[u8]) -> Vec<u8> {
        let end = total as u64 - 1;
        let mut out = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {offset}-{end}/{total}\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn download_resumes_from_a_mid_body_drop_and_verifies() {
        let payload: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
        let ck = Checksum::Sha256(sha256_hex(&payload));
        let (p1, p2) = (payload.clone(), payload.clone());
        let url = scripted_server(vec![
            // Attempt 1: claim 2000 bytes, send only 1000, then drop.
            Box::new(move |_off| http_200(2000, &p1[..1000])),
            // Attempt 2: Range → 206 from the offset, send the remainder.
            Box::new(move |off| http_206(off, 2000, &p2[off as usize..])),
        ])
        .await;

        let dest = std::env::temp_dir().join(format!("rexenv-dl-resume-{}", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let r = download(&url, &dest, Some(&ck), None).await;
        assert!(r.is_ok(), "resume should complete: {r:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), payload, "assembled file is byte-complete");
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn download_resume_with_corrupted_bytes_fails_closed_and_removes_partial() {
        // The safety-critical half: a resumed range that serves a corrupted byte
        // must end in a checksum error with the partial removed — NEVER a silent
        // pass. The final SHA-256 over the whole assembled file is the gate.
        let payload: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
        let ck = Checksum::Sha256(sha256_hex(&payload));
        let p1 = payload.clone();
        let mut corrupt = payload.clone();
        corrupt[1500] ^= 0xFF; // flip a byte inside the resumed range
        let url = scripted_server(vec![
            Box::new(move |_off| http_200(2000, &p1[..1000])),
            Box::new(move |off| http_206(off, 2000, &corrupt[off as usize..])),
        ])
        .await;

        let dest = std::env::temp_dir().join(format!("rexenv-dl-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let r = download(&url, &dest, Some(&ck), None).await;
        assert!(r.is_err(), "a corrupted resume must NOT silently pass");
        assert!(
            r.unwrap_err().to_string().to_lowercase().contains("checksum"),
            "must fail on the checksum"
        );
        assert!(!dest.exists(), "the bad partial must be removed (fail closed)");
    }

    #[tokio::test]
    async fn download_restarts_when_the_server_ignores_range_with_200() {
        // If the server answers a Range request with 200 (full body from 0), we
        // must TRUNCATE and restart — appending would duplicate the prefix.
        let payload: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
        let ck = Checksum::Sha256(sha256_hex(&payload));
        let (p1, p2) = (payload.clone(), payload.clone());
        let url = scripted_server(vec![
            Box::new(move |_off| http_200(2000, &p1[..1000])), // drop → partial on disk
            Box::new(move |_off| http_200(2000, &p2)),         // ignores Range → full 200
        ])
        .await;

        let dest = std::env::temp_dir().join(format!("rexenv-dl-ignore-{}", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let r = download(&url, &dest, Some(&ck), None).await;
        assert!(r.is_ok(), "a 200 to a Range request must restart cleanly: {r:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), payload, "no duplicated prefix");
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn cached_bundle_dir_requires_the_member_present() {
        // A bundle dir is only valid when its member exists — the same gate
        // resolve_bundle uses, so adopt_startup never wires a half-extracted
        // tree (B28). Mirrors cached_bin's existence check.
        let base = std::env::temp_dir().join(format!("rexenv-cbd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        // No dir at all → None.
        assert_eq!(cached_bundle_dir_in(&base, "httpd", "2.4.68", "bin/httpd"), None);

        // Dir exists but the member is missing (the half-extracted case) → None.
        let dir = base.join("httpd-2.4.68");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        assert_eq!(cached_bundle_dir_in(&base, "httpd", "2.4.68", "bin/httpd"), None);

        // Member present → Some(the dir).
        std::fs::write(dir.join("bin/httpd"), b"#!/bin/sh\n").unwrap();
        assert_eq!(
            cached_bundle_dir_in(&base, "httpd", "2.4.68", "bin/httpd"),
            Some(dir.clone())
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_member_extracted_fails_loud_when_the_pinned_member_is_absent() {
        let base = std::env::temp_dir().join(format!("rexenv-bundle-member-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("bin")).unwrap();

        // Missing member → loud error naming it (never a silent publish → the
        // infinite re-download of B35).
        let e = ensure_member_extracted(&base, "redis", "8.0.0", "bin/redis-server")
            .unwrap_err()
            .to_string();
        assert!(e.contains("bin/redis-server") && e.contains("missing"), "{e}");

        // Present member → Ok (the normal, complete-extract path).
        std::fs::write(base.join("bin/redis-server"), b"x").unwrap();
        assert!(ensure_member_extracted(&base, "redis", "8.0.0", "bin/redis-server").is_ok());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn manifest_pins_caddy_per_arch() {
        let arm = manifest("caddy", CADDY_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("caddy_2.11.4_mac_arm64.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGz);
        assert_eq!(arm.member, "caddy");
        assert!(matches!(arm.checksum, Checksum::Sha512(_)));
        assert_eq!(checksum_hex(&arm.checksum).len(), 128); // SHA-512 hex

        let amd = manifest("caddy", CADDY_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("caddy_2.11.4_mac_amd64.tar.gz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_php_cli_and_fpm() {
        let cli = manifest("php", PHP_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(cli.url.ends_with("php-8.3.31-cli-macos-aarch64.tar.gz"));
        assert_eq!(cli.member, "php");
        assert!(matches!(cli.checksum, Checksum::Sha256(_)));
        assert_eq!(checksum_hex(&cli.checksum).len(), 64); // SHA-256 hex

        let fpm = manifest("php-fpm", PHP_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(fpm.url.ends_with("php-8.3.31-fpm-macos-x86_64.tar.gz"));
        assert_eq!(fpm.member, "php-fpm");
        assert_ne!(checksum_hex(&cli.checksum), checksum_hex(&fpm.checksum));
    }

    #[test]
    fn php_debug_variant_is_wired_but_unresolvable_until_hosted() {
        // §11.2: the debug variant exists in the manifest but is gated on a pinned
        // checksum. The artifact isn't built/hosted yet, so it MUST NOT resolve —
        // we never want to fetch a non-existent file.
        for arch in [Arch::Arm64, Arch::X86_64] {
            assert!(php_debug_sha256("cli", arch).is_none(), "debug cli unexpectedly pinned");
            assert!(php_debug_sha256("fpm", arch).is_none(), "debug fpm unexpectedly pinned");
            assert!(manifest("php-debug", PHP_DEBUG_VERSION, "macos", arch).is_none());
            assert!(manifest("php-fpm-debug", PHP_DEBUG_VERSION, "macos", arch).is_none());
        }
    }

    /// A tag that does not exist, standing in for the one an upload will create.
    /// The URL SHAPE is knowable today; only the tag is not.
    const SAMPLE_DEBUG_TAG: &str = "php-8.3.31-xdebug-1";

    #[test]
    fn php_debug_spec_has_the_expected_url_and_member_shape() {
        // The pure URL builder defines the contract the hosted artifact must meet:
        // `-xdebug` tagged, under the release tag, with the right member binary.
        let cli = php_debug_url(SAMPLE_DEBUG_TAG, "cli", Arch::Arm64);
        assert!(cli.ends_with("php-8.3.31-cli-xdebug-macos-aarch64.tar.gz"), "{cli}");
        assert_eq!(php_debug_url(SAMPLE_DEBUG_TAG, "fpm", Arch::X86_64).split('/').next_back(),
            Some("php-8.3.31-fpm-xdebug-macos-x86_64.tar.gz"));

        // The host, asserted against a LITERAL rather than the const it is built
        // from. The previous form was `starts_with(PHP_DEBUG_BASE_URL)` on a URL
        // formatted from that same const — it could not fail, and so said nothing
        // when the const named a host B33 had already retired.
        assert!(
            cli.starts_with("https://github.com/rexenv/runtimes/releases/download/"),
            "B33: the debug build is hosted in rexenv/runtimes, not on a bucket: {cli}"
        );
        assert!(!cli.contains("dl.rexenv.dev"), "the retired host is back: {cli}");
    }

    /// **Hosting the debug build takes two edits, and either one alone must not
    /// resolve.**
    ///
    /// The recipe's natural order is upload → hash → pin, so the digests get
    /// filled first; the release tag is the edit that is easy to forget because
    /// nothing fails without it. Before `PHP_DEBUG_TAG` existed the URL host was
    /// a const nobody would revisit at that moment, which is how a retired host
    /// stayed wired in for a week after the ruling that retired it.
    ///
    /// This asserts the TAG is load-bearing, not just present: with digests
    /// present but no tag, the spec is still `None`.
    #[test]
    fn a_debug_build_with_digests_but_no_release_tag_does_not_resolve() {
        // Today both halves are empty, so the whole thing is unresolvable.
        assert!(PHP_DEBUG_TAG.is_empty(), "the tag is filled — update this test with the pin");
        assert!(php_debug_spec("cli", Arch::Arm64).is_none());

        // The half-done states, driven through the gate's parameters because
        // neither is reachable from the consts today.
        let digest = Some("0".repeat(64));
        let digest = digest.as_deref();
        for kind in ["cli", "fpm"] {
            for arch in [Arch::Arm64, Arch::X86_64] {
                // Digests pinned, tag forgotten — the order the recipe produces.
                assert!(
                    php_debug_spec_from("", digest, kind, arch).is_none(),
                    "{kind}/{arch:?} resolved with digests but no release tag"
                );
                // Tag filled, digests forgotten — the pre-existing gate, kept.
                assert!(
                    php_debug_spec_from(SAMPLE_DEBUG_TAG, None, kind, arch).is_none(),
                    "{kind}/{arch:?} resolved with a tag but no digest"
                );
                // Both, which is what hosting actually means.
                let spec = php_debug_spec_from(SAMPLE_DEBUG_TAG, digest, kind, arch)
                    .expect("tag + digest ⇒ resolvable");
                assert!(spec.url.contains(SAMPLE_DEBUG_TAG), "{}", spec.url);
                assert_eq!(spec.member, if kind == "fpm" { "php-fpm" } else { "php" });
            }
        }
    }

    #[test]
    fn bundle_manifest_pins_redis_per_arch() {
        for arch in [Arch::Arm64, Arch::X86_64] {
            let bundle = bundle_manifest("redis", REDIS_VERSION, "macos", arch).unwrap();
            assert_eq!(bundle.member, "bin/redis-server");
            assert_eq!(bundle.parts.len(), 2);
            let formulas: Vec<&str> = bundle.parts.iter().map(|p| p.formula).collect();
            assert_eq!(formulas, vec!["redis", "openssl@3"]);
            for part in &bundle.parts {
                // ghcr blobs are content-addressed: the URL must embed the
                // exact digest we pin, so bytes can never drift under a URL.
                let digest = checksum_hex(&part.checksum);
                assert_eq!(digest.len(), 64);
                assert!(part.url.ends_with(&format!("blobs/sha256:{digest}")), "{}", part.url);
                assert!(!part.include.is_empty());
            }
            // The versioned-formula path maps `@` to `/` on ghcr.
            assert!(bundle.parts[1].url.contains("homebrew/core/openssl/3/blobs/"));
            // Only runtime dylibs from openssl — never headers/static libs.
            assert_eq!(
                bundle.parts[1].include,
                &["lib/libssl.3.dylib", "lib/libcrypto.3.dylib"]
            );
        }
        // Arches pin distinct bottles.
        let arm = bundle_manifest("redis", REDIS_VERSION, "macos", Arch::Arm64).unwrap();
        let amd = bundle_manifest("redis", REDIS_VERSION, "macos", Arch::X86_64).unwrap();
        assert_ne!(
            checksum_hex(&arm.parts[0].checksum),
            checksum_hex(&amd.parts[0].checksum)
        );
        // Bundles and plain manifests are disjoint namespaces.
        assert!(manifest("redis", REDIS_VERSION, "macos", Arch::Arm64).is_none());
        assert!(bundle_manifest("mysql", MYSQL_VERSION, "macos", Arch::Arm64).is_none());
        assert!(bundle_manifest("redis", "0.0.1", "macos", Arch::Arm64).is_none());
    }

    #[test]
    fn bundle_manifest_pins_mariadb_per_arch() {
        for arch in [Arch::Arm64, Arch::X86_64] {
            let bundle = bundle_manifest("mariadb", MARIADB_VERSION, "macos", arch).unwrap();
            assert_eq!(bundle.member, "bin/mariadbd");
            let formulas: Vec<&str> = bundle.parts.iter().map(|p| p.formula).collect();
            assert_eq!(formulas, vec!["mariadb", "openssl@3", "pcre2"]);
            for part in &bundle.parts {
                let digest = checksum_hex(&part.checksum);
                assert!(part.url.ends_with(&format!("blobs/sha256:{digest}")), "{}", part.url);
            }
            // Server + both bundled clients + the runtime share data.
            let mdb = bundle.parts[0].include;
            for needed in [
                "bin/mariadbd",
                "bin/mariadb",
                "bin/mariadb-dump",
                "share/mysql/english",
                "share/mysql/charsets",
            ] {
                assert!(mdb.contains(&needed), "{needed} missing");
            }
            // mariadbd links exactly these two extra formulas' dylibs.
            assert_eq!(bundle.parts[2].include, &["lib/libpcre2-8.0.dylib"]);
        }
        let arm = bundle_manifest("mariadb", MARIADB_VERSION, "macos", Arch::Arm64).unwrap();
        let amd = bundle_manifest("mariadb", MARIADB_VERSION, "macos", Arch::X86_64).unwrap();
        assert_ne!(
            checksum_hex(&arm.parts[0].checksum),
            checksum_hex(&amd.parts[0].checksum)
        );
        assert!(manifest("mariadb", MARIADB_VERSION, "macos", Arch::Arm64).is_none());
    }

    #[test]
    fn bundle_manifest_pins_xdebug_per_supported_minor() {
        for minor in ["8.1", "8.2", "8.3", "8.4", "8.5"] {
            assert!(xdebug_supported(minor), "{minor}");
            let (name, version) = xdebug_bundle_id(minor).unwrap();
            assert_eq!(name, format!("xdebug-{minor}"));
            // These minors are all inside Xdebug's current support window, so
            // they sit at the default — asserted from the SAME accessor the UI
            // reads, not from the constant, so a minor frozen at an older
            // release would show up here rather than hide behind the constant.
            assert_eq!(version, XDEBUG_VERSION);
            assert_eq!(xdebug_version_for(minor), Some(version));
            for arch in [Arch::Arm64, Arch::X86_64] {
                let bundle = bundle_manifest(&name, version, "macos", arch).unwrap();
                assert_eq!(bundle.member, "xdebug.so");
                assert_eq!(bundle.parts.len(), 1, "one-part bundle");
                let part = &bundle.parts[0];
                assert_eq!(part.formula, format!("xdebug@{minor}"));
                assert_eq!(part.include, &["xdebug.so"]);
                let digest = checksum_hex(&part.checksum);
                assert_eq!(digest.len(), 64, "pinned digest, not a stub");
                // shivammathur tap root, NOT homebrew/core; content-addressed.
                assert_eq!(
                    part.url,
                    format!(
                        "https://ghcr.io/v2/shivammathur/extensions/xdebug/{minor}/blobs/sha256:{digest}"
                    )
                );
            }
            let arm = bundle_manifest(&name, version, "macos", Arch::Arm64).unwrap();
            let amd = bundle_manifest(&name, version, "macos", Arch::X86_64).unwrap();
            assert_ne!(
                checksum_hex(&arm.parts[0].checksum),
                checksum_hex(&amd.parts[0].checksum)
            );
        }
    }

    /// **Every PHP rexenv OFFERS must have an explicit Xdebug verdict — the
    /// undecided state is unreachable for a shipped version.**
    ///
    /// This is the guard the whole `XdebugStatus` split exists for. Splitting
    /// the two absences apart fixes today's message; it does not stop tomorrow's
    /// minor from falling through `_ => NotPinned` and picking up a reason
    /// nobody chose. Adding a version to `PHP_VERSIONS` now fails here until
    /// somebody decides which of the three it is, which is the one moment the
    /// answer is actually known.
    ///
    /// Derived from `PHP_VERSIONS`, so a new minor joins the guard by existing
    /// rather than by being remembered — the same derivation #336 and #377 had
    /// to be corrected into after keying on names.
    #[test]
    fn every_offered_php_minor_has_an_explicit_xdebug_verdict() {
        let mut available = 0;
        let mut cannot_load = 0;
        for v in PHP_VERSIONS {
            let minor = v.rsplit_once('.').map(|(m, _)| m).unwrap_or(v);
            match xdebug_status(minor) {
                XdebugStatus::Available(b) => {
                    available += 1;
                    assert_eq!(
                        checksum_hex(&Checksum::Sha256(b.arm64.into())).len(),
                        64,
                        "{minor}: available but its arm64 digest is not a SHA-256"
                    );
                }
                XdebugStatus::CannotLoadExtensions { measured } => {
                    cannot_load += 1;
                    // A measurement with no provenance is an assumption wearing
                    // a measurement's clothes.
                    assert!(
                        measured.contains("20"),
                        "{minor}: CannotLoadExtensions with no date — say when it was measured"
                    );
                }
                XdebugStatus::NotPinned => panic!(
                    "PHP {minor} is in PHP_VERSIONS but has no Xdebug verdict. Decide in \
                     `xdebug_status`: pin a bottle (Available), record the nm -gU result \
                     (CannotLoadExtensions), or take the version out of PHP_VERSIONS. \
                     Falling through to NotPinned tells the user a reason nobody chose."
                ),
            }
        }
        // Landmarks. A table gutted to one arm would satisfy every assertion
        // above by having nothing to iterate.
        assert!(available >= 5, "only {available} minors with a pinned bottle");
        assert_eq!(cannot_load, 2, "expected exactly 7.4 and 8.0 to be unloadable");

        // NotPinned must still be REACHABLE — it is the honest answer for a
        // minor rexenv does not offer, and a version that no longer exists.
        assert!(matches!(xdebug_status("8.9"), XdebugStatus::NotPinned));
    }

    /// The refusal a user reads differs with the reason, and neither sentence
    /// may be the other's.
    ///
    /// The old single message ("its static build can't load extensions") was
    /// true of both absences that existed, which is exactly why the conflation
    /// survived: it was correct until the day it silently was not.
    #[test]
    fn the_xdebug_refusal_says_which_kind_of_unavailable_it_is() {
        assert!(xdebug_unavailable_reason("8.3").is_none(), "8.3 has a pinned bottle");

        for minor in ["7.4", "8.0"] {
            let why = xdebug_unavailable_reason(minor).expect("no bottle for this minor");
            assert!(why.contains(minor), "{minor}: the message does not name the version");
            assert!(why.contains("exports no Zend symbols"), "{minor}: {why}");
            assert!(why.contains("8.1 or newer"), "{minor}: no way out offered — {why}");
            // The permanence is the point: a user who reads "not yet" will wait
            // for a fix that is not coming.
            assert!(why.contains("No version of Xdebug can change that"), "{minor}: {why}");
        }

        let unpinned = xdebug_unavailable_reason("8.9").expect("8.9 has no bottle");
        assert!(unpinned.contains("8.9"));
        assert!(
            !unpinned.contains("exports no Zend symbols"),
            "an unpinned minor must not be told its PHP is broken: {unpinned}"
        );
        assert!(
            !unpinned.contains("8.1 or newer"),
            "'switch to something newer' is not advice for a minor NEWER than the table: {unpinned}"
        );
    }

    #[test]
    fn xdebug_is_refused_for_php_80_and_unknown_minors() {
        // 8.0's static build exports no Zend symbols — dlopen fails, so the
        // toggle must be unofferable by construction.
        for minor in ["8.0", crate::core::php::unshipped_minor(), "9.0", ""] {
            assert!(!xdebug_supported(minor), "{minor}");
            assert!(xdebug_bundle_id(minor).is_none());
            assert!(xdebug_version_for(minor).is_none(), "{minor}");
            assert!(bundle_manifest(&format!("xdebug-{minor}"), XDEBUG_VERSION, "macos", Arch::Arm64)
                .is_none());
        }
        // Wrong version never resolves (cache-dir identity is honest).
        assert!(bundle_manifest("xdebug-8.4", "0.0.1", "macos", Arch::Arm64).is_none());
    }

    /// The version a minor gets is that minor's ROW, never one app-wide pin.
    /// 7.4's last Xdebug is 3.1.6 and no later one will exist, so a single
    /// constant would make `xdebug-7.4` unresolvable the day 7.4 ships —
    /// silently, because `bundle_manifest` returning None reads exactly like
    /// "this minor has no Xdebug" (`docs/PLAN-php-74-support.md` §4.6).
    #[test]
    fn a_minors_xdebug_version_comes_from_its_own_row() {
        // A row frozen OFF the default — the shape 7.4 will have. Built here
        // rather than added to the table so the mechanism is proven BEFORE the
        // first frozen minor ships, which is the only time the proof is worth
        // anything: once 7.4 is in the table, a broken gate is a live bug.
        let frozen = XdebugBottle {
            version: "3.1.6", // the last Xdebug for PHP 7.4; there will be no other
            formula: "xdebug@7.4",
            arm64: XDEBUG_PHP81_BOTTLE_ARM64_SHA256, // stand-ins: identity is what's under test
            amd64: XDEBUG_PHP81_BOTTLE_AMD64_SHA256,
        };
        assert_ne!(frozen.version, XDEBUG_VERSION, "the fixture must differ from the default");

        // The spec follows the ROW: its formula, its digests, its tap path.
        let spec = xdebug_spec(&frozen, Arch::Arm64);
        assert_eq!(spec.member, "xdebug.so");
        assert_eq!(spec.parts[0].formula, "xdebug@7.4");
        assert!(
            spec.parts[0].url.starts_with("https://ghcr.io/v2/shivammathur/extensions/xdebug/7.4/"),
            "{}",
            spec.parts[0].url
        );
        // …and the two arches stay distinct through the row, as everywhere else.
        assert_ne!(
            checksum_hex(&spec.parts[0].checksum),
            checksum_hex(&xdebug_spec(&frozen, Arch::X86_64).parts[0].checksum)
        );

        // The GATE is the row's version, not the constant. For a supported
        // minor, its own version resolves and any other — including a frozen
        // minor's — does not. A single app-wide pin would invert this: the
        // frozen minor would resolve to nothing, silently, and `None` reads
        // exactly like "this minor has no Xdebug".
        assert!(bundle_manifest("xdebug-8.4", XDEBUG_VERSION, "macos", Arch::Arm64).is_some());
        assert!(bundle_manifest("xdebug-8.4", frozen.version, "macos", Arch::Arm64).is_none());
        assert_eq!(xdebug_version_for("8.4"), Some(XDEBUG_VERSION));
    }

    #[test]
    fn filtered_tree_extract_strips_and_includes() {
        // Build a bottle-shaped tar.gz in memory: `<formula>/<version>/…`.
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, data) in [
            ("redis/8.8.0/bin/redis-server", "elf"),
            ("redis/8.8.0/.brew/redis.rb", "receipt"),
            ("redis/8.8.0/README.md", "docs"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, data.as_bytes()).unwrap();
        }
        let gz = builder.into_inner().unwrap().finish().unwrap();

        let dest = std::env::temp_dir().join("rexenv-bottle-extract-test");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        extract_tar_gz_tree_filtered(&gz[..], &dest, 2, Some(&["bin"])).unwrap();
        // The included subtree landed post-strip; everything else was skipped.
        assert!(dest.join("bin/redis-server").is_file());
        assert!(!dest.join(".brew").exists());
        assert!(!dest.join("README.md").exists());
        assert!(!dest.join("redis").exists(), "strip must drop formula/version dirs");
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn manifest_pins_every_pinned_php_version() {
        for v in PHP_VERSIONS {
            for arch in [Arch::Arm64, Arch::X86_64] {
                let cli = manifest("php", v, "macos", arch).unwrap();
                assert!(cli.url.contains(&format!("php-{v}-cli-macos-")));
                assert_eq!(cli.member, "php");
                assert!(matches!(cli.checksum, Checksum::Sha256(_)));
                assert_eq!(checksum_hex(&cli.checksum).len(), 64); // SHA-256 hex

                let fpm = manifest("php-fpm", v, "macos", arch).unwrap();
                assert!(fpm.url.contains(&format!("php-{v}-fpm-macos-")));
                assert_eq!(fpm.member, "php-fpm");
                // cli and fpm of the same version/arch are distinct artifacts.
                assert_ne!(checksum_hex(&cli.checksum), checksum_hex(&fpm.checksum));
            }
            // arm64 and x86_64 builds of a version are distinct artifacts.
            let arm = manifest("php", v, "macos", Arch::Arm64).unwrap();
            let amd = manifest("php", v, "macos", Arch::X86_64).unwrap();
            assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
            // …and it points at the source that actually PUBLISHES it. Shape
            // alone is not enough: every 404 in this family has the right shape.
            let expected_host = if php_self_hosted_tag(v).is_some() {
                "https://github.com/rexenv/runtimes/releases/download/"
            } else {
                "https://dl.static-php.dev/static-php-cli/bulk/"
            };
            assert!(arm.url.starts_with(expected_host), "{v}: {}", arm.url);
        }
    }

    /// The cache must key on the pinned BYTES, not on a file existing.
    ///
    /// rexenv's own 7.4.33 shipped without `phar`, was rebuilt at the SAME
    /// version, and every machine holding the broken artifact would have kept it
    /// forever — `resolve` returned early on existence and the dir name
    /// (`<name>-<version>`) had not changed. Proven against a real stale cache on
    /// 15 Aug 2026: the marker mismatched, the dir was dropped, the fixed build
    /// downloaded, and `php_tools_check` went from FAIL to PASS.
    #[test]
    fn a_cache_dir_is_stale_when_its_recorded_digest_is_not_the_pin() {
        let dir = std::env::temp_dir().join(format!("rexenv-pin-marker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pin = Checksum::Sha256("a".repeat(64));

        // Recorded and matching → cached.
        write_pin_marker(&dir, &pin);
        assert!(cache_matches_pin(&dir, "7.4.33", &pin));

        // Recorded but DIFFERENT → stale, which is the re-issued-artifact case.
        write_pin_marker(&dir, &Checksum::Sha256("b".repeat(64)));
        assert!(!cache_matches_pin(&dir, "7.4.33", &pin));

        // ABSENT is asymmetric on purpose. A self-hosted version is one whose
        // bytes can be re-issued at an unchanged version, so an unmarked cache
        // of one is refetched once; an upstream version is grandfathered, or
        // every existing install would re-download everything to catch a case
        // upstream has not caused.
        std::fs::remove_file(dir.join(PIN_MARKER)).unwrap();
        assert!(php_self_hosted_tag("7.4.33").is_some());
        assert!(!cache_matches_pin(&dir, "7.4.33", &pin), "self-hosted, unmarked → refetch");
        assert!(php_self_hosted_tag(PHP_VERSION).is_none());
        assert!(cache_matches_pin(&dir, PHP_VERSION, &pin), "upstream, unmarked → grandfathered");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S0.3, the bundle sibling of the pin marker above: a published bundle
    /// tree is cached only while it carries a CURRENT prepare receipt. The
    /// member file existing cannot see "dyld refuses this tree" — #319
    /// published exactly that, and the member-stat early return made it
    /// unrecoverable in the field. Absence is stale HERE (unlike the pin
    /// marker's grandfathering) because every pre-receipt tree is from the
    /// era that includes the broken predicate, and no stat can tell a good
    /// one from a poisoned one.
    #[test]
    fn a_bundle_tree_is_cached_only_with_a_current_prepare_receipt() {
        let dir =
            std::env::temp_dir().join(format!("rexenv-prep-receipt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Absent → stale (the whole point: pre-receipt trees refetch once).
        assert!(!bundle_prepared_current(&dir), "an unreceipted tree must be stale");

        // Current rev → cached.
        write_prepare_receipt(&dir).unwrap();
        assert!(bundle_prepared_current(&dir));

        // A PAST rev → stale: bumping PREPARE_REV is how a prepare-logic fix
        // reaches machines already holding the broken output.
        std::fs::write(dir.join(PREPARE_RECEIPT), "0").unwrap();
        assert!(!bundle_prepared_current(&dir), "an old-rev tree must be stale");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The trap this branch exists to defuse. `manifest`'s PHP arms gate ONLY on
    /// `php_sha256(..).is_some()`, so pinning a self-built version's four hashes
    /// while the URL was still hardcoded to static-php.dev would have produced a
    /// manifest resolving to a permanent 404 — and the test above would have
    /// stayed green, because it asserted the URL's SHAPE. The failure would have
    /// surfaced on a user's machine as "php-fpm 7.4 download failed".
    ///
    /// So the source branch is asserted DIRECTLY, on the pure URL builder, while
    /// the version is still unresolvable. It cannot wait for the pin: by then the
    /// mistake has already shipped.
    #[test]
    fn a_self_hosted_php_never_points_at_static_php_dev() {
        for kind in ["cli", "fpm"] {
            for arch in [Arch::Arm64, Arch::X86_64] {
                let url = php_url(kind, "7.4.33", arch);
                assert!(!url.contains("dl.static-php.dev"), "{url}");
                // The FULL immutable tag is in the URL, not a stable base that a
                // rebuild could quietly refill (see `php_self_hosted_tag`). The
                // tag is READ, not spelled: it was written as `php-7.4.33-1`
                // here and the pin has moved twice since (-4, then -6), so a
                // literal would fail for being right about the wrong thing.
                // Every sentence in this file that spelled a tag went stale;
                // this assertion did not, because it asks the code.
                let tag = php_self_hosted_tag("7.4.33").expect("7.4.33 is self-hosted");
                assert!(url.contains(&format!("/releases/download/{tag}/")), "{url}");
                assert!(url.ends_with(&format!("php-7.4.33-{kind}-macos-{}.tar.gz", php_arch(arch))));
                // A version static-php.dev DOES publish still comes from there.
                assert!(php_url(kind, PHP_VERSION, arch).contains("dl.static-php.dev"));
            }
        }
        // It RESOLVES now — the artifact exists (first pinned 14 Aug 2026; the
        // live tag is `php_self_hosted_tag`, deliberately not repeated here).
        // This assertion was the inverse until then: "wired but
        // unresolvable", which is what an empty checksum const buys. Flipping it
        // in the same commit as the pin is the point — the two facts must never
        // disagree, because a version that resolves without a real artifact is
        // a permanent 404 on a user's machine.
        for name in ["php", "php-fpm"] {
            for arch in [Arch::Arm64, Arch::X86_64] {
                let spec = manifest(name, "7.4.33", "macos", arch)
                    .unwrap_or_else(|| panic!("{name} 7.4.33 must resolve now that it is pinned"));
                assert_eq!(checksum_hex(&spec.checksum).len(), 64, "{name}: real digest, not a stub");
            }
        }
        assert!(PHP_VERSIONS.contains(&"7.4.33"), "7.4 is pinned but not offered");
        // cli and fpm are distinct artifacts; so are the two arches.
        let a = manifest("php", "7.4.33", "macos", Arch::Arm64).unwrap();
        let b = manifest("php-fpm", "7.4.33", "macos", Arch::Arm64).unwrap();
        let c = manifest("php", "7.4.33", "macos", Arch::X86_64).unwrap();
        assert_ne!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
        assert_ne!(checksum_hex(&a.checksum), checksum_hex(&c.checksum));
    }

    /// **If we built it, its licence texts ship with it — derived, never listed.**
    ///
    /// The obligation attaches to the act of DISTRIBUTING, so the guard is keyed
    /// on the artifact's HOST. A second self-built runtime therefore inherits the
    /// check by existing: pin its binaries, forget its licences, and this fails
    /// before it ships. That direction matters more than today's row — the 7.4
    /// licences are present and will stay present; the NEXT one is the one nobody
    /// is thinking about, and shipping somebody's code without its licence is not
    /// a defect you get to fix in the following release.
    ///
    /// **The previous version of this guard said exactly that and was wrong.** It
    /// keyed on `php_self_hosted_tag` behind a `name == "php" || name == "php-fpm"`
    /// check, and the second self-built runtime was already in the file:
    /// `php-debug` is a custom static-php compile rexenv hosts itself (see
    /// `PHP_DEBUG_TAG`), and the name check excluded it. See
    /// `the_debug_build_is_ours_too`.
    #[test]
    fn every_php_we_distribute_ourselves_ships_its_licences() {
        let mut ours = 0;
        for v in PHP_VERSIONS {
            for name in ["php", "php-fpm"] {
                for arch in [Arch::Arm64, Arch::X86_64] {
                    let spec = manifest(name, v, "macos", arch).expect("pinned");
                    if !is_self_distributed(&spec.url) {
                        // Somebody else's build: we owe nothing, and must not
                        // invent an obligation by fetching an archive that does
                        // not exist.
                        assert!(
                            licenses_spec(&spec.url, name, v, arch).unwrap().is_none(),
                            "{name} {v} is upstream's build — rexenv is not its distributor"
                        );
                        continue;
                    }
                    ours += 1;
                    // An artifact we host with no licence pin is an ERROR, never
                    // a quiet "nothing owed" — that is the whole fail direction.
                    let lic = licenses_spec(&spec.url, name, v, arch)
                        .unwrap_or_else(|e| panic!("{e}"))
                        .expect("ours ⇒ a licence spec");
                    // Same immutable release as the bytes it covers, and now by
                    // CONSTRUCTION rather than by assertion: the licence URL is a
                    // sibling of the artifact's own URL. A licence archive from a
                    // DIFFERENT build documents a different set of statically
                    // linked deps, which is a quiet way to be wrong.
                    let dir = &spec.url[..spec.url.rfind('/').unwrap()];
                    assert!(
                        lic.url.starts_with(dir),
                        "licences must come from the same release as the binary: {} vs {}",
                        lic.url,
                        spec.url
                    );
                    assert_eq!(checksum_hex(&lic.checksum).len(), 64, "real digest, not a stub");
                    assert_eq!(lic.archive, Archive::TarGzTree, "a tree of texts, not one file");
                    // Both arches pinned, and pinned SEPARATELY — the archives
                    // carry the same file list but are distinct artifacts, and a
                    // copy-paste that pointed both at one digest would fail the
                    // download for the other rather than being caught here.
                    let arm = manifest(name, v, "macos", Arch::Arm64).unwrap().url;
                    let amd = manifest(name, v, "macos", Arch::X86_64).unwrap().url;
                    assert_ne!(
                        checksum_hex(
                            &licenses_spec(&arm, name, v, Arch::Arm64).unwrap().unwrap().checksum
                        ),
                        checksum_hex(
                            &licenses_spec(&amd, name, v, Arch::X86_64).unwrap().unwrap().checksum
                        ),
                        "both arches share a licence digest — one of them is wrong"
                    );
                }
            }
            // Only what WE host is ours to cover; nothing else grows the duty.
            let caddy = manifest("caddy", CADDY_VERSION, "macos", Arch::Arm64).unwrap();
            assert!(!is_self_distributed(&caddy.url));
            assert!(licenses_spec(&caddy.url, "caddy", v, Arch::Arm64).unwrap().is_none());
        }
        assert!(ours > 0, "no self-hosted PHP — this guard is now vacuous, delete or fix it");
    }

    /// **Every artifact rexenv hosts takes a path that carries the obligation.**
    ///
    /// The whole-surface version of #336, and the reason it exists: the licence
    /// step lived inside [`resolve`] only, so the obligation attached to the
    /// SHAPE 7.4 happens to have rather than to the artifact. An entry that is a
    /// tree or a plain file took a path with no licence step at all — and
    /// `php-debug` is self-hosted AND will be a tree, one pin away from exactly
    /// that.
    ///
    /// Derived by enumerating every artifact the manifests can produce, so a new
    /// one joins by existing rather than by being added to a list here. It goes
    /// green today (only 7.4 is ours, and it is `Single`); its whole value is
    /// failing the day something we host moves shape.
    #[test]
    fn nothing_we_host_can_reach_a_resolve_path_without_the_licence_step() {
        // Shapes whose resolve calls `stage_licenses` + `licenses_satisfied`.
        const CARRIES_OBLIGATION: &[Shape] = &[Shape::Single, Shape::File, Shape::Dir];
        let mut ours = 0;

        for (name, versions) in every_pinned_artifact() {
            for v in versions {
                for arch in [Arch::Arm64, Arch::X86_64] {
                    // Bundles are handled by their own refusal (no part has a
                    // version to pin a licence against) — asserted separately.
                    if shape_of(&name) == Shape::Bundle {
                        if let Some(b) = bundle_manifest(&name, &v, "macos", arch) {
                            assert!(
                                refuse_self_distributed_bundle(&b, &name, &v).is_ok(),
                                "a bundle part is now self-hosted — see the refusal's message"
                            );
                        }
                        continue;
                    }
                    let Some(spec) = manifest(&name, &v, "macos", arch) else {
                        continue;
                    };
                    if !is_self_distributed(&spec.url) {
                        continue;
                    }
                    ours += 1;
                    assert!(
                        CARRIES_OBLIGATION.contains(&shape_of(&name)),
                        "{name} {v} is served from our own infrastructure ({}) but resolves \
                         through a path with no licence step",
                        spec.url
                    );
                    // …and it must actually be able to name them.
                    licenses_spec(&spec.url, &name, &v, arch)
                        .unwrap_or_else(|e| panic!("{e}"))
                        .unwrap_or_else(|| panic!("{name} {v} is ours but names no licences"));
                }
            }
        }
        assert!(ours > 0, "nothing self-hosted — this guard is now vacuous, delete or fix it");

        // The assert above says "these shapes carry the obligation". That is only
        // true if their resolves actually call it, which is SOURCE TEXT here, not
        // behaviour — the same honest limit ledger #175's ordering guard states.
        // A real proof needs a resolve against a self-hosted tree, which is L1
        // (`php_versions_check`). Worth having anyway: the defect being prevented
        // is someone deleting the call, and that is exactly what this sees.
        // Matched on the CALL FORM, not on the name: a substring search finds
        // this assertion's own literal too, which is the scanner-counts-itself
        // trap the copy-scan guards already record.
        const SRC: &str = include_str!("binaries.rs");
        let calls = SRC
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("stage_licenses(&spec") && l.ends_with(".await?;"))
            .count();
        assert_eq!(
            calls, 3,
            "each single-spec resolve (resolve, resolve_file, resolve_dir) must stage the \
             licences it owes — found {calls} call sites"
        );
    }

    /// Every (name, versions) pair the manifests can produce. Derived from the
    /// version constants rather than listed, so this cannot silently stop
    /// covering something.
    fn every_pinned_artifact() -> Vec<(String, Vec<String>)> {
        let one = |v: &str| vec![v.to_string()];
        let mut out: Vec<(String, Vec<String>)> = vec![
            ("caddy".into(), one(CADDY_VERSION)),
            ("nginx".into(), one(NGINX_VERSION)),
            ("mailpit".into(), one(MAILPIT_VERSION)),
            ("frankenphp".into(), one(FRANKENPHP_VERSION)),
            ("composer".into(), one(COMPOSER_VERSION)),
            ("wp-cli".into(), one(WP_CLI_VERSION)),
            ("adminer".into(), one(ADMINER_VERSION)),
            ("cloudflared".into(), one(CLOUDFLARED_VERSION)),
        ];
        let all = |vs: &[&str]| vs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for n in ["php", "php-fpm"] {
            out.push((n.into(), all(PHP_VERSIONS)));
        }
        for n in ["php-debug", "php-fpm-debug"] {
            out.push((n.into(), one(PHP_DEBUG_VERSION)));
        }
        out.push(("mysql".into(), all(MYSQL_VERSIONS)));
        out.push(("postgres".into(), all(POSTGRES_VERSIONS)));
        out.push(("mariadb".into(), all(MARIADB_VERSIONS)));
        out.push(("redis".into(), all(REDIS_VERSIONS)));
        out.push(("httpd".into(), one(HTTPD_VERSION)));
        out
    }

    /// **A manifest can ADD a version; it can never move one the app pins.**
    ///
    /// `php_spec` asks the compiled-in table FIRST and only then the catalog, so
    /// a signed document — however valid — cannot point 8.3.31 at different
    /// bytes. That ordering IS the "compiled-in pins remain the floor" property:
    /// without it one manifest reaches every version on every install, and the
    /// signature becomes the only thing between a user and arbitrary bytes for
    /// software they already have.
    ///
    /// Asserted by installing a catalog directly — the TYPE is the proof (only
    /// `updates::verify` builds one in a shipping build), so this tests the
    /// PRECEDENCE, not the verification, which `core::updates` covers.
    #[test]
    fn a_catalog_can_add_a_version_but_never_override_a_pin() {
        let pinned = PHP_VERSION;
        let before = manifest("php", pinned, "macos", Arch::Arm64).expect("the pin resolves");
        let added = format!("{}.9999", crate::core::php::minor_of(pinned));

        let _catalog = catalog_test_lock();
        install_catalog(crate::core::updates::catalog_for_tests(&[
            ("php", pinned, "arm64", "https://dl.static-php.dev/evil", &"b".repeat(64)),
            ("php", &added, "arm64", "https://dl.static-php.dev/new", &"c".repeat(64)),
        ]));

        // The pin is UNMOVED — same digest and URL as before the catalog existed.
        let after = manifest("php", pinned, "macos", Arch::Arm64).unwrap();
        assert_eq!(checksum_hex(&after.checksum), checksum_hex(&before.checksum));
        assert_eq!(after.url, before.url);
        assert_ne!(checksum_hex(&after.checksum), "b".repeat(64), "the manifest moved a pin");

        // …and the ADDED version resolves, which is the feature.
        let new_spec =
            manifest("php", &added, "macos", Arch::Arm64).expect("a catalog version must resolve");
        assert_eq!(checksum_hex(&new_spec.checksum), "c".repeat(64));
        assert_eq!(new_spec.url, "https://dl.static-php.dev/new");
        assert_eq!(new_spec.member, "php");
        // The other arch was not offered, so it must not resolve — `php_sha256`'s
        // both-arches invariant holds for the catalog too.
        assert!(manifest("php", &added, "macos", Arch::X86_64).is_none());

        // Leave no catalog behind for the rest of this test binary.
        install_catalog(crate::core::updates::VersionCatalog::default());
        assert!(manifest("php", &added, "macos", Arch::Arm64).is_none());
    }

    /// **A self-built PHP must say which source it was built from, in the file
    /// that decides what gets downloaded.**
    ///
    /// For every PHP rexenv builds itself there is no upstream release to name:
    /// 7.4 comes from `shivammathur/php-src-backports`, a REBASED branch, so the
    /// branch name is not an identifier — it is rewritten rather than appended,
    /// and two builds a year apart can share it while sharing no code.
    /// `docs/PLAN-php-74-support.md` §11 asks for the commit to be recorded in
    /// the pin comment for exactly that reason.
    ///
    /// It was already in `THIRD-PARTY-NOTICES.md`, which is why this is a
    /// CONSISTENCY guard and not just a second copy. Recording the same fact in
    /// two files is the defect family `docs/TESTING.md` §3.1 is about; the fix
    /// is not to pick one, because the two files serve different readers (a
    /// licence auditor reads the notices; a resolve reads this file), but to
    /// make them unable to disagree.
    ///
    /// **Derived from `php_self_hosted_tag`, never from the string "7.4"**, so a
    /// second self-built PHP inherits the requirement by existing — the same
    /// derivation #336 had to be corrected INTO after keying on names.
    #[test]
    fn the_pinned_74_names_the_source_it_was_built_from() {
        const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");
        // Landmark: a gutted file would satisfy every `contains` below vacuously
        // on the ban half and fail confusingly on the must-say half.
        assert!(
            NOTICES.contains("# Third-party notices"),
            "THIRD-PARTY-NOTICES.md is not the file this guard thinks it is"
        );

        let self_built: Vec<&str> =
            PHP_VERSIONS.iter().copied().filter(|v| php_self_hosted_tag(v).is_some()).collect();
        assert!(
            !self_built.is_empty(),
            "no self-built PHP — if that is now true, delete this guard rather than \
             leaving it passing over an empty list"
        );

        // A short hash is a plausible-looking thing to type wrong, so assert its
        // SHAPE as well as its presence: 12 lowercase hex, which is what the
        // release publishes.
        assert_eq!(PHP_7_4_33_SOURCE_COMMIT.len(), 12, "not a short git hash");
        assert!(
            PHP_7_4_33_SOURCE_COMMIT.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "not lowercase hex: {PHP_7_4_33_SOURCE_COMMIT}"
        );

        for v in self_built {
            let tag = php_self_hosted_tag(v).expect("filtered on it");
            assert!(
                NOTICES.contains(tag),
                "THIRD-PARTY-NOTICES.md does not name the release {tag} it describes"
            );
            assert!(
                NOTICES.contains(PHP_7_4_33_SOURCE_COMMIT),
                "THIRD-PARTY-NOTICES.md and this file disagree about which source \
                 {v} was built from — this file says {PHP_7_4_33_SOURCE_COMMIT}"
            );
        }
    }

    /// **The Xdebug debug build is ours, and the old guard could not see it.**
    ///
    /// `php-debug` / `php-fpm-debug` are a custom static-php compile
    /// (`docs/xdebug-debug-build.md`) hosted in `rexenv/runtimes` alongside the
    /// 7.4 builds (`PHP_DEBUG_TAG`) — rexenv builds those bytes and serves them, so PHP License 3.01 §2 attaches
    /// exactly as it does to 7.4. The obligation used to be
    /// `(name == "php" || name == "php-fpm") && php_self_hosted_tag(version)`,
    /// which answered **false** for both, under a doc comment promising that a
    /// second self-built runtime "inherits the obligation by existing".
    ///
    /// It is unreachable today only because the debug digests are empty, so
    /// `manifest()` returns `None` and nothing resolves. That is a pin away from
    /// shipping, which is precisely when nobody re-reads a licence guard.
    #[test]
    fn the_debug_build_is_ours_too() {
        for kind in ["cli", "fpm"] {
            for arch in [Arch::Arm64, Arch::X86_64] {
                let url = php_debug_url(SAMPLE_DEBUG_TAG, kind, arch);
                assert!(
                    is_self_distributed(&url),
                    "rexenv builds and hosts the debug build: {url}"
                );
                // …and with no licence pin, resolving it is an ERROR rather than
                // a silent publish. This is the fail-closed half.
                let err = licenses_spec(&url, "php-debug", PHP_DEBUG_VERSION, arch)
                    .expect_err("ours + unpinned licences ⇒ refuse");
                let msg = err.to_string();
                assert!(msg.contains("distributor"), "{msg}");
                assert!(msg.contains(PHP_DEBUG_VERSION), "{msg}");
            }
        }
    }

    /// The host rule must not sweep in the seven OTHER `github.com` orgs rexenv
    /// downloads from — a bare-domain prefix would make us the distributor of
    /// Caddy, cloudflared, WP-CLI, Adminer, nginx and PHP upstream.
    #[test]
    fn the_self_distributed_host_rule_is_not_a_bare_domain() {
        assert!(is_self_distributed(
            "https://github.com/rexenv/runtimes/releases/download/php-7.4.33-6/php.tar.gz"
        ));
        assert!(is_self_distributed("https://dl.rexenv.dev/php-debug/php.tar.gz"));
        for foreign in [
            "https://github.com/caddyserver/caddy/releases/download/v2/caddy.tar.gz",
            "https://github.com/cloudflare/cloudflared/releases/download/x/cloudflared",
            "https://github.com/wp-cli/wp-cli/releases/download/x/wp-cli.phar",
            "https://github.com/php/frankenphp/releases/download/x/frankenphp",
            "https://github.com/vrana/adminer/releases/download/x/adminer.php",
            "https://github.com/axllent/mailpit/releases/download/x/mailpit.tar.gz",
            "https://github.com/theseus-rs/postgresql-binaries/releases/download/x/pg.tar.gz",
            "https://jirutka.github.io/nginx-binaries/nginx-1.30.3-arm64-darwin",
            "https://dl.static-php.dev/static-php-cli/bulk/php-8.3.31-cli-macos-aarch64.tar.gz",
        ] {
            assert!(!is_self_distributed(foreign), "not ours: {foreign}");
        }
    }

    /// **The notices file cannot claim rexenv distributes nothing while it does.**
    ///
    /// # Why this guard exists, which is not "check the notices"
    ///
    /// `THIRD-PARTY-NOTICES.md` opened with a sentence saying rexenv redistributes
    /// none of the binaries it downloads. That went false the day 7.4.33 shipped —
    /// a PHP rexenv builds and hosts — and it shipped false, in a public repo, for
    /// a day. README carried the same claim in its own words.
    ///
    /// The part worth encoding is that **it was flagged in advance and shipped
    /// anyway**. `docs/PLAN-php-74-support.md` §6.5 named this exact file and line
    /// range, and called it the one item on the plan that a later commit could not
    /// fix. Then the build landed, the docs sweep ran, and the sentence did not
    /// move. So the lesson is not "remember the notices" — an author who had
    /// written down that this specific sentence was un-fixable-later still did not
    /// fix it. Flagging is not a mechanism. This is the same finding
    /// `core::copy_scan` records at greater length: writing a lesson down does not
    /// install it, and the thing that catches it is a check that runs.
    ///
    /// Keyed on `is_self_distributed` rather than on "7.4", so it is a rule: the
    /// day rexenv self-builds a second runtime the guard already covers it, and
    /// the day it stops self-building anything the guard stands down on its own.
    ///
    /// Both halves, for the enable-moment reason (`mcp_server`): a BAN alone is
    /// satisfied by deleting the false sentence and saying nothing, which leaves a
    /// notices file that is no longer wrong and still does not discharge the duty.
    ///
    /// **One rule this imposes on the prose:** the banned sentence may not appear
    /// even as a quotation of its own history. A scanner cannot tell a quote from
    /// a claim, and neither can somebody skimming for what rexenv redistributes.
    #[test]
    fn the_notices_cannot_disclaim_distribution_while_we_distribute() {
        const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");
        const README: &str = include_str!("../../../README.md");

        // The live fact. Everything below is conditional on it, so this reads as
        // a rule rather than as a list of today's strings.
        let distributing: Vec<&str> = PHP_VERSIONS
            .iter()
            .copied()
            .filter(|v| is_self_distributed(&php_url("cli", v, Arch::Arm64)))
            .collect();
        if distributing.is_empty() {
            return; // rexenv distributes nobody else's bytes — nothing is owed.
        }

        // Landmarks: if a file is ever gutted, every `contains` below would pass
        // vacuously on the ban half and fail confusingly on the must-say half.
        for (file, name, landmark) in [
            (NOTICES, "THIRD-PARTY-NOTICES.md", "# Third-party notices"),
            (README, "README.md", "## Licence"),
        ] {
            assert!(file.contains(landmark), "{name} is not the file this guard thinks it is");
        }

        // Sentences that are ONLY true while rexenv distributes nothing of its
        // own. Assembled from fragments so the phrase does not appear literally
        // in this file — otherwise a future guard that scanned Rust sources too
        // would trip on its own ban list (#228's canary trap, one layer over).
        let banned: Vec<String> = [
            ["redistributes", "none of them"],
            ["are", "not redistributed by rexenv"],
        ]
        .iter()
        .map(|parts| parts.join(" "))
        .collect();

        for (file, name) in [(NOTICES, "THIRD-PARTY-NOTICES.md"), (README, "README.md")] {
            for phrase in &banned {
                assert!(
                    !file.contains(phrase.as_str()),
                    "{name} still says \"{phrase}\", but rexenv BUILDS and hosts {:?} — it is \
                     the distributor of those bytes and PHP License 3.01 attaches. This exact \
                     sentence shipped false once, after the plan had named it as the one thing \
                     a later commit could not fix. Scope the claim to the builds somebody else \
                     publishes.",
                    distributing
                );
            }
        }

        // ...and what the file must SAY once it does distribute. Deleting the
        // false sentence and adding nothing would pass a ban-only guard while
        // leaving the obligation undocumented — the erosion direction, and the
        // likelier one, because the shortest edit that clears a ban is a delete.
        const MUST_SAY: &[(&str, &str)] = &[
            ("PHP License 3.01", "WHICH licence attaches to the PHP we build"),
            (
                "licenses-<arch>.tar.gz",
                "WHERE the texts are published beside the artifacts",
            ),
            (
                "licenses/",
                "that the texts also land on the user's machine beside the binary",
            ),
        ];
        for (phrase, why) in MUST_SAY {
            assert!(
                NOTICES.contains(phrase),
                "THIRD-PARTY-NOTICES.md no longer says {why} (looked for \"{phrase}\"). rexenv \
                 distributes {distributing:?}; this file is the \"other materials provided with \
                 the distribution\" that §2 asks for."
            );
        }
        // README is a summary, so it owes the licence NAME and nothing more —
        // the detail lives in the notices file and duplicating it here would be
        // a second copy to drift.
        assert!(
            README.contains("PHP License 3.01"),
            "README describes what rexenv does and does not redistribute; it must name the \
             licence of the one thing it DOES."
        );
    }

    /// The cache test's two directions. A dir owing nothing is satisfied whatever
    /// is on disk; a dir owing texts is NOT satisfied by an absent or empty
    /// `licenses/`, which is what every cache from before this shipped looks like.
    #[test]
    fn a_cache_that_owes_licences_is_stale_until_they_are_there() {
        let tmp = std::env::temp_dir().join(format!("rexenv-lic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let upstream = php_url("cli", PHP_VERSION, Arch::Arm64);
        let ours = php_url("cli", "7.4.33", Arch::Arm64);

        // Upstream's build: nothing owed, so an empty dir is fine.
        assert!(licenses_satisfied(&tmp, &upstream));
        // Ours: the same empty dir is stale — this is the pre-existing-cache case.
        assert!(!licenses_satisfied(&tmp, &ours));
        // …and one we host but have NOT pinned licences for is stale too, rather
        // than reading as nothing-owed. The old form asked the licence SPEC, and
        // so answered "satisfied" for exactly the case that is a violation:
        // an artifact rexenv serves whose licences nobody remembered to pin.
        assert!(!licenses_satisfied(&tmp, &php_debug_url(SAMPLE_DEBUG_TAG, "cli", Arch::Arm64)));
        // An EMPTY licenses/ is stale too. A tarball that unpacked to nothing
        // would otherwise read as satisfied, which is the vacuous-green shape.
        std::fs::create_dir_all(tmp.join(LICENSES_DIR)).unwrap();
        assert!(!licenses_satisfied(&tmp, &ours));
        std::fs::write(tmp.join(LICENSES_DIR).join("PHP-3.01.txt"), "…").unwrap();
        assert!(licenses_satisfied(&tmp, &ours));
        // A binary we do not distribute never gains the requirement.
        let caddy = manifest("caddy", CADDY_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(licenses_satisfied(&tmp, &caddy.url));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn php_versions_distinct_and_include_default() {
        assert!(PHP_VERSIONS.contains(&PHP_VERSION));
        let mut seen = std::collections::HashSet::new();
        for v in PHP_VERSIONS {
            assert!(seen.insert(v), "duplicate PHP version pinned: {v}");
        }
        // Every pinned version's digests resolve for both kinds + arches.
        for v in PHP_VERSIONS {
            for kind in ["cli", "fpm"] {
                assert!(php_sha256(kind, v, Arch::Arm64).is_some());
                assert!(php_sha256(kind, v, Arch::X86_64).is_some());
            }
        }
        // An unpinned version is rejected.
        assert!(manifest("php", "8.0.0", "macos", Arch::Arm64).is_none());
        assert!(php_sha256("cli", "8.0.0", Arch::Arm64).is_none());
    }

    #[test]
    fn outdated_php_cache_rule_is_narrow() {
        // A registry where no minor has a selection: every effective patch IS
        // the pin. Built from `PHP_VERSIONS` rather than passed as `&[]`, because
        // an empty effective set now means "the registry could not be read" and
        // the keep-set refuses to answer it.
        let pins: Vec<String> = PHP_VERSIONS.iter().map(|v| (*v).to_string()).collect();
        let keep = php_caches_to_keep(&pins, &[]).expect("a populated registry answers");
        // An old patch of a pinned minor — for both the cli and fpm dirs.
        assert!(is_outdated_php_cache("php-8.3.30", &keep));
        assert!(is_outdated_php_cache("php-fpm-8.3.30", &keep));
        // The pinned patch itself is never outdated.
        for v in PHP_VERSIONS {
            assert!(!is_outdated_php_cache(&format!("php-{v}"), &keep));
            assert!(!is_outdated_php_cache(&format!("php-fpm-{v}"), &keep));
        }
        // Everything else is left alone: debug builds, other binaries, staging
        // dirs, non-x.y.z names, unpinned minors (a newer app's cache).
        assert!(!is_outdated_php_cache("php-debug-8.3.31", &keep));
        assert!(!is_outdated_php_cache("php-fpm-debug-8.3.31", &keep));
        assert!(!is_outdated_php_cache("caddy-2.11.4", &keep));
        assert!(!is_outdated_php_cache("nginx-1.30.3", &keep));
        assert!(!is_outdated_php_cache(".staging-php-8.3.31-123-0", &keep));
        assert!(!is_outdated_php_cache("php-8.3", &keep));
        assert!(!is_outdated_php_cache("php-8.3.31.1", &keep));
        assert!(!is_outdated_php_cache("php-8.6.1", &keep)); // unpinned minor (newer app)
        // …and an unpinned minor OLDER than the set. This was `php-7.4.33`, and
        // that literal would keep this assert GREEN the day 7.4 gains a pin —
        // for the opposite reason (7.4.33 becomes the pinned patch, so "not
        // outdated" is trivially true and the unpinned branch stops being
        // covered). Derived, so it cannot rot that way. See `php::unshipped_minor`.
        let older = format!("php-{}", crate::core::php::unshipped_patch());
        assert!(!is_outdated_php_cache(&older, &keep), "{older}");
        assert!(!is_outdated_php_cache(
            &format!("php-fpm-{}", crate::core::php::unshipped_patch()),
            &keep
        ));
    }

    /// A registry where one minor moved onto a selection and the rest follow
    /// their pins — the shape every assertion below is about.
    fn effective_with_selection(selected: &str) -> Vec<String> {
        let minor = crate::core::php::minor_of(selected);
        PHP_VERSIONS
            .iter()
            .map(|v| {
                if crate::core::php::minor_of(v) == minor {
                    selected.to_string()
                } else {
                    (*v).to_string()
                }
            })
            .collect()
    }

    /// **The selected tree survives; the pin it replaced does NOT.**
    ///
    /// Two failures in one rule, and each was live at some point:
    ///
    /// - Keying the sweep on the PIN deleted, at the next launch, the exact tree
    ///   the user just chose and is serving from — `docs/PLAN-binary-updates.md`
    ///   §6's landmine.
    /// - Keeping the pin unconditionally ALONGSIDE the selection leaked ~180 MB
    ///   per updated minor, forever. A user who updated 8.2 and 8.3 found 358 MB
    ///   of trees that nothing resolves, in a `bin/` already at 3.3 GB — reported
    ///   by the person whose disk it was on. The argument for keeping them ("a
    ///   floor whose bytes were deleted is not a floor") does not survive
    ///   `updates::floored`, which returns the pin ONLY when the pin is what the
    ///   minor will run — and then the pin already IS the effective patch.
    ///
    /// The fixture is a patch of a REAL pinned minor that is not the pin, so it
    /// takes the same branch a runtime selection would: prefix-stripped, `x.y.z`,
    /// minor has a pin.
    #[test]
    fn the_gc_keeps_the_selected_patch_and_collects_the_pin_it_replaced() {
        let minor = crate::core::php::minor_of(PHP_VERSION);
        let selected = format!("{minor}.9999");

        // Nowhere in the registry: it is garbage, exactly as before.
        let pins: Vec<String> = PHP_VERSIONS.iter().map(|v| (*v).to_string()).collect();
        let bare = php_caches_to_keep(&pins, &[]).unwrap();
        assert!(is_outdated_php_cache(&format!("php-{selected}"), &bare));
        assert!(is_outdated_php_cache(&format!("php-fpm-{selected}"), &bare));

        let keep = php_caches_to_keep(&effective_with_selection(&selected), &[]).unwrap();
        // Selected: both of its trees survive.
        assert!(!is_outdated_php_cache(&format!("php-{selected}"), &keep));
        assert!(!is_outdated_php_cache(&format!("php-fpm-{selected}"), &keep));
        // The PIN IT REPLACED is collected. This is the ~180 MB per minor.
        assert!(
            is_outdated_php_cache(&format!("php-{PHP_VERSION}"), &keep),
            "the superseded pin {PHP_VERSION} must be swept, not kept forever"
        );
        assert!(is_outdated_php_cache(&format!("php-fpm-{PHP_VERSION}"), &keep));
        // …while every minor still FOLLOWING its pin keeps that pin, because for
        // those the pin IS the effective patch. Deleting these would be the
        // original landmine wearing the fix's clothes.
        for v in PHP_VERSIONS.iter().filter(|v| crate::core::php::minor_of(v) != minor) {
            assert!(!is_outdated_php_cache(&format!("php-{v}"), &keep), "{v}");
            assert!(!is_outdated_php_cache(&format!("php-fpm-{v}"), &keep), "{v}");
        }
        // A superseded patch is still collected while a selection is live —
        // keeping the selected tree must not turn the sweep off.
        assert!(is_outdated_php_cache(&format!("php-{minor}.9998"), &keep));
    }

    /// **A pool LIVE on the pin holds its tree against the registry.**
    ///
    /// The window is ordinary, not exotic: the apply persists the selection and
    /// then restarts, so a restart that failed partway leaves the master on the
    /// old patch — and the launch sweep runs right after. Unlinking a running
    /// master's tree does not kill it (macOS keeps it alive on the inode); it
    /// makes the pool unrestartable, which is the worst of both.
    #[test]
    fn a_live_master_holds_its_tree_against_the_registry() {
        let minor = crate::core::php::minor_of(PHP_VERSION);
        let selected = format!("{minor}.9999");
        let keep = php_caches_to_keep(
            &effective_with_selection(&selected),
            &[PHP_VERSION.to_string()],
        )
        .unwrap();
        assert!(
            !is_outdated_php_cache(&format!("php-{PHP_VERSION}"), &keep),
            "swept a tree a live master is executing"
        );
        assert!(!is_outdated_php_cache(&format!("php-fpm-{PHP_VERSION}"), &keep));
    }

    /// **An unreadable registry SKIPS the sweep; it never means "keep nothing".**
    ///
    /// `lib.rs` builds the effective map with `unwrap_or_default`, so a DB failure
    /// arrives here as an EMPTY list. The compiled-in pins used to be an
    /// unconditional half of the keep-set and masked that; removing them made an
    /// empty set mean "delete every PHP tree on the machine". The sweep deletes
    /// what is NOT in the set, so not-knowing must cost a skipped sweep — the same
    /// rule the unidentifiable-pool guard already obeys.
    #[test]
    fn an_empty_registry_refuses_to_answer_rather_than_answering_nothing() {
        assert!(php_caches_to_keep(&[], &[]).is_none());
        // Not even a live pool makes it answerable: knowing one master's patch is
        // not knowing what the OTHER minors should keep.
        assert!(php_caches_to_keep(&[], &[PHP_VERSION.to_string()]).is_none());

        // And the real entry point removes nothing rather than sweeping — asserted
        // against files on disk, because "returns an empty Vec" and "deleted
        // nothing" are different claims and only the second one matters.
        let (plat, root) = cache_fixture("gc-empty");
        for d in ["php-0.0.1", &format!("php-{PHP_VERSION}")] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        assert!(gc_outdated_php_caches(&plat, &[], &[]).is_empty());
        assert!(root.join("php-0.0.1").exists(), "the sweep ran with an empty keep-set");
        assert!(root.join(format!("php-{PHP_VERSION}")).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The keep-set never duplicates, so a patch that is both live and effective
    /// — every minor, on an ordinary launch — does not grow the list.
    #[test]
    fn the_keep_set_is_deduplicated() {
        let pins: Vec<String> = PHP_VERSIONS.iter().map(|v| (*v).to_string()).collect();
        let keep = php_caches_to_keep(&pins, &pins).unwrap();
        assert_eq!(keep.iter().filter(|k| *k == PHP_VERSION).count(), 1);
        assert_eq!(keep.len(), PHP_VERSIONS.len());
    }

    #[test]
    fn manifest_pins_nginx_as_raw_binary() {
        let arm = manifest("nginx", NGINX_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("nginx-1.30.3-arm64-darwin"));
        assert_eq!(arm.archive, Archive::Raw);
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));

        let amd = manifest("nginx", NGINX_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("nginx-1.30.3-x86_64-darwin"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn every_offered_db_version_is_pinned_in_the_manifest() {
        for arch in [Arch::Arm64, Arch::X86_64] {
            for v in MYSQL_VERSIONS {
                let m = manifest("mysql", v, "macos", arch).expect(v);
                assert!(m.url.contains(&format!("mysql-{v}-macos15-")), "{}", m.url);
                // The CDN archives folder follows the series.
                assert!(m.url.contains(&format!("archives/mysql-{}/", mysql_series(v))));
            }
            for v in POSTGRES_VERSIONS {
                let m = manifest("postgres", v, "macos", arch).expect(v);
                assert!(m.url.contains(&format!("postgresql-{v}-")), "{}", m.url);
            }
            for v in MARIADB_VERSIONS {
                assert!(bundle_manifest("mariadb", v, "macos", arch).is_some(), "{v}");
            }
            for v in REDIS_VERSIONS {
                assert!(bundle_manifest("redis", v, "macos", arch).is_some(), "{v}");
            }
        }
        // Defaults are members of their offered sets.
        assert!(MYSQL_VERSIONS.contains(&MYSQL_VERSION));
        assert!(POSTGRES_VERSIONS.contains(&POSTGRES_VERSION));
        assert!(MARIADB_VERSIONS.contains(&MARIADB_VERSION));
        assert!(REDIS_VERSIONS.contains(&REDIS_VERSION));
        // The versioned-formula ghcr path maps `@` → `/`.
        let lts = bundle_manifest("mariadb", "11.4.12", "macos", Arch::Arm64).unwrap();
        assert!(lts.parts[0].url.contains("homebrew/core/mariadb/11.4/blobs/"), "{}", lts.parts[0].url);
        // Unpinned versions never resolve.
        assert!(manifest("mysql", "5.7.44", "macos", Arch::Arm64).is_none());
        assert!(manifest("postgres", "15.0.0", "macos", Arch::Arm64).is_none());
        assert!(bundle_manifest("mariadb", "10.11.0", "macos", Arch::Arm64).is_none());
        // Series helper: CDN folder key.
        assert_eq!(mysql_series("8.0.44"), "8.0");
        assert_eq!(mysql_series("8.4.6"), "8.4");
    }

    #[test]
    fn manifest_pins_mysql_as_tree() {
        let arm = manifest("mysql", MYSQL_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("mysql-8.4.6-macos15-arm64.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGzTree);
        assert_eq!(arm.member, "bin/mysqld");
        let amd = manifest("mysql", MYSQL_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("mysql-8.4.6-macos15-x86_64.tar.gz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_postgres_as_tree() {
        let arm = manifest("postgres", POSTGRES_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("postgresql-18.4.0-aarch64-apple-darwin.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGzTree);
        assert_eq!(arm.member, "bin/postgres");
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));
        let amd = manifest("postgres", POSTGRES_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("postgresql-18.4.0-x86_64-apple-darwin.tar.gz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_frankenphp_as_raw_binary() {
        let arm = manifest("frankenphp", FRANKENPHP_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("v1.12.4/frankenphp-mac-arm64"));
        assert_eq!(arm.archive, Archive::Raw);
        assert_eq!(arm.member, "frankenphp");
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));

        let amd = manifest("frankenphp", FRANKENPHP_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("frankenphp-mac-x86_64"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_mailpit_as_tar_member() {
        let arm = manifest("mailpit", MAILPIT_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("v1.30.3/mailpit-darwin-arm64.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGz);
        assert_eq!(arm.member, "mailpit");
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));

        let amd = manifest("mailpit", MAILPIT_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("mailpit-darwin-amd64.tar.gz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_cloudflared_as_tar_member() {
        let arm = manifest("cloudflared", CLOUDFLARED_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("2026.6.1/cloudflared-darwin-arm64.tgz"));
        assert_eq!(arm.archive, Archive::TarGz);
        assert_eq!(arm.member, "cloudflared");
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));
        let amd = manifest("cloudflared", CLOUDFLARED_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("cloudflared-darwin-amd64.tgz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_pins_wp_cli_os_agnostic() {
        // Same phar on every OS/arch.
        let a = manifest("wp-cli", WP_CLI_VERSION, "macos", Arch::Arm64).unwrap();
        let b = manifest("wp-cli", WP_CLI_VERSION, "linux", Arch::X86_64).unwrap();
        assert!(a.url.ends_with("wp-cli-2.12.0.phar"));
        assert_eq!(a.member, "wp-cli.phar");
        assert_eq!(a.archive, Archive::Raw);
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    #[test]
    fn manifest_pins_composer_os_agnostic() {
        // Same phar on every OS/arch — run via the SITE's bundled PHP.
        let a = manifest("composer", COMPOSER_VERSION, "macos", Arch::Arm64).unwrap();
        let b = manifest("composer", COMPOSER_VERSION, "linux", Arch::X86_64).unwrap();
        assert!(a.url.ends_with("download/2.10.2/composer.phar"));
        assert_eq!(a.member, "composer.phar");
        assert_eq!(a.archive, Archive::Raw);
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    #[test]
    fn manifest_pins_adminer_os_agnostic() {
        let a = manifest("adminer", ADMINER_VERSION, "macos", Arch::Arm64).unwrap();
        let b = manifest("adminer", ADMINER_VERSION, "linux", Arch::X86_64).unwrap();
        assert!(a.url.ends_with("v5.4.2/adminer-5.4.2-en.php"));
        assert_eq!(a.member, "adminer.php");
        assert_eq!(a.archive, Archive::Raw);
        assert!(matches!(a.checksum, Checksum::Sha256(_)));
        // Same artifact on every OS/arch.
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    #[test]
    fn manifest_unknown_is_none() {
        assert!(manifest("nginx", "1.0", "macos", Arch::Arm64).is_none());
        assert!(manifest("caddy", "9.9.9", "macos", Arch::Arm64).is_none());
        assert!(manifest("caddy", CADDY_VERSION, "windows", Arch::X86_64).is_none());
        assert!(manifest("php", "9.9.9", "macos", Arch::Arm64).is_none());
    }

    #[test]
    fn stream_hasher_verifies_sha256_and_sha512_incrementally() {
        // SHA-256("abc") and SHA-512("abc"), fed in split chunks — the streaming
        // path must equal the one-shot digest.
        let s256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let s512 = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        let c256 = Checksum::Sha256(s256.into());
        let mut h = StreamHasher::new(&c256);
        h.update(b"ab");
        h.update(b"c");
        assert!(h.finish().eq_ignore_ascii_case(checksum_hex(&c256)));

        let c512 = Checksum::Sha512(s512.into());
        let mut h = StreamHasher::new(&c512);
        h.update(b"a");
        h.update(b"bc");
        assert!(h.finish().eq_ignore_ascii_case(checksum_hex(&c512)));

        // A mismatch names the URL so the user knows WHICH download went stale.
        let msg = checksum_mismatch_message("https://x/y.tar.gz", s256, "deadbeef");
        assert!(msg.contains("https://x/y.tar.gz"), "{msg}");
        assert!(msg.contains("deadbeef"), "{msg}");
    }

    #[test]
    fn hex_lower_encodes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let base = Path::new("/cache/mysql");
        assert_eq!(
            safe_join(base, Path::new("bin/mysqld")).unwrap(),
            Path::new("/cache/mysql/bin/mysqld")
        );
        // `.` is fine; `..`, absolute, and mid-path escapes are not.
        assert_eq!(
            safe_join(base, Path::new("./lib/x")).unwrap(),
            Path::new("/cache/mysql/lib/x")
        );
        assert!(safe_join(base, Path::new("../evil")).is_err());
        assert!(safe_join(base, Path::new("a/../../evil")).is_err());
        assert!(safe_join(base, Path::new("/etc/passwd")).is_err());
    }

    #[test]
    fn link_stays_within_allows_intree_but_rejects_escape() {
        let dest = Path::new("/cache/mysql");
        // Real MySQL dylib symlinks: relative `../lib/…` targets resolve back inside
        // the tree (target is relative to the link's directory).
        assert!(link_stays_within(
            dest,
            Path::new("/cache/mysql/bin"),
            Path::new("../lib/libprotobuf.24.4.0.dylib")
        ));
        assert!(link_stays_within(
            dest,
            Path::new("/cache/mysql/lib/plugin"),
            Path::new("../../lib/libcom_err.3.0.dylib")
        ));
        assert!(link_stays_within(dest, Path::new("/cache/mysql/lib"), Path::new("libssl.3.dylib")));
        // Escapes: climbing above the tree, a sibling with a shared prefix, or absolute.
        assert!(!link_stays_within(
            dest,
            Path::new("/cache/mysql/bin"),
            Path::new("../../../../etc/passwd")
        ));
        assert!(!link_stays_within(dest, Path::new("/cache/mysql"), Path::new("../mysql-evil/x")));
        assert!(!link_stays_within(dest, Path::new("/cache/mysql/lib"), Path::new("/etc/passwd")));
    }

    /// A fresh, unique temp base dir for the publish tests (no network / platform).
    fn tmp_base(tag: &str) -> PathBuf {
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("rexenv-h4-{tag}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn publish_moves_staging_into_place_when_target_absent() {
        let base = tmp_base("absent");
        let staging = base.join("staging");
        let dir = base.join("final");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("caddy"), b"bin").unwrap();
        publish(&staging, &dir, "caddy").unwrap();
        assert!(dir.join("caddy").exists());
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn publish_keeps_race_winner_and_discards_staging() {
        // A concurrent resolve already published the final binary → keep theirs.
        let base = tmp_base("winner");
        let staging = base.join("staging");
        let dir = base.join("final");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("caddy"), b"ours").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("caddy"), b"winner").unwrap();
        publish(&staging, &dir, "caddy").unwrap();
        assert_eq!(std::fs::read(dir.join("caddy")).unwrap(), b"winner");
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn publish_replaces_stale_partial_target() {
        // The H4 case: a prior crash left `dir` WITHOUT the final marker. Publishing
        // must replace the poisoned leftover wholesale, not keep it.
        let base = tmp_base("stale");
        let staging = base.join("staging");
        let dir = base.join("final");
        std::fs::create_dir_all(dir.join("junk")).unwrap(); // partial, no "caddy"
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("caddy"), b"good").unwrap();
        publish(&staging, &dir, "caddy").unwrap();
        assert_eq!(std::fs::read(dir.join("caddy")).unwrap(), b"good");
        assert!(!dir.join("junk").exists()); // stale content removed
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn download_retries_only_transient_statuses() {
        use reqwest::StatusCode;
        // 5xx → retry; 4xx → give up immediately (§2.2 retry policy).
        assert!(status_is_transient(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(status_is_transient(StatusCode::BAD_GATEWAY));
        assert!(!status_is_transient(StatusCode::NOT_FOUND));
        assert!(!status_is_transient(StatusCode::FORBIDDEN));
    }

    #[tokio::test]
    async fn in_flight_serializes_same_binary_but_not_different_ones() {
        use std::time::Duration;
        use tokio::time::timeout;
        // Same (name, version): the second caller must WAIT for the first.
        let held = in_flight("flight-test", "1.0").await;
        assert!(
            timeout(Duration::from_millis(100), in_flight("flight-test", "1.0"))
                .await
                .is_err(),
            "second resolve of the same binary must wait for the in-flight one"
        );
        // A different (name, version) is an independent flight — no blocking.
        assert!(
            timeout(Duration::from_millis(100), in_flight("flight-test", "2.0"))
                .await
                .is_ok(),
            "a different version must not share the flight lock"
        );
        // Releasing the first lets the waiter proceed (cache re-check path).
        drop(held);
        assert!(
            timeout(Duration::from_millis(100), in_flight("flight-test", "1.0"))
                .await
                .is_ok(),
            "the flight lock must be released when the first resolve finishes"
        );
    }

    /// **The pin wins over any manifest, for Adminer as for PHP — and a version
    /// the app was never built with resolves only through the VERIFIED catalog.**
    ///
    /// The precedence is the whole "compiled-in pins remain the floor" property.
    /// Asserted here rather than reasoned about, because `adminer_spec` is the
    /// second place in the codebase where a signed document can name bytes this
    /// build will execute, and the first one (`php_spec`) got its own test.
    #[test]
    fn the_adminer_pin_outranks_the_catalog_and_an_unpinned_version_needs_one() {
        let pinned = manifest("adminer", ADMINER_VERSION, "macos", Arch::Arm64)
            .expect("the pinned version resolves with no catalog at all");
        assert!(matches!(pinned.checksum, Checksum::Sha256(ref h) if h == ADMINER_5_4_2_SHA256));
        assert!(matches!(pinned.archive, Archive::Raw));
        assert_eq!(pinned.member, "adminer.php");
        // Same on the other Mac: one file, no arch in the answer.
        let intel = manifest("adminer", ADMINER_VERSION, "macos", Arch::X86_64).unwrap();
        assert_eq!(intel.url, pinned.url);

        // Unpinned, no catalog → nothing. The floor is not a default.
        assert!(manifest("adminer", "6.0.1", "macos", Arch::Arm64).is_none());

        // Install a catalog that ALSO tries to re-point the pinned version.
        let evil = "b".repeat(64);
        let _catalog = catalog_test_lock();
        install_catalog(crate::core::updates::catalog_for_tests(&[
            (
                "adminer",
                ADMINER_VERSION,
                crate::core::updates::ANY_ARCH,
                "https://github.com/vrana/adminer/releases/download/v0/x.php",
                &evil,
            ),
            (
                "adminer",
                "6.0.1",
                crate::core::updates::ANY_ARCH,
                "https://github.com/vrana/adminer/releases/download/v6.0.1/adminer-6.0.1-en.php",
                &"c".repeat(64),
            ),
        ]));
        let still_pinned = manifest("adminer", ADMINER_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(
            matches!(still_pinned.checksum, Checksum::Sha256(ref h) if h == ADMINER_5_4_2_SHA256),
            "a signed manifest moved a version the app already pins onto other bytes"
        );
        assert_eq!(still_pinned.url, pinned.url);

        // …while the version the app was built before now resolves, and takes
        // its archive/member from the NAME rather than from the document.
        let offered = manifest("adminer", "6.0.1", "macos", Arch::Arm64)
            .expect("a catalogued version must resolve");
        assert!(matches!(offered.checksum, Checksum::Sha256(ref h) if h == &"c".repeat(64)));
        assert!(matches!(offered.archive, Archive::Raw));
        assert_eq!(offered.member, "adminer.php");
        // Arch-free: the same answer on the other Mac.
        assert_eq!(manifest("adminer", "6.0.1", "macos", Arch::X86_64).unwrap().url, offered.url);
        install_catalog(Default::default());
    }

}
