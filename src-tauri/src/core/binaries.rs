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
const CADDY_VERSION: &str = "2.11.4";
/// Default PHP version (static-php build; provides `php` cli and `php-fpm`).
/// Used where a single version is implied (Phase 1 paths). Must be in [`PHP_VERSIONS`].
const PHP_VERSION: &str = "8.3.32";
/// All PHP versions with pinned static-php "bulk" builds (one minor each, newest
/// last). The per-version FPM pool manager + UI (Phase 2 §1.2/§1.5) install from
/// this set; each caches independently under `bin_dir/php-<version>/`.
/// **Not all from one source, and only one of them is somebody else's now.**
/// 8.0.30 is static-php.dev's bulk build; **7.4.33 and 8.1-8.5 are OURS**
/// (`rexenv/runtimes`, immutable release assets) — 7.4 because nobody publishes
/// a portable one, the 8.x rows because the bulk builds' `pdo_pgsql` does not
/// connect. `php_url` picks the source; this list only says which versions exist.
/// 7.4 and 8.0 are both upstream-EOL and say so in the UI (`php::eol_since`).
///
/// **8.2 and 8.3 pin `.32` rather than `.31`, and that is a fix rather than a
/// bump.** Both patches exist upstream and the signed update manifest offers
/// them, so a machine that took the in-app update was running static-php.dev's
/// `.32` — no PostgreSQL driver — while rexenv judged the minor by OUR `.31`.
/// A pin beats the catalog (`php_spec` asks the compiled-in table first), so
/// pinning the same version numbers to our own build is what makes an updated
/// machine get the driver too. The digests differ from upstream's for the same
/// version string, which is exactly what `cache_matches_pin` exists to notice.
const PHP_VERSIONS: &[&str] =
    &["7.4.33", "8.0.30", "8.1.34", "8.2.32", "8.3.32", "8.4.23", "8.5.8"];
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
/// `rexenv/runtimes` (`docs/archive/PLAN-php-74-support.md` §6/§9) and
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
/// Pinned nginx version — rexenv's OWN macOS build (`rexenv/runtimes`), not a
/// third party's, since 30 Aug 2026. See the SHA-256 constants for why.
const NGINX_VERSION: &str = "1.30.4";
/// Default MySQL version (official macOS tarball — a full bin/lib/share tree).
/// Must be in [`MYSQL_VERSIONS`].
const MYSQL_VERSION: &str = "8.4.6";
/// All MySQL versions with pinned tarballs (per-engine version switch — the
/// Databases page picker). Each SERIES keeps its own datadir; the default
/// series stays on the legacy `mysql/data` path.
const MYSQL_VERSIONS: &[&str] = &["8.4.6", "8.0.44"];
/// Pinned WP-CLI version (a .phar run via the bundled PHP; OS-agnostic).
const WP_CLI_VERSION: &str = "2.12.0";
/// Pinned Composer version (a .phar, ALWAYS run via the SITE's bundled PHP so
/// `composer install` platform checks match the PHP the plugin runs on; a
/// system composer is never executed — it can be a non-phar wrapper, e.g.
/// Herd's). Downloaded + sha-verified against getcomposer.org's published
/// .sha256sum + run-tested on the static PHP at pin time.
const COMPOSER_VERSION: &str = "2.10.2";
/// Pinned FrankenPHP version (one static binary: embedded PHP + Caddy). Used as a
/// per-site override server on an internal loopback port — Phase 2 §2.
const FRANKENPHP_VERSION: &str = "1.12.4";
/// The PHP compiled INTO the pinned FrankenPHP binary. FrankenPHP does not use
/// rexenv's php-fpm pools — a FrankenPHP site is served by THIS PHP whatever its
/// `php_version` says. Recorded as data (it was only ever a sentence in the
/// re-pin comment below) so `core::sites` can refuse a combination FrankenPHP
/// cannot honour, instead of the site quietly running something else.
/// **Moves with `FRANKENPHP_VERSION`** — the re-pin procedure prints it
/// (`frankenphp version` → "FrankenPHP v1.12.4 PHP 8.5.8 Caddy v2.11.4").
const FRANKENPHP_EMBEDDED_PHP: &str = "8.5.8";
/// Default PostgreSQL version (theseus-rs portable build — a full bin/lib/share
/// tree, like MySQL). Phase 2 §5.3. Must be in [`POSTGRES_VERSIONS`].
const POSTGRES_VERSION: &str = "18.6.0";
/// All PostgreSQL versions with pinned builds (per-engine version switch).
/// PG major datadirs are mutually INCOMPATIBLE — per-series datadirs are load-
/// bearing here, not just tidy.
const POSTGRES_VERSIONS: &[&str] = &["18.6.0", "17.11.0", "16.15.0"];
/// Pinned Mailpit version (one static Go binary: SMTP sink + web UI/API). Phase 3 §2.1.
const MAILPIT_VERSION: &str = "1.30.3";
/// Pinned Adminer version (a single `adminer.php`, all drivers, run via the bundled
/// PHP — OS-agnostic, like WP-CLI). Phase 3 §5.1.
const ADMINER_VERSION: &str = "6.1.0";
/// Pinned cloudflared version (one static Go binary; quick-tunnel public sharing). Phase 3 §9.1.
const CLOUDFLARED_VERSION: &str = "2026.6.1";
/// Pinned Redis version — the FIRST Homebrew-bottle BUNDLE (no portable static
/// build exists): the redis bottle's `bin/` merged with the openssl@3 bottle's
/// two dylibs, relinked to `@loader_path` by `prepare_binary_tree` (the shipped
/// "Deferred services" plan — docs/archive/SHIPPED-2026-07.md). Resolved via
/// [`resolve_bundle`].
/// Which pin set this host gets — the ONE fact `docs/PLAN-macos-13-floor.md` §6
/// hangs on.
///
/// macOS 15 is the STANDARD: every feature, every latest pin. The app also runs
/// on 13 and 14, where some upstreams publish nothing that loads (`minos` above
/// the host's dyld), so those hosts resolve an older, measured build per
/// component, or a refusal where none exists. The tier is the name for that
/// choice; the pin sets it selects land in T2.
///
/// **Derived from the host every launch, never stored** (ledger #707). macOS only
/// moves UP, so a stored tier could only ever be too LOW — a 13 host that became
/// a 15 host would keep downloading the legacy set for as long as the row
/// survived, and every legacy pin is a security patch or more behind by design.
/// Re-deriving costs one `sw_vers` read; storing costs a stale fact with a
/// lifetime nobody tracks (the same shape as the one-time-check guards in
/// `docs/CLAIM-LEDGER.md`'s defect families).
///
/// The platform derives it (`Platform::binary_tier`), because a host version is
/// an OS fact and `core/` may not name one: macOS reads `core::macho::host_macos`,
/// every other OS answers `Standard` (a tier is a macOS concept — Windows has one
/// floor, the installer's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryTier {
    /// macOS ≥ 15, and every non-macOS host. The compiled-in `*_VERSION` pins.
    Standard,
    /// macOS 14.x. Differs from Standard in exactly the rows whose standard pin
    /// declares `minos 15.0` (`docs/PORTS.md` §"Measured macOS floors").
    Legacy14,
    /// macOS 13.x. The last 13-capable build per component, or a refusal.
    Legacy13,
}

impl BinaryTier {
    /// The tier for a host macOS version — `None` below 13.0, which is NOT a tier:
    /// `tauri.conf.json`'s `minimumSystemVersion` keeps such a host from launching
    /// the app at all, so a caller seeing `None` is looking at a version read that
    /// went wrong, not at a supported host.
    ///
    /// Major-only on purpose: Apple's deployment target is a major (`minos 14.0`),
    /// and a patch level never moved one in any build this project has measured.
    pub fn for_host(host: (u32, u32, u32)) -> Option<Self> {
        match host.0 {
            0..=12 => None,
            13 => Some(Self::Legacy13),
            14 => Some(Self::Legacy14),
            _ => Some(Self::Standard),
        }
    }

    /// The floor this tier promises, as `minimumSystemVersion` spells it — what
    /// `examples/macos_floor_check.rs` compares a tier's `max(minos)` against.
    pub fn floor(self) -> (u32, u32, u32) {
        match self {
            Self::Standard => (15, 0, 0),
            Self::Legacy14 => (14, 0, 0),
            Self::Legacy13 => (13, 0, 0),
        }
    }
}

/// Every version the app resolves, as ONE value chosen by the host's
/// [`BinaryTier`] — the only door to the `*_VERSION` constants, which are private
/// to this file on purpose.
///
/// **Why a struct and not the constants.** Before this, 18 files read
/// `binaries::MYSQL_VERSION` and friends directly; a tier that hands a macOS 13
/// host an older MySQL would have had to find and patch every one of them, and
/// the one it missed would resolve the standard pin on a host that cannot load
/// it. Making the constants private turns "every consumer goes through the tier"
/// from a rule someone remembers into a compile error — the structural guard
/// `docs/PLAN-macos-13-floor.md` §7 T1 asks for, without a source scan.
///
/// Fields are the Standard set's literals, so every value is `'static` and the
/// struct is `Copy`; a call to [`pins`] costs one lock read. The legacy sets
/// are `..STANDARD_PINS` with the measured rows overridden, so a row that does
/// not differ cannot drift from the standard one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinSet {
    pub php_versions: &'static [&'static str],
    pub mysql_versions: &'static [&'static str],
    pub postgres_versions: &'static [&'static str],
    pub redis_versions: &'static [&'static str],
    pub mariadb_versions: &'static [&'static str],
    pub frankenphp_embedded_php: &'static str,
    pub caddy: &'static str,
    pub php: &'static str,
    pub nginx: &'static str,
    pub mysql: &'static str,
    pub wp_cli: &'static str,
    pub composer: &'static str,
    pub frankenphp: &'static str,
    pub postgres: &'static str,
    pub mailpit: &'static str,
    pub adminer: &'static str,
    pub cloudflared: &'static str,
    pub redis: &'static str,
    pub mariadb: &'static str,
    pub httpd: &'static str,
    pub xdebug: &'static str,
    pub bundled_openssl: &'static str,
    pub bundled_pcre2: &'static str,
    pub bundled_apr: &'static str,
    pub bundled_apr_util: &'static str,
}

/// The compiled-in pins: macOS 15+ and every non-macOS host.
const STANDARD_PINS: PinSet = PinSet {
    php_versions: PHP_VERSIONS,
    mysql_versions: MYSQL_VERSIONS,
    postgres_versions: POSTGRES_VERSIONS,
    redis_versions: REDIS_VERSIONS,
    mariadb_versions: MARIADB_VERSIONS,
    frankenphp_embedded_php: FRANKENPHP_EMBEDDED_PHP,
    caddy: CADDY_VERSION,
    php: PHP_VERSION,
    nginx: NGINX_VERSION,
    mysql: MYSQL_VERSION,
    wp_cli: WP_CLI_VERSION,
    composer: COMPOSER_VERSION,
    frankenphp: FRANKENPHP_VERSION,
    postgres: POSTGRES_VERSION,
    mailpit: MAILPIT_VERSION,
    adminer: ADMINER_VERSION,
    cloudflared: CLOUDFLARED_VERSION,
    redis: REDIS_VERSION,
    mariadb: MARIADB_VERSION,
    httpd: HTTPD_VERSION,
    xdebug: XDEBUG_VERSION,
    bundled_openssl: BUNDLED_OPENSSL_VERSION,
    bundled_pcre2: BUNDLED_PCRE2_VERSION,
    bundled_apr: BUNDLED_APR_VERSION,
    bundled_apr_util: BUNDLED_APR_UTIL_VERSION,
};

/// macOS 14: the standard set except the two rows whose standard pin declares
/// `minos 15.0` — cloudflared and PostgreSQL. Every other standard pin is 14.0
/// or lower on both slices (`docs/PORTS.md`).
const LEGACY14_PINS: PinSet = PinSet {
    cloudflared: LEGACY_CLOUDFLARED_VERSION,
    postgres: LEGACY14_POSTGRES_VERSION,
    postgres_versions: LEGACY14_POSTGRES_VERSIONS,
    ..STANDARD_PINS
};

/// macOS 13: `docs/PLAN-macos-13-floor.md` §6.2, row for row. PostgreSQL has
/// NO build that loads on Apple Silicon at 13 and PHP 8.0.30 is 14.0 there, so
/// their offered sets are empty / without 8.0 — the refusal that explains it to
/// the user is T3's. `postgres` keeps a version that exists so nothing here is a
/// sentinel string; an empty `postgres_versions` is what says "not offered".
const LEGACY13_PINS: PinSet = PinSet {
    cloudflared: LEGACY_CLOUDFLARED_VERSION,
    php_versions: LEGACY13_PHP_VERSIONS,
    mysql: LEGACY13_MYSQL_VERSION,
    mysql_versions: LEGACY13_MYSQL_VERSIONS,
    postgres: LEGACY14_POSTGRES_VERSION,
    postgres_versions: &[],
    redis: LEGACY13_REDIS_VERSION,
    redis_versions: LEGACY13_REDIS_VERSIONS,
    mariadb: LEGACY13_MARIADB_VERSION,
    mariadb_versions: LEGACY13_MARIADB_VERSIONS,
    httpd: LEGACY13_HTTPD_VERSION,
    xdebug: LEGACY13_XDEBUG_VERSION,
    bundled_openssl: LEGACY13_BUNDLED_OPENSSL_VERSION,
    bundled_pcre2: LEGACY13_BUNDLED_PCRE2_VERSION,
    ..STANDARD_PINS
};

impl PinSet {
    /// The pin set a tier resolves — `docs/PLAN-macos-13-floor.md` §6.2.
    pub const fn for_tier(tier: BinaryTier) -> PinSet {
        match tier {
            BinaryTier::Standard => STANDARD_PINS,
            BinaryTier::Legacy14 => LEGACY14_PINS,
            BinaryTier::Legacy13 => LEGACY13_PINS,
        }
    }

    /// Every tier, for a test or a sweep that must cover all of them.
    pub const ALL_TIERS: [BinaryTier; 3] = [BinaryTier::Standard, BinaryTier::Legacy14, BinaryTier::Legacy13];
}

/// The pins for THIS host — [`PinSet::for_tier`] of [`tier`]. Every consumer
/// outside this file reads versions here and nowhere else.
pub fn pins() -> PinSet {
    PinSet::for_tier(tier())
}

/// The Standard set, for a surface that must NAME what a legacy host is missing
/// (a refused PHP minor's row still shows the patch it would have run).
pub fn standard_pins() -> PinSet {
    STANDARD_PINS
}

// ── Tier refusals (`docs/PLAN-macos-13-floor.md` §6.3, ledger #710) ────────────
//
// A feature the host's tier cannot serve is refused with the macOS it NEEDS —
// derived, never listed: the answer is "the lowest tier whose pin set offers
// it", read off the same `PinSet`s the resolver uses, so a row that moves
// between tiers moves the sentence with it. `None` means "not a tier refusal":
// either the feature is offered here, or it exists on no tier at all (an
// unknown minor), which is a different sentence owned elsewhere.

/// Tiers from the lowest floor up, so the FIRST that offers a thing is the
/// oldest macOS it runs on — the number the refusal should name.
const TIERS_LOWEST_FIRST: [BinaryTier; 3] = [BinaryTier::Legacy13, BinaryTier::Legacy14, BinaryTier::Standard];

fn needs_macos_for(offered: impl Fn(&PinSet) -> bool) -> Option<u32> {
    if offered(&pins()) {
        return None;
    }
    TIERS_LOWEST_FIRST
        .into_iter()
        .find(|t| offered(&PinSet::for_tier(*t)))
        .map(|t| t.floor().0)
}

/// The macOS major a PHP minor needs, when this host's tier does not offer it
/// but a newer tier does (8.0 on a 13 host → `Some(14)`). `None` when offered
/// here, or shipped nowhere.
pub fn php_minor_needs_macos(minor: &str) -> Option<u32> {
    needs_macos_for(|p| p.php_versions.iter().any(|v| crate::core::php::minor_of(v) == minor))
}

/// The macOS major a database engine (by `DbEngine::key`) needs, when this
/// host's tier offers no version of it but a newer tier does (PostgreSQL on a
/// 13 host → `Some(14)`).
pub fn engine_needs_macos(key: &str) -> Option<u32> {
    needs_macos_for(|p| {
        !match key {
            "mysql" => p.mysql_versions,
            "mariadb" => p.mariadb_versions,
            "postgres" => p.postgres_versions,
            "redis" => p.redis_versions,
            _ => &[],
        }
        .is_empty()
    })
}

/// Every PHP minor the Standard set ships that this host's tier does not, with
/// the macOS each needs — the rows a legacy host's PHP list shows DISABLED with
/// the reason, rather than omitting (§6.3: never a silent omission).
pub fn refused_php_minors() -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for v in STANDARD_PINS.php_versions {
        let minor = crate::core::php::minor_of(v);
        if let Some(major) = php_minor_needs_macos(&minor) {
            if !out.iter().any(|(m, _)| *m == minor) {
                out.push((minor, major));
            }
        }
    }
    out
}

/// What a legacy host is told ONCE (onboarding): which OS it runs, what that
/// costs, which OS gets everything. `None` on a Standard host — the ordinary
/// case renders nothing. What is missing is DERIVED from the same refusals the
/// surfaces show, so the notice cannot promise a feature the tier refuses or
/// name one it offers.
pub fn legacy_notice() -> Option<String> {
    if tier() == BinaryTier::Standard {
        return None;
    }
    let running = crate::core::macho::host_macos()?;
    let mut missing: Vec<String> = Vec::new();
    for e in ["mysql", "mariadb", "postgres", "redis"] {
        if engine_needs_macos(e).is_some() {
            missing.push(
                match e {
                    "mysql" => "MySQL",
                    "mariadb" => "MariaDB",
                    "postgres" => "PostgreSQL",
                    _ => "Redis",
                }
                .to_string(),
            );
        }
    }
    for (minor, _) in refused_php_minors() {
        missing.push(format!("PHP {minor}"));
    }
    Some(crate::platform::words::current().legacy_notice(running, &missing, BinaryTier::Standard.floor().0))
}

/// The one sentence every tier refusal shows (`PlatformWords::needs_newer_os`),
/// with the host's version when it can be read.
pub fn needs_macos_sentence(major: u32) -> String {
    crate::platform::words::current().needs_newer_os(major, crate::core::macho::host_macos())
}


/// The tier this process resolves pins for. `Standard` until `install_tier`
/// runs — which is every example and every non-macOS launch, so the empty
/// state is exactly today's behaviour and not a degraded one.
#[cfg(not(test))]
static TIER: std::sync::RwLock<BinaryTier> = std::sync::RwLock::new(BinaryTier::Standard);

// Under `cargo test` the tier is PER THREAD, not per process. Every lib test
// runs on its own thread, so a test that installs Legacy13 to look at a
// refusal cannot make a words test on the next thread see PostgreSQL vanish —
// which is exactly what happened the first time two of them ran together
// (`the_credits_line_names_only_the_engines_this_os_ships`, 23 Sep 2026).
// A lock would have to be taken by every READER too, and readers are every
// test that touches a pin; a thread-local needs nothing of anyone.
#[cfg(test)]
thread_local! {
    static TEST_TIER: std::cell::Cell<BinaryTier> = const { std::cell::Cell::new(BinaryTier::Standard) };
}

/// Publish the host's tier to the resolve path. Called once at launch from the
/// platform's answer; nothing else may call it in production (a tier that
/// changes mid-run would hand two pin sets to one cache).
pub fn install_tier(tier: BinaryTier) {
    #[cfg(test)]
    TEST_TIER.with(|t| t.set(tier));
    #[cfg(not(test))]
    if let Ok(mut w) = TIER.write() {
        *w = tier;
    }
}

/// The tier every pin lookup consults, through [`pins`].
pub fn tier() -> BinaryTier {
    #[cfg(test)]
    {
        TEST_TIER.with(|t| t.get())
    }
    #[cfg(not(test))]
    {
        TIER.read().map(|t| *t).unwrap_or(BinaryTier::Standard)
    }
}

