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
/// Pinned PHP version (static-php build; provides `php` cli and `php-fpm`).
pub const PHP_VERSION: &str = "8.3.31";
/// Pinned nginx version (jirutka/nginx-binaries static build).
pub const NGINX_VERSION: &str = "1.30.3";

/// How a downloaded artifact is packaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    /// gzip-compressed tar; extract `member` from it.
    TarGz,
    /// the download is the binary itself.
    Raw,
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

// static-php.dev SHA-256 (computed at pin time — source publishes no checksums).
const PHP_8_3_31_CLI_MAC_ARM64_SHA256: &str = "5a173ce3b1cbffab395b4984f84b6cbc0347bab35d774c45e98e1a39526c44de";
const PHP_8_3_31_CLI_MAC_AMD64_SHA256: &str = "c3b3b9507e3559fcd8df60aaee6207fcc05f258c325b902b182ea7a7fae52724";
const PHP_8_3_31_FPM_MAC_ARM64_SHA256: &str = "e683ff6c2b4760d8e6ecb6f700dc2fde8a9702744868dda4467c7c8362d86865";
const PHP_8_3_31_FPM_MAC_AMD64_SHA256: &str = "f533184a7e1a044f13f8bcb980620a9eabd9bd40124a946916e02199fc04d71e";

// jirutka/nginx-binaries SHA-256 (computed at pin time; cross-checked vs the
// project's published SHA-1).
const NGINX_1_30_3_MAC_ARM64_SHA256: &str = "b6c4e80357977457b9395a43497f5709dc989ff8fce9102bb558ed5d2e066b15";
const NGINX_1_30_3_MAC_AMD64_SHA256: &str = "fe1df1fdf5de7c5b778a16b1c73aa73d0c22d48219bd712e51094c0e8655a641";

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

fn pick(arch: Arch, arm: &str, amd: &str) -> String {
    match arch {
        Arch::Arm64 => arm,
        Arch::X86_64 => amd,
    }
    .to_string()
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
        ("php", "macos", "8.3.31") => Some(BinarySpec {
            url: format!(
                "https://dl.static-php.dev/static-php-cli/common/php-{version}-cli-macos-{}.tar.gz",
                php_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                PHP_8_3_31_CLI_MAC_ARM64_SHA256,
                PHP_8_3_31_CLI_MAC_AMD64_SHA256,
            )),
            archive: Archive::TarGz,
            member: "php",
        }),
        ("php-fpm", "macos", "8.3.31") => Some(BinarySpec {
            url: format!(
                "https://dl.static-php.dev/static-php-cli/common/php-{version}-fpm-macos-{}.tar.gz",
                php_arch(arch)
            ),
            checksum: Checksum::Sha256(pick(
                arch,
                PHP_8_3_31_FPM_MAC_ARM64_SHA256,
                PHP_8_3_31_FPM_MAC_AMD64_SHA256,
            )),
            archive: Archive::TarGz,
            member: "php-fpm",
        }),
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
    }

    platform.permissions().set_executable(&bin_path)?;
    platform.binaries().prepare_binary(&bin_path)?;
    Ok(bin_path)
}

async fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = reqwest::get(url)
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
