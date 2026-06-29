//! core::binaries — BinaryProvider orchestration (Phase 1 task 4.1).
//!
//! Platform-agnostic: a manifest (os+arch+version → url + checksum) plus the
//! download → verify → extract → cache flow. OS-specific steps (arch detection,
//! make-executable, ad-hoc codesign/de-quarantine) are delegated to the
//! `BinaryProvider` / `PermissionManager` platform traits. No binaries are
//! bundled; everything is fetched on demand and checksum-verified.

use crate::error::{Error, Result};
use crate::platform::traits::{Arch, Platform};
use std::path::{Path, PathBuf};

/// Pinned Caddy version (edge router).
pub const CADDY_VERSION: &str = "2.11.4";
/// Default PHP version (static-php build; provides `php` cli and `php-fpm`).
/// Used where a single version is implied (Phase 1 paths). Must be in [`PHP_VERSIONS`].
pub const PHP_VERSION: &str = "8.3.31";
/// All PHP versions with pinned static-php "bulk" builds (one minor each, newest
/// last). The per-version FPM pool manager + UI (Phase 2 §1.2/§1.5) install from
/// this set; each caches independently under `bin_dir/php-<version>/`.
pub const PHP_VERSIONS: &[&str] = &["8.1.34", "8.2.31", "8.3.31"];
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
/// Pinned MySQL version (official macOS tarball — a full bin/lib/share tree).
pub const MYSQL_VERSION: &str = "8.4.6";
/// Pinned WP-CLI version (a .phar run via the bundled PHP; OS-agnostic).
pub const WP_CLI_VERSION: &str = "2.12.0";
/// Pinned FrankenPHP version (one static binary: embedded PHP + Caddy). Used as a
/// per-site override server on an internal loopback port — Phase 2 §2.
pub const FRANKENPHP_VERSION: &str = "1.12.4";
/// Pinned PostgreSQL version (theseus-rs portable build — a full bin/lib/share
/// tree, like MySQL). Phase 2 §5.3.
pub const POSTGRES_VERSION: &str = "18.4.0";
/// Pinned Mailpit version (one static Go binary: SMTP sink + web UI/API). Phase 3 §2.1.
pub const MAILPIT_VERSION: &str = "1.30.3";
/// Pinned Adminer version (a single `adminer.php`, all drivers, run via the bundled
/// PHP — OS-agnostic, like WP-CLI). Phase 3 §5.1.
pub const ADMINER_VERSION: &str = "5.4.2";
/// Pinned cloudflared version (one static Go binary; quick-tunnel public sharing). Phase 3 §9.1.
pub const CLOUDFLARED_VERSION: &str = "2026.6.1";

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

// Official Caddy SHA-512 checksums (from caddy_<ver>_checksums.txt).
const CADDY_2_11_4_MAC_ARM64_SHA512: &str = "3190ae0df98b59ab4b6021556fa35adc3c526a4f3e138776b0eaec8a037cc26121cbbb1ad53453f565551b47d37d5ba4755e2c2c3652256737fe2ce9e53c8ec0";
const CADDY_2_11_4_MAC_AMD64_SHA512: &str = "e04eb10f9ce7e2e079bc9bff1bd5d3a3164888d1edbb1a49e5d15be4eab691b57e89ed36bb29c65ba43f1ba8d9279e0967b1003991c13fe4cb78384c3caf25de";