/// The binaries a DEFAULT install actually runs ON A TIER, and therefore the set
/// that tier's floor is a claim about. The app's stated floor (`tauri.conf.json`
/// `minimumSystemVersion`) is the LOWEST tier's — 13.0 since T4 of
/// `docs/PLAN-macos-13-floor.md` — and `macos_floor_check` holds every tier's
/// stack under its own floor.
///
/// **Why this is a list in production code and not in the check that reads it.**
/// `docs/PORTS.md` maintains the floor by hand — "the MAX across the binaries the
/// default stack requires" — and a hand-maintained rule is only as good as the
/// last person to re-read it. PostgreSQL sat ABOVE the stated floor at `minos
/// 26.0` for a fortnight (15–30 Aug 2026) and nothing moved, because nothing was
/// comparing. `macos_floor_check` does the comparing; this is the list it
/// compares over, kept beside the version constants so a pin bump and the floor
/// question stay in the same file.
///
/// **Deliberately NOT here: the optional engines.** MySQL/MariaDB/PostgreSQL/Redis
/// are user-chosen, so their floors bind the user who enables them rather than
/// the app's minimum, and they are the multi-hundred-MB trees a check would have
/// to download in both arches to read 32 bytes. Their floors live in PORTS.md's
/// table with the arch caveat that table carries.
pub fn default_stack(tier: BinaryTier) -> [(&'static str, &'static str); 6] {
    let p = PinSet::for_tier(tier);
    [
        ("caddy", p.caddy),
        ("nginx", p.nginx),
        ("php", p.php),
        ("php-fpm", p.php),
        ("mailpit", p.mailpit),
        // Not started by a default launch, but shipped and run on the user's first
        // share — a floor it raised would be discovered by a user, not by us.
        ("cloudflared", p.cloudflared),
    ]
}

const REDIS_VERSION: &str = "8.8.0";
/// Offered Redis versions (single — homebrew-core keeps no versioned redis
/// formula worth pinning; the picker hides for a one-entry set).
const REDIS_VERSIONS: &[&str] = &["8.8.0"];
/// Pinned MariaDB version (bottle bundle: server/client/dump + bootstrap SQL/
/// errmsg/charsets from the mariadb bottle, plus openssl@3 + pcre2 dylibs —
/// the ONLY libs `mariadbd`/clients actually link. groonga/lz4/lzo/xz/zstd are
/// PLUGIN-only deps (mroonga/connect); those plugins are excluded, so their
/// libs aren't bundled).
const MARIADB_VERSION: &str = "12.3.2";
/// All MariaDB versions with pinned bottle bundles (per-engine version switch).
/// 11.4 is the long-term-support series many hosts run (versioned formula
/// `mariadb@11.4` — same bottle layout, same runtime closure, verified).
const MARIADB_VERSIONS: &[&str] = &["12.3.2", "11.4.12"];
/// openssl@3 version bundled INTO dylib bundles (redis, mariadb).
/// Not a standalone binary — only ever a [`BundlePart`].
const BUNDLED_OPENSSL_VERSION: &str = "3.6.3";
/// pcre2 version bundled into the mariadb + httpd bundles (both link libpcre2-8).
const BUNDLED_PCRE2_VERSION: &str = "10.47";
/// Pinned Apache httpd version (bottle bundle: httpd + apr + apr-util + pcre2).
/// The `bin/httpd` core links ONLY apr/apr-util/pcre2 (+ system expat/iconv);
/// openssl/brotli/nghttp2 are deps of mod_ssl/mod_brotli/mod_http2 — those
/// modules are excluded (TLS/H2 are the edge's job), so their libs never enter
/// the bundle. Runs per-site as a loopback OVERRIDE backend (`core/apache.rs`).
const HTTPD_VERSION: &str = "2.4.68";
/// apr / apr-util versions bundled into the httpd bundle.
const BUNDLED_APR_VERSION: &str = "1.7.6";
const BUNDLED_APR_UTIL_VERSION: &str = "1.6.3";
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
/// ships (`docs/archive/PLAN-php-74-support.md` §4.6). The per-minor version lives in
/// [`xdebug_row`]'s table beside the digests it must agree with — one row, one
/// version, one pair of hashes, so a minor cannot end up asking for a `.so` that
/// was never pinned for it.
const XDEBUG_VERSION: &str = "3.5.3";

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
    /// xz-compressed tar of a full directory tree, extracted whole like `TarGzTree`.
    /// Oracle publishes its generic Linux MySQL only as `.tar.xz`. Decoded through
    /// `platform::xz_decoder`, which only the Linux build carries (docs/PLAN-linux-port.md L2).
    TarXzTree,
    /// A zip holding one executable; extract `member` (Windows: `caddy.exe`).
    Zip,
    /// A zip of a whole directory tree, extracted after dropping `strip` leading
    /// path components — zips disagree about a top directory (nginx has one, PHP
    /// does not), so the manifest arm says which rather than a guess at unpack.
    ZipTree { strip: usize },
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

// PHP artifact digests. **8.0.30 is static-php.dev's; 8.1-8.5 are OURS** since
// 10 Sep 2026 (release `php-8x-1`, immutable) — see `php_self_hosted_tag` for
// why, in one line: the bulk builds' `pdo_pgsql` does not exist while their PDO
// says it does.
//
// The two sources differ in a way that matters to whoever re-pins:
//
// - **Ours cannot change under a pin.** A rebuild is a new tag, never a
//   re-upload, so a stale digest 404s rather than silently fetching new bytes.
//   Re-pinning means bumping the tag AND all four digests for that version.
// - **static-php.dev REBUILDS in place** (same URL, new bytes), so a sudden
//   mismatch there usually means a rebuild rather than tampering. On mismatch:
//   download, verify (`php -v` version + `php -m` has mysqli, Mach-O arch), then
//   re-pin. 8.2/8.3 were re-pinned 2026-07-05 after the 2026-07-01 rebuild;
//   8.0.30 was pinned 2026-07-11, downloaded, hashed, extracted and RUN.
//
// One digest per version × {cli,fpm} × {arm64,amd64}; both arches pinned
// together. The 8.1-8.5 rows below were taken from `php-8x-1`'s own SHA256SUMS,
// whose artifacts CI had already run: parity against the bulk module list for
// that minor, and a live PostgreSQL connection through PDO.
const PHP_8_0_30_CLI_MAC_ARM64_SHA256: &str = "13c77c837cd50c027e1c614c192b25205123311a30d2335e9a3c8f82d23acb9a";
const PHP_8_0_30_CLI_MAC_AMD64_SHA256: &str = "b025f2c343916dd97d4cad543f0cc4c07ac882af10bc72773ad27ab8f6c9f59a";
const PHP_8_0_30_FPM_MAC_ARM64_SHA256: &str = "e91bc2624c4469ceb0d7f7d93d643e1aebadd451556b0b1cc8d5142531b14138";
const PHP_8_0_30_FPM_MAC_AMD64_SHA256: &str = "ec02cd54162c190c0029ddccbc382e21442b4941aedef9455b8d6fd32bc472d5";
const PHP_8_1_34_CLI_MAC_ARM64_SHA256: &str = "9cddc02aeeae9ef7c3a79fa58142547c893bac1702fc3d6f8e7a26cdd57777d4";
const PHP_8_1_34_CLI_MAC_AMD64_SHA256: &str = "0778f3f65ef994cf97e19f41a87adb6af38c72b8932d7dd791030f87586b734d";
const PHP_8_1_34_FPM_MAC_ARM64_SHA256: &str = "afbfd758b43103ca9ac6956ee4cba57cff5e248ff7bca81dcbe11d8172a2577a";
const PHP_8_1_34_FPM_MAC_AMD64_SHA256: &str = "8c315735ca1d99bad83cf551889454085d1046f46f10cd75820b245bb7f7f545";
const PHP_8_2_32_CLI_MAC_ARM64_SHA256: &str = "5cae3c0fcc9c7733bd0c244bbee0e6484de6b28cb84d225fbbd7c224999affc1";
const PHP_8_2_32_CLI_MAC_AMD64_SHA256: &str = "cc465b13417cb3f06b284a0c2660acbdcc7da469cc63adf668cff343cdf7ed44";
const PHP_8_2_32_FPM_MAC_ARM64_SHA256: &str = "a76b49225850caa3716f59e95a1c04d23baca17f626838d4f312c0d3067d3efd";
const PHP_8_2_32_FPM_MAC_AMD64_SHA256: &str = "1f5243d034cad68210f15617b05b31543a53a71e85f45d371a8a05b69deaad10";
const PHP_8_3_32_CLI_MAC_ARM64_SHA256: &str = "3ee43b6834b2289a98665a80379cb969a02a7f960f36834e25c3a8cf2d236ef9";
const PHP_8_3_32_CLI_MAC_AMD64_SHA256: &str = "26e1c61ab8901b62527e05a04649f4c1c2ca06a12d4f7101e8faf402845ecb35";
const PHP_8_3_32_FPM_MAC_ARM64_SHA256: &str = "f0f20a3644804aab89a24709504bc0bbc7e594cfdeae818f8eb21ccba2aa53c9";
const PHP_8_3_32_FPM_MAC_AMD64_SHA256: &str = "d5e0e999dfc5786cca0c0e33331761a92b188c36eba232839b9c893893e249d2";
const PHP_8_4_23_CLI_MAC_ARM64_SHA256: &str = "0846e569be64d357f29b071134f4e017cc148497c78fd0acd98e492f797e4574";
const PHP_8_4_23_CLI_MAC_AMD64_SHA256: &str = "5d1bef66c2705f8d7b04ad30ddcc5597ebc1c34a77fa2b8f69597710b07a7755";
const PHP_8_4_23_FPM_MAC_ARM64_SHA256: &str = "7ee88472a02579f9502284de26d919d518b8cbc17df93d7346bc53503dce2dbb";
const PHP_8_4_23_FPM_MAC_AMD64_SHA256: &str = "af7ffbb4f411f5fdb7b556923b618900f8017724a6707abf9055e3fdcb5ed522";
const PHP_8_5_8_CLI_MAC_ARM64_SHA256: &str = "b8cdfee96d192fd0e23be8af8d856189e5da63a0ee4f5a5ad04d88cb2b813dd9";
const PHP_8_5_8_CLI_MAC_AMD64_SHA256: &str = "c2684de199ae1dd7baa51958f4014fc12f322773a141d858ab57a3db573439c0";
const PHP_8_5_8_FPM_MAC_ARM64_SHA256: &str = "b3f6adad9dae74d73b1477c83fb91cb1b1ed61cad47d20fe2af2abaaef46b82a";
const PHP_8_5_8_FPM_MAC_AMD64_SHA256: &str = "df62c644c8328f8e3fa7b0275a0b1c175a6d112ae13c36e75c16c1a0e1d0b2b1";

// nginx — OURS since 30 Aug 2026 (`rexenv/runtimes`, release `nginx-1.30.4-2`,
// immutable), and the reason is the macOS FLOOR, not the version.
//
// The old pin was jirutka/nginx-binaries. `macos_floor_check` measured what its
// two slices actually DECLARE: arm64 `minos 15.0`, x86_64 **`minos 26.0`** —
// eleven majors above the floor this app states, on the one binary every default
// site's requests go through. Upstream had rebuilt on macOS-26 runners, so x86_64
// is 26.0 for every version from 1.26.3 up and arm64 followed from 1.28.3: the
// next routine bump would have taken Apple Silicon too, and the only escape
// upstream offered was pinning a 2024 mainline release.
//
// Ours declares **12.0 on BOTH slices** (asserted per artifact in the build) and
// links nothing but `/usr/lib/libSystem.B.dylib`, because PCRE2 — its only
// dependency — is compiled in from source, and it is built with no ssl module at
// all (the edge owns TLS). So `prepare_binary`'s relink step has nothing to do
// for it. Being the builder makes rexenv the DISTRIBUTOR, the same standing PHP
// 7.4 already put us in.
const NGINX_1_30_4_MAC_ARM64_SHA256: &str = "f95252a853a6a295da3b87bf389ff85df43dfd92b0c7980667a61ab89e12622e";
const NGINX_1_30_4_MAC_AMD64_SHA256: &str = "490a2646a3ea911fb061ff4f6f7f89794f4bc0b56e8e3f323ad1fe0850933fae";

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
        // Legacy13 (`macos14` builds, minos 13.0) — see LEGACY13_MYSQL_VERSIONS.
        "8.4.3" => (MYSQL_8_4_3_MAC_ARM64_SHA256, MYSQL_8_4_3_MAC_AMD64_SHA256),
        "8.0.40" => (MYSQL_8_0_40_MAC_ARM64_SHA256, MYSQL_8_0_40_MAC_AMD64_SHA256),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
}

/// The `macosNN` token in a MySQL tarball's name — the macOS it was BUILT on,
/// and one above the macOS it RUNS on (deployment target NN−1, measured). Not
/// derivable from the version: Oracle moved 8.4 to `macos15` at 8.4.4 and 8.0 at
/// 8.0.41, so this is a per-pin fact and a re-pin edits it beside the digest.
fn mysql_macos_build(version: &str) -> &'static str {
    match version {
        "8.4.3" | "8.0.40" => "macos14",
        _ => "macos15",
    }
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
        "18.6.0" => (POSTGRES_18_6_0_MAC_ARM64_SHA256, POSTGRES_18_6_0_MAC_AMD64_SHA256),
        "17.11.0" => (POSTGRES_17_11_0_MAC_ARM64_SHA256, POSTGRES_17_11_0_MAC_AMD64_SHA256),
        "16.15.0" => (POSTGRES_16_15_0_MAC_ARM64_SHA256, POSTGRES_16_15_0_MAC_AMD64_SHA256),
        // Legacy14 — see LEGACY14_POSTGRES_VERSIONS.
        "16.4.0" => (POSTGRES_16_4_0_MAC_ARM64_SHA256, POSTGRES_16_4_0_MAC_AMD64_SHA256),
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

// Adminer single-file SHA-256 (GitHub release `adminer-6.1.0-en.php`; same on every
// OS/arch — a PHP script). English UI, all DB drivers (MySQL + PostgreSQL).
//
// Re-pinned 21 Sep 2026 from 5.4.2, which was EIGHT releases behind the published
// manifest (rexenv/rexenv#1): the pin is what a fresh install downloads, so a stale
// one ships an old console to every new machine while the update path quietly offers
// a newer one. The digest is the signed manifest's, checked against the release file
// itself before it landed here.
const ADMINER_6_1_0_SHA256: &str = "d21891f420eac5553e9a85d8af2ef3375c1066c4b5c0f573d8e4f936666d2000";

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
//
// Re-pinned 2026-09-18: the assets were rebuilt in place again (arm64 on
// 2026-07-19, x86_64 on 2026-07-20 — the daily cron, same tag). Caught by the
// clean-VM smoke test, where the checksum guard made FrankenPHP uninstallable
// (`checksum mismatch for …/frankenphp-mac-arm64`) — on the dev Mac the cache
// still held the July bytes, so nothing there could see it. Same procedure:
// GitHub API `digest` matched a fresh download of each asset; the sigstore
// attestations (`repos/dunglas/frankenphp/attestations/sha256:<hash>`) name
// each file as the subject, workflow `.github/workflows/static.yaml` of
// `php/frankenphp`, commit e41b848e; both run — arm64 natively and x86_64 under
// Rosetta: "FrankenPHP v1.12.4 PHP 8.5.8 Caddy v2.11.4" (embedded PHP unchanged,
// so `FRANKENPHP_EMBEDDED_PHP` stays).
const FRANKENPHP_1_12_4_MAC_ARM64_SHA256: &str = "fb38e69514a04875b83900da0e1585d611fe0f52f3a904c50bef5605347e5dec";
const FRANKENPHP_1_12_4_MAC_AMD64_SHA256: &str = "0e97b3e2bb8e98c0f8c6275921e1d44fb6459abfaa3bc0525e56c07ae13aa55b";

// PostgreSQL portable build SHA-256 (theseus-rs/postgresql-binaries — the project
// PUBLISHES these `.sha256` files; cross-checked against a fresh download). A
// relocatable bin/lib/share tree (unsigned Mach-O that runs as-is on Apple Silicon).
//
// RE-PINNED 30 Aug 2026 — 18.4.0/17.10.0/16.14.0 → 18.6.0/17.11.0/16.15.0, and the
// reason is the macOS FLOOR rather than the patch level. The June builds were made on
// macOS-26 runners and every Mach-O in them carries `minos 26.0`, which presumed the
// whole engine dead below macOS 26 and could not be tested from a machine that runs
// 26 (dyld there enforces minos for nothing). The 15 Aug upstream builds went back to
// `minos 15.0` — measured across EVERY executable and dylib in all six tarballs, not
// just `bin/postgres` — so the question the old pins raised is answered by not asking
// it: 15.0 is the app's own floor. A newer patch level came along for free; the floor
// is what the bump is for.
const POSTGRES_18_6_0_MAC_ARM64_SHA256: &str = "a257bcdb8aa3301a13d6a5bcec48f8c9517045b7cbae71e50788b2615539e95b";
const POSTGRES_18_6_0_MAC_AMD64_SHA256: &str = "f8918fbe747e0d79bda58ad8f8afcf23ac384b847ff3af18856f0825ba54b031";
// 17/16 series (per-engine version switch) — same published-.sha256 source, each
// cross-checked against a fresh download 30 Aug 2026.
const POSTGRES_17_11_0_MAC_ARM64_SHA256: &str = "fd4b62794b160e26973a768a1eef3248aef9d2ff23ebd6d884a4299485e28e57";
const POSTGRES_17_11_0_MAC_AMD64_SHA256: &str = "e43a81b15e1cfe7f9d8fd79c6d4d0366e9001a5f690e322224dca704656602f7";
const POSTGRES_16_15_0_MAC_ARM64_SHA256: &str = "46f6382024d9b633d1f4b4903ffef8c2404e00ae83098d628c00591048cb0512";
const POSTGRES_16_15_0_MAC_AMD64_SHA256: &str = "dfae21b2b931f1d8d1103afb6e8e108fdd94c6f407ce24d35b6cd0ad34f286d1";

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

// ── Legacy tiers: the pins a macOS 13 / 14 host resolves ──────────────────────
//
// `docs/PLAN-macos-13-floor.md` §6.2. Every row below is the NEWEST upstream build
// whose `minos` fits the tier, measured on both slices from the artifact's own load
// commands (§3 of that plan) — never inferred from a filename or a runner label.
// Digests: every artifact downloaded and hashed here on 23 Sep 2026; the ghcr
// blobs' hashes equal their content-addressed digests, as ghcr's contract says
// they must (a pin that TRUSTED that would still be a pin we never checked).
//
// These sit in the SAME manifest tables as the standard pins, keyed by their own
// version strings: the tables are tier-blind, the tier only decides which version
// a consumer asks for (`PinSet::for_tier`). So a legacy artifact resolves on a
// standard host too — which is what lets this Mac sweep, hash and load-test them.

/// The last cloudflared built for macOS 13: 2025.4.2 moved to a macOS-15 SDK and
/// declares `minos 15.0` on both slices from there on. arm64 13.0 / x86_64 10.13
/// (the older `LC_VERSION_MIN_MACOSX` form, which `core::macho` reads).
const LEGACY_CLOUDFLARED_VERSION: &str = "2025.4.0";
const CLOUDFLARED_2025_4_0_MAC_ARM64_SHA256: &str = "7a9f9d72895cf3c5374327f65472789259eaedd82af93ac16f82035109fa121f";
const CLOUDFLARED_2025_4_0_MAC_AMD64_SHA256: &str = "b5e8afdb58fc89f7f9cf499d6a593aa5284644054bc5f5c46649d3628812c0e2";

/// MySQL's `macosNN` tarball is built with deployment target NN−1 (measured on
/// every version from 8.0.33 to 8.4.6, both slices, ~100 Mach-Os each): the
/// `macos14` builds are `minos 13.0`. 8.4.3 and 8.0.40 are the last of them —
/// 8.4.4 / 8.0.41 onward are `macos15` = 14.0. See [`mysql_macos_build`].
const LEGACY13_MYSQL_VERSION: &str = "8.4.3";
const LEGACY13_MYSQL_VERSIONS: &[&str] = &["8.4.3", "8.0.40"];
const MYSQL_8_4_3_MAC_ARM64_SHA256: &str = "af1af43030ac66b73dc2d5dcf645a61cdf9e7cf5404cf04bdf8e194447b0f153";
const MYSQL_8_4_3_MAC_AMD64_SHA256: &str = "b690dfaad2108889390d40df388c16453e345f69a77784444687e8e308855af6";
const MYSQL_8_0_40_MAC_ARM64_SHA256: &str = "a0b8449c19ef59ca688c93ffd89d42f5d78abe6cc136c0d754c6ccb3b202fb9a";
const MYSQL_8_0_40_MAC_AMD64_SHA256: &str = "a416ee86e72f22089c41911bfee08be0d4dab3b816923be7b465f65df555b36d";

