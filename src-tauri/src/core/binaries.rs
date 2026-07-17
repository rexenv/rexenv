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
use std::io::Write;
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
/// 7.4 is deliberately absent: static-php.dev never published it — offering it
/// needs a self-built + self-hosted artifact (same blocked path as the Xdebug
/// debug build). 8.0 is upstream-EOL, frozen at 8.0.30 (its only bulk build).
pub const PHP_VERSIONS: &[&str] =
    &["8.0.30", "8.1.34", "8.2.31", "8.3.31", "8.4.23", "8.5.8"];
/// PHP minor used for the **debug build** (Xdebug compiled in) that backs the §8.2
/// per-site Xdebug debug pool. The stock static-php "bulk" builds ship NO Xdebug
/// and a static PHP can't `dlopen` an external `xdebug.so` (§8.1), so this is a
/// SEPARATE custom static-php compile — see `docs/xdebug-debug-build.md` for the
/// reproducible `spc` recipe. The artifact is self-hosted; until it's uploaded and
/// its SHA-256 pinned below, `php-debug`/`php-fpm-debug` stay UNRESOLVABLE (§11.2).
pub const PHP_DEBUG_VERSION: &str = "8.3.31";
/// Xdebug version compiled into the debug build (recorded for the recipe + UI).
pub const PHP_DEBUG_XDEBUG_VERSION: &str = "3.4.5";
/// Where the self-built debug artifacts will be hosted (filled at hosting time).
/// A maintainer builds with the recipe, uploads here, then pins the checksums.
const PHP_DEBUG_BASE_URL: &str = "https://dl.rexenv.dev/php-debug";
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
/// two dylibs, relinked to `@loader_path` by `prepare_binary_tree` (TODO
/// "Deferred services"). Resolved via [`resolve_bundle`].
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
/// Pinned Xdebug version (per-site toggle, §8.2). ONE `xdebug.so` per PHP minor
/// from shivammathur/homebrew-extensions bottles (the tap GitHub Actions
/// setup-php uses on macOS) — they dlopen straight into our EXISTING static-php
/// binaries (ABI = Zend API nr + NTS + non-debug, all matching; live-proven
/// cli+fpm on 8.1–8.5, full DBGp handshake). No debug PHP build needed; the
/// debug pool is the same fpm binary + `-d zend_extension`. PHP 8.0 is
/// EXCLUDED: the Nov 2024 static 8.0.30 build exports no Zend symbols, so any
/// external .so fails to dlopen (`_OnUpdateBool` unresolved).
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
// upstream can only 404, never swap bytes silently). arm64 = the arm64_sonoma
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

/// The formula + per-arch digests of the Xdebug bottle matching a PHP minor.
/// `None` = no Xdebug for that minor (8.0's static build can't dlopen — see
/// [`XDEBUG_VERSION`]). The single source of which minors support the toggle.
fn xdebug_bottle(minor: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match minor {
        "8.1" => Some(("xdebug@8.1", XDEBUG_PHP81_BOTTLE_ARM64_SHA256, XDEBUG_PHP81_BOTTLE_AMD64_SHA256)),
        "8.2" => Some(("xdebug@8.2", XDEBUG_PHP82_BOTTLE_ARM64_SHA256, XDEBUG_PHP82_BOTTLE_AMD64_SHA256)),
        "8.3" => Some(("xdebug@8.3", XDEBUG_PHP83_BOTTLE_ARM64_SHA256, XDEBUG_PHP83_BOTTLE_AMD64_SHA256)),
        "8.4" => Some(("xdebug@8.4", XDEBUG_PHP84_BOTTLE_ARM64_SHA256, XDEBUG_PHP84_BOTTLE_AMD64_SHA256)),
        "8.5" => Some(("xdebug@8.5", XDEBUG_PHP85_BOTTLE_ARM64_SHA256, XDEBUG_PHP85_BOTTLE_AMD64_SHA256)),
        _ => None,
    }
}

/// Whether the per-site Xdebug toggle is available for a PHP minor.
pub fn xdebug_supported(minor: &str) -> bool {
    xdebug_bottle(minor).is_some()
}