// static-php.dev "bulk" build SHA-256 (computed at pin time — source publishes
// no checksums; downloaded and hashed each artifact). The bulk build includes
// mysqli (required by WordPress) + a wide extension set, unlike "common".
// One digest per version × {cli,fpm} × {arm64,amd64}; both arches pinned together.
const PHP_8_1_34_CLI_MAC_ARM64_SHA256: &str = "b721271659d6e3448c29c0dc5755ffc4b8a1498c4709e1aba6602cfb584a84e4";
const PHP_8_1_34_CLI_MAC_AMD64_SHA256: &str = "5fe69256365f96a270e34208ec574be7012c8c08a23bdf52948d0d16d4d8ec6a";
const PHP_8_1_34_FPM_MAC_ARM64_SHA256: &str = "c5faad9eac5ce9753c30a17fb2a2023dcf72e5b367e0ac76b81d006647ea0e52";
const PHP_8_1_34_FPM_MAC_AMD64_SHA256: &str = "eab87df298d83c8182f296e3f56ac4025cdb2a74baa4b6587d27ea39ac31b5e6";
const PHP_8_2_31_CLI_MAC_ARM64_SHA256: &str = "6d200388047dcc1f6296775d54ff250dcbacf0afccb208b651c242de588b074e";
const PHP_8_2_31_CLI_MAC_AMD64_SHA256: &str = "6eb5901e0ea85f621951a5813eeac88917e827e89f6a8ffc6b74234b184cefd3";
const PHP_8_2_31_FPM_MAC_ARM64_SHA256: &str = "52043aa04dc70c929e3aebe306f5de77e18e7d3ffe6c28f0c7cd50f0958d6474";
const PHP_8_2_31_FPM_MAC_AMD64_SHA256: &str = "aaca332df658e3a5e0d58e94980aae6c285e5cd7b6291192173224abebd75598";
const PHP_8_3_31_CLI_MAC_ARM64_SHA256: &str = "058e11878840ad42eb5e59fe111eb49a712d512fad383b28aef1b8bbd498a44e";
const PHP_8_3_31_CLI_MAC_AMD64_SHA256: &str = "15b6e94f4d5f1c7e3ba7a646095bfe4a7bdae8f7480c4129152096b4a6f1652e";
const PHP_8_3_31_FPM_MAC_ARM64_SHA256: &str = "6b0605c82a8126e6431fce70cb9488fb35c35126eef238ece339e93afb356bc4";
const PHP_8_3_31_FPM_MAC_AMD64_SHA256: &str = "08958f8c80a2c380eff1b7584fed09136fb1ce8b51a73981e382d255fc134b14";

// jirutka/nginx-binaries SHA-256 (computed at pin time; cross-checked vs the
// project's published SHA-1).
const NGINX_1_30_3_MAC_ARM64_SHA256: &str = "b6c4e80357977457b9395a43497f5709dc989ff8fce9102bb558ed5d2e066b15";
const NGINX_1_30_3_MAC_AMD64_SHA256: &str = "fe1df1fdf5de7c5b778a16b1c73aa73d0c22d48219bd712e51094c0e8655a641";

// Official MySQL macOS tarball SHA-256 (computed at pin time from dev.mysql.com).
const MYSQL_8_4_6_MAC_ARM64_SHA256: &str = "56ac9150b9d8fc757a36a2661a1214f5b09e5352d0a220e7a6c302685a5fca10";
const MYSQL_8_4_6_MAC_AMD64_SHA256: &str = "257d36d7ae26c4d1cc616dacf58cd1498c9b3b6dc592f90a63d7e7ecd83be844";

// WP-CLI phar SHA-256 (GitHub release; same artifact on every OS/arch).
const WP_CLI_2_12_0_SHA256: &str = "ce34ddd838f7351d6759068d09793f26755463b4a4610a5a5c0a97b68220d85c";

// Adminer single-file SHA-256 (GitHub release `adminer-5.4.2-en.php`; same on every
// OS/arch — a PHP script). English UI, all DB drivers (MySQL + PostgreSQL).
const ADMINER_5_4_2_SHA256: &str = "f8b1cdc676d72e88d2d470dd05f2dcb7212bf6cdcf78f1eadb7fc292f4cefd39";

// cloudflared static Go binary SHA-256 (computed at pin time from the GitHub
// release `.tgz`). De-quarantined + ad-hoc signed by prepare_binary (no relink).
const CLOUDFLARED_2026_6_1_MAC_ARM64_SHA256: &str = "f6d4c439c6c782b83264951d327989ce5e23373acc5942b872411601fedb020d";
const CLOUDFLARED_2026_6_1_MAC_AMD64_SHA256: &str = "d7a66b525fe76820da6e5406611b61e48b40de682368ac00454d9158f085be4b";

// FrankenPHP static binary SHA-256 (computed at pin time from the GitHub release).
// A fully static Mach-O (embeds PHP + Caddy), so no Homebrew relink is needed.
const FRANKENPHP_1_12_4_MAC_ARM64_SHA256: &str = "dd08f3a5ff45780fd0498afae8530bcd548a7e5d4dab7402934aeb622f6faeb8";
const FRANKENPHP_1_12_4_MAC_AMD64_SHA256: &str = "a262f0003447363b91f032706748998f206cd7038f1100f7b0952bdb93d5daf1";