/// theseus-rs builds arm64 on Apple Silicon runners that were never older than
/// macOS 14, so NO PostgreSQL build loads on an Apple Silicon Mac at 13 — that
/// engine is refused there (T3). 16.4.0 is the newest at 14.0 arm64 / 13.0
/// x86_64; the 16.6.0+ builds are 15.0 on arm64. Published `.sha256` cross-checked
/// against a fresh download of each slice.
const LEGACY14_POSTGRES_VERSION: &str = "16.4.0";
const LEGACY14_POSTGRES_VERSIONS: &[&str] = &["16.4.0"];
const POSTGRES_16_4_0_MAC_ARM64_SHA256: &str = "0ec91e77eff381e43e3963f012aff3acb9de12ad3739a625e57cce9671b28b0f";
const POSTGRES_16_4_0_MAC_AMD64_SHA256: &str = "3193b9747c610139990c9913ff5fd5ad73cd38cefcd5ffcdc46079fd1479406e";

// Homebrew stopped building for macOS 13 in 2025; `formulae.brew.sh` lists no
// `ventura` bottle for any of these formulas today. The rows below are the NEWEST
// tag of each whose ghcr OCI index still carries an `os.version: macOS 13.x`
// platform (walked newest→oldest), and the blob is fetched by digest exactly as
// the sonoma ones are — content-addressed, so it can 404 on a registry prune but
// never change. arm64 = `arm64_ventura`, x86_64 = `ventura`; every blob swept:
// `minos 13.0` on both slices.
const LEGACY13_REDIS_VERSION: &str = "8.2.1";
const LEGACY13_REDIS_VERSIONS: &[&str] = &["8.2.1"];
const REDIS_8_2_1_BOTTLE_ARM64_SHA256: &str = "97c7c21a13227b52c5634809c2a4ae6f1acde764367277cc20d3dfc9ab4a37ea";
const REDIS_8_2_1_BOTTLE_AMD64_SHA256: &str = "d2fdc1571a609a3866aa45e15e9c051e685d4d90dfcef3ce88d8a8d36c177162";
const LEGACY13_BUNDLED_OPENSSL_VERSION: &str = "3.5.2";
const OPENSSL_3_5_2_BOTTLE_ARM64_SHA256: &str = "13545dea2fcfac0542556f969011892b2e0001b5ef0b71a787ca4ad714567ef5";
const OPENSSL_3_5_2_BOTTLE_AMD64_SHA256: &str = "b51da4aaa601358273a5161c8aea4b19998ac1c48224a4b067c4c3e5475f9482";
const LEGACY13_MARIADB_VERSION: &str = "12.0.2";
const LEGACY13_MARIADB_VERSIONS: &[&str] = &["12.0.2", "11.4.8"];
const MARIADB_12_0_2_BOTTLE_ARM64_SHA256: &str = "ccd5bf7fa727cb7fab1e133fcd20e9102d1939b0a8099b115e0943d8bc5aa99f";
const MARIADB_12_0_2_BOTTLE_AMD64_SHA256: &str = "1dea9cef316370d57a541a4278ca5e787e46b95ea53c36d3fc9fe938010a7e65";
const MARIADB_11_4_8_BOTTLE_ARM64_SHA256: &str = "d43c7c65aa17f0192ef7aaca3d265cd34325881c0cbc04867c3c23071756d5bd";
const MARIADB_11_4_8_BOTTLE_AMD64_SHA256: &str = "f3d60f3215670b686323ad11eda4d9d3359f4d607c7904d11baa59aa55fa49fc";
const LEGACY13_BUNDLED_PCRE2_VERSION: &str = "10.46";
const PCRE2_10_46_BOTTLE_ARM64_SHA256: &str = "6b38069079a641040e1cf8c408afbf902384d70bcc8fdbd445a9acc31e1e70d1";
const PCRE2_10_46_BOTTLE_AMD64_SHA256: &str = "e71e438a81766aafffd719f90ba93cdaa158f91c08e19d96428fa841da15bd5a";
const LEGACY13_HTTPD_VERSION: &str = "2.4.65";
const HTTPD_2_4_65_BOTTLE_ARM64_SHA256: &str = "c335d95bee9dc0d6c39abd07c94a57f019ae6f123c7f7b86929748858bea25b3";
const HTTPD_2_4_65_BOTTLE_AMD64_SHA256: &str = "f256ac4ff3824cde9a0b9cad87b5c6316f690b88d43d2662df54010f283ccdd0";
// apr is the one formula that still publishes a ventura bottle for its CURRENT
// version: same 1.7.6, a different blob. apr-util's last ventura tag is 1.6.3_1.
const APR_1_7_6_VENTURA_BOTTLE_ARM64_SHA256: &str = "c9c536ea3504e24b30b5cf6187100f746eba704e237d3839d0c04feb98df623e";
const APR_1_7_6_VENTURA_BOTTLE_AMD64_SHA256: &str = "327273dae10ae18781b2f347531253d968e0c06533c913a80d775a5972e65477";
const APR_UTIL_1_6_3_VENTURA_BOTTLE_ARM64_SHA256: &str = "cb73075171b2079d2b8e8028f42766dffa5db08882261c3f5aff59d8eb9638a9";
const APR_UTIL_1_6_3_VENTURA_BOTTLE_AMD64_SHA256: &str = "127d4d4523d49a73e7dbf610f3e439ac2051a383edbf28cc18438faf78945ef0";
// Xdebug 3.4.5 — the last shivammathur tag with ventura blobs, for 8.1–8.4.
// **8.5 has NO legacy row**: every `xdebug@8.5` ventura blob (3.3.0 → 3.4.5-3)
// was built before PHP 8.5 GA against the 8.4 Zend API (`Xdebug requires Zend
// Engine API version 420240925`, measured against php 8.5.8 on 23 Sep 2026,
// `legacy_pins_check`), and Homebrew stopped building for 13 before 8.5
// shipped. So on a 13 host the 8.5 toggle is `XdebugStatus::NotPinned` —
// honest: there is nothing to pin.
const LEGACY13_XDEBUG_VERSION: &str = "3.4.5";
const XDEBUG_PHP81_VENTURA_BOTTLE_ARM64_SHA256: &str = "0b9b84f40049789dcf39f6b30f341ee13bd2d6de50c0c60e7e50b94bed49b5ba";
const XDEBUG_PHP81_VENTURA_BOTTLE_AMD64_SHA256: &str = "1cfb328038fb6af6f52aed030ae55d98a27cd077b909d74a19769a06ef240b98";
const XDEBUG_PHP82_VENTURA_BOTTLE_ARM64_SHA256: &str = "d283de8feab11add522c15fe94342d16c318f969e9f547943a0b169258719b86";
const XDEBUG_PHP82_VENTURA_BOTTLE_AMD64_SHA256: &str = "c85b6987673863d31814ee4e3724168525c307b961c7612ec0408e7edc78f0dc";
const XDEBUG_PHP83_VENTURA_BOTTLE_ARM64_SHA256: &str = "619210495b3787c4f65a77390f14eb52a6c8ee783464ac9de7b6ed53b765a9db";
const XDEBUG_PHP83_VENTURA_BOTTLE_AMD64_SHA256: &str = "fae1e4d5fd38c308731f64a631d5ca5b1fd952343402116489295ea6d5c41ee2";
const XDEBUG_PHP84_VENTURA_BOTTLE_ARM64_SHA256: &str = "100d4377d2d5420d4cdc5d419147694752a60bdec15387883ca3b1124feaa910";
const XDEBUG_PHP84_VENTURA_BOTTLE_AMD64_SHA256: &str = "e6a028aa2b5feaaad579ea5cda8e641801fc1b3dd22e0ee22865f8db38e3fc80";

/// PHP 8.0.30 is static-php.dev's build and `minos 14.0` on arm64; there is no
/// older artifact to pin (they rebuild in place) and no self-build (its x86_64
/// build aborts — see `php_self_hosted_tag`). So a 13 host is not offered it.
const LEGACY13_PHP_VERSIONS: &[&str] = &["7.4.33", "8.1.34", "8.2.32", "8.3.32", "8.4.23", "8.5.8"];

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
    /// The minor HAS a pinned bottle, but not on this OS — D4: Xdebug is not in
    /// Windows v1 (its DLLs must match PHP's NTS build and compiler, which is a
    /// pin and a trust decision per minor). A fourth variant rather than reusing
    /// [`Self::NotPinned`] for the same reason that one exists: "we have not
    /// pinned it yet" and "it is not part of this platform's v1" are different
    /// facts, and the advice differs — there is nothing to wait for here, and
    /// no newer PHP to switch to (W10, ledger #642).
    NotOnThisOs,
}

/// The Xdebug status for a PHP minor on a NAMED os: a pinned row, or the REASON
/// there is none. The single source of which minors support the toggle, of which
/// release each one gets, and of what to tell a user who asks for one that has none.
///
/// There is no host-reading wrapper beside this one: every caller either knows the
/// os it means (`xdebug_bottle_on`/`xdebug_unavailable_reason` supply
/// `std::env::consts::OS`) or is a test naming the os whose pins it is about. One
/// name for one question — the wrapper that used to sit here became unreachable the
/// moment its callers took the os as a parameter (W12).
fn xdebug_status_on(minor: &str, os: &str) -> XdebugStatus {
    let pinned = match xdebug_row(minor) {
        Some(b) => XdebugStatus::Available(b),
        None => return xdebug_absence(minor),
    };
    // …and a pinned bottle still has to EXIST on this os. Asked of the same table a download
    // would read (`ships_on`), never of a per-os list kept here, so the gate cannot drift
    // from what the downloader can fetch.
    //
    // Asked of the ROW rather than through this function's own callers: `ships_on` reaches
    // `bundle_manifest`, whose xdebug arm needs the row. Calling `xdebug_bottle_on` there — as
    // the first version of this did — is a cycle, and it aborted the test binary with a
    // stack overflow rather than failing an assertion (W10, ledger #642).
    match pinned {
        XdebugStatus::Available(b) if !ships_on(&format!("xdebug-{minor}"), b.version, os) => {
            XdebugStatus::NotOnThisOs
        }
        other => other,
    }
}

/// Why a minor has NO pinned row — the two absences, kept apart.
fn xdebug_absence(minor: &str) -> XdebugStatus {
    match minor {
        "7.4" => XdebugStatus::CannotLoadExtensions {
            measured: "rexenv's own 7.4.33 build, 14 Aug 2026",
        },
        "8.0" => XdebugStatus::CannotLoadExtensions {
            measured: "static-php.dev's 8.0.30 build, Nov 2024",
        },
        _ => XdebugStatus::NotPinned,
    }
}

/// The pinned Xdebug ROW for a minor — the table alone, with no os rule applied.
///
/// Separate from [`xdebug_status`] because [`bundle_manifest`] needs the row while
/// `xdebug_status` now carries a POLICY that asks `bundle_manifest` back. Row and policy in
/// one function is what made that a cycle.
fn xdebug_row(minor: &str) -> Option<XdebugBottle> {
    xdebug_row_at(minor, pins().xdebug)
}

/// The pinned row for a minor AT A NAMED Xdebug version — the table itself,
/// tier-blind like every other manifest table here: the Standard rows (3.5.3,
/// sonoma blobs) and the Legacy13 rows (3.4.5, ventura blobs) both resolve on
/// any host, and [`xdebug_row`] picks by the tier's pin. `bundle_manifest`'s
/// xdebug arm asks this with the version it was handed, so a cache dir's
/// identity is exactly one (minor, version) row.
fn xdebug_row_at(minor: &str, version: &str) -> Option<XdebugBottle> {
    let row = |version, formula, arm64, amd64| Some(XdebugBottle { version, formula, arm64, amd64 });
    match (minor, version) {
        // Measured, not assumed. Both exports checked with `nm -gU`; 7.4's build
        // shows ~22,400 symbols and not the one that matters.
        ("8.1", XDEBUG_VERSION) => row(XDEBUG_VERSION, "xdebug@8.1", XDEBUG_PHP81_BOTTLE_ARM64_SHA256, XDEBUG_PHP81_BOTTLE_AMD64_SHA256),
        ("8.2", XDEBUG_VERSION) => row(XDEBUG_VERSION, "xdebug@8.2", XDEBUG_PHP82_BOTTLE_ARM64_SHA256, XDEBUG_PHP82_BOTTLE_AMD64_SHA256),
        ("8.3", XDEBUG_VERSION) => row(XDEBUG_VERSION, "xdebug@8.3", XDEBUG_PHP83_BOTTLE_ARM64_SHA256, XDEBUG_PHP83_BOTTLE_AMD64_SHA256),
        ("8.4", XDEBUG_VERSION) => row(XDEBUG_VERSION, "xdebug@8.4", XDEBUG_PHP84_BOTTLE_ARM64_SHA256, XDEBUG_PHP84_BOTTLE_AMD64_SHA256),
        ("8.5", XDEBUG_VERSION) => row(XDEBUG_VERSION, "xdebug@8.5", XDEBUG_PHP85_BOTTLE_ARM64_SHA256, XDEBUG_PHP85_BOTTLE_AMD64_SHA256),
        ("8.1", LEGACY13_XDEBUG_VERSION) => row(LEGACY13_XDEBUG_VERSION, "xdebug@8.1", XDEBUG_PHP81_VENTURA_BOTTLE_ARM64_SHA256, XDEBUG_PHP81_VENTURA_BOTTLE_AMD64_SHA256),
        ("8.2", LEGACY13_XDEBUG_VERSION) => row(LEGACY13_XDEBUG_VERSION, "xdebug@8.2", XDEBUG_PHP82_VENTURA_BOTTLE_ARM64_SHA256, XDEBUG_PHP82_VENTURA_BOTTLE_AMD64_SHA256),
        ("8.3", LEGACY13_XDEBUG_VERSION) => row(LEGACY13_XDEBUG_VERSION, "xdebug@8.3", XDEBUG_PHP83_VENTURA_BOTTLE_ARM64_SHA256, XDEBUG_PHP83_VENTURA_BOTTLE_AMD64_SHA256),
        ("8.4", LEGACY13_XDEBUG_VERSION) => row(LEGACY13_XDEBUG_VERSION, "xdebug@8.4", XDEBUG_PHP84_VENTURA_BOTTLE_ARM64_SHA256, XDEBUG_PHP84_VENTURA_BOTTLE_AMD64_SHA256),
        // ("8.5", LEGACY13_XDEBUG_VERSION): deliberately absent — see LEGACY13_XDEBUG_VERSION.
        _ => None,
    }
}
    // …and a pinned bottle still has to EXIST on this OS. Asked of the same table a
    // download would read, never of a per-OS list kept here (`ships_on`): the gate cannot
    // then drift from what the downloader can fetch. Asked with the bottle already in hand

/// The pinned row for a minor ON A NAMED OS, dropping the reason. For callers that only
/// need to know WHETHER, never why — everything user-facing goes through
/// [`xdebug_unavailable_reason`] instead.
///
/// Takes the os for the same reason `ships_on` and `xdebug_status_on` do: on Windows these
/// answer `None`, and a test that can only ask the HOST cannot state what macOS pins while
/// running on the Dell (W12). The host-reading wrapper that used to sit beside it became
/// unreachable the moment `xdebug_bundle_id` took an os too — the third time that happened
/// in this port (`xdebug_status`, `ensure_server_available`), and each time only the bar
/// saw it: `-D dead-code` is a clippy/`cargo check` verdict, and `cargo test --lib` stays
/// green with the function sitting there uncalled.
fn xdebug_bottle_on(minor: &str, os: &str) -> Option<XdebugBottle> {
    match xdebug_status_on(minor, os) {
        XdebugStatus::Available(b) => Some(b),
        _ => None,
    }
}

/// Whether the per-site Xdebug toggle is available for a PHP minor.
pub fn xdebug_supported(minor: &str) -> bool {
    xdebug_supported_on(minor, std::env::consts::OS)
}

/// [`xdebug_supported`] for a NAMED os.
pub fn xdebug_supported_on(minor: &str, os: &str) -> bool {
    xdebug_bottle_on(minor, os).is_some()
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
    xdebug_unavailable_reason_on(minor, std::env::consts::OS)
}

/// [`xdebug_unavailable_reason`] for a NAMED os — the sentence a user on `os` would read.
///
/// `pub(crate)` so `core::sites::set_xdebug_on` can name the os too: the toggle's own
/// refusal is the first thing a Windows run hits, before the flag behaviour under test.
pub(crate) fn xdebug_unavailable_reason_on(minor: &str, os: &str) -> Option<String> {
    match xdebug_status_on(minor, os) {
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
        // No way out is offered on purpose: unlike the two above, this is not about the
        // site's PHP at all, so "switch to a newer PHP" would send the user to change
        // something that would not help.
        XdebugStatus::NotOnThisOs => Some(format!(
            "Xdebug isn't part of rexenv on {} yet — its builds have to match each PHP \
             version's compiler exactly, so they are pinned per version and none is \
             pinned here. Nothing is wrong with your site or your PHP {minor}.",
            if os == "windows" { "Windows" } else { crate::platform::words::current().os_name }
        )),
    }
}

/// The Xdebug release pinned for a PHP minor, or `None` where the toggle isn't
/// offered. For the UI: the version a debug pool will actually load, which is
/// not app-wide (see [`XDEBUG_VERSION`]).
pub fn xdebug_version_for(minor: &str) -> Option<&'static str> {
    xdebug_version_for_on(minor, std::env::consts::OS)
}

/// [`xdebug_version_for`] for a NAMED os.
pub fn xdebug_version_for_on(minor: &str, os: &str) -> Option<&'static str> {
    xdebug_bottle_on(minor, os).map(|b| b.version)
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
    xdebug_bundle_id_on(minor, std::env::consts::OS)
}

/// [`xdebug_bundle_id`] for a NAMED os — the rest of the Xdebug accessors take one for the
/// same reason (#642, #657): nothing is pinned for Windows, so the host-reading form is
/// `None` there and a test about the ID's SHAPE has to name the os whose pins it means.
pub fn xdebug_bundle_id_on(minor: &str, os: &str) -> Option<(String, &'static str)> {
    xdebug_bottle_on(minor, os).map(|b| (format!("xdebug-{minor}"), b.version))
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
/// `aarch64`/`x86_64` — the spelling OUR OWN nginx release uses, matching the
/// PHP 7.4 artifacts from the same repo. The previous third-party pin spelled it
/// `arm64`, and this function still returning that is what failed
/// `manifest_pins_nginx_as_raw_binary` the moment the pin moved: an arch name is
/// part of a URL, so it belongs to whoever publishes the file.
fn nginx_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "aarch64",
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

// ── Windows x64 artifacts (docs/PLAN-windows-port.md W2) ─────────────────────
// Every one downloaded and hashed on 12 Sep 2026. Caddy's SHA-512 and PostgreSQL's
// SHA-256 matched the digests their publishers post; the rest post none we can pin
// against (php.net's archive has no sha256sum.txt, Mailpit and nginx.org publish no
// digest, MySQL publishes MD5), so they are pinned from our own download — the
// practice the macOS rows already follow. x64 ONLY, for every `Arch`: Windows on
// ARM runs x64 under emulation, and PHP, MySQL, nginx and PostgreSQL publish no
// arm64 Windows build, so one pinned set is also the only proof there is.
const CADDY_2_11_4_WIN_AMD64_SHA512: &str = "cd5ccfd86a4b40732cf715890d0dca5bf3f63adefec5a7914de85adf240c60ce7e5d2791631b88ef9758e46b23bb1730e020b9c5d696889740b284ffd4788e35";
const NGINX_1_30_4_WIN_AMD64_SHA256: &str = "159294214d403f34f0bb4ae598801ab1f6a0d8c8da707f8f08748e294a222a01";
const MAILPIT_1_30_3_WIN_AMD64_SHA256: &str = "7e9bdf9a299ae6df2c3e73d8c666af8a084dd3ddf5f218522c9cfbdb54e0a015";
const CLOUDFLARED_2026_6_1_WIN_AMD64_SHA256: &str = "5253e66f1f493c4e13539749f1aa86fd0c61e3072900fec29a44ba046a6d97e2";

/// php.net's Windows builds: NTS x64 zips from the permanent `archives/` path,
/// which keeps every release (the current-releases directory moves on).
fn php_windows_sha256(version: &str) -> Option<&'static str> {
    match version {
        "7.4.33" => Some("14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace"),
        "8.0.30" => Some("dfb70498ffa2c617f2f655a155564697e3c9cca41709938fd1a5997d1d5b0785"),
        "8.1.34" => Some("9cfe246cb144076c16f5913a3ef88a474c3dd7e60f0f0c8bb95faf68674016cc"),
        "8.2.32" => Some("44e561948d6e336ac91cba3e59f074b687e22d9ceb824056f28ed8dfbbfdbe35"),
        "8.3.32" => Some("67c724e7b675b50d8f0476d816c3e2a3064ce3a53d572575d63c321cc0a3a6cf"),
        "8.4.23" => Some("826efa189b21f46314ad497ff31467de9f0953292f42b235542be4feea182b48"),
        "8.5.8" => Some("63a3f6493f37c9ff3e288ec16621222a6cda5167dd1abffec0019e7f18c8e7e9"),
        _ => None,
    }
}