/// The bundle (name, version) whose cached tree holds `xdebug.so` for a PHP
/// minor — pass to [`resolve_bundle`]. The minor is baked into the NAME (the
/// .so is ABI-bound to it); the VERSION is Xdebug's, so a pin bump busts the
/// cache dir like every other binary.
pub fn xdebug_bundle_id(minor: &str) -> Option<(String, &'static str)> {
    xdebug_bottle(minor).map(|_| (format!("xdebug-{minor}"), XDEBUG_VERSION))
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

/// Pinned SHA-256 for a static-php "bulk" artifact, or `None` if the
/// version isn't pinned. `kind` is `"cli"` or `"fpm"`. Both arches are pinned
/// together, so a `Some` for one arch implies a `Some` for the other.
fn php_sha256(kind: &str, version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match (kind, version) {
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
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
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

/// The download spec for a debug (Xdebug) build artifact. Pure — same URL/archive
/// shape as a bulk build, but from the self-hosted debug bucket and tagged
/// `-xdebug`. The checksum is filled once hosted; `manifest` only surfaces this
/// when [`php_debug_sha256`] is `Some` (so it's a no-op until then).
fn php_debug_spec(kind: &str, arch: Arch) -> BinarySpec {
    BinarySpec {
        url: format!(
            "{PHP_DEBUG_BASE_URL}/php-{PHP_DEBUG_VERSION}-{kind}-xdebug-macos-{}.tar.gz",
            php_arch(arch)
        ),
        checksum: Checksum::Sha256(php_debug_sha256(kind, arch).unwrap_or_default().to_string()),
        archive: Archive::TarGz,
        member: if kind == "fpm" { "php-fpm" } else { "php" },
    }
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
        // PHP is version-driven: any version pinned in `php_sha256` resolves (the
        // static-php URL is templated; only the checksum varies per version/arch).
        ("php", "macos", v) if php_sha256("cli", v, arch).is_some() => Some(BinarySpec {
            url: format!(
                "https://dl.static-php.dev/static-php-cli/bulk/php-{v}-cli-macos-{}.tar.gz",
                php_arch(arch)
            ),
            checksum: Checksum::Sha256(php_sha256("cli", v, arch).unwrap().to_string()),
            archive: Archive::TarGz,
            member: "php",
        }),
        ("php-fpm", "macos", v) if php_sha256("fpm", v, arch).is_some() => Some(BinarySpec {
            url: format!(
                "https://dl.static-php.dev/static-php-cli/bulk/php-{v}-fpm-macos-{}.tar.gz",
                php_arch(arch)
            ),
            checksum: Checksum::Sha256(php_sha256("fpm", v, arch).unwrap().to_string()),
            archive: Archive::TarGz,
            member: "php-fpm",
        }),
        // Debug builds (Xdebug compiled in) for the §8.2 debug pool. Only resolve
        // once the self-built artifact is hosted + its checksum pinned (§11.2);
        // until then `php_debug_sha256` is None and these stay unresolvable.
        ("php-debug", "macos", v)
            if v == PHP_DEBUG_VERSION && php_debug_sha256("cli", arch).is_some() =>
        {
            Some(php_debug_spec("cli", arch))
        }
        ("php-fpm-debug", "macos", v)
            if v == PHP_DEBUG_VERSION && php_debug_sha256("fpm", arch).is_some() =>
        {
            Some(php_debug_spec("fpm", arch))
        }
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
        ("adminer", _, "5.4.2") => Some(BinarySpec {
            url: format!(
                "https://github.com/vrana/adminer/releases/download/v{version}/adminer-{version}-en.php"
            ),
            checksum: Checksum::Sha256(ADMINER_5_4_2_SHA256.to_string()),
            archive: Archive::Raw,
            member: "adminer.php",
        }),
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
/// a self-contained tree (TODO "Deferred services"). Disjoint from
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
        (n, "macos", v) if n.starts_with("xdebug-") && v == XDEBUG_VERSION => {
            let minor = n.strip_prefix("xdebug-")?;
            let (formula, arm_sha, amd_sha) = xdebug_bottle(minor)?;
            let digest = pick(arch, arm_sha, amd_sha);
            Some(BundleSpec {
                member: "xdebug.so",
                parts: vec![BundlePart {
                    formula,
                    url: tap_bottle_url("shivammathur/extensions", formula, &digest),
                    checksum: Checksum::Sha256(digest),
                    include: &["xdebug.so"],
                }],
            })
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
    if dir.join(spec.member).exists() {
        return Ok(dir);
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

/// Whether `name`@`version` is already fully published in the binary cache —
/// the same marker files the resolves' early-return checks use (a partially
/// published dir never has its marker: the staging→rename publish is atomic,
/// H4). Unknown manifest = not cached. Used by the download planner to split
/// an action's binary set into cached vs to-download without resolving.
pub fn is_cached(platform: &dyn Platform, name: &str, version: &str) -> bool {
    let Ok(bin_dir) = platform.paths().bin_dir() else {
        return false;
    };
    let dir = bin_dir.join(format!("{name}-{version}"));
    let arch = platform.binaries().arch();
    match manifest(name, version, std::env::consts::OS, arch) {
        // `member` is the marker for file/tree distributions; executables are
        // published at `dir/<name>` (for those, member == name anyway).
        Some(spec) => dir.join(spec.member).exists() || dir.join(name).exists(),
        // Bottle bundles (redis) publish their own `member` marker.
        None => match bundle_manifest(name, version, std::env::consts::OS, arch) {
            Some(bundle) => dir.join(bundle.member).exists(),
            None => false,
        },
    }
}

/// Path of an ALREADY-CACHED executable — [`resolve`]'s cache-hit fast path
/// without the download. `None` when absent. For deriving config values that
/// only need the binary's location (e.g. the Mailpit sendmail shim) in sync
/// contexts like startup adoption, where triggering a download is wrong.
pub fn cached_bin(platform: &dyn Platform, name: &str, version: &str) -> Option<PathBuf> {
    let bin = platform
        .paths()
        .bin_dir()
        .ok()?
        .join(format!("{name}-{version}"))
        .join(name);
    bin.exists().then_some(bin)
}

/// Whether a binary-cache dir name holds an OUTDATED patch of a pinned PHP
/// minor — `php-8.3.30/` or `php-fpm-8.3.30/` once the pin moved to 8.3.31.
/// Pure (name-only) so the GC rule is unit-testable. Deliberately narrow:
/// only `php-`/`php-fpm-` dirs, only a strict `x.y.z` numeric version, only
/// minors that HAVE a pin, and never the pinned patch itself — so the debug
/// builds (`php-debug-…`), other binaries, staging dirs, and versions from a
/// NEWER app (downgrade) are all left alone.
pub fn is_outdated_php_cache(dir_name: &str) -> bool {
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
    match crate::core::php::patch_for_minor(&minor) {
        Some(pinned) => version != pinned,
        None => false, // unpinned minor (newer app's cache) — don't touch
    }
}

/// Remove cache dirs left behind by a PHP patch bump (Option A updates: pins
/// move with an app release; the old `php-<oldpatch>/` trees would otherwise
/// accumulate ~60MB per bump forever). Best-effort — a dir that can't be
/// removed is skipped, never an error. Returns the removed dir names.
pub fn gc_outdated_php_caches(platform: &dyn Platform) -> Vec<String> {
    let Ok(bin_dir) = platform.paths().bin_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&bin_dir) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_outdated_php_cache(&name) && std::fs::remove_dir_all(entry.path()).is_ok() {
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
    if bin_path.exists() {
        return Ok(bin_path);
    }

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
    if path.exists() {
        return Ok(path);
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
    if dir.join(spec.member).exists() {
        return Ok(dir);
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
/// retried before giving up. A 4xx (not-found / forbidden) is NOT retried.
const DOWNLOAD_ATTEMPTS: usize = 3;

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
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match fetch_to_file(&client, url, dest, checksum, item, headers).await {
            Ok(()) => return Ok(()),
            // A permanent failure (4xx / checksum mismatch / local write error)
            // won't get better on retry — surface it immediately.
            Err(FetchError::Permanent(msg)) => {
                let _ = std::fs::remove_file(dest);
                return Err(Error::Other(msg));
            }
            Err(FetchError::Transient(msg)) => {
                let _ = std::fs::remove_file(dest);
                last_err = msg;
                log::warn!("rexenv: download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed: {last_err}");
                if attempt < DOWNLOAD_ATTEMPTS {
                    // Linear backoff (0.4s, 0.8s) between attempts.
                    tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64)).await;
                }
            }
        }
    }
    Err(Error::Other(format!(
        "{last_err} (gave up after {DOWNLOAD_ATTEMPTS} attempts)"
    )))
}

/// Outcome of a single download attempt: a permanent error short-circuits the
/// retry loop; a transient one is retried.
enum FetchError {
    Permanent(String),
    Transient(String),
}

async fn fetch_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    checksum: Option<&Checksum>,
    item: Option<&str>,
    headers: &[(&str, &str)],
) -> std::result::Result<(), FetchError> {
    let mut req = client.get(url);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        // Connect/timeout/transport problems are transient (retry); they're also
        // what "no internet" looks like, so use the connectivity-aware message.
        Err(e) => return Err(FetchError::Transient(download_error_message(url, &e))),
    };
    let mut resp = match resp.error_for_status() {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("download {url} failed: {e}");
            // 5xx is a server-side blip → retry; 4xx (not found / forbidden) won't
            // change → permanent.
            return Err(match e.status() {
                Some(s) if status_is_transient(s) => FetchError::Transient(msg),
                _ => FetchError::Permanent(msg),
            });
        }
    };
    // Content-Length when the server sends one; `None` → the UI shows an
    // indeterminate bar. Report 0/total up front so a slow first chunk still
    // renders as an active download.
    let total = resp.content_length();
    if let Some(id) = item {
        downloads::hub().item_progress(id, 0, total);
    }
    let mut file = std::fs::File::create(dest)
        .map_err(|e| FetchError::Permanent(format!("can't write {}: {e}", dest.display())))?;
    let mut hasher = checksum.map(StreamHasher::new);
    let mut downloaded: u64 = 0;
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
        if let Some(id) = item {
            downloads::hub().item_progress(id, downloaded, total);
        }
    }
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

    #[test]
    fn manifest_resolves_caddy_per_arch() {
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
    fn manifest_resolves_php_cli_and_fpm() {
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

    #[test]
    fn php_debug_spec_has_the_expected_url_and_member_shape() {
        // The pure spec builder defines the contract the hosted artifact must meet:
        // `-xdebug` tagged, from the debug bucket, with the right member binary.
        let cli = php_debug_spec("cli", Arch::Arm64);
        assert!(cli.url.ends_with("php-8.3.31-cli-xdebug-macos-aarch64.tar.gz"), "{}", cli.url);
        assert!(cli.url.starts_with(PHP_DEBUG_BASE_URL));
        assert_eq!(cli.member, "php");
        assert!(matches!(cli.archive, Archive::TarGz));

        let fpm = php_debug_spec("fpm", Arch::X86_64);
        assert!(fpm.url.ends_with("php-8.3.31-fpm-xdebug-macos-x86_64.tar.gz"), "{}", fpm.url);
        assert_eq!(fpm.member, "php-fpm");
    }

    #[test]
    fn bundle_manifest_resolves_redis_per_arch() {
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
    fn bundle_manifest_resolves_mariadb_per_arch() {
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
    fn bundle_manifest_resolves_xdebug_per_supported_minor() {
        for minor in ["8.1", "8.2", "8.3", "8.4", "8.5"] {
            assert!(xdebug_supported(minor), "{minor}");
            let (name, version) = xdebug_bundle_id(minor).unwrap();
            assert_eq!(name, format!("xdebug-{minor}"));
            assert_eq!(version, XDEBUG_VERSION);
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

    #[test]
    fn xdebug_is_refused_for_php_80_and_unknown_minors() {
        // 8.0's static build exports no Zend symbols — dlopen fails, so the
        // toggle must be unofferable by construction.
        for minor in ["8.0", "7.4", "9.0", ""] {
            assert!(!xdebug_supported(minor), "{minor}");
            assert!(xdebug_bundle_id(minor).is_none());
            assert!(bundle_manifest(&format!("xdebug-{minor}"), XDEBUG_VERSION, "macos", Arch::Arm64)
                .is_none());
        }
        // Wrong version never resolves (cache-dir identity is honest).
        assert!(bundle_manifest("xdebug-8.4", "0.0.1", "macos", Arch::Arm64).is_none());
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
    fn manifest_resolves_every_pinned_php_version() {
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
        }
    }

    #[test]
    fn php_versions_distinct_and_include_default() {
        assert!(PHP_VERSIONS.contains(&PHP_VERSION));
        assert!(PHP_VERSIONS.len() >= 2);
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
        // An old patch of a pinned minor — for both the cli and fpm dirs.
        assert!(is_outdated_php_cache("php-8.3.30"));
        assert!(is_outdated_php_cache("php-fpm-8.3.30"));
        // The pinned patch itself is never outdated.
        for v in PHP_VERSIONS {
            assert!(!is_outdated_php_cache(&format!("php-{v}")));
            assert!(!is_outdated_php_cache(&format!("php-fpm-{v}")));
        }
        // Everything else is left alone: debug builds, other binaries, staging
        // dirs, non-x.y.z names, unpinned minors (a newer app's cache).
        assert!(!is_outdated_php_cache("php-debug-8.3.31"));
        assert!(!is_outdated_php_cache("php-fpm-debug-8.3.31"));
        assert!(!is_outdated_php_cache("caddy-2.11.4"));
        assert!(!is_outdated_php_cache("nginx-1.30.3"));
        assert!(!is_outdated_php_cache(".staging-php-8.3.31-123-0"));
        assert!(!is_outdated_php_cache("php-8.3"));
        assert!(!is_outdated_php_cache("php-8.3.31.1"));
        assert!(!is_outdated_php_cache("php-8.6.1")); // unpinned minor
        assert!(!is_outdated_php_cache("php-7.4.33")); // unpinned minor
    }

    #[test]
    fn manifest_resolves_nginx_as_raw_binary() {
        let arm = manifest("nginx", NGINX_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("nginx-1.30.3-arm64-darwin"));
        assert_eq!(arm.archive, Archive::Raw);
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));

        let amd = manifest("nginx", NGINX_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("nginx-1.30.3-x86_64-darwin"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn every_offered_db_version_is_pinned_and_resolves() {
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
    fn manifest_resolves_mysql_as_tree() {
        let arm = manifest("mysql", MYSQL_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("mysql-8.4.6-macos15-arm64.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGzTree);
        assert_eq!(arm.member, "bin/mysqld");
        let amd = manifest("mysql", MYSQL_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("mysql-8.4.6-macos15-x86_64.tar.gz"));
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    #[test]
    fn manifest_resolves_postgres_as_tree() {
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
    fn manifest_resolves_frankenphp_as_raw_binary() {
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
    fn manifest_resolves_mailpit_as_tar_member() {
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
    fn manifest_resolves_cloudflared_as_tar_member() {
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
    fn manifest_resolves_wp_cli_os_agnostic() {
        // Same phar on every OS/arch.
        let a = manifest("wp-cli", WP_CLI_VERSION, "macos", Arch::Arm64).unwrap();
        let b = manifest("wp-cli", WP_CLI_VERSION, "linux", Arch::X86_64).unwrap();
        assert!(a.url.ends_with("wp-cli-2.12.0.phar"));
        assert_eq!(a.member, "wp-cli.phar");
        assert_eq!(a.archive, Archive::Raw);
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    #[test]
    fn manifest_resolves_composer_os_agnostic() {
        // Same phar on every OS/arch — run via the SITE's bundled PHP.
        let a = manifest("composer", COMPOSER_VERSION, "macos", Arch::Arm64).unwrap();
        let b = manifest("composer", COMPOSER_VERSION, "linux", Arch::X86_64).unwrap();
        assert!(a.url.ends_with("download/2.10.2/composer.phar"));
        assert_eq!(a.member, "composer.phar");
        assert_eq!(a.archive, Archive::Raw);
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    #[test]
    fn manifest_resolves_adminer_os_agnostic() {
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
}
