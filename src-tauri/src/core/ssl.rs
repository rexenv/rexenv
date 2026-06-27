//! core::ssl — local certificate authority (Phase 1 §3).
//!
//! Platform-agnostic: generates and loads the root CA material. Trusting the CA
//! in the OS store is the per-OS `CertTrustManager` step (3.3); issuing per-site
//! certs signed by this CA is task 3.2.

use crate::error::{Error, Result};
use crate::platform::traits::{Paths, PermissionManager};
use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
    KeyUsagePurpose,
};
use std::fs;
use std::path::{Path, PathBuf};

pub const CA_CERT_FILE: &str = "rexenv-ca.pem";
pub const CA_KEY_FILE: &str = "rexenv-ca-key.pem";

/// Loaded CA material (PEM) plus its on-disk locations.
#[derive(Debug, Clone)]
pub struct LocalCa {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Directory holding CA material under app-data.
pub fn ca_dir(paths: &dyn Paths) -> Result<PathBuf> {
    Ok(paths.app_data_dir()?.join("ca"))
}

/// Generate a fresh self-signed root CA. Returns `(cert_pem, key_pem)`. Pure — no fs.
pub fn generate_ca() -> Result<(String, String)> {
    let mut params =
        CertificateParams::new(Vec::new()).map_err(|e| Error::Other(format!("ca params: {e}")))?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "rexenv Local CA");
    dn.push(DnType::OrganizationName, "rexenv");
    params.distinguished_name = dn;

    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    // 10-year validity (local dev CA).
    params.not_before = date_time_ymd(2024, 1, 1);
    params.not_after = date_time_ymd(2034, 1, 1);

    let key_pair = KeyPair::generate().map_err(|e| Error::Other(format!("ca keygen: {e}")))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| Error::Other(format!("ca self-sign: {e}")))?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

/// Idempotent load-or-create at explicit paths (no OS perms applied — see
/// [`load_or_create`] for the app-data variant that hardens the key file).
pub fn load_or_create_at(cert_path: &Path, key_path: &Path) -> Result<LocalCa> {
    if cert_path.exists() && key_path.exists() {
        return Ok(LocalCa {
            cert_pem: fs::read_to_string(cert_path)?,
            key_pem: fs::read_to_string(key_path)?,
            cert_path: cert_path.to_path_buf(),
            key_path: key_path.to_path_buf(),
        });
    }
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let (cert_pem, key_pem) = generate_ca()?;
    fs::write(cert_path, &cert_pem)?;
    fs::write(key_path, &key_pem)?;
    Ok(LocalCa {
        cert_pem,
        key_pem,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

/// Idempotent CA load-or-create under app-data; the private key file is hardened
/// to owner-only via `PermissionManager`.
pub fn load_or_create(paths: &dyn Paths, perms: &dyn PermissionManager) -> Result<LocalCa> {
    let dir = ca_dir(paths)?;
    let ca = load_or_create_at(&dir.join(CA_CERT_FILE), &dir.join(CA_KEY_FILE))?;
    perms.set_private(&ca.key_path)?;
    Ok(ca)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_ca_produces_a_usable_ca() {
        let (cert_pem, key_pem) = generate_ca().unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(key_pem.contains("PRIVATE KEY"));
        // Re-loading as a CA proves it's a valid signing CA (the 3.2 path).
        CertificateParams::from_ca_cert_pem(&cert_pem).unwrap();
    }

    #[test]
    fn load_or_create_is_idempotent() {
        let dir = std::env::temp_dir().join("rexenv-ca-test");
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(CA_CERT_FILE);
        let key = dir.join(CA_KEY_FILE);

        let a = load_or_create_at(&cert, &key).unwrap();
        assert!(cert.exists() && key.exists());
        // Second call returns the SAME stored material (not regenerated).
        let b = load_or_create_at(&cert, &key).unwrap();
        assert_eq!(a.cert_pem, b.cert_pem);
        assert_eq!(a.key_pem, b.key_pem);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