/// The MSVC toolset a php.net Windows file name carries: `vc15` for 7.4, `vs16`
/// for 8.0–8.3, `vs17` from 8.4 on.
fn php_windows_toolset(version: &str) -> &'static str {
    if version.starts_with("7.") {
        "vc15"
    } else if ["8.0.", "8.1.", "8.2.", "8.3."].iter().any(|p| version.starts_with(p)) {
        "vs16"
    } else {
        "vs17"
    }
}

fn mysql_windows_sha256(version: &str) -> Option<&'static str> {
    match version {
        "8.4.6" => Some("b6c152f9f3aaa7294eb47db698e47974d37b261bf3cab4f90dc1243bb5ecd204"),
        "8.0.44" => Some("4d9316d2955b0bd2b484e08bda67c18d7979e0622b84f2e9146d97f04ed9a2fc"),
        _ => None,
    }
}

/// theseus-rs publishes a `.sha256` beside each Windows tarball; these are those.
fn postgres_windows_sha256(version: &str) -> Option<&'static str> {
    match version {
        "18.6.0" => Some("7da44c2dbcda3b49688ea08ce8cf99cfe677adf565f53a9145bf9002c74db7d5"),
        "17.11.0" => Some("a013f0e082826f53985c7bc2a0fe1c2dcc47a9f3797023938cf1ff8c9d6d1792"),
        "16.15.0" => Some("157bd7322f8c653f06b0373d1b2280e9cb238b6a6d3bd05541aeb8f33885a8ad"),
        _ => None,
    }
}

// ── Linux artifacts (docs/PLAN-linux-port.md L2) ──────────────────────────────
// Every one streamed through sha256/sha512 on the Mac on 24 Sep 2026 (the Mac had no
// disk for the files). Caddy's SHA-512 and PostgreSQL's SHA-256 match the digests their
// publishers post; static-php.dev, jirutka, Mailpit, cloudflared, FrankenPHP and Oracle
// post none we pin against (Oracle posts MD5), so those are pinned from our download —
// the macOS rows' practice. BOTH archs, unlike Windows: every upstream here publishes
// aarch64 (D-L5). PHP 7.4 has no static Linux build anywhere, so it is not in Linux v1.
// nginx is jirutka's static build (the third-party pin macOS used before its own), until
// `rexenv/runtimes` publishes a Linux one. MySQL ships only `.tar.xz` for Linux, which is
// why `Archive::TarXzTree` exists.
const CADDY_2_11_4_LINUX_AMD64_SHA512: &str = "8220d1f013b6f27510247b2360c9e0ca9f018feebd82515f07635318b34ff9777ccc8fd0b6e6f2486ce3a33fe389fbb7db12d05baa474f4587509fb4f5ebf1c9";
const CADDY_2_11_4_LINUX_ARM64_SHA512: &str = "d5a7c423853c24a799765e0e8210d5c7c22a8f56ed37a3cae2fb9f58be138853c02b4efd6b59d576e6d8c7c0d30b9c1592deeaa6a536ff69bcca23b8c1ea709c";
const NGINX_1_30_4_LINUX_X86_64_SHA256: &str = "9c0b53e93e43a33b0cd7876e7977099ce82b8bcf9bea5bd5b631db6080ad17a2";
const NGINX_1_30_4_LINUX_AARCH64_SHA256: &str = "b09cca8c5d2fb9443ea8c27b033ee04f050f253425bfbf2e0c50960c488a00f9";
const MAILPIT_1_30_3_LINUX_AMD64_SHA256: &str = "6c7af993fb4054def4adfc7c85b40f9570fd6172eaccd0221c4969f2cc7a6294";
const MAILPIT_1_30_3_LINUX_ARM64_SHA256: &str = "4211e158fcf46862b9b15bacd1fb10253a1865617ed5f38f27cbec230f89ec84";
const CLOUDFLARED_2026_6_1_LINUX_AMD64_SHA256: &str = "67fc63f72ce3ffbfd797893d7cd76224717117eef4397190ce412755108b789c";
const CLOUDFLARED_2026_6_1_LINUX_ARM64_SHA256: &str = "8b95e9b2f59edb022a7609a8d59b4ee46f62aff172ead28a20ebe0c9c3d2d539";
const FRANKENPHP_1_12_4_LINUX_X86_64_SHA256: &str = "db0f336e97f841eb3a606279cf6787d68f7fe9016b1c331489844d2c07e1aa5e";
const FRANKENPHP_1_12_4_LINUX_AARCH64_SHA256: &str = "6fafdaa593b223391b96ba2b7aaf65da3e4c5092efd15411724ca6c1355808c5";

/// The Linux spelling of an arch as static-php, jirutka, theseus-rs, FrankenPHP and
/// Oracle name it (`x86_64`/`aarch64`); Caddy, Mailpit and cloudflared use Go's
/// (`amd64`/`arm64`, `caddy_arch`).
fn linux_arch(arch: Arch) -> &'static str {
    php_arch(arch)
}

/// static-php.dev's Linux builds: `(aarch64, x86_64)` per `(kind, version)`.
fn php_linux_sha256(kind: &str, version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match (kind, version) {
        ("cli", "8.0.30") => ("2c3fbc72d878862cf3b3d18849d4ab2ca7c02d38821d880eb8fb74a2e6f5c1d3", "f8f139c6fbca27ad335a78b9ec962a72350846027af50fc8cf005905412ea6df"),
        ("fpm", "8.0.30") => ("b693b0c1150b12287bfe915a67bd57d3189a791e8b0c0de534e803e04fade625", "08176de6fe5125b7c1486f723f601f5c1273663bc5f719abc9c62adaa55cafc7"),
        ("cli", "8.1.34") => ("fcaad73ceadad5afe6820ab3510de4452dadc875b3a295bf22dc2678845922b2", "e2932192836731163bf8d16d634f4bdda76c422ceb40c2c4d7c55ae63680f1aa"),
        ("fpm", "8.1.34") => ("a020585084d0d624e10ab795430816f160a73ad4e827130c55bc030be4f9ceb3", "c20b1283a4a635d4c062b473858a46d687e94bf8ef623f26e356592557065b15"),
        ("cli", "8.2.32") => ("e94cc88f535f4e20a931093bbd36aa74b1b48f4121954aa6d7310912a8aad26e", "866532d2b463ff256d063432d529e966cc53e4a039109ccbfc4ee4dc51d649f1"),
        ("fpm", "8.2.32") => ("d16bd173d30d8d8d663146bddc785d4350f584d5ce105c52f0a0c85305ba5e51", "e324b63d961bedb7273c1a3585f9b209cc1e057016056ed34b8bb1b32526db3e"),
        ("cli", "8.3.32") => ("4c29858ecfa30d93e854d17494f422b593afa6da8ec80ae979191e69604dc955", "7928396c17aabcd5bb074fe82e07b4cb693c6d04360a4415799a143a9f9686c6"),
        ("fpm", "8.3.32") => ("f0d50fa3d8c5114a93cef02b44a1c01642018bc1dab96644d29ef2d1034eb624", "02964d89c54810471c35bbd3877e1a420693305cb2e33aab18636d0db29ff7bc"),
        ("cli", "8.4.23") => ("1faabbea9c500daab9ba202cb75ea9876191a2a8f902244e9d0464addb9b55b1", "5fa2b5f1cc9d7f79b19718926b4f4f0bb6949db52073b2fabf8ae52de3993af5"),
        ("fpm", "8.4.23") => ("8ca6b8d44f12f981cdd96b5aa31f6bf5373a89d201a4a49ed0feb5f242631dec", "40e341db82e5e90450122462aa6038fe653b63da951c1da1f10b15d0da4a26a0"),
        ("cli", "8.5.8") => ("86ea1fe2f6d2415eff78c2e0bcb479b221d5637c89d72e19809b65c4c0527c27", "bfaa12e0f6f5788259bc20d0d3f08e1e851bce7f05d43e38fb77041e12a32663"),
        ("fpm", "8.5.8") => ("4bd94d191f1f8f620a1ca76236f679d2e2491ecfa987036fc6d7a7c08e663f3b", "d2a5c031ae23a72b97ed5ab739bd5b8fe75dd09754d2f406a9216b7d7c8311d8"),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
}

/// Oracle's generic Linux tarballs (`linux-glibc2.28`, `.tar.xz`): `(aarch64, x86_64)`.
fn mysql_linux_sha256(version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match version {
        "8.4.6" => ("ab2c553ee4349a6bbe62325d4efcb427204e389264c4759ca32a92eae9aa2abc", "60ab7b0f63494d788a7267589c5073c9222f7a6a288306cc42cc6852df51729b"),
        "8.0.44" => ("1cc9217d0584a7e5e49469424160e5aaedb34a367f8801a0c26f1a0897ce2fe6", "be3b92f01e61555468528ada57aace433e3a4f1eab58569197304c328f4bfb6f"),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
}

/// theseus-rs's `unknown-linux-gnu` builds: `(aarch64, x86_64)`, each matching the
/// project's published `.sha256`.
fn postgres_linux_sha256(version: &str, arch: Arch) -> Option<&'static str> {
    let (arm, amd) = match version {
        "18.6.0" => ("5d2b5e8be9e96bfc3d7f22090057a836ec36d5d092542d8d3b93cc145cf4ee13", "bb3d09f876b2383e25a8c9ce09e32d185a03656a23d074f5195542b1b1b3ca61"),
        "17.11.0" => ("abffda09209280ec1502b73720dc4d254fb7fff9a072e324926c600a5b16c221", "b7a1ba6bae6499d8296e3e81b0171eecfd1766ca9aaa0057e41ad3e844e5e2e0"),
        "16.15.0" => ("f6d49ffbaea28cf8732785b18afb75bded905667d4ec8e2308a13a30fe0a24d1", "77dd669eda3985ea8be26256f6af28d3c8414152ada713797555bb7423b4486a"),
        _ => return None,
    };
    Some(match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    })
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
/// (`docs/archive/PLAN-php-74-support.md` §2/§6).
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

        // 8.1-8.5 are ALSO ours now, and for a reason that has nothing to do
        // with availability: static-php.dev's bulk builds ship `pgsql` and NO
        // `pdo_pgsql`, while their PDO advertises `pgsql` — so a PDO connection
        // is accepted and then stalls until PostgreSQL closes it. Laravel's
        // `pgsql` driver IS that call (ledger #545). These builds add the driver
        // and PROVE it in CI by connecting to a real cluster, and they hold
        // extension PARITY with the artifacts they replace as a build gate, per
        // minor, against a module list generated from the shipped bulk binary.
        //
        // **8.0.30 is deliberately NOT here.** It builds and passes every gate on
        // arm64 and aborts on x86_64 inside static-php-cli's own sanity check
        // (`php -n -r 'echo "hello";'` exits 6 with no output), reproducibly, with
        // three different extension subsets excluded in turn. Cause unknown, EOL
        // since Nov 2023, and half an architecture is not shippable — so it keeps
        // coming from static-php.dev, without a PDO PostgreSQL driver. That is
        // the whole reason `php::pdo_pgsql_supported` answers per MINOR.
        // One tag per version, and each is the build that produced exactly those
        // bytes. They were built as five SEPARATE runs rather than one matrix —
        // a shape change fails identically on every version, so a matrix spends
        // ten runner slots to learn one fact, and 8.1 did fail alone (it needed
        // swoole 6.1.7, where the others were fine).
        //
        // -1 and -2 are superseded and still published: those builds were missing
        // mbstring's regex half and gd's avif, which `php -m` could not see —
        // every Laravel `artisan` command died on `mb_split`. The releases here
        // are the first whose parity with upstream is checked by FUNCTION and by
        // CONFIGURE FLAG, not by module name.
        "8.1.34" => Some("php-8x-3"),
        "8.2.32" => Some("php-8x-4"),
        "8.3.32" => Some("php-8x-5"),
        "8.4.23" => Some("php-8x-6"),
        "8.5.8" => Some("php-8x-7"),

        _ => None,
    }
}

/// Whether the PHP artifact for this EXACT version has a working `pdo_pgsql`.
///
/// **Per PATCH, not per minor, and that distinction cost a hung provision.**
/// The record started as "8.1+ has the driver", which is true of the artifacts
/// rexenv BUILDS — and rexenv also runs patches it did not build: the signed
/// update manifest offers static-php.dev's newer patches for the same minor, so
/// a machine on 8.3 can be running 8.3.32 (upstream, no driver) while 8.3.31
/// (ours, has it) is what the minor was judged by. On 10 Sep 2026 that is
/// exactly what happened: the New-site dialog offered PostgreSQL for PHP 8.3, a
/// Laravel site was created, and `artisan migrate` sat at 99% CPU for four
/// minutes — the same missing driver as ledger #545, in its worst shape yet,
/// because this build BUSY-LOOPS rather than idling, so even the step's
/// no-output watchdog cannot fire.
///
/// So the question is asked of the artifact, here, beside the pins that decide
/// which artifact that is — `php_self_hosted_tag` says whose build a version is,
/// and only the 8.x releases we build carry the driver (our 7.4 does not).
pub fn php_has_pdo_pgsql(version: &str) -> bool {
    php_self_hosted_tag(version).is_some_and(|_| !version.starts_with("7."))
}

