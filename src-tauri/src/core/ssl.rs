//! core::ssl — local certificate authority (Phase 1 §3).
//!
//! Platform-agnostic: generates and loads the root CA material. Trusting the CA
//! in the OS store is the per-OS `CertTrustManager` step (3.3); issuing per-site
//! certs signed by this CA is task 3.2.

use crate::error::{Error, Result};
use crate::platform::traits::{Paths, PermissionManager, Platform};
use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use std::fs;
use std::path::{Path, PathBuf};

pub const CA_CERT_FILE: &str = "rexenv-ca.pem";
pub const CA_KEY_FILE: &str = "rexenv-ca-key.pem";

pub const SITE_CERT_FILE: &str = "cert.pem";
pub const SITE_KEY_FILE: &str = "key.pem";

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

/// A signed per-site leaf certificate (PEM) plus its on-disk locations.
#[derive(Debug, Clone)]
pub struct SiteCert {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Generate a per-site leaf cert for `domain`, signed by `ca`, with SANs
/// `domain` + `*.domain` (the wildcard makes subdomain multisite work). Pure — no fs.
pub fn generate_site_cert(ca: &LocalCa, domain: &str) -> Result<(String, String)> {
    // Reconstruct the CA as an issuer from the stored PEM. self_signed rebuilds a
    // Certificate carrying the CA's DN/key-usages; combined with the CA key it
    // signs leaves that chain to the trusted CA (same key ⇒ same SPKI/AKI).
    let ca_params = CertificateParams::from_ca_cert_pem(&ca.cert_pem)
        .map_err(|e| Error::Other(format!("load ca cert: {e}")))?;
    let ca_key =
        KeyPair::from_pem(&ca.key_pem).map_err(|e| Error::Other(format!("load ca key: {e}")))?;
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .map_err(|e| Error::Other(format!("rebuild ca cert: {e}")))?;

    let sans = vec![domain.to_string(), format!("*.{domain}")];
    let mut params =
        CertificateParams::new(sans).map_err(|e| Error::Other(format!("site params: {e}")))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, domain);
    params.distinguished_name = dn;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = date_time_ymd(2024, 1, 1);
    params.not_after = date_time_ymd(2034, 1, 1);

    let site_key = KeyPair::generate().map_err(|e| Error::Other(format!("site keygen: {e}")))?;
    let cert = params
        .signed_by(&site_key, &ca_cert, &ca_key)
        .map_err(|e| Error::Other(format!("sign site cert: {e}")))?;
    Ok((cert.pem(), site_key.serialize_pem()))
}

/// Directory holding a site's cert material under app-data.
pub fn site_cert_dir(paths: &dyn Paths, domain: &str) -> Result<PathBuf> {
    Ok(paths.app_data_dir()?.join("certs").join(domain))
}

/// Idempotent per-site cert issue-or-reuse at explicit paths (no OS perms).
pub fn ensure_site_cert_at(
    cert_path: &Path,
    key_path: &Path,
    ca: &LocalCa,
    domain: &str,
) -> Result<SiteCert> {
    if cert_path.exists() && key_path.exists() {
        return Ok(SiteCert {
            cert_pem: fs::read_to_string(cert_path)?,
            key_pem: fs::read_to_string(key_path)?,
            cert_path: cert_path.to_path_buf(),
            key_path: key_path.to_path_buf(),
        });
    }
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let (cert_pem, key_pem) = generate_site_cert(ca, domain)?;
    fs::write(cert_path, &cert_pem)?;
    fs::write(key_path, &key_pem)?;
    Ok(SiteCert {
        cert_pem,
        key_pem,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

/// Issue (or reuse) a site cert under app-data; the key file is hardened 0600.
pub fn ensure_site_cert(
    paths: &dyn Paths,
    perms: &dyn PermissionManager,
    ca: &LocalCa,
    domain: &str,
) -> Result<SiteCert> {
    let dir = site_cert_dir(paths, domain)?;
    let cert = ensure_site_cert_at(&dir.join(SITE_CERT_FILE), &dir.join(SITE_KEY_FILE), ca, domain)?;
    perms.set_private(&cert.key_path)?;
    Ok(cert)
}

/// Trust the local CA in the user trust store (macOS: login keychain — shows a
/// native auth dialog; no root).
pub fn trust_ca(platform: &dyn Platform, ca: &LocalCa) -> Result<()> {
    platform.cert_trust().trust_ca(&ca.cert_path)
}

/// Remove the local CA's trust.
pub fn untrust_ca(platform: &dyn Platform, ca: &LocalCa) -> Result<()> {
    platform.cert_trust().untrust_ca(&ca.cert_path)
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

    /// Collect the DNS SAN entries from a PEM cert.
    fn dns_sans(cert_pem: &str) -> Vec<String> {
        use x509_parser::extensions::{GeneralName, ParsedExtension};
        let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes()).unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).unwrap();
        let mut out = Vec::new();
        for ext in cert.extensions() {
            if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
                for gn in &san.general_names {
                    if let GeneralName::DNSName(d) = gn {
                        out.push(d.to_string());
                    }
                }
            }
        }
        out
    }

    fn issuer_cn(cert_pem: &str) -> String {
        let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes()).unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).unwrap();
        let cn = cert
            .issuer()
            .iter_common_name()
            .next()
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        cn
    }

    #[test]
    fn site_cert_has_wildcard_san_and_ca_issuer() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let (cert_pem, key_pem) = generate_site_cert(&ca, "mysite.test").unwrap();
        assert!(key_pem.contains("PRIVATE KEY"));

        let sans = dns_sans(&cert_pem);
        assert!(sans.contains(&"mysite.test".to_string()), "sans: {sans:?}");
        assert!(sans.contains(&"*.mysite.test".to_string()), "sans: {sans:?}");
        // Signed by our CA.
        assert_eq!(issuer_cn(&cert_pem), "rexenv Local CA");
    }

    #[test]
    fn ensure_site_cert_is_idempotent() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let dir = std::env::temp_dir().join("rexenv-sitecert-test");
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(SITE_CERT_FILE);
        let key = dir.join(SITE_KEY_FILE);

        let a = ensure_site_cert_at(&cert, &key, &ca, "mysite.test").unwrap();
        let b = ensure_site_cert_at(&cert, &key, &ca, "mysite.test").unwrap();
        // Reused, not re-issued.
        assert_eq!(a.cert_pem, b.cert_pem);
        assert_eq!(a.key_pem, b.key_pem);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
