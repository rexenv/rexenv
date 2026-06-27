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

/// How a downloaded artifact is packaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    /// gzip-compressed tar; extract `member` from it.
    TarGz,
    /// the download is the binary itself.
    Raw,
}

/// A resolved download target: where to get it, how to verify and unpack it.
#[derive(Debug, Clone)]
pub struct BinarySpec {
    pub url: String,
    /// Lowercase hex SHA-512 of the downloaded artifact (Caddy publishes SHA-512).
    pub sha512: String,
    pub archive: Archive,
    /// File name to extract from a `TarGz` archive (ignored for `Raw`).
    pub member: &'static str,
}

fn arch_token(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "arm64",
        Arch::X86_64 => "amd64",
    }
}

// Official Caddy SHA-512 checksums (from caddy_<ver>_checksums.txt).
const CADDY_2_11_4_MAC_ARM64_SHA512: &str = "3190ae0df98b59ab4b6021556fa35adc3c526a4f3e138776b0eaec8a037cc26121cbbb1ad53453f565551b47d37d5ba4755e2c2c3652256737fe2ce9e53c8ec0";
const CADDY_2_11_4_MAC_AMD64_SHA512: &str = "e04eb10f9ce7e2e079bc9bff1bd5d3a3164888d1edbb1a49e5d15be4eab691b57e89ed36bb29c65ba43f1ba8d9279e0967b1003991c13fe4cb78384c3caf25de";

/// Look up the download spec for `name`@`version` on `os`+`arch`, or `None` if
/// unknown.
pub fn manifest(name: &str, version: &str, os: &str, arch: Arch) -> Option<BinarySpec> {
    match (name, os, version) {
        ("caddy", "macos", "2.11.4") => {
            let token = arch_token(arch);
            let sha512 = match arch {
                Arch::Arm64 => CADDY_2_11_4_MAC_ARM64_SHA512,
                Arch::X86_64 => CADDY_2_11_4_MAC_AMD64_SHA512,
            };
            Some(BinarySpec {
                url: format!(
                    "https://github.com/caddyserver/caddy/releases/download/v{version}/caddy_{version}_mac_{token}.tar.gz"
                ),
                sha512: sha512.to_string(),
                archive: Archive::TarGz,
                member: "caddy",
            })
        }
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
            arch_token(arch)
        ))
    })?;

    let bytes = http_get(&spec.url).await?;
    verify_sha512(&bytes, &spec.sha512)?;

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

fn verify_sha512(bytes: &[u8], expected_hex: &str) -> Result<()> {
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(bytes);
    let got = hex_lower(&hasher.finalize());
    if got.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "checksum mismatch: expected {expected_hex}, got {got}"
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

    #[test]
    fn manifest_resolves_caddy_per_arch() {
        let arm = manifest("caddy", CADDY_VERSION, "macos", Arch::Arm64).unwrap();
        assert!(arm.url.ends_with("caddy_2.11.4_mac_arm64.tar.gz"));
        assert_eq!(arm.archive, Archive::TarGz);
        assert_eq!(arm.member, "caddy");
        assert_eq!(arm.sha512.len(), 128); // SHA-512 hex

        let amd = manifest("caddy", CADDY_VERSION, "macos", Arch::X86_64).unwrap();
        assert!(amd.url.ends_with("caddy_2.11.4_mac_amd64.tar.gz"));
        assert_ne!(arm.sha512, amd.sha512);
    }

    #[test]
    fn manifest_unknown_is_none() {
        assert!(manifest("nginx", "1.0", "macos", Arch::Arm64).is_none());
        assert!(manifest("caddy", "9.9.9", "macos", Arch::Arm64).is_none());
        assert!(manifest("caddy", CADDY_VERSION, "windows", Arch::X86_64).is_none());
    }

    #[test]
    fn sha512_verification() {
        // SHA-512("abc")
        let expected = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        assert!(verify_sha512(b"abc", expected).is_ok());
        assert!(verify_sha512(b"abcd", expected).is_err());
    }

    #[test]
    fn hex_lower_encodes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }
}