// PostgreSQL portable build SHA-256 (theseus-rs/postgresql-binaries — the project
// PUBLISHES these `.sha256` files; cross-checked against a fresh download). A
// relocatable bin/lib/share tree (unsigned Mach-O that runs as-is on Apple Silicon).
const POSTGRES_18_4_0_MAC_ARM64_SHA256: &str = "1b68828f524b638a24918e258b173d0f16773547a0d3b83d9ba74473b61649f2";
const POSTGRES_18_4_0_MAC_AMD64_SHA256: &str = "cbc38067a795d10bbddc730e61c835df0b351c36a7bd2544d388790fcf50aa4d";

// Mailpit static binary SHA-256 (computed at pin time — the project publishes no
// checksums file; each darwin tarball downloaded and hashed). A static Go Mach-O
// (no Homebrew deps to relink), de-quarantined + ad-hoc signed by prepare_binary.
const MAILPIT_1_30_3_MAC_ARM64_SHA256: &str = "46b68e5701c32f2137e97d325605f7e8f0fbb6518e567b7589147c3534bd943e";
const MAILPIT_1_30_3_MAC_AMD64_SHA256: &str = "ea8c2f5ac717ece100b453de282474b46e8f4c327d3e61bee6348f60989eade3";

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
        ("cli", "8.1.34") => (PHP_8_1_34_CLI_MAC_ARM64_SHA256, PHP_8_1_34_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.1.34") => (PHP_8_1_34_FPM_MAC_ARM64_SHA256, PHP_8_1_34_FPM_MAC_AMD64_SHA256),
        ("cli", "8.2.31") => (PHP_8_2_31_CLI_MAC_ARM64_SHA256, PHP_8_2_31_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.2.31") => (PHP_8_2_31_FPM_MAC_ARM64_SHA256, PHP_8_2_31_FPM_MAC_AMD64_SHA256),
        ("cli", "8.3.31") => (PHP_8_3_31_CLI_MAC_ARM64_SHA256, PHP_8_3_31_CLI_MAC_AMD64_SHA256),
        ("fpm", "8.3.31") => (PHP_8_3_31_FPM_MAC_ARM64_SHA256, PHP_8_3_31_FPM_MAC_AMD64_SHA256),
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
        ("mysql", "macos", "8.4.6") => Some(BinarySpec {
            // Direct CDN URL (the dev.mysql.com/get redirector 403s non-curl clients).
            url: format!(
                "https://cdn.mysql.com/archives/mysql-8.4/mysql-{version}-macos15-{}.tar.gz",
                mysql_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                MYSQL_8_4_6_MAC_ARM64_SHA256,
                MYSQL_8_4_6_MAC_AMD64_SHA256,
            )),
            archive: Archive::TarGzTree,
            member: "bin/mysqld", // primary binary within the extracted tree
        }),
        ("postgres", "macos", "18.4.0") => Some(BinarySpec {
            // theseus-rs portable PostgreSQL — a bin/lib/share tree (one top dir).
            url: format!(
                "https://github.com/theseus-rs/postgresql-binaries/releases/download/{version}/postgresql-{version}-{}-apple-darwin.tar.gz",
                postgres_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                POSTGRES_18_4_0_MAC_ARM64_SHA256,
                POSTGRES_18_4_0_MAC_AMD64_SHA256,
            )),
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
        _ => None,
    }
}

/// Resolve `name`@`version` to a ready-to-run cached binary path, downloading +
/// verifying + extracting + signing on first use. Idempotent: a cached binary is
/// returned without re-downloading.
pub async fn resolve(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    let dir = platform.paths().bin_dir()?.join(format!("{name}-{version}"));
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

    let bytes = http_get(&spec.url).await?;
    verify_checksum(&bytes, &spec.checksum)?;

    std::fs::create_dir_all(&dir)?;
    match spec.archive {
        Archive::TarGz => extract_tar_gz_member(&bytes, spec.member, &bin_path)?,
        Archive::Raw => std::fs::write(&bin_path, &bytes)?,
        Archive::TarGzTree => {
            return Err(Error::Other(format!(
                "{name} is a directory distribution — use resolve_dir"
            )))
        }
    }

    platform.permissions().set_executable(&bin_path)?;
    platform.binaries().prepare_binary(&bin_path)?;
    Ok(bin_path)
}