/// Test helper: whether a version is one rexenv builds. Exists so a test can
/// state "ours, and still without the driver" about 7.4 rather than implying it.
#[cfg(test)]
pub(crate) fn php_self_hosted_tag_is_some(version: &str) -> bool {
    php_self_hosted_tag(version).is_some()
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
/// was built before, which is the whole property `docs/archive/PLAN-binary-updates.md`
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
            checksum: Checksum::Sha256(ADMINER_6_1_0_SHA256.to_string()),
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
/// OpenSSL 3.6). `docs/archive/PLAN-php-74-support.md` §11 states the risk this const
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
// nginx's, from the same release as the binaries (build 2 — build 1 shipped
// without them, which rexenv's own resolve refused, and refusing was correct).
// nginx is BSD-2-Clause and PCRE2 BSD-3-Clause: both require the notice to
// travel with a redistribution, and we are the redistributor.
const NGINX_1_30_4_LICENSES_MAC_ARM64_SHA256: &str = "517f6656ae4bae3f1aa29e64ce379abbcd4f27166284fd1b984920a89300ffd2";
const NGINX_1_30_4_LICENSES_MAC_AMD64_SHA256: &str = "cbf546b81a7b02bd9e71e50f0ef1a1da549e527c7e528e346514b511bc1f9d2b";
const PHP_7_4_33_LICENSES_MAC_ARM64_SHA256: &str = "d8fd80a258f1d8e6609d3e0e95a3b62e5c30820a3dba8e0390c78e4494491478";
const PHP_7_4_33_LICENSES_MAC_AMD64_SHA256: &str = "fa1ae808cb2febdb01e2df2975b0618dba4c0caff39f51161328ee97c2b60ba2";
const PHP_8_1_34_LICENSES_MAC_ARM64_SHA256: &str = "1b5793dbc13fc3dbb2da66014b3308683db2ecf063cf5ae861a942b2c190e06a";
const PHP_8_1_34_LICENSES_MAC_AMD64_SHA256: &str = "d2145bd6e70f110edc82fa02c202cd9c5ac2944c10baa0cf86ac96efab988db6";
const PHP_8_2_32_LICENSES_MAC_ARM64_SHA256: &str = "3dc8c2545172bcf07accf4cd0a3df8ca36b47a94363da1e1b76c59b8537aa8f8";
const PHP_8_2_32_LICENSES_MAC_AMD64_SHA256: &str = "cd96a3db1d17334f4b5527701d0a5daf186797d235089e3ad6b1e561ea2c48a8";
const PHP_8_3_32_LICENSES_MAC_ARM64_SHA256: &str = "c581ead815d3f4431975757f71e39292fcf11743fcfa159e722ed6c48ecda411";
const PHP_8_3_32_LICENSES_MAC_AMD64_SHA256: &str = "5daeadb3b0a0e84122b3bca54a38184727dd2875af8dec65006776390ca88542";
const PHP_8_4_23_LICENSES_MAC_ARM64_SHA256: &str = "e209273668ad40482b33e7d63b6ffed52a4caff0084d7c1a0d49af0afe61f287";
const PHP_8_4_23_LICENSES_MAC_AMD64_SHA256: &str = "c402d2f07b71db269271bccd0ad393d5add8071ca54a70f05764ac4a0b6cb1a0";
const PHP_8_5_8_LICENSES_MAC_ARM64_SHA256: &str = "d9865d73d2dc46f9597d7e05bb5e4612f5127b8dd21596f8c359c5a1d1eddbd7";
const PHP_8_5_8_LICENSES_MAC_AMD64_SHA256: &str = "b94fa2d4f09470782bfad454db57582db8c2e6fa274f772e3d3332248efd2303";

/// Directory inside a published cache dir holding the artifact's licence texts.
/// Matches the tarball's own top-level dir, so extraction is `strip = 0`.
pub const LICENSES_DIR: &str = "licenses";

/// The manifest artifact NAME carrying those texts for a PHP version rexenv
/// serves. One constant, because the publisher in `rexenv/runtimes` writes this
/// exact string and a typo either side is an update that resolves its
/// interpreter and then refuses on licences.
pub const LICENSES_ARTIFACT: &str = "php-licenses";

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

/// **Does `name`@`version` ship on `os` at all?** — the one question a feature gate asks.
///
/// Reads the same two disjoint tables a resolve would ([`manifest`] and [`bundle_manifest`]),
/// so D4's "what is in Windows v1" list is not a SECOND copy of the pins that could drift from
/// them: it IS the pins. Redis has no official Windows build, Apache on Windows would mean a
/// third-party trust decision, and MariaDB has no Windows pin — so none of them has a Windows
/// arm, and this returns false there without anyone maintaining a list (W10, ledger #642).
///
/// Arch-independent by construction: both tables match on `(name, os, version)` and differ only
/// in what each arm BUILDS from the arch, so either answers the same. Pinned by a test rather
/// than trusted, because a future arm could match on arch and quietly make this half-true.
pub fn ships_on(name: &str, version: &str, os: &str) -> bool {
    manifest(name, version, os, Arch::X86_64).is_some()
        || bundle_manifest(name, version, os, Arch::X86_64).is_some()
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
    // Keyed on (name, version): two different artifacts are now self-built, and
    // keying on the version alone would have made "1.30.4" answer for whatever
    // else ever carries that number.
    // …and the FILE NAME travels with the digests. A release carrying ONE
    // artifact can call its licences `licenses-<arch>.tar.gz`; `php-8x-1` holds
    // five versions, so theirs are `licenses-php-<version>-<arch>.tar.gz`. That
    // is a property of each release, not a convention — deriving one name for
    // all of them would 404 half the tree, and only on a machine that had not
    // cached PHP yet.
    let (arm, amd, file) = match (name, version) {
        ("php", "7.4.33") | ("php-fpm", "7.4.33") => (
            PHP_7_4_33_LICENSES_MAC_ARM64_SHA256,
            PHP_7_4_33_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-{}.tar.gz", php_arch(arch)),
        ),
        ("php", "8.1.34") | ("php-fpm", "8.1.34") => (
            PHP_8_1_34_LICENSES_MAC_ARM64_SHA256,
            PHP_8_1_34_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-php-8.1.34-{}.tar.gz", php_arch(arch)),
        ),
        ("php", "8.2.32") | ("php-fpm", "8.2.32") => (
            PHP_8_2_32_LICENSES_MAC_ARM64_SHA256,
            PHP_8_2_32_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-php-8.2.32-{}.tar.gz", php_arch(arch)),
        ),
        ("php", "8.3.32") | ("php-fpm", "8.3.32") => (
            PHP_8_3_32_LICENSES_MAC_ARM64_SHA256,
            PHP_8_3_32_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-php-8.3.32-{}.tar.gz", php_arch(arch)),
        ),
        ("php", "8.4.23") | ("php-fpm", "8.4.23") => (
            PHP_8_4_23_LICENSES_MAC_ARM64_SHA256,
            PHP_8_4_23_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-php-8.4.23-{}.tar.gz", php_arch(arch)),
        ),
        ("php", "8.5.8") | ("php-fpm", "8.5.8") => (
            PHP_8_5_8_LICENSES_MAC_ARM64_SHA256,
            PHP_8_5_8_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-php-8.5.8-{}.tar.gz", php_arch(arch)),
        ),
        ("nginx", "1.30.4") => (
            NGINX_1_30_4_LICENSES_MAC_ARM64_SHA256,
            NGINX_1_30_4_LICENSES_MAC_AMD64_SHA256,
            format!("licenses-{}.tar.gz", php_arch(arch)),
        ),
        _ => ("", "", String::new()),
    };
    let hex = match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    };
    let missing = || {
        Error::Other(format!(
            "{name} {version} is served from rexenv's own infrastructure ({url}) — rexenv is \
             its distributor, and every licence in this tree's self-built artifacts requires \
             the notice to travel with the bytes (PHP License 3.01 §2; nginx BSD-2-Clause; \
             PCRE2 BSD-3-Clause) — but no licence archive is pinned for it. Pin \
             `licenses-{}.tar.gz` from the same release, or serve the artifact from whoever \
             built it.",
            php_arch(arch)
        ))
    };
    if hex.is_empty() {
        // Not pinned — ask the verified catalog, the same precedence `php_spec`
        // uses and for the same reason. The manifest now carries rexenv's OWN
        // builds for the versions it publishes (upstream's have no `pdo_pgsql`),
        // and a version that arrives that way has no compiled-in licence digest
        // by construction: it did not exist when this binary was built. Without
        // this arm the update would resolve the interpreter and then hard-error
        // on its licences — an offer that cannot be installed.
        //
        // The refusal below still stands when the catalog has nothing either:
        // an artifact rexenv serves without its licence texts is a licence
        // violation, and "we could not find them" is not a reason to ship it.
        let arch_s = crate::core::updates::catalog_arch(arch);
        if crate::core::updates::nameable(LICENSES_ARTIFACT) {
            if let Some(a) = CATALOG
                .read()
                .ok()
                .and_then(|g| g.as_ref()?.artifact(LICENSES_ARTIFACT, version, arch_s).cloned())
            {
                return Ok(Some(BinarySpec {
                    url: a.url,
                    checksum: Checksum::Sha256(a.sha256),
                    archive: Archive::TarGzTree,
                    member: LICENSES_DIR,
                }));
            }
        }
        return Err(missing());
    }
    Ok(Some(BinarySpec {
        url: sibling_url(url, &file).ok_or_else(missing)?,
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
        ("cli", "8.2.32") => (PHP_8_2_32_CLI_MAC_ARM64_SHA256, PHP_8_2_32_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.2.32") => (PHP_8_2_32_FPM_MAC_ARM64_SHA256, PHP_8_2_32_FPM_MAC_AMD64_SHA256),
        ("cli", "8.3.32") => (PHP_8_3_32_CLI_MAC_ARM64_SHA256, PHP_8_3_32_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.3.32") => (PHP_8_3_32_FPM_MAC_ARM64_SHA256, PHP_8_3_32_FPM_MAC_AMD64_SHA256),
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
        ("nginx", "macos", "1.30.4") => Some(BinarySpec {
            // Our own build: a single binary per arch (not an archive), the same
            // shape the previous third-party pin had. Immutable release tag, so
            // this URL can 404 but can never resolve to different bytes.
            url: format!(
                "https://github.com/rexenv/runtimes/releases/download/nginx-{version}-2/nginx-{version}-macos-{}",
                nginx_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                NGINX_1_30_4_MAC_ARM64_SHA256,
                NGINX_1_30_4_MAC_AMD64_SHA256,
            )),
            archive: Archive::Raw,
            member: "nginx",
        }),
        // MySQL is version-driven like PHP: any version pinned in `mysql_sha256`
        // resolves (the CDN URL is templated per series).
        ("mysql", "macos", v) if mysql_sha256(v, arch).is_some() => Some(BinarySpec {
            // Direct CDN URL (the dev.mysql.com/get redirector 403s non-curl clients).
            url: format!(
                "https://cdn.mysql.com/archives/mysql-{series}/mysql-{v}-{build}-{}.tar.gz",
                mysql_arch(arch),
                series = mysql_series(v),
                build = mysql_macos_build(v),
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
        ("cloudflared", "macos", v @ ("2026.6.1" | "2025.4.0")) => Some(BinarySpec {
            // One static Go binary per arch, inside a .tgz (member `cloudflared`).
            url: format!(
                "https://github.com/cloudflare/cloudflared/releases/download/{version}/cloudflared-darwin-{}.tgz",
                cloudflared_arch(arch)
            ),
            checksum: Checksum::Sha256(match v {
                "2026.6.1" => pick(arch, CLOUDFLARED_2026_6_1_MAC_ARM64_SHA256, CLOUDFLARED_2026_6_1_MAC_AMD64_SHA256),
                // Legacy tiers — see LEGACY_CLOUDFLARED_VERSION.
                _ => pick(arch, CLOUDFLARED_2025_4_0_MAC_ARM64_SHA256, CLOUDFLARED_2025_4_0_MAC_AMD64_SHA256),
            }),
            archive: Archive::TarGz,
            member: "cloudflared",
        }),
        // ── Windows (x64 for every `Arch` — see the Windows pins above) ─────────
        ("caddy", "windows", "2.11.4") => Some(BinarySpec {
            url: format!(
                "https://github.com/caddyserver/caddy/releases/download/v{version}/caddy_{version}_windows_amd64.zip"
            ),
            checksum: Checksum::Sha512(CADDY_2_11_4_WIN_AMD64_SHA512.to_string()),
            archive: Archive::Zip,
            member: "caddy.exe",
        }),
        // A TREE on Windows: `php.exe` and `php-cgi.exe` beside `ext/*.dll`. There is
        // no `php-fpm` on Windows at all — the pool is php-cgi (plan D1).
        ("php", "windows", v) => php_windows_sha256(v).map(|hex| BinarySpec {
            url: format!(
                "https://downloads.php.net/~windows/releases/archives/php-{v}-nts-Win32-{}-x64.zip",
                php_windows_toolset(v)
            ),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::ZipTree { strip: 0 },
            member: "php.exe",
        }),
        // nginx.org's own Windows build — a tree (`conf/`, `html/`, `logs/` beside the
        // exe), unlike the single binary rexenv builds for macOS.
        ("nginx", "windows", "1.30.4") => Some(BinarySpec {
            url: format!("https://nginx.org/download/nginx-{version}.zip"),
            checksum: Checksum::Sha256(NGINX_1_30_4_WIN_AMD64_SHA256.to_string()),
            archive: Archive::ZipTree { strip: 1 },
            member: "nginx.exe",
        }),
        ("mysql", "windows", v) => mysql_windows_sha256(v).map(|hex| BinarySpec {
            url: format!(
                "https://cdn.mysql.com/archives/mysql-{series}/mysql-{v}-winx64.zip",
                series = mysql_series(v),
            ),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::ZipTree { strip: 1 },
            member: "bin/mysqld.exe",
        }),
        ("postgres", "windows", v) => postgres_windows_sha256(v).map(|hex| BinarySpec {
            url: format!(
                "https://github.com/theseus-rs/postgresql-binaries/releases/download/{v}/postgresql-{v}-x86_64-pc-windows-msvc.tar.gz"
            ),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarGzTree,
            member: "bin/postgres.exe",
        }),
        ("mailpit", "windows", "1.30.3") => Some(BinarySpec {
            url: format!(
                "https://github.com/axllent/mailpit/releases/download/v{version}/mailpit-windows-amd64.zip"
            ),
            checksum: Checksum::Sha256(MAILPIT_1_30_3_WIN_AMD64_SHA256.to_string()),
            archive: Archive::Zip,
            member: "mailpit.exe",
        }),
        ("cloudflared", "windows", "2026.6.1") => Some(BinarySpec {
            url: format!(
                "https://github.com/cloudflare/cloudflared/releases/download/{version}/cloudflared-windows-amd64.exe"
            ),
            checksum: Checksum::Sha256(CLOUDFLARED_2026_6_1_WIN_AMD64_SHA256.to_string()),
            archive: Archive::Raw,
            member: "cloudflared.exe",
        }),
        // ── Linux (docs/PLAN-linux-port.md L2; both archs, see the Linux pins above) ──
        ("caddy", "linux", "2.11.4") => Some(BinarySpec {
            url: format!(
                "https://github.com/caddyserver/caddy/releases/download/v{version}/caddy_{version}_linux_{}.tar.gz",
                caddy_arch(arch)
            ),
            checksum: Checksum::Sha512(pick(arch, CADDY_2_11_4_LINUX_ARM64_SHA512, CADDY_2_11_4_LINUX_AMD64_SHA512)),
            archive: Archive::TarGz,
            member: "caddy",
        }),
        // static-php.dev's Linux builds — the same publisher and the same tarball shape as
        // its macOS ones (a single static `php` / `php-fpm`), so the pool model is macOS's.
        ("php", "linux", v) => php_linux_sha256("cli", v, arch).map(|hex| BinarySpec {
            url: format!("https://dl.static-php.dev/static-php-cli/bulk/php-{v}-cli-linux-{}.tar.gz", linux_arch(arch)),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarGz,
            member: "php",
        }),
        ("php-fpm", "linux", v) => php_linux_sha256("fpm", v, arch).map(|hex| BinarySpec {
            url: format!("https://dl.static-php.dev/static-php-cli/bulk/php-{v}-fpm-linux-{}.tar.gz", linux_arch(arch)),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarGz,
            member: "php-fpm",
        }),
        // jirutka's static build (raw binary), the interim until rexenv/runtimes has a Linux nginx.
        ("nginx", "linux", "1.30.4") => Some(BinarySpec {
            url: format!("https://jirutka.github.io/nginx-binaries/nginx-{version}-{}-linux", linux_arch(arch)),
            checksum: Checksum::Sha256(pick(arch, NGINX_1_30_4_LINUX_AARCH64_SHA256, NGINX_1_30_4_LINUX_X86_64_SHA256)),
            archive: Archive::Raw,
            member: "nginx",
        }),
        ("mysql", "linux", v) => mysql_linux_sha256(v, arch).map(|hex| BinarySpec {
            url: format!(
                "https://cdn.mysql.com/archives/mysql-{series}/mysql-{v}-linux-glibc2.28-{}.tar.xz",
                linux_arch(arch),
                series = mysql_series(v),
            ),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarXzTree,
            member: "bin/mysqld",
        }),
        ("postgres", "linux", v) => postgres_linux_sha256(v, arch).map(|hex| BinarySpec {
            url: format!(
                "https://github.com/theseus-rs/postgresql-binaries/releases/download/{v}/postgresql-{v}-{}-unknown-linux-gnu.tar.gz",
                linux_arch(arch)
            ),
            checksum: Checksum::Sha256(hex.to_string()),
            archive: Archive::TarGzTree,
            member: "bin/postgres",
        }),
        ("frankenphp", "linux", "1.12.4") => Some(BinarySpec {
            url: format!("https://github.com/php/frankenphp/releases/download/v{version}/frankenphp-linux-{}", linux_arch(arch)),
            checksum: Checksum::Sha256(pick(arch, FRANKENPHP_1_12_4_LINUX_AARCH64_SHA256, FRANKENPHP_1_12_4_LINUX_X86_64_SHA256)),
            archive: Archive::Raw,
            member: "frankenphp",
        }),
        ("mailpit", "linux", "1.30.3") => Some(BinarySpec {
            url: format!("https://github.com/axllent/mailpit/releases/download/v{version}/mailpit-linux-{}.tar.gz", mailpit_arch(arch)),
            checksum: Checksum::Sha256(pick(arch, MAILPIT_1_30_3_LINUX_ARM64_SHA256, MAILPIT_1_30_3_LINUX_AMD64_SHA256)),
            archive: Archive::TarGz,
            member: "mailpit",
        }),
        ("cloudflared", "linux", "2026.6.1") => Some(BinarySpec {
            url: format!("https://github.com/cloudflare/cloudflared/releases/download/{version}/cloudflared-linux-{}", cloudflared_arch(arch)),
            checksum: Checksum::Sha256(pick(arch, CLOUDFLARED_2026_6_1_LINUX_ARM64_SHA256, CLOUDFLARED_2026_6_1_LINUX_AMD64_SHA256)),
            archive: Archive::Raw,
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
    openssl_part_from(arch, OPENSSL_3_6_3_BOTTLE_ARM64_SHA256, OPENSSL_3_6_3_BOTTLE_AMD64_SHA256)
}

/// [`openssl_part`] for the Legacy13 bundles: the 3.5.2 ventura blobs.
fn legacy13_openssl_part(arch: Arch) -> BundlePart {
    openssl_part_from(arch, OPENSSL_3_5_2_BOTTLE_ARM64_SHA256, OPENSSL_3_5_2_BOTTLE_AMD64_SHA256)
}

fn openssl_part_from(arch: Arch, arm_sha: &'static str, amd_sha: &'static str) -> BundlePart {
    bottle_part("openssl@3", arch, arm_sha, amd_sha, &["lib/libssl.3.dylib", "lib/libcrypto.3.dylib"])
}

/// The pcre2 runtime dylib part (mariadb + httpd link `libpcre2-8`).
fn pcre2_part(arch: Arch, arm_sha: &'static str, amd_sha: &'static str) -> BundlePart {
    bottle_part("pcre2", arch, arm_sha, amd_sha, &["lib/libpcre2-8.0.dylib"])
}

/// The mariadb bottle's include list — server, the two clients the DB features
/// need, the bootstrap SQL `core::mariadb::initialize` feeds over stdin, and the
/// runtime share data. Identical across 11.4 and 12.x (verified from the bottles).
const MARIADB_INCLUDE: &[&str] = &[
    "bin/mariadbd",
    "bin/mariadb",
    "bin/mariadb-dump",
    "share/mysql/english",
    "share/mysql/charsets",
    "share/mysql/mariadb_system_tables.sql",
    "share/mysql/mariadb_performance_tables.sql",
    "share/mysql/mariadb_system_tables_data.sql",
];

/// The httpd bottle's include list: the server binary, ONLY the modules our
/// generated conf loads (mod_ssl/mod_http2/mod_brotli would drag openssl/nghttp2/
/// brotli into the closure), and the real mime map (bottles stage etc/ under
/// `.bottle/`).
const HTTPD_INCLUDE: &[&str] = &[
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
];

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
        // ── Legacy13 bundles (ventura blobs) — same layouts, older tags ─────────
        ("redis", "macos", "8.2.1") => Some(BundleSpec {
            member: "bin/redis-server",
            parts: vec![
                bottle_part("redis", arch, REDIS_8_2_1_BOTTLE_ARM64_SHA256, REDIS_8_2_1_BOTTLE_AMD64_SHA256, &["bin"]),
                legacy13_openssl_part(arch),
            ],
        }),
        ("httpd", "macos", "2.4.65") => Some(BundleSpec {
            member: "bin/httpd",
            parts: vec![
                bottle_part("httpd", arch, HTTPD_2_4_65_BOTTLE_ARM64_SHA256, HTTPD_2_4_65_BOTTLE_AMD64_SHA256, HTTPD_INCLUDE),
                bottle_part("apr", arch, APR_1_7_6_VENTURA_BOTTLE_ARM64_SHA256, APR_1_7_6_VENTURA_BOTTLE_AMD64_SHA256, &["lib/libapr-1.0.dylib"]),
                bottle_part("apr-util", arch, APR_UTIL_1_6_3_VENTURA_BOTTLE_ARM64_SHA256, APR_UTIL_1_6_3_VENTURA_BOTTLE_AMD64_SHA256, &["lib/libaprutil-1.0.dylib"]),
                pcre2_part(arch, PCRE2_10_46_BOTTLE_ARM64_SHA256, PCRE2_10_46_BOTTLE_AMD64_SHA256),
            ],
        }),
        ("mariadb", "macos", "11.4.8") => Some(BundleSpec {
            member: "bin/mariadbd",
            parts: vec![
                bottle_part("mariadb@11.4", arch, MARIADB_11_4_8_BOTTLE_ARM64_SHA256, MARIADB_11_4_8_BOTTLE_AMD64_SHA256, MARIADB_INCLUDE),
                legacy13_openssl_part(arch),
                pcre2_part(arch, PCRE2_10_46_BOTTLE_ARM64_SHA256, PCRE2_10_46_BOTTLE_AMD64_SHA256),
            ],
        }),
        ("mariadb", "macos", "12.0.2") => Some(BundleSpec {
            member: "bin/mariadbd",
            parts: vec![
                bottle_part("mariadb", arch, MARIADB_12_0_2_BOTTLE_ARM64_SHA256, MARIADB_12_0_2_BOTTLE_AMD64_SHA256, MARIADB_INCLUDE),
                legacy13_openssl_part(arch),
                pcre2_part(arch, PCRE2_10_46_BOTTLE_ARM64_SHA256, PCRE2_10_46_BOTTLE_AMD64_SHA256),
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
            // The ROW, never `xdebug_bottle_on`: that one applies the os policy, which asks
            // this table back (see `xdebug_status_on`).
            // Cache-dir identity is honest: only a (minor, version) the table
            // holds resolves — never one app-wide constant, and never the
            // tier's pick (a standard host must be able to sweep the legacy
            // rows). See [`XdebugBottle::version`] and [`xdebug_row_at`].
            let bottle = xdebug_row_at(n.strip_prefix("xdebug-")?, v)?;
            Some(xdebug_spec(&bottle, arch))
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

/// The distribution shape of `name` on THIS machine — see [`Shape`].
pub fn shape_of(name: &str) -> Shape {
    shape_of_on(name, std::env::consts::OS)
}

/// The distribution shape of `name` on `os`. It depends on the OS because one
/// name can be a different KIND of artifact per OS: macOS gets a single static
/// `php` and a single `nginx`, while their Windows builds are zip TREES (`php.exe`
/// beside `ext/*.dll`, `nginx.exe` beside `conf/`). A host-only answer would send a
/// Windows resolve looking for `dir/php` inside a tree (docs/PLAN-windows-port.md W2).
pub fn shape_of_on(name: &str, os: &str) -> Shape {
    match name {
        "mysql" | "postgres" => Shape::Dir,
        "php" | "nginx" if os == "windows" => Shape::Dir,
        "redis" | "mariadb" | "httpd" => Shape::Bundle,
        n if n.starts_with("xdebug-") => Shape::Bundle,
        "wp-cli" | "adminer" | "composer" => Shape::File,
        _ => Shape::Single,
    }
}

/// The file a single executable is published as: `name` itself, and `name.exe` on
/// Windows, which will not start a file that lacks the extension.
pub(crate) fn exe_name(name: &str, os: &str) -> String {
    if os == "windows" {
        format!("{name}.exe")
    } else {
        name.to_string()
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
    Some(match shape_of_on(name, os) {
        Shape::Bundle => dir.join(bundle_manifest(name, version, os, arch)?.member),
        Shape::Single => dir.join(exe_name(name, os)),
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

    match shape_of_on(name, os) {
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
                Shape::Single => dir.join(exe_name(name, os)),
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

/// The EXECUTABLE a program runs from, whatever shape it ships in on this OS: the
/// single binary itself, or the manifest's member inside a directory distribution.
/// For a program that is a single binary on one OS and a tree on another (nginx: a
/// static binary on macOS, `nginx.exe` beside `conf\` in the Windows zip) — a caller
/// that wants "the thing to spawn" must not have to know which.
pub async fn resolve_program(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    match shape_of(name) {
        Shape::Single => resolve(platform, name, version).await,
        Shape::Dir => {
            let dir = resolve_dir(platform, name, version).await?;
            program_member(platform, name, version).map(|m| dir.join(m)).ok_or_else(|| {
                Error::Other(format!("no binary manifest for {name} {version} on this platform"))
            })
        }
        other => Err(Error::Other(format!("{name} is a {other:?} distribution, not a program to run"))),
    }
}

/// [`resolve_program`] without the download: the cached executable, or `None`.
pub fn cached_program(platform: &dyn Platform, name: &str, version: &str) -> Option<PathBuf> {
    match shape_of(name) {
        Shape::Single => cached_path(platform, name, version),
        Shape::Dir => {
            let dir = cached_path(platform, name, version)?;
            Some(dir.join(program_member(platform, name, version)?))
        }
        _ => None,
    }
}

fn program_member(platform: &dyn Platform, name: &str, version: &str) -> Option<&'static str> {
    manifest(name, version, std::env::consts::OS, platform.binaries().arch()).map(|spec| spec.member)
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
///   8 Aug draft of `docs/archive/PLAN-binary-updates.md` §6 promised "the new tree is
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
    let bin_path = dir.join(exe_name(name, os));

    // The manifest is consulted BEFORE the cache check, because "is this cached"
    // now means "are these the bytes we pin" — not merely "is a file there".
    let spec = manifest(name, version, os, arch).ok_or_else(|| {
        Error::Other(format!(
            "no binary manifest for {name} {version} on {os}/{}",
            php_arch(arch)
        ))
    })?;
    if matches!(spec.archive, Archive::TarGzTree | Archive::TarXzTree | Archive::ZipTree { .. }) {
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
    let staged_bin = staging.join(exe_name(name, os));
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
            Archive::Zip => {
                let archive = staging.join(".archive.zip");
                download(&spec.url, &archive, Some(&spec.checksum), Some(&id)).await?;
                downloads::hub().item_preparing(&id);
                extract_zip_member(&archive, spec.member, &staged_bin)?;
                std::fs::remove_file(&archive)?;
            }
            Archive::TarGzTree | Archive::TarXzTree | Archive::ZipTree { .. } => unreachable!("trees returned above"),
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
        publish(&staging, &dir, &exe_name(name, os))
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
/// Resolve a directory distribution — and, for the PHP a php-cgi platform serves, make sure
/// the tree carries its CLI `php.ini` (`php_cgi::ensure_cli_ini`, ledger #603). Here, at the
/// one resolve every PHP tree passes through (the pool, the CLI via `resolve_program`, the
/// settings gate), rather than at each caller.
pub async fn resolve_dir(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let dir = resolve_dir_tree(platform, name, version).await?;
    super::php_cgi::ensure_cli_ini(platform, name, &dir)?;
    Ok(dir)
}

async fn resolve_dir_tree(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
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

    if !matches!(spec.archive, Archive::TarGzTree | Archive::TarXzTree | Archive::ZipTree { .. }) {
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
    // attribute, so there's no ad-hoc re-signing step here — `prepare_binary_dir`
    // is a no-op on macOS. On Windows it checks the tree can run before it is
    // published (owner ruling 13 Sep 2026, ledger #598).
    let id = downloads::item_id(name, version);
    downloads::hub().item_started(name, version);
    let staging = staging_path(&bin_dir, name, version);
    let staged: Result<()> = async {
        std::fs::create_dir_all(&staging)?;
        // A tar tree strips its one top-level dir; a zip tree strips what its
        // manifest arm says (a PHP zip is flat, an nginx zip is not).
        let archive = match spec.archive {
            Archive::ZipTree { .. } => staging.join(".archive.zip"),
            Archive::TarXzTree => staging.join(".archive.tar.xz"),
            _ => staging.join(".archive.tar.gz"),
        };
        download(&spec.url, &archive, Some(&spec.checksum), Some(&id)).await?;
        downloads::hub().item_preparing(&id);
        match spec.archive {
            Archive::ZipTree { strip } => extract_zip_tree(&archive, &staging, strip)?,
            Archive::TarXzTree => extract_tar_xz_tree(open_buffered(&archive)?, &staging)?,
            _ => extract_tar_gz_tree(open_buffered(&archive)?, &staging)?,
        }
        // Drop the archive BEFORE publishing so the cached tree doesn't carry a
        // dead 600MB archive into the final dir.
        std::fs::remove_file(&archive)?;
        // Inside the staged block, so a tree that fails it is removed with the
        // staging dir and never becomes the cached tree.
        platform.binaries().prepare_binary_dir(&staging)?;
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
pub(crate) async fn download(
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
pub(crate) fn extract_tar_gz_tree(reader: impl std::io::Read, dest: &Path) -> Result<()> {
    extract_tar_gz_tree_filtered(reader, dest, 1, None)
}

/// [`extract_tar_gz_tree`] for an xz-compressed tar (MySQL's Linux tarballs). The decoder
/// is the platform's: only the Linux build links liblzma, and every other OS answers
/// `Unsupported` here rather than carrying a C library for an archive it never fetches.
pub(crate) fn extract_tar_xz_tree(reader: impl std::io::Read + Send + 'static, dest: &Path) -> Result<()> {
    let decoded = crate::platform::xz_decoder(Box::new(reader))?;
    extract_tar_tree_filtered(decoded, dest, 1, None)
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
    extract_tar_tree_filtered(flate2::read::GzDecoder::new(reader), dest, strip, include)
}

/// The tar half of [`extract_tar_gz_tree_filtered`], over an already-decompressed stream —
/// one extractor for gzip and xz, so the escape guards below are written once.
fn extract_tar_tree_filtered(
    reader: impl std::io::Read,
    dest: &Path,
    strip: usize,
    include: Option<&[&str]>,
) -> Result<()> {
    use std::path::PathBuf;
    use tar::Archive as TarArchive;

    let mut archive = TarArchive::new(reader);
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

/// Open a downloaded zip for reading.
fn open_zip(archive: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(zip_err)
}

fn zip_err(e: zip::result::ZipError) -> Error {
    Error::Other(format!("zip archive: {e}"))
}

/// Extract the ONE file named `member` from a zip into `dest` — matched by file
/// NAME, as [`extract_tar_gz_member`] matches, because the Windows single-binary
/// zips (Caddy, Mailpit) hold the executable beside a README and a LICENSE.
fn extract_zip_member(archive: &Path, member: &str, dest: &Path) -> Result<()> {
    let mut zip = open_zip(archive)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_err)?;
        if entry.is_dir() {
            continue;
        }
        let is_member = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n == member))
            .unwrap_or(false);
        if is_member {
            if entry.is_symlink() {
                return Err(Error::Other(format!(
                    "refusing a symlink in a zip archive: {}",
                    entry.name()
                )));
            }
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(Error::Other(format!("member '{member}' not found in archive")))
}

/// Extract a whole zip tree into `dest`, dropping `strip` leading path components
/// (`nginx-1.30.4/nginx.exe` with strip 1 → `<dest>/nginx.exe`; PHP's Windows zip
/// is flat, strip 0).
///
/// The tar path's guards, applied to a zip's own hazards (docs/PLAN-windows-port.md
/// W2). An entry whose name is not ENCLOSED — `..` that climbs out, an absolute
/// path, a drive prefix — is refused, not skipped: a pinned archive that carries
/// one is not the archive we pinned. `safe_join` then checks the post-strip path
/// again. And a symlink entry is refused outright: no artifact rexenv pins from a
/// zip ships one, and a link is how a later entry gets written outside `dest`.
/// File modes are not carried over — the zips pinned here are Windows builds,
/// where there is no executable bit to lose.
pub(crate) fn extract_zip_tree(archive: &Path, dest: &Path, strip: usize) -> Result<()> {
    let mut zip = open_zip(archive)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_err)?;
        let name = entry.name().to_string();
        // Checked on the RAW name, by this host's path rules — not on
        // `enclosed_name()` alone, which may hand back a sanitized path that stays
        // inside `dest` for a name that tried to climb out. Nothing would escape
        // then, but nothing would be refused either, and a pinned archive carrying
        // such a name is not the archive we pinned. Checked BEFORE the strip, so a
        // stripped component can never be the `/` or `..` that made a name unsafe.
        let raw = Path::new(&name);
        if entry.enclosed_name().is_none() || safe_join(dest, raw).is_err() {
            return Err(Error::Other(format!("unsafe path in archive: {name}")));
        }
        if entry.is_symlink() {
            return Err(Error::Other(format!("refusing a symlink in a zip archive: {name}")));
        }
        let rel: PathBuf = raw.components().skip(strip).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = safe_join(dest, &rel)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut file)?;
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
mod tier_tests {
    use super::{
        engine_needs_macos, install_tier, php_minor_needs_macos, pins, refused_php_minors, tier,
        BinaryTier, PinSet, PHP_VERSION, PHP_VERSIONS,
    };

    /// §6.3 — a refusal names the LOWEST tier that offers the thing, derived from
    /// the pin sets; a feature offered here, or nowhere, is not a tier refusal.
    #[test]
    fn a_refusal_names_the_oldest_macos_that_offers_the_thing() {
        install_tier(BinaryTier::Legacy13);
        assert_eq!(php_minor_needs_macos("8.0"), Some(14), "8.0.30 is 14.0 on arm64");
        assert_eq!(php_minor_needs_macos("8.3"), None, "offered here");
        assert_eq!(php_minor_needs_macos("7.4"), None, "offered here");
        assert_eq!(php_minor_needs_macos("9.9"), None, "shipped nowhere — not a tier refusal");
        assert_eq!(engine_needs_macos("postgres"), Some(14), "16.4.0 is the Legacy14 pin");
        assert_eq!(engine_needs_macos("mysql"), None);
        assert_eq!(engine_needs_macos("mariadb"), None);
        assert_eq!(engine_needs_macos("redis"), None);
        assert_eq!(engine_needs_macos("nosuch"), None);
        assert_eq!(refused_php_minors(), vec![("8.0".to_string(), 14)]);
        // Plant: with `needs_macos_for` returning the HIGHEST tier instead of the
        // lowest, PostgreSQL read `Some(15)` here and this line failed.
        install_tier(BinaryTier::Legacy14);
        assert_eq!(php_minor_needs_macos("8.0"), None);
        assert_eq!(engine_needs_macos("postgres"), None);
        assert!(refused_php_minors().is_empty());
        install_tier(BinaryTier::Standard);
        assert_eq!(php_minor_needs_macos("8.0"), None);
        assert_eq!(engine_needs_macos("postgres"), None);
        assert!(refused_php_minors().is_empty());
    }

    #[test]
    fn below_13_is_not_a_tier() {
        // The installer refuses these hosts; a tier for them would be a promise
        // nothing measured. 12.7.6 is the last Monterey.
        assert_eq!(BinaryTier::for_host((12, 7, 6)), None);
        assert_eq!(BinaryTier::for_host((11, 0, 0)), None);
        assert_eq!(BinaryTier::for_host((0, 0, 0)), None);
    }

    #[test]
    fn each_major_maps_to_its_tier_regardless_of_patch() {
        assert_eq!(BinaryTier::for_host((13, 0, 0)), Some(BinaryTier::Legacy13));
        assert_eq!(BinaryTier::for_host((13, 7, 8)), Some(BinaryTier::Legacy13));
        assert_eq!(BinaryTier::for_host((14, 0, 0)), Some(BinaryTier::Legacy14));
        assert_eq!(BinaryTier::for_host((14, 8, 2)), Some(BinaryTier::Legacy14));
        assert_eq!(BinaryTier::for_host((15, 0, 0)), Some(BinaryTier::Standard));
        assert_eq!(BinaryTier::for_host((15, 6, 1)), Some(BinaryTier::Standard));
        // The version scheme jumped 15 → 26; anything newer is still Standard.
        assert_eq!(BinaryTier::for_host((26, 4, 0)), Some(BinaryTier::Standard));
        assert_eq!(BinaryTier::for_host((99, 0, 0)), Some(BinaryTier::Standard));
    }

    #[test]
    fn a_tiers_floor_is_the_major_it_is_named_for() {
        assert_eq!(BinaryTier::Legacy13.floor(), (13, 0, 0));
        assert_eq!(BinaryTier::Legacy14.floor(), (14, 0, 0));
        assert_eq!(BinaryTier::Standard.floor(), (15, 0, 0));
        // A tier's own floor maps back to that tier — the two tables agree.
        for t in [BinaryTier::Legacy13, BinaryTier::Legacy14, BinaryTier::Standard] {
            assert_eq!(BinaryTier::for_host(t.floor()), Some(t));
        }
    }

    #[test]
    fn pins_follow_the_installed_tier() {
        // Every tier answers the Standard set until T2 — but through the tier,
        // not around it. Plant: with `pins()` returning `STANDARD_PINS` directly
        // this still passes today, so the load-bearing half is `for_tier` being
        // the one match arm T2 edits; the test pins the SHAPE (a tier in, a set
        // out) and T2's tests pin the values.
        install_tier(BinaryTier::Legacy13);
        assert_eq!(pins(), PinSet::for_tier(BinaryTier::Legacy13));
        install_tier(BinaryTier::Standard);
        assert_eq!(pins(), PinSet::for_tier(BinaryTier::Standard));
        assert_eq!(pins().php, PHP_VERSION);
        assert_eq!(pins().php_versions, PHP_VERSIONS);
    }

    /// §6.2 row for row: every legacy pin resolves on BOTH slices, its digests
    /// differ per slice, and a legacy digest is never a standard one — a row
    /// that reused the sonoma blob under a ventura version would pass the
    /// resolve and hand a 13 host the very bytes the tier exists to avoid.
    #[test]
    fn legacy_pins_resolve_on_both_slices_with_their_own_bytes() {
        use super::{bundle_manifest, manifest, Checksum};
        use crate::platform::traits::Arch;
        fn hex(c: &Checksum) -> String {
            match c {
                Checksum::Sha256(h) | Checksum::Sha512(h) => h.clone(),
            }
        }
        let std = PinSet::for_tier(BinaryTier::Standard);
        for tier in [BinaryTier::Legacy14, BinaryTier::Legacy13] {
            let p = PinSet::for_tier(tier);
            assert_ne!(p, std, "{tier:?} must differ from Standard");
            // Singles: cloudflared on every legacy tier.
            let a = manifest("cloudflared", p.cloudflared, "macos", Arch::Arm64).unwrap();
            let x = manifest("cloudflared", p.cloudflared, "macos", Arch::X86_64).unwrap();
            assert!(a.url.contains("/2025.4.0/"), "{}", a.url);
            assert_ne!(hex(&a.checksum), hex(&x.checksum));
            let s = manifest("cloudflared", std.cloudflared, "macos", Arch::Arm64).unwrap();
            assert_ne!(hex(&a.checksum), hex(&s.checksum));
        }
        // Legacy14: PostgreSQL 16.4.0, and NOTHING else moves.
        let l14 = PinSet::for_tier(BinaryTier::Legacy14);
        assert_eq!(l14.postgres_versions, &["16.4.0"]);
        assert_eq!(PinSet { cloudflared: std.cloudflared, postgres: std.postgres, postgres_versions: std.postgres_versions, ..l14 }, std);
        // Legacy13: every engine row, both slices, own bytes.
        let l13 = PinSet::for_tier(BinaryTier::Legacy13);
        assert!(!l13.php_versions.contains(&"8.0.30"), "8.0.30 is minos 14.0 on arm64");
        assert!(l13.php_versions.contains(&"7.4.33") && l13.php_versions.contains(&"8.5.8"));
        for (name, versions) in [("mysql", l13.mysql_versions), ("postgres", &["16.4.0"][..])] {
            for v in versions {
                let a = manifest(name, v, "macos", Arch::Arm64).unwrap();
                let x = manifest(name, v, "macos", Arch::X86_64).unwrap();
                assert_ne!(hex(&a.checksum), hex(&x.checksum), "{name} {v}");
            }
        }
        for (name, versions) in [("redis", l13.redis_versions), ("mariadb", l13.mariadb_versions), ("httpd", &[l13.httpd][..])] {
            for v in versions {
                let a = bundle_manifest(name, v, "macos", Arch::Arm64).unwrap();
                let x = bundle_manifest(name, v, "macos", Arch::X86_64).unwrap();
                let std_v = match name { "redis" => std.redis, "mariadb" => std.mariadb, _ => std.httpd };
                let sv = bundle_manifest(name, std_v, "macos", Arch::Arm64).unwrap();
                for (i, part) in a.parts.iter().enumerate() {
                    assert_ne!(hex(&part.checksum), hex(&x.parts[i].checksum), "{name} {v} {}", part.formula);
                    // Same formula in the standard bundle ⇒ a different (ventura) blob.
                    if let Some(sp) = sv.parts.iter().find(|q| q.formula == part.formula) {
                        assert_ne!(hex(&part.checksum), hex(&sp.checksum), "{name} {v} {} reuses the sonoma blob", part.formula);
                    }
                    assert!(part.url.contains("/blobs/sha256:"), "{}", part.url);
                }
            }
        }
        // Xdebug: one ventura row per minor that HAS one, at the legacy version —
        // and none for 8.5, whose ventura blobs predate PHP 8.5 GA (measured:
        // they load into no 8.5). A row added there would resolve, prepare and
        // fail at dlopen on the user's machine.
        assert!(bundle_manifest("xdebug-8.5", l13.xdebug, "macos", Arch::Arm64).is_none(), "8.5 has no 13-capable Xdebug");
        for minor in ["8.1", "8.2", "8.3", "8.4"] {
            let name = format!("xdebug-{minor}");
            let a = bundle_manifest(&name, l13.xdebug, "macos", Arch::Arm64).unwrap();
            let x = bundle_manifest(&name, l13.xdebug, "macos", Arch::X86_64).unwrap();
            let s = bundle_manifest(&name, std.xdebug, "macos", Arch::Arm64).unwrap();
            assert_ne!(hex(&a.parts[0].checksum), hex(&x.parts[0].checksum), "{name}");
            assert_ne!(hex(&a.parts[0].checksum), hex(&s.parts[0].checksum), "{name} reuses the sonoma blob");
            assert!(a.parts[0].url.contains(&format!("xdebug/{minor}/blobs/")), "{}", a.parts[0].url);
        }
        // …and the tier's pick is what `xdebug_row` reads.
        install_tier(BinaryTier::Legacy13);
        assert_eq!(super::xdebug_row("8.4").unwrap().version, "3.4.5");
        install_tier(BinaryTier::Standard);
        assert_eq!(super::xdebug_row("8.4").unwrap().version, "3.5.3");
    }

    #[test]
    fn the_empty_state_is_standard_and_install_replaces_it() {
        // Plant: with `install_tier` a no-op this read stayed Standard and the
        // second assertion failed — the accessor is wired to the store.
        assert_eq!(tier(), BinaryTier::Standard);
        install_tier(BinaryTier::Legacy13);
        assert_eq!(tier(), BinaryTier::Legacy13);
        install_tier(BinaryTier::Standard);
        assert_eq!(tier(), BinaryTier::Standard);
    }
}

#[cfg(test)]
mod tests {

    /// **Nothing asks the FILE resolver for a program that is a DIRECTORY on some OS.**
    ///
    /// `resolve` refuses a tree with "`{name}` is a directory distribution — use resolve_dir",
    /// and on Windows `php` and `nginx` ARE trees. Six callers asked it for `"php"` anyway:
    /// the site Terminal tab, the Adminer verifier, three `repo` paths and the MCP scratch
    /// runner. Every one of them was dead on Windows, and the Terminal tab rendered that
    /// sentence — a function name — to the user (seen on the Dell, 19 Sep 2026).
    ///
    /// `resolve_program` already existed and says in its own doc comment that it is for
    /// exactly this ("whatever shape it ships in on this OS"). So the bug was never the
    /// missing capability; it was six callers reaching past it. A guard, not a fix, because
    /// the seventh caller is the one nobody will remember.
    ///
    /// **Only LITERAL names can be checked.** A call whose name is a variable is counted and
    /// reported, so this test says what it cannot see rather than implying it saw everything.
    #[test]
    fn no_caller_asks_the_file_resolver_for_a_directory_distribution() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let (mut offenders, mut dynamic, mut scanned) = (Vec::new(), 0usize, 0usize);
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                scanned += 1;
                let src = std::fs::read_to_string(&path).expect("read");
                let lines: Vec<&str> = src.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if line.trim_start().starts_with("//") || !line.contains("binaries::resolve(") {
                        continue;
                    }
                    // The name is the second argument, which may sit on a later line.
                    let window = lines[i..(i + 5).min(lines.len())].join(" ");
                    let after = window.split("binaries::resolve(").nth(1).unwrap_or("");
                    let Some(name) = after.split('"').nth(1) else {
                        dynamic += 1;
                        continue;
                    };
                    if ["macos", "windows"].iter().any(|os| shape_of_on(name, os) == Shape::Dir) {
                        offenders.push(format!("{}:{}: resolve(…, {name:?}, …)", path.display(), i + 1));
                    }
                }
            }
        }
        assert!(scanned > 50, "scanned only {scanned} files — the walk is not reading the source");
        assert!(
            offenders.is_empty(),
            "these ask the FILE resolver for a program that is a DIRECTORY on some OS — \
             `resolve_program` is the one that answers on both ({dynamic} more calls name \
             the binary through a variable and cannot be checked here):\n{}",
            offenders.join("\n")
        );
    }
    use super::*;

    /// A fixture zip at a fixture-owned temp path. A name ending in `/` is a
    /// directory, a body starting `link:` makes a symlink to the rest, anything
    /// else is a DEFLATED file — the compression every real Windows zip uses.
    fn write_zip(tag: &str, entries: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("rexenv-zip-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            if let Some(target) = body.strip_prefix("link:") {
                w.add_symlink(*name, target, opts).unwrap();
            } else if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                std::io::Write::write_all(&mut w, body.as_bytes()).unwrap();
            }
        }
        w.finish().unwrap();
        (root, path)
    }

    #[test]
    fn a_zip_member_is_found_by_file_name_and_a_missing_one_fails_loud() {
        let (root, zip) = write_zip(
            "member",
            &[("README.md", "readme"), ("caddy.exe", "MZ-caddy"), ("LICENSE", "apache")],
        );
        let dest = root.join("caddy.exe");
        extract_zip_member(&zip, "caddy.exe", &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "MZ-caddy");
        let missing = extract_zip_member(&zip, "nope.exe", &root.join("nope.exe")).unwrap_err();
        assert!(missing.to_string().contains("not found"), "{missing}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zip_tree_strips_what_it_is_told_and_keeps_the_layout() {
        // nginx's shape: one top directory, stripped.
        let (root, zip) = write_zip(
            "tree",
            &[
                ("nginx-1.30.4/", ""),
                ("nginx-1.30.4/nginx.exe", "MZ-nginx"),
                ("nginx-1.30.4/conf/nginx.conf", "worker_processes 1;"),
            ],
        );
        let dest = root.join("out");
        extract_zip_tree(&zip, &dest, 1).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("nginx.exe")).unwrap(), "MZ-nginx");
        assert!(dest.join("conf/nginx.conf").is_file());
        assert!(!dest.join("nginx-1.30.4").exists(), "the top directory must be stripped");

        // PHP's shape: flat, nothing stripped.
        let (flat_root, flat) = write_zip("flat", &[("php.exe", "MZ-php"), ("ext/php_curl.dll", "dll")]);
        let flat_dest = flat_root.join("out");
        extract_zip_tree(&flat, &flat_dest, 0).unwrap();
        assert!(flat_dest.join("php.exe").is_file() && flat_dest.join("ext/php_curl.dll").is_file());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&flat_root);
    }

    /// Zip-slip: an entry that climbs out of the destination, or names an absolute
    /// path, is REFUSED — and nothing lands outside `dest` on the way.
    #[test]
    fn a_zip_entry_that_escapes_the_destination_is_refused_and_writes_nothing_outside() {
        for (tag, hostile) in [("dotdot", "top/../../escaped.txt"), ("absolute", "/escaped-abs.txt")] {
            let (root, zip) = write_zip(tag, &[("top/ok.txt", "fine"), (hostile, "pwned")]);
            let dest = root.join("out");
            let err = extract_zip_tree(&zip, &dest, 0).unwrap_err();
            assert!(err.to_string().contains("unsafe path"), "{tag}: {err}");
            assert!(!root.join("escaped.txt").exists(), "{tag}: an entry was written outside dest");
            assert!(!std::path::Path::new("/escaped-abs.txt").exists(), "{tag}: an absolute entry was written");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn a_symlink_in_a_zip_is_refused() {
        let (root, zip) = write_zip("symlink", &[("top/link", "link:../../outside"), ("top/f.txt", "x")]);
        let err = extract_zip_tree(&zip, &root.join("out"), 0).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

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
        fn app_bundle(&self) -> &dyn crate::platform::traits::AppBundle { unimplemented!() }
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
        // The member name this OS publishes — `php` on unix, `php.exe` on Windows, from
        // the same helper `published_member` uses. Writing the literal `php` made the
        // whole test vacuous on Windows: nothing was where the resolve looks, so
        // `needs_repair` said false and the fixture, not the claim, was what failed (W12).
        let member = exe_name("php", std::env::consts::OS);
        std::fs::write(dir.join(&member), "not really a binary").unwrap();

        // The binary exists — the OLD `is_cached` stopped here and said yes.
        assert!(dir.join(&member).exists());
        // …but it is not resolvable: no pin marker, and 7.4 is ours, so an
        // absent marker is stale. Planner and resolve now agree it is not.
        assert!(!is_cached(&platform, "php", v));
        assert!(cached_path(&platform, "php", v).is_none());
        assert!(cached_bin(&platform, "php", v).is_none());
        // And it reads as a REPAIR, not as "never downloaded" — the distinction
        // that keeps login-start offline (ledger #175).
        assert!(needs_repair(&platform, "php", v));

        // Give it the right marker and the licences it owes, and all three flip.
        // THIS host's pin, not macOS's: the assertions around it go through `is_cached`
        // and `needs_repair`, which read `std::env::consts::OS` via the platform they are
        // handed. Naming macOS here made the test compare a macOS checksum against a
        // Windows resolve, and the claim — planner and resolve agree — is os-neutral (W12).
        let spec = manifest("php", v, std::env::consts::OS, Arch::Arm64).unwrap();
        write_pin_marker(&dir, &spec.checksum);
        std::fs::create_dir_all(dir.join(LICENSES_DIR)).unwrap();
        std::fs::write(dir.join(LICENSES_DIR).join("PHP-3.01.txt"), "…").unwrap();
        assert!(is_cached(&platform, "php", v));
        // **What a cache hit RESOLVES TO follows the artifact's shape on this os, and PHP
        // has two.** `shape_of_on("php", "windows")` is `Dir` (the php.net zip is a tree),
        // `Single` on macOS — so `cached_path` hands back the directory there and the
        // member here, and `cached_bin`, which only ever answers for `Single`, is `None`
        // on Windows. Asserting the macOS spelling on both hosts made this fail on the
        // Dell for behaviour that is correct there (W12); deriving both from the same
        // `shape_of_on` the code reads keeps one claim instead of two spellings.
        let shape = shape_of_on("php", std::env::consts::OS);
        let expected = if shape == Shape::Dir { dir.clone() } else { dir.join(&member) };
        assert_eq!(cached_path(&platform, "php", v).unwrap(), expected);
        assert_eq!(
            cached_bin(&platform, "php", v),
            (shape == Shape::Single).then(|| dir.join(&member)),
            "cached_bin answers only for a single-binary shape"
        );
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
        // Everything else is a single executable published at `dir/<name>` — on macOS,
        // which is the os every `manifest` call here names. `shape_of` read the HOST, so
        // on Windows this compared a macOS pin against the Windows shape (nginx ships as
        // a tree there) and failed for a reason that was not the claim (W12).
        for n in ["php", "php-fpm", "caddy", "nginx", "mailpit", "frankenphp", "cloudflared"] {
            assert_eq!(shape_of_on(n, "macos"), Shape::Single, "{n}");
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
        assert!(cli.url.ends_with(&format!("php-{PHP_VERSION}-cli-macos-aarch64.tar.gz")), "{}", cli.url);
        assert_eq!(cli.member, "php");
        assert!(matches!(cli.checksum, Checksum::Sha256(_)));
        assert_eq!(checksum_hex(&cli.checksum).len(), 64); // SHA-256 hex

        let fpm = manifest("php-fpm", PHP_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(fpm.url.ends_with(&format!("php-{PHP_VERSION}-fpm-macos-x86_64.tar.gz")), "{}", fpm.url);
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
            assert!(xdebug_supported_on(minor, "macos"), "{minor}");
            let (name, version) = xdebug_bundle_id_on(minor, "macos").unwrap();
            assert_eq!(name, format!("xdebug-{minor}"));
            // These minors are all inside Xdebug's current support window, so
            // they sit at the default — asserted from the SAME accessor the UI
            // reads, not from the constant, so a minor frozen at an older
            // release would show up here rather than hide behind the constant.
            assert_eq!(version, XDEBUG_VERSION);
            // Asked OF macOS, like the `bundle_manifest` calls below: the rows are
            // macOS-only pins, and on a Windows host the host-reading form answers `None`
            // (W12 — the accessor now takes the os for exactly this reason).
            assert_eq!(xdebug_version_for_on(minor, "macos"), Some(version));
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
        // The table's own verdicts, asked of macOS explicitly — so this half reads the same
        // on any host that runs it.
        let mut not_here = 0;
        for v in PHP_VERSIONS {
            let minor = v.rsplit_once('.').map(|(m, _)| m).unwrap_or(v);
            match xdebug_status_on(minor, "macos") {
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
                // Also an explicit verdict — the minor HAS a bottle, this OS has none
                // (D4/W10, #642). Counted, not panicked: on a host where Xdebug does not
                // ship this is the right answer for every offered minor.
                XdebugStatus::NotOnThisOs => not_here += 1,
            }
        }
        // Landmarks. A table gutted to one arm would satisfy every assertion above by
        // having nothing to iterate.
        //
        // Which landmark applies depends on whether Xdebug ships here — asked of the
        // manifest table DIRECTLY, never through `xdebug_status`. Asking the function under
        // test would make this vacuous in the one direction that matters: a post-check that
        // wrongly fired on macOS would flip the expectation with it and still pass.
        assert!(available >= 5, "only {available} minors with a pinned bottle");
        assert_eq!(not_here, 0, "bottles ship on macOS, so no minor may answer NotOnThisOs there");
        assert_eq!(cannot_load, 2, "expected exactly 7.4 and 8.0 to be unloadable");

        // …and D4's half, measured from this host rather than only on Windows: every minor
        // whose bottle exists answers NotOnThisOs there, because no xdebug bundle has a
        // Windows arm. The two absences stay apart — 7.4 and 8.0 keep saying why.
        let mut windows_not_here = 0;
        for v in PHP_VERSIONS {
            let minor = v.rsplit_once('.').map(|(m, _)| m).unwrap_or(v);
            match (xdebug_status_on(minor, "macos"), xdebug_status_on(minor, "windows")) {
                (XdebugStatus::Available(_), XdebugStatus::NotOnThisOs) => windows_not_here += 1,
                (XdebugStatus::CannotLoadExtensions { .. }, XdebugStatus::CannotLoadExtensions { .. }) => {}
                (mac, win) => panic!("{minor}: macOS {mac:?} but Windows {win:?}"),
            }
        }
        assert_eq!(windows_not_here, available, "every pinned bottle must be absent on Windows (D4)");

        // NotPinned must still be REACHABLE — it is the honest answer for a
        // minor rexenv does not offer, and a version that no longer exists.
        assert!(matches!(xdebug_status_on("8.9", "macos"), XdebugStatus::NotPinned));
    }

    /// The refusal a user reads differs with the reason, and neither sentence
    /// may be the other's.
    ///
    /// The old single message ("its static build can't load extensions") was
    /// true of both absences that existed, which is exactly why the conflation
    /// survived: it was correct until the day it silently was not.
    #[test]
    fn the_xdebug_refusal_says_which_kind_of_unavailable_it_is() {
        assert!(xdebug_unavailable_reason_on("8.3", "macos").is_none(), "8.3 has a pinned bottle");

        // The THIRD kind of unavailable, read from either host. It may not borrow either
        // other sentence: nothing is wrong with the PHP build, and there is no newer PHP to
        // move to, so no way out is offered.
        let why = xdebug_unavailable_reason_on("8.3", "windows").expect("no Xdebug ships on Windows");
        assert!(why.contains("Windows"), "the sentence must name the OS it is about: {why}");
        assert!(why.contains("8.3"), "it must name the version the user asked about: {why}");
        assert!(!why.contains("exports no Zend symbols"), "that is a claim about the PHP build: {why}");
        assert!(!why.contains("or newer"), "there is no way out to offer here: {why}");

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
        // Asked OF macOS: on Windows nothing is pinned at all, so the host-reading form
        // makes every line below true for the wrong reason — a refusal that proves
        // nothing because there is nothing to refuse (W12).
        for minor in ["8.0", crate::core::php::unshipped_minor(), "9.0", ""] {
            assert!(!xdebug_supported_on(minor, "macos"), "{minor}");
            assert!(xdebug_bundle_id(minor).is_none());
            assert!(xdebug_version_for_on(minor, "macos").is_none(), "{minor}");
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
    /// "this minor has no Xdebug" (`docs/archive/PLAN-php-74-support.md` §4.6).
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
        assert_eq!(xdebug_version_for_on("8.4", "macos"), Some(XDEBUG_VERSION));
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
        // The upstream half is ASKED for, not spelled: `PHP_VERSION` was the
        // literal here until the default minor became one of ours (10 Sep 2026),
        // at which point this test asserted the two halves of an asymmetry using
        // the same side twice.
        let upstream = upstream_php_version();
        assert!(php_self_hosted_tag(upstream).is_none());
        assert!(cache_matches_pin(&dir, upstream, &pin), "upstream, unmarked → grandfathered");

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
    /// A PHP version that still comes from static-php.dev — read from the table
    /// rather than named, because which versions are ours changes (7.4 in Aug
    /// 2026, then 8.1-8.5 in Sep) and every test that spelled one asserted a
    /// stale fact the day after it moved.
    fn upstream_php_version() -> &'static str {
        PHP_VERSIONS
            .iter()
            .copied()
            .find(|v| php_self_hosted_tag(v).is_none())
            .expect("at least one PHP version still comes from upstream")
    }

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
                // A version we do NOT self-host still comes from static-php.dev.
                // Read from the table rather than named: `PHP_VERSION` was the
                // literal here and stopped being true the day the default minor
                // became one of ours (10 Sep 2026) — the same
                // spelled-vs-asked mistake the tag comment above records.
                assert!(php_url(kind, upstream_php_version(), arch).contains("dl.static-php.dev"));
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
        // …and every version a LEGACY tier asks for that the standard set does
        // not (dedup by name: a version already listed is the same artifact).
        for tier in [BinaryTier::Legacy14, BinaryTier::Legacy13] {
            let p = PinSet::for_tier(tier);
            let extra: Vec<(&str, Vec<String>)> = vec![
                ("cloudflared", one(p.cloudflared)),
                ("mysql", all(p.mysql_versions)),
                ("postgres", all(p.postgres_versions)),
                ("mariadb", all(p.mariadb_versions)),
                ("redis", all(p.redis_versions)),
                ("httpd", one(p.httpd)),
            ];
            for (name, versions) in extra {
                let slot = out.iter_mut().find(|(n, _)| n == name).expect(name);
                for v in versions {
                    if !slot.1.contains(&v) {
                        slot.1.push(v);
                    }
                }
            }
        }
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
    /// **A version the manifest ADDS still cannot ship without its licences.**
    ///
    /// The manifest carries rexenv's own builds for the versions it publishes —
    /// upstream's have no `pdo_pgsql` — and such a version has no compiled-in
    /// licence digest by construction: it did not exist when this binary was
    /// built. So the licences must come from the same document, and if they do
    /// not, the artifact must REFUSE to resolve rather than install without
    /// them. Both halves are asserted, because each without the other is a
    /// different bug: the fallback alone would let a silent omission through,
    /// and the refusal alone would make every manifest-supplied update fail.
    #[test]
    fn a_manifest_version_of_ours_brings_its_licences_or_does_not_resolve() {
        let added = "8.3.99";
        let ours = format!("{RUNTIMES_RELEASE_BASE}/php-8x-9/php-{added}-cli-macos-aarch64.tar.gz");
        let lic = format!("{RUNTIMES_RELEASE_BASE}/php-8x-9/licenses-php-{added}-aarch64.tar.gz");

        // Without a licence entry: the refusal, naming what to do about it.
        let cat = crate::core::updates::catalog_for_tests(&[(
            "php", added, "arm64", &ours, &"a".repeat(64),
        )]);
        install_catalog(cat);
        let err = licenses_spec(&ours, "php", added, Arch::Arm64)
            .expect_err("a self-distributed artifact with no licences must refuse");
        assert!(err.to_string().contains("licence"), "{err}");

        // With one: it resolves, from the SAME release as the bytes.
        let cat = crate::core::updates::catalog_for_tests(&[
            ("php", added, "arm64", &ours, &"a".repeat(64)),
            (LICENSES_ARTIFACT, added, "arm64", &lic, &"b".repeat(64)),
        ]);
        install_catalog(cat);
        let spec = licenses_spec(&ours, "php", added, Arch::Arm64)
            .expect("licences in the catalog resolve")
            .expect("a self-distributed artifact owes licences");
        assert_eq!(spec.url, lic);
        assert_eq!(spec.member, LICENSES_DIR);

        // …and nothing changed for an artifact somebody else distributes.
        let theirs = php_url("cli", upstream_php_version(), Arch::Arm64);
        assert!(
            licenses_spec(&theirs, "php", upstream_php_version(), Arch::Arm64)
                .expect("upstream owes nothing")
                .is_none()
        );
    }

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
    /// `docs/archive/PLAN-php-74-support.md` §11 asks for the commit to be recorded in
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
    /// anyway**. `docs/archive/PLAN-php-74-support.md` §6.5 named this exact file and line
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

        // Asked for, not spelled: `PHP_VERSION` was the literal until the
        // default minor became one of ours (10 Sep 2026), which turned the
        // "nothing owed" half of this test into a second copy of the other half.
        let upstream = php_url("cli", upstream_php_version(), Arch::Arm64);
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
    ///   the user just chose and is serving from — `docs/archive/PLAN-binary-updates.md`
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
        assert!(arm.url.ends_with("nginx-1.30.4-macos-aarch64"), "{}", arm.url);
        assert_eq!(arm.archive, Archive::Raw);
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));

        let amd = manifest("nginx", NGINX_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("nginx-1.30.4-macos-x86_64"), "{}", amd.url);
        assert_ne!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum));
    }

    /// #434 — nginx is OURS now, so the licence obligation must follow the
    /// bytes. This is the guard that fired the moment the pin moved and refused
    /// to resolve: a self-hosted artifact with no licence archive beside it is a
    /// redistribution without its notice, and both licences here (nginx
    /// BSD-2-Clause, PCRE2 BSD-3-Clause) require one.
    #[test]
    fn the_self_built_nginx_carries_its_licences() {
        for arch in [Arch::Arm64, Arch::X86_64] {
            let spec = manifest("nginx", NGINX_VERSION, "macos", arch).unwrap();
            assert!(
                is_self_distributed(&spec.url),
                "nginx moved off rexenv's infrastructure; if that is deliberate, the licence \
                 rule below stops applying — decide it, don't drift into it"
            );
            let lic = licenses_spec(&spec.url, "nginx", NGINX_VERSION, arch)
                .expect("licences must be pinned for an artifact we distribute")
                .expect("self-distributed ⇒ Some");
            // Same release as the binary, by construction — not a URL typed twice.
            let dir = |u: &str| u.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap();
            assert_eq!(dir(&lic.url), dir(&spec.url));
        }
    }

    #[test]
    fn every_offered_db_version_is_pinned_in_the_manifest() {
        // Asked of EVERY tier's set, not the constants: a legacy tier that
        // offers a version the manifest cannot resolve is the exact failure
        // this test exists for, and the standard constants would never show it.
        for tier in PinSet::ALL_TIERS {
            let p = PinSet::for_tier(tier);
            for arch in [Arch::Arm64, Arch::X86_64] {
                for v in p.mysql_versions {
                    let m = manifest("mysql", v, "macos", arch).expect(v);
                    // The CDN name carries the macOS the tarball was BUILT on —
                    // a per-pin fact, not one template (8.4.3 is `macos14`).
                    assert!(m.url.contains(&format!("mysql-{v}-{}-", mysql_macos_build(v))), "{}", m.url);
                    // The CDN archives folder follows the series.
                    assert!(m.url.contains(&format!("archives/mysql-{}/", mysql_series(v))));
                }
                for v in p.postgres_versions {
                    let m = manifest("postgres", v, "macos", arch).expect(v);
                    assert!(m.url.contains(&format!("postgresql-{v}-")), "{}", m.url);
                }
                for v in p.mariadb_versions {
                    assert!(bundle_manifest("mariadb", v, "macos", arch).is_some(), "{v}");
                }
                for v in p.redis_versions {
                    assert!(bundle_manifest("redis", v, "macos", arch).is_some(), "{v}");
                }
                assert!(bundle_manifest("httpd", p.httpd, "macos", arch).is_some(), "{tier:?}");
                assert!(manifest("cloudflared", p.cloudflared, "macos", arch).is_some(), "{tier:?}");
            }
            // Defaults are members of their offered sets — where a set exists.
            // Legacy13 offers NO PostgreSQL (no build loads on arm64 there); its
            // `postgres` field names a real pin so nothing is a sentinel, and the
            // empty set is the fact T3's refusal renders.
            assert!(p.mysql_versions.contains(&p.mysql), "{tier:?}");
            assert!(p.mariadb_versions.contains(&p.mariadb), "{tier:?}");
            assert!(p.redis_versions.contains(&p.redis), "{tier:?}");
            if tier == BinaryTier::Legacy13 {
                assert!(p.postgres_versions.is_empty());
                assert!(manifest("postgres", p.postgres, "macos", Arch::Arm64).is_some());
            } else {
                assert!(p.postgres_versions.contains(&p.postgres), "{tier:?}");
            }
        }
        assert_eq!(mysql_macos_build("8.4.6"), "macos15");
        assert_eq!(mysql_macos_build("8.4.3"), "macos14");
        assert_eq!(mysql_macos_build("8.0.40"), "macos14");
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

    /// #433 — the floor check's SUBJECT must exist before its measurement means
    /// anything. Every default-stack entry has to resolve to a manifest for
    /// BOTH arches, because the failure the check exists for is precisely a pin
    /// whose two slices disagree — an entry that silently resolves for one arch
    /// would drop half the comparison and still report a max.
    #[test]
    fn every_default_stack_entry_is_pinned_for_both_arches() {
        for tier in PinSet::ALL_TIERS {
            let stack = default_stack(tier);
            assert!(!stack.is_empty(), "an empty stack makes the floor check vacuous");
            for (name, version) in stack {
                for arch in [Arch::Arm64, Arch::X86_64] {
                    assert!(
                        manifest(name, version, "macos", arch).is_some(),
                        "{name} {version} ({tier:?}) has no macOS manifest for {arch:?} — the floor \
                         check would compare one slice against nothing and call it a match"
                    );
                }
            }
        }
        // The legacy stacks differ from the standard one in cloudflared ALONE:
        // every other default-stack pin is 12.0 on both slices already.
        let std = default_stack(BinaryTier::Standard);
        for tier in [BinaryTier::Legacy14, BinaryTier::Legacy13] {
            let legacy = default_stack(tier);
            for (i, (name, v)) in legacy.iter().enumerate() {
                if *name == "cloudflared" {
                    assert_ne!(*v, std[i].1, "{tier:?} must move cloudflared");
                } else {
                    assert_eq!(*v, std[i].1, "{tier:?} moved {name} — is its floor measured?");
                }
            }
        }
        // The engines are deliberately absent: user-chosen, and PORTS.md's table
        // owns their floors. A future edit that adds one here would quietly
        // change what the app's stated minimum CLAIMS to cover.
        for engine in ["mysql", "mariadb", "postgres", "redis"] {
            assert!(
                !default_stack(BinaryTier::Standard).iter().any(|(n, _)| *n == engine),
                "{engine} joined the default stack — an optional engine's floor binds the user who \
                 enables it, not the app's minimum. Decide that deliberately, in PORTS.md too"
            );
        }
    }

    #[test]
    fn manifest_pins_postgres_as_tree() {
        let arm = manifest("postgres", POSTGRES_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("postgresql-18.6.0-aarch64-apple-darwin.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGzTree);
        assert_eq!(arm.member, "bin/postgres");
        assert!(matches!(arm.checksum, Checksum::Sha256(_)));
        let amd = manifest("postgres", POSTGRES_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("postgresql-18.6.0-x86_64-apple-darwin.tar.gz"));
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
        // The URL is built FROM the pin, so the assertion is too — spelling the
        // version here made this the one place a re-pin had to be remembered
        // (21 Sep 2026: 5.4.2 → 6.1.0, rexenv/rexenv#1).
        assert!(a.url.ends_with(&format!("v{v}/adminer-{v}-en.php", v = ADMINER_VERSION)), "{}", a.url);
        assert_eq!(a.member, "adminer.php");
        assert_eq!(a.archive, Archive::Raw);
        assert!(matches!(a.checksum, Checksum::Sha256(_)));
        // Same artifact on every OS/arch.
        assert_eq!(checksum_hex(&a.checksum), checksum_hex(&b.checksum));
    }

    /// W2 — the Windows arms: source, archive kind and member per artifact, and
    /// ONE x64 artifact whichever `Arch` asks (Windows on ARM runs x64; there is no
    /// arm64 Windows PHP, MySQL, nginx or PostgreSQL to pin).
    #[test]
    fn manifest_pins_the_windows_x64_artifacts() {
        let cases: &[(&str, &str, &str, Archive, &str)] = &[
            ("caddy", CADDY_VERSION, "caddy_2.11.4_windows_amd64.zip", Archive::Zip, "caddy.exe"),
            ("nginx", NGINX_VERSION, "nginx.org/download/nginx-1.30.4.zip", Archive::ZipTree { strip: 1 }, "nginx.exe"),
            ("mailpit", MAILPIT_VERSION, "mailpit-windows-amd64.zip", Archive::Zip, "mailpit.exe"),
            ("cloudflared", CLOUDFLARED_VERSION, "cloudflared-windows-amd64.exe", Archive::Raw, "cloudflared.exe"),
            ("mysql", MYSQL_VERSION, "archives/mysql-8.4/mysql-8.4.6-winx64.zip", Archive::ZipTree { strip: 1 }, "bin/mysqld.exe"),
            ("postgres", POSTGRES_VERSION, "postgresql-18.6.0-x86_64-pc-windows-msvc.tar.gz", Archive::TarGzTree, "bin/postgres.exe"),
            ("php", PHP_VERSION, "archives/php-8.3.32-nts-Win32-vs16-x64.zip", Archive::ZipTree { strip: 0 }, "php.exe"),
        ];
        for (name, version, tail, archive, member) in cases {
            let arm = manifest(name, version, "windows", Arch::Arm64)
                .unwrap_or_else(|| panic!("{name} {version}: no Windows arm"));
            let amd = manifest(name, version, "windows", Arch::X86_64).unwrap();
            assert!(arm.url.ends_with(tail), "{name}: {}", arm.url);
            assert_eq!(arm.archive, *archive, "{name}");
            assert_eq!(arm.member, *member, "{name}");
            assert_eq!(arm.url, amd.url, "{name}: Windows on ARM must get the same x64 artifact");
            assert_eq!(checksum_hex(&arm.checksum), checksum_hex(&amd.checksum), "{name}");
        }
        // The toolset in PHP's file name follows the minor.
        for (v, toolset) in [("7.4.33", "vc15"), ("8.0.30", "vs16"), ("8.3.32", "vs16"), ("8.4.23", "vs17"), ("8.5.8", "vs17")] {
            let url = manifest("php", v, "windows", Arch::X86_64).unwrap().url;
            assert!(url.ends_with(&format!("php-{v}-nts-Win32-{toolset}-x64.zip")), "{url}");
        }
    }

    /// Every version the app OFFERS has a Windows pin, and what Windows does not
    /// get in v1 resolves to nothing rather than to a macOS artifact.
    #[test]
    fn every_offered_version_has_a_windows_pin_and_the_rest_resolve_to_nothing() {
        for v in PHP_VERSIONS {
            assert!(manifest("php", v, "windows", Arch::X86_64).is_some(), "php {v}");
            assert!(manifest("php-fpm", v, "windows", Arch::X86_64).is_none(), "php-fpm {v}: Windows has no FPM");
        }
        for v in MYSQL_VERSIONS {
            assert!(manifest("mysql", v, "windows", Arch::X86_64).is_some(), "mysql {v}");
        }
        for v in POSTGRES_VERSIONS {
            assert!(manifest("postgres", v, "windows", Arch::X86_64).is_some(), "postgres {v}");
        }
        // The default stack, minus php-fpm: on Windows the pool is php-cgi, which
        // ships inside the php zip (plan D1).
        for (name, version) in default_stack(BinaryTier::Standard) {
            if name == "php-fpm" {
                continue;
            }
            assert!(manifest(name, version, "windows", Arch::X86_64).is_some(), "{name} {version}");
        }
        // Not in Windows v1 (plan D4): no macOS artifact may leak through.
        assert!(manifest("frankenphp", FRANKENPHP_VERSION, "windows", Arch::X86_64).is_none());
        for (name, version) in [("redis", REDIS_VERSION), ("mariadb", MARIADB_VERSION), ("httpd", HTTPD_VERSION)] {
            assert!(bundle_manifest(name, version, "windows", Arch::X86_64).is_none(), "{name}");
            // …and the feature gate reads that same absence rather than a list of its own.
            assert!(!ships_on(name, version, "windows"), "{name} must not ship on Windows (D4)");
            assert!(ships_on(name, version, "macos"), "{name} ships on macOS");
        }
    }

    /// Ledger #642 — **`ships_on` answers the same for either arch.** Both tables match on
    /// `(name, os, version)` and differ only in what each arm builds from the arch, so the
    /// gate may ask with one. An arm that ever matched on arch would make this half-true and
    /// silently gate a feature on the machine's CPU, so it is measured, not assumed.
    #[test]
    fn ships_on_does_not_depend_on_the_arch() {
        let names: Vec<(&str, &str)> = default_stack(BinaryTier::Standard)
            .into_iter()
            .chain([
                ("redis", REDIS_VERSION),
                ("mariadb", MARIADB_VERSION),
                ("httpd", HTTPD_VERSION),
                ("frankenphp", FRANKENPHP_VERSION),
            ])
            .collect();
        assert!(names.len() > 5, "too few names to be a real sweep");
        for (name, version) in names {
            for os in ["macos", "windows", "linux"] {
                let arm = manifest(name, version, os, Arch::Arm64).is_some()
                    || bundle_manifest(name, version, os, Arch::Arm64).is_some();
                assert_eq!(arm, ships_on(name, version, os), "{name} {version} on {os}");
            }
        }
    }

    /// A single executable publishes as `name.exe` on Windows, and each Windows
    /// shape agrees with what its manifest arm unpacks.
    #[test]
    fn windows_names_executables_with_their_extension_and_shapes_match_the_archives() {
        assert_eq!(exe_name("caddy", "windows"), "caddy.exe");
        assert_eq!(exe_name("caddy", "macos"), "caddy");
        for n in ["php", "nginx"] {
            assert_eq!(shape_of_on(n, "windows"), Shape::Dir, "{n}");
            assert_eq!(shape_of_on(n, "macos"), Shape::Single, "{n}");
        }
        for (name, version) in [
            ("php", PHP_VERSION),
            ("nginx", NGINX_VERSION),
            ("mysql", MYSQL_VERSION),
            ("postgres", POSTGRES_VERSION),
            ("caddy", CADDY_VERSION),
            ("mailpit", MAILPIT_VERSION),
            ("cloudflared", CLOUDFLARED_VERSION),
        ] {
            let spec = manifest(name, version, "windows", Arch::X86_64).unwrap();
            let tree = matches!(spec.archive, Archive::TarGzTree | Archive::ZipTree { .. });
            assert_eq!(
                shape_of_on(name, "windows") == Shape::Dir,
                tree,
                "{name}: its Windows shape and archive disagree — a resolve would look in the wrong place"
            );
        }
    }

    /// L2 (docs/PLAN-linux-port.md) — the Linux arms: source, archive kind and member per
    /// artifact, BOTH archs with their own digest (every upstream publishes aarch64), and
    /// the spellings each publisher uses for an arch.
    #[test]
    fn manifest_pins_the_linux_artifacts_for_both_archs() {
        let cases: &[(&str, &str, &str, &str, Archive, &str)] = &[
            ("caddy", CADDY_VERSION, "caddy_2.11.4_linux_amd64.tar.gz", "caddy_2.11.4_linux_arm64.tar.gz", Archive::TarGz, "caddy"),
            ("php", PHP_VERSION, "php-8.3.32-cli-linux-x86_64.tar.gz", "php-8.3.32-cli-linux-aarch64.tar.gz", Archive::TarGz, "php"),
            ("php-fpm", PHP_VERSION, "php-8.3.32-fpm-linux-x86_64.tar.gz", "php-8.3.32-fpm-linux-aarch64.tar.gz", Archive::TarGz, "php-fpm"),
            ("nginx", NGINX_VERSION, "nginx-binaries/nginx-1.30.4-x86_64-linux", "nginx-binaries/nginx-1.30.4-aarch64-linux", Archive::Raw, "nginx"),
            ("mysql", MYSQL_VERSION, "mysql-8.4.6-linux-glibc2.28-x86_64.tar.xz", "mysql-8.4.6-linux-glibc2.28-aarch64.tar.xz", Archive::TarXzTree, "bin/mysqld"),
            ("postgres", POSTGRES_VERSION, "postgresql-18.6.0-x86_64-unknown-linux-gnu.tar.gz", "postgresql-18.6.0-aarch64-unknown-linux-gnu.tar.gz", Archive::TarGzTree, "bin/postgres"),
            ("frankenphp", FRANKENPHP_VERSION, "frankenphp-linux-x86_64", "frankenphp-linux-aarch64", Archive::Raw, "frankenphp"),
            ("mailpit", MAILPIT_VERSION, "mailpit-linux-amd64.tar.gz", "mailpit-linux-arm64.tar.gz", Archive::TarGz, "mailpit"),
            ("cloudflared", CLOUDFLARED_VERSION, "cloudflared-linux-amd64", "cloudflared-linux-arm64", Archive::Raw, "cloudflared"),
        ];
        for (name, version, amd_tail, arm_tail, archive, member) in cases {
            let amd = manifest(name, version, "linux", Arch::X86_64).unwrap_or_else(|| panic!("{name} {version}: no Linux x86_64 arm"));
            let arm = manifest(name, version, "linux", Arch::Arm64).unwrap_or_else(|| panic!("{name} {version}: no Linux aarch64 arm"));
            assert!(amd.url.ends_with(amd_tail), "{name}: {}", amd.url);
            assert!(arm.url.ends_with(arm_tail), "{name}: {}", arm.url);
            assert_eq!(amd.archive, *archive, "{name}");
            assert_eq!(amd.member, *member, "{name}");
            assert_ne!(checksum_hex(&amd.checksum), checksum_hex(&arm.checksum), "{name}: two builds, two digests");
            for spec in [&amd, &arm] {
                let hex = checksum_hex(&spec.checksum);
                let want = if matches!(spec.checksum, Checksum::Sha512(_)) { 128 } else { 64 };
                assert_eq!(hex.len(), want, "{name}: not a full digest: {hex}");
            }
        }
    }

    /// Every version the app OFFERS has a Linux pin, except PHP 7.4 (no static Linux build
    /// exists to pin — D-L8), and what Linux does not get in v1 resolves to nothing.
    #[test]
    fn every_offered_version_but_php_74_has_a_linux_pin_and_the_rest_resolve_to_nothing() {
        for v in PHP_VERSIONS {
            let want = !v.starts_with("7.4");
            assert_eq!(manifest("php", v, "linux", Arch::X86_64).is_some(), want, "php {v}");
            assert_eq!(manifest("php-fpm", v, "linux", Arch::X86_64).is_some(), want, "php-fpm {v}");
        }
        for v in MYSQL_VERSIONS {
            assert!(manifest("mysql", v, "linux", Arch::X86_64).is_some(), "mysql {v}");
        }
        for v in POSTGRES_VERSIONS {
            assert!(manifest("postgres", v, "linux", Arch::X86_64).is_some(), "postgres {v}");
        }
        for (name, version) in default_stack(BinaryTier::Standard) {
            assert!(manifest(name, version, "linux", Arch::X86_64).is_some(), "{name} {version}");
        }
        for (name, version) in [("redis", REDIS_VERSION), ("mariadb", MARIADB_VERSION), ("httpd", HTTPD_VERSION)] {
            assert!(bundle_manifest(name, version, "linux", Arch::X86_64).is_none(), "{name}");
            assert!(!ships_on(name, version, "linux"), "{name} must not ship on Linux v1 (D-L8)");
        }
        // Shapes: php and nginx are single static files on Linux, as on macOS.
        for n in ["php", "nginx", "caddy"] {
            assert_eq!(shape_of_on(n, "linux"), Shape::Single, "{n}");
        }
        assert_eq!(exe_name("caddy", "linux"), "caddy");
        for (name, version) in [("mysql", MYSQL_VERSION), ("postgres", POSTGRES_VERSION)] {
            let spec = manifest(name, version, "linux", Arch::X86_64).unwrap();
            assert!(matches!(spec.archive, Archive::TarGzTree | Archive::TarXzTree), "{name} is a tree");
            assert_eq!(shape_of_on(name, "linux"), Shape::Dir);
        }
    }

    /// The xz path is refused, not silently gzip-parsed, on a build without the decoder.
    #[test]
    fn an_xz_tree_on_a_build_without_the_decoder_is_a_named_refusal() {
        let dir = std::env::temp_dir().join(format!("rexenv-xz-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let r = extract_tar_xz_tree(std::io::Cursor::new(vec![0u8; 8]), &dir);
        let _ = std::fs::remove_dir_all(&dir);
        if cfg!(target_os = "linux") {
            assert!(r.is_err(), "eight zero bytes are not an xz stream");
        } else {
            assert!(matches!(r, Err(Error::Unsupported(_))), "{r:?}");
        }
    }

    #[test]
    fn manifest_unknown_is_none() {
        assert!(manifest("nginx", "1.0", "macos", Arch::Arm64).is_none());
        assert!(manifest("caddy", "9.9.9", "macos", Arch::Arm64).is_none());
        // Windows HAS arms since W2 (this line used to assert it had none); an unknown
        // version there still resolves to nothing, and so does an OS with no arms yet.
        assert!(manifest("caddy", "9.9.9", "windows", Arch::X86_64).is_none());
        assert!(manifest("caddy", "9.9.9", "linux", Arch::X86_64).is_none());
        assert!(manifest("caddy", CADDY_VERSION, "freebsd", Arch::X86_64).is_none(), "an OS with no arms");
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
        assert!(matches!(pinned.checksum, Checksum::Sha256(ref h) if h == ADMINER_6_1_0_SHA256));
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
            matches!(still_pinned.checksum, Checksum::Sha256(ref h) if h == ADMINER_6_1_0_SHA256),
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