/// Resolve a raw, non-executable artifact (a `Raw` archive that is NOT a native
/// binary — e.g. the WP-CLI `.phar`, run via the bundled PHP). Downloads +
/// verifies + writes it under `bin_dir/<name>-<version>/<member>`; does NOT
/// chmod +x or codesign (it's a script, not a Mach-O). Idempotent.
pub async fn resolve_file(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;
    let spec = manifest(name, version, os, arch)
        .ok_or_else(|| Error::Other(format!("no binary manifest for {name} {version}")))?;
    if spec.archive != Archive::Raw {
        return Err(Error::Other(format!("{name} is not a raw file artifact")));
    }
    let dir = platform.paths().bin_dir()?.join(format!("{name}-{version}"));
    let path = dir.join(spec.member);
    if path.exists() {
        return Ok(path);
    }
    let bytes = http_get(&spec.url).await?;
    verify_checksum(&bytes, &spec.checksum)?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

/// Resolve a directory-distribution (`TarGzTree`, e.g. MySQL) to its extracted
/// base dir, downloading + verifying + extracting on first use. The single
/// top-level dir in the tarball is stripped, so the base dir directly contains
/// `bin/`, `lib/`, `share/`. Idempotent: a cached tree is returned as-is.
pub async fn resolve_dir(platform: &dyn Platform, name: &str, version: &str) -> Result<PathBuf> {
    let arch = platform.binaries().arch();
    let os = std::env::consts::OS;

    let dir = platform.paths().bin_dir()?.join(format!("{name}-{version}"));
    if dir.join("bin").is_dir() {
        return Ok(dir);
    }

    let spec = manifest(name, version, os, arch).ok_or_else(|| {
        Error::Other(format!(
            "no binary manifest for {name} {version} on {os}/{}",
            php_arch(arch)
        ))
    })?;
    if spec.archive != Archive::TarGzTree {
        return Err(Error::Other(format!(
            "{name} is not a directory distribution — use resolve"
        )));
    }

    let bytes = http_get(&spec.url).await?;
    verify_checksum(&bytes, &spec.checksum)?;
    std::fs::create_dir_all(&dir)?;
    extract_tar_gz_tree(&bytes, &dir)?;
    // MySQL's binaries are Oracle-signed + notarized, and a reqwest download adds
    // no quarantine attribute, so no ad-hoc re-signing is needed.
    Ok(dir)
}

async fn http_get(url: &str) -> Result<Vec<u8>> {
    // Some CDNs (e.g. dev.mysql.com) reject the default reqwest User-Agent with
    // 403; present a browser-like UA so downloads are accepted everywhere.
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .build()
        .map_err(|e| Error::Other(format!("http client: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Other(format!("download {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("download {url}: {e}")))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("read {url}: {e}")))?;
    Ok(bytes.to_vec())
}

fn verify_checksum(bytes: &[u8], checksum: &Checksum) -> Result<()> {
    use sha2::{Digest, Sha256, Sha512};
    let (got, expected) = match checksum {
        Checksum::Sha256(hex) => {
            let mut h = Sha256::new();
            h.update(bytes);
            (hex_lower(&h.finalize()), hex)
        }
        Checksum::Sha512(hex) => {
            let mut h = Sha512::new();
            h.update(bytes);
            (hex_lower(&h.finalize()), hex)
        }
    };
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "checksum mismatch: expected {expected}, got {got}"
        )))
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn extract_tar_gz_member(bytes: &[u8], member: &str, dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive as TarArchive;

    let mut archive = TarArchive::new(GzDecoder::new(bytes));
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
fn extract_tar_gz_tree(bytes: &[u8], dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::path::PathBuf;
    use tar::Archive as TarArchive;

    let mut archive = TarArchive::new(GzDecoder::new(bytes));
    for entry in archive.entries()? {
        let mut entry = entry?;
        // Drop the leading top-level component.
        let rel: PathBuf = entry.path()?.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&rel);
        if entry.header().entry_type().is_dir() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn checksum_hex(c: &Checksum) -> &str {
        match c {
            Checksum::Sha256(h) | Checksum::Sha512(h) => h,
        }
    }

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
    fn checksum_verification_sha256_and_sha512() {
        // SHA-256("abc") and SHA-512("abc")
        let s256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let s512 = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        assert!(verify_checksum(b"abc", &Checksum::Sha256(s256.into())).is_ok());
        assert!(verify_checksum(b"abc", &Checksum::Sha512(s512.into())).is_ok());
        assert!(verify_checksum(b"abcd", &Checksum::Sha256(s256.into())).is_err());
    }

    #[test]
    fn hex_lower_encodes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }
}
