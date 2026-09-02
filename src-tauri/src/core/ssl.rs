//! core::ssl — local certificate authority (Phase 1 §3).
//!
//! Platform-agnostic: generates and loads the root CA material. Trusting the CA
//! in the OS store is the per-OS `CertTrustManager` step (3.3); issuing per-site
//! certs signed by this CA is task 3.2.

use crate::error::{Error, Result};
use crate::platform::traits::{Paths, PermissionManager, Platform};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use std::fs;
use std::path::{Path, PathBuf};

pub const CA_CERT_FILE: &str = "rexenv-ca.pem";
pub const CA_KEY_FILE: &str = "rexenv-ca-key.pem";

pub const SITE_CERT_FILE: &str = "cert.pem";
pub const SITE_KEY_FILE: &str = "key.pem";

/// Leaf (server) cert lifetime in days. Apple/WebKit rejects any TLS *leaf* whose
/// validity exceeds **398 days** (macOS Big Sur+), surfacing as
/// `CSSMERR_TP_CERT_SUSPENDED` — Safari fails to load even when the CA is trusted,
/// while Chrome (which exempts locally-trusted roots) still works. Stay under the
/// cap with margin. The CA *root* has no such lifetime limit.
const LEAF_VALIDITY_DAYS: i64 = 397;

/// Now-anchored leaf validity window (`not_before`, `not_after`), ≤398 days.
/// Anchored to the system clock — fixed calendar dates would eventually fall
/// outside the 398-day window or expire. 1-day back-date absorbs clock skew.
fn leaf_validity_window() -> (time::OffsetDateTime, time::OffsetDateTime) {
    let now = time::OffsetDateTime::now_utc();
    (
        now - time::Duration::days(1),
        now + time::Duration::days(LEAF_VALIDITY_DAYS - 1),
    )
}

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
    // Now-anchored 10-year validity (B13). The old fixed 2024→2034 window was a
    // GLOBAL cliff: a CA minted in late 2033 lived months, and every install
    // died at the same 2034 wall regardless of install date. Anchoring to the
    // clock makes it install+10y (1-day back-date absorbs clock skew, like the
    // leaves). Only reachable for NEW installs — an existing CA on disk is
    // loaded, never regenerated, so its cert (and the keychain trust pinned to
    // it) stays byte-identical. Honest residual: any finite CA still has an
    // end-of-life; leaves (≤398d) can outlive the CA in an install's ninth
    // year — de-globalized and pushed out, not eliminated.
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);

    let key_pair = KeyPair::generate().map_err(|e| Error::Other(format!("ca keygen: {e}")))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| Error::Other(format!("ca self-sign: {e}")))?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

/// Write private-key bytes: with a `PermissionManager` the file is BORN 0600
/// (`write_private` — no world-readable window, B6); without one (`None`, the
/// test/example paths) a plain write. Cert files stay plain writes everywhere —
/// they're public material.
fn write_key(path: &Path, pem: &str, perms: Option<&dyn PermissionManager>) -> Result<()> {
    match perms {
        Some(p) => p.write_private(path, pem.as_bytes()),
        None => Ok(fs::write(path, pem)?),
    }
}

/// Idempotent load-or-create at explicit paths. `perms: Some` creates the key
/// file atomically owner-only (B6); `None` (tests/examples) writes plainly —
/// see [`load_or_create`] for the app-data variant.
pub fn load_or_create_at(
    cert_path: &Path,
    key_path: &Path,
    perms: Option<&dyn PermissionManager>,
) -> Result<LocalCa> {
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
    write_key(key_path, &key_pem, perms)?;
    Ok(LocalCa {
        cert_pem,
        key_pem,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

/// Idempotent CA load-or-create under app-data; the private key file is BORN
/// owner-only at create (B6), and `set_private` still runs on every load as the
/// belt that heals any pre-existing key's perms.
pub fn load_or_create(paths: &dyn Paths, perms: &dyn PermissionManager) -> Result<LocalCa> {
    let dir = ca_dir(paths)?;
    let ca = load_or_create_at(&dir.join(CA_CERT_FILE), &dir.join(CA_KEY_FILE), Some(perms))?;
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
    generate_site_cert_for(ca, domain, &[])
}

/// [`generate_site_cert`] plus EXTRA hostnames (a site's alias domains, v42).
///
/// One certificate for every name a site answers on, rather than one per name:
/// the edge serves them all from the same site block, and a browser that got a
/// cert whose SANs omit the host it asked for shows a full-page interstitial —
/// the most alarming failure this product can produce, over a name the user
/// deliberately added.
pub fn generate_site_cert_for(
    ca: &LocalCa,
    domain: &str,
    extra: &[String],
) -> Result<(String, String)> {
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

    let mut sans = vec![domain.to_string(), format!("*.{domain}")];
    for name in extra {
        sans.push(name.clone());
        sans.push(format!("*.{name}"));
    }
    let mut params =
        CertificateParams::new(sans).map_err(|e| Error::Other(format!("site params: {e}")))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, domain);
    params.distinguished_name = dn;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    // Leaf lifetime must stay ≤398 days or Safari/WebKit rejects it — see LEAF_VALIDITY_DAYS.
    let (not_before, not_after) = leaf_validity_window();
    params.not_before = not_before;
    params.not_after = not_after;

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

/// Idempotent per-site cert issue-or-reuse at explicit paths. `perms: Some`
/// creates the key atomically owner-only (B6); `None` writes plainly.
pub fn ensure_site_cert_at(
    cert_path: &Path,
    key_path: &Path,
    ca: &LocalCa,
    domain: &str,
    perms: Option<&dyn PermissionManager>,
) -> Result<SiteCert> {
    ensure_site_cert_at_for(cert_path, key_path, ca, domain, &[], perms)
}

/// The name of the sidecar recording WHICH hostnames a cached cert covers.
///
/// Existence used to be the whole cache test, which was right while a site had
/// exactly one name. With alias domains it is not: adding one leaves a perfectly
/// valid certificate on disk for the OLD name set, and the browser meets a
/// full-page interstitial on the name the user just added. Reading the SANs back
/// out of the PEM would need a parser; recording what we asked for is one line
/// and cannot disagree with itself.
pub const SITE_SANS_FILE: &str = "sans.txt";

/// [`ensure_site_cert_at`] for a site with extra hostnames. Reissues whenever
/// the covered set differs from what is recorded beside the cert.
pub fn ensure_site_cert_at_for(
    cert_path: &Path,
    key_path: &Path,
    ca: &LocalCa,
    domain: &str,
    extra: &[String],
    perms: Option<&dyn PermissionManager>,
) -> Result<SiteCert> {
    let sans_path = cert_path.with_file_name(SITE_SANS_FILE);
    let want = sans_record(domain, extra);
    let covered = || -> bool {
        match fs::read_to_string(&sans_path) {
            Ok(have) => have.trim() == want.trim(),
            // No sidecar: a cert issued before this existed. Trust it ONLY when
            // there are no extra names to cover — that is exactly the case it
            // was issued for, and reissuing every single-domain site on upgrade
            // would replace certs the user has already been prompted to trust.
            Err(_) => extra.is_empty(),
        }
    };
    if cert_path.exists() && key_path.exists() && covered() {
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
    let (cert_pem, key_pem) = generate_site_cert_for(ca, domain, extra)?;
    fs::write(cert_path, &cert_pem)?;
    write_key(key_path, &key_pem, perms)?;
    // Best-effort: a missing sidecar costs one reissue on the next rebuild,
    // never a wrong cert served.
    let _ = fs::write(&sans_path, &want);
    Ok(SiteCert {
        cert_pem,
        key_pem,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

/// The recorded name set: the primary plus its extras, sorted, one per line.
/// SORTED so a reordering of the alias list is not a reissue — the certificate
/// covers a SET, and treating it as a sequence would churn certs (and the
/// browser trust that goes with them) on nothing.
fn sans_record(domain: &str, extra: &[String]) -> String {
    let mut names: Vec<&str> = std::iter::once(domain).chain(extra.iter().map(String::as_str)).collect();
    names.sort_unstable();
    names.dedup();
    names.join("\n")
}

/// Issue (or reuse) a site cert under app-data; the key file is born 0600 at
/// create (B6), with the per-load `set_private` belt kept.
pub fn ensure_site_cert(
    paths: &dyn Paths,
    perms: &dyn PermissionManager,
    ca: &LocalCa,
    domain: &str,
) -> Result<SiteCert> {
    ensure_site_cert_with(paths, perms, ca, domain, &[])
}

/// [`ensure_site_cert`] covering a site's extra domains too (v42). The cert dir
/// stays keyed on the PRIMARY domain — one site, one directory, whatever else
/// it answers to.
pub fn ensure_site_cert_with(
    paths: &dyn Paths,
    perms: &dyn PermissionManager,
    ca: &LocalCa,
    domain: &str,
    extra: &[String],
) -> Result<SiteCert> {
    let dir = site_cert_dir(paths, domain)?;
    let cert = ensure_site_cert_at_for(
        &dir.join(SITE_CERT_FILE),
        &dir.join(SITE_KEY_FILE),
        ca,
        domain,
        extra,
        Some(perms),
    )?;
    perms.set_private(&cert.key_path)?;
    Ok(cert)
}

/// Always re-issue a site cert at explicit paths, atomically: the new pair is
/// generated and written to temp files first, then renamed over the old ones.
/// A failure at any point leaves the previous cert/key fully intact — NEVER
/// delete-first (a failed issuance after a delete leaves the site cert-less and
/// the Caddyfile pointing at missing files, which breaks the next edge start).
pub fn reissue_site_cert_at(
    cert_path: &Path,
    key_path: &Path,
    ca: &LocalCa,
    domain: &str,
    perms: Option<&dyn PermissionManager>,
) -> Result<SiteCert> {
    let (cert_pem, key_pem) = generate_site_cert(ca, domain)?;
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cert_tmp = cert_path.with_extension("pem.tmp");
    let key_tmp = key_path.with_extension("pem.tmp");
    let write_both = || -> Result<()> {
        fs::write(&cert_tmp, &cert_pem)?;
        // The TEMP key must be born 0600: rename preserves the SOURCE file's
        // perms, so a umask-mode temp renamed over an existing 0600 key would
        // DEGRADE the hardened key until the wrapper's re-harden (B6).
        write_key(&key_tmp, &key_pem, perms)?;
        fs::rename(&key_tmp, key_path)?;
        fs::rename(&cert_tmp, cert_path)?;
        Ok(())
    };
    if let Err(e) = write_both() {
        let _ = fs::remove_file(&cert_tmp);
        let _ = fs::remove_file(&key_tmp);
        return Err(e);
    }
    Ok(SiteCert {
        cert_pem,
        key_pem,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

/// Always re-issue a site's cert under app-data (atomic — see
/// [`reissue_site_cert_at`]); the key file is hardened 0600. Use for the
/// Regenerate actions; first-time issue stays [`ensure_site_cert`].
pub fn reissue_site_cert(
    paths: &dyn Paths,
    perms: &dyn PermissionManager,
    ca: &LocalCa,
    domain: &str,
) -> Result<SiteCert> {
    let dir = site_cert_dir(paths, domain)?;
    let cert = reissue_site_cert_at(
        &dir.join(SITE_CERT_FILE),
        &dir.join(SITE_KEY_FILE),
        ca,
        domain,
        Some(perms),
    )?;
    perms.set_private(&cert.key_path)?;
    Ok(cert)
}

/// Read-only identity of a site's on-disk leaf cert (Settings tab). Dates are
/// RFC 3339 UTC; `days_left` counts whole days until `not_after` (negative once
/// expired). Issue/re-issue stays in [`ensure_site_cert`].
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteCertInfo {
    pub not_before: String,
    pub not_after: String,
    pub days_left: i64,
    pub sans: Vec<String>,
    pub cert_dir: String,
}

/// Parse the site's existing leaf cert under app-data; `None` when no cert has
/// been issued yet (a site gets one at provision, so this is the fresh-DB case).
pub fn site_cert_info(paths: &dyn Paths, domain: &str) -> Result<Option<SiteCertInfo>> {
    let dir = site_cert_dir(paths, domain)?;
    let cert_path = dir.join(SITE_CERT_FILE);
    if !cert_path.exists() {
        return Ok(None);
    }
    parse_cert_info(&fs::read_to_string(&cert_path)?, &dir).map(Some)
}

/// PEM → [`SiteCertInfo`]. Split from the path lookup so tests can feed a PEM
/// directly.
fn parse_cert_info(cert_pem: &str, dir: &Path) -> Result<SiteCertInfo> {
    use x509_parser::extensions::{GeneralName, ParsedExtension};
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
        .map_err(|e| Error::Other(format!("parse cert pem: {e}")))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)
        .map_err(|e| Error::Other(format!("parse cert: {e}")))?;

    let not_before = cert.validity().not_before.to_datetime();
    let not_after = cert.validity().not_after.to_datetime();
    let days_left = (not_after - time::OffsetDateTime::now_utc()).whole_days();

    let mut sans = Vec::new();
    for ext in cert.extensions() {
        if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
            for gn in &san.general_names {
                if let GeneralName::DNSName(d) = gn {
                    sans.push(d.to_string());
                }
            }
        }
    }

    let rfc3339 = &time::format_description::well_known::Rfc3339;
    let fmt = |t: time::OffsetDateTime| {
        t.format(rfc3339)
            .map_err(|e| Error::Other(format!("format cert date: {e}")))
    };
    Ok(SiteCertInfo {
        not_before: fmt(not_before)?,
        not_after: fmt(not_after)?,
        days_left,
        sans,
        cert_dir: dir.display().to_string(),
    })
}

/// Trust the local CA in the user trust store (macOS: login keychain — shows a
/// native auth dialog; no root). Also best-effort makes Firefox honor that
/// trust (its own NSS store ignores the keychain unless its OS-roots import
/// pref is on — see `core::firefox`); Firefox failure never fails the trust.
pub fn trust_ca(platform: &dyn Platform, ca: &LocalCa) -> Result<()> {
    platform.cert_trust().trust_ca(&ca.cert_path)?;
    if let Some(root) = platform.cert_trust().firefox_profiles_root() {
        match crate::core::firefox::enable_in_profiles(&root) {
            Ok(n) if n > 0 => log::info!("ssl: enabled OS-root import in {n} Firefox profile(s)"),
            Ok(_) => {}
            Err(e) => log::warn!("ssl: could not update Firefox profiles: {e}"),
        }
    }
    Ok(())
}

/// Remove the local CA's trust.
pub fn untrust_ca(platform: &dyn Platform, ca: &LocalCa) -> Result<()> {
    platform.cert_trust().untrust_ca(&ca.cert_path)
}

#[cfg(test)]
mod tests {

    /// #154 — **a Firefox failure never fails the trust.**
    ///
    /// The trust that matters is the OS one: `cert_trust().trust_ca` shows the
    /// user a native auth dialog and, when they approve it, every ordinary
    /// browser on the machine trusts rexenv's CA. The Firefox step is a
    /// courtesy on top — Firefox keeps its own NSS store and ignores the
    /// keychain unless a pref is set — and it touches profile directories that
    /// may be missing, locked by a running Firefox, or shaped in a way this
    /// build has never seen.
    ///
    /// So its failure must not propagate: `?` on that call would turn "your
    /// Firefox profile is unusual" into "rexenv could not trust its CA", and
    /// the user would be told HTTPS is broken while every other browser on
    /// their machine already works. The OS trust, by contrast, uses `?` on
    /// purpose — if THAT fails there is nothing to be optimistic about.
    #[test]
    fn a_firefox_failure_never_fails_the_trust() {
        let src = crate::core::copy_scan::production_source(include_str!("ssl.rs"));
        let body = src
            .split("pub fn trust_ca(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("trust_ca");

        // The OS trust is the one that may fail the call.
        assert!(
            body.contains("cert_trust().trust_ca(&ca.cert_path)?"),
            "the OS trust is no longer propagating its error — if the keychain refuses, there \
             is nothing to be optimistic about and the caller must hear it"
        );
        // The Firefox step is handled, not propagated.
        let firefox = body
            .find("firefox::enable_in_profiles(")
            .expect("the Firefox courtesy step is gone — if it moved, move this guard with it");
        let call_line = body[firefox..].lines().next().unwrap_or_default();
        assert!(
            !call_line.contains('?'),
            "the Firefox step propagates its error: an unusual or locked profile would then be \
             reported as \"rexenv could not trust its CA\", while every other browser on the \
             machine already trusts it:\n    {}",
            call_line.trim()
        );
        assert!(
            body[firefox..].contains("Err(e) => log::warn!"),
            "the Firefox failure is not even logged — a courtesy that fails silently is one \
             nobody can debug when Firefox alone shows a warning"
        );
        // And the function still returns Ok after it: a `return Err` in that
        // arm would be the same defect wearing a different shape.
        let after = &body[firefox..];
        assert!(
            !after.contains("return Err("),
            "the Firefox arm returns an error — same defect as `?`, different spelling"
        );
    }
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
    fn ca_validity_is_now_anchored_ten_years_not_a_fixed_wall() {
        // B13: the window must be install-relative (now-1d .. now+10y), killing
        // any regression to the fixed 2024→2034 dates by construction — a fixed
        // wall would fail these bounds as soon as the clock moved.
        let (cert_pem, _) = generate_ca().unwrap();
        let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes()).unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).unwrap();
        let now = time::OffsetDateTime::now_utc();
        let nb = cert.validity().not_before.to_datetime();
        let na = cert.validity().not_after.to_datetime();
        assert!(nb <= now, "not_before is back-dated for clock skew");
        assert!(now - nb < time::Duration::days(2), "back-date is ~1 day, not a fixed 2024");
        let days_out = (na - now).whole_days();
        assert!(
            (3648..=3651).contains(&days_out),
            "expires ~10y from NOW (install-relative), got {days_out} days"
        );
    }

    /// Mode bits (0o777 mask) of a path — unix-only test helper.
    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn keys_are_born_owner_only_without_the_wrapper_chmod() {
        // With a PermissionManager the key file must be 0600 STRAIGHT from the
        // _at fn — no wrapper set_private has run — proving there is no
        // world-readable window between write and harden (B6).
        let plat = crate::platform::current();
        let perms = plat.permissions();
        let dir = std::env::temp_dir().join(format!("rexenv-b6-born-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let ca =
            load_or_create_at(&dir.join(CA_CERT_FILE), &dir.join(CA_KEY_FILE), Some(perms)).unwrap();
        assert_eq!(mode_of(&ca.key_path), 0o600, "CA key born owner-only");

        let cert = ensure_site_cert_at(
            &dir.join(SITE_CERT_FILE),
            &dir.join(SITE_KEY_FILE),
            &ca,
            "born.test",
            Some(perms),
        )
        .unwrap();
        assert_eq!(mode_of(&cert.key_path), 0o600, "site key born owner-only");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn reissue_over_a_hardened_key_keeps_it_owner_only() {
        // The degradation fix (B6): the OLD path wrote the temp key at umask and
        // renamed it over the existing 0600 key — rename preserves the SOURCE's
        // perms, so a hardened key went 0644 until the wrapper re-hardened it.
        // With the temp born 0600, the key must be 0600 the moment the rename
        // lands, with no wrapper chmod involved.
        let plat = crate::platform::current();
        let perms = plat.permissions();
        let dir = std::env::temp_dir().join(format!("rexenv-b6-reissue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(SITE_CERT_FILE);
        let key = dir.join(SITE_KEY_FILE);

        let ca = load_or_create_at(&dir.join(CA_CERT_FILE), &dir.join(CA_KEY_FILE), Some(perms)).unwrap();
        ensure_site_cert_at(&cert, &key, &ca, "re.test", Some(perms)).unwrap();
        assert_eq!(mode_of(&key), 0o600, "precondition: hardened key");

        reissue_site_cert_at(&cert, &key, &ca, "re.test", Some(perms)).unwrap();
        assert_eq!(mode_of(&key), 0o600, "reissued key stays owner-only after the rename");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_create_is_idempotent() {
        let dir = std::env::temp_dir().join("rexenv-ca-test");
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(CA_CERT_FILE);
        let key = dir.join(CA_KEY_FILE);

        let a = load_or_create_at(&cert, &key, None).unwrap();
        assert!(cert.exists() && key.exists());
        // Second call returns the SAME stored material (not regenerated).
        let b = load_or_create_at(&cert, &key, None).unwrap();
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
    fn parse_cert_info_reads_dates_sans_and_days_left() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let (cert_pem, _) = generate_site_cert(&ca, "info.test").unwrap();

        let dir = PathBuf::from("/tmp/certs/info.test");
        let info = parse_cert_info(&cert_pem, &dir).unwrap();

        assert!(info.sans.contains(&"info.test".to_string()), "sans: {:?}", info.sans);
        assert!(info.sans.contains(&"*.info.test".to_string()), "sans: {:?}", info.sans);
        assert_eq!(info.cert_dir, dir.display().to_string());
        // Freshly issued: not_after is LEAF_VALIDITY_DAYS-1 out (1-day back-date),
        // so whole days left is that ±1 for the sub-day remainder.
        assert!(
            (LEAF_VALIDITY_DAYS - 3..=LEAF_VALIDITY_DAYS).contains(&info.days_left),
            "days_left: {}",
            info.days_left
        );
        // RFC 3339 round-trip proves the format the frontend will Date-parse.
        let rfc = &time::format_description::well_known::Rfc3339;
        let nb = time::OffsetDateTime::parse(&info.not_before, rfc).unwrap();
        let na = time::OffsetDateTime::parse(&info.not_after, rfc).unwrap();
        assert!(na > nb);
    }

    #[test]
    fn site_cert_validity_stays_under_safari_398_day_cap() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let (cert_pem, _) = generate_site_cert(&ca, "mysite.test").unwrap();
        let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes()).unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).unwrap();
        let v = cert.validity();
        let span_days = (v.not_after.timestamp() - v.not_before.timestamp()) / 86_400;
        // Apple/WebKit rejects leaves > 398 days (CSSMERR_TP_CERT_SUSPENDED).
        assert!(span_days <= 398, "leaf validity {span_days} days exceeds Safari's 398-day cap");
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

        let a = ensure_site_cert_at(&cert, &key, &ca, "mysite.test", None).unwrap();
        let b = ensure_site_cert_at(&cert, &key, &ca, "mysite.test", None).unwrap();
        // Reused, not re-issued.
        assert_eq!(a.cert_pem, b.cert_pem);
        assert_eq!(a.key_pem, b.key_pem);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Adding an extra domain REISSUES the certificate; reordering does not.**
    ///
    /// The cache test used to be "the files exist", which was right while a site
    /// had exactly one name. With alias domains it is not: a perfectly valid
    /// certificate for the OLD name set stays on disk, and the browser meets a
    /// full-page interstitial on the name the user just added — the loudest
    /// failure this product can produce, over a deliberate action. The recorded
    /// name set is SORTED, so a reordered alias list is not a reissue: the cert
    /// covers a set, and churning certs (and the trust the user has already
    /// granted them) over sequence would be its own bug.
    #[test]
    fn a_new_extra_domain_reissues_the_cert_and_a_reorder_does_not() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let dir = std::env::temp_dir().join(format!("rexenv-sans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(SITE_CERT_FILE);
        let key = dir.join(SITE_KEY_FILE);

        let one = ensure_site_cert_at_for(&cert, &key, &ca, "mysite.test", &[], None).unwrap();
        // Same names → reused.
        let again = ensure_site_cert_at_for(&cert, &key, &ca, "mysite.test", &[], None).unwrap();
        assert_eq!(one.cert_pem, again.cert_pem, "an unchanged name set must not reissue");

        // A new name → a NEW certificate that actually covers it.
        let extra = vec!["shop.test".to_string()];
        let two = ensure_site_cert_at_for(&cert, &key, &ca, "mysite.test", &extra, None).unwrap();
        assert_ne!(
            one.cert_pem, two.cert_pem,
            "the cert was reused after an extra domain was added — the browser gets an \
             interstitial on the name the user just added"
        );
        let info = parse_cert_info(&two.cert_pem, &dir).expect("parse the reissued cert");
        for name in ["mysite.test", "*.mysite.test", "shop.test", "*.shop.test"] {
            assert!(
                info.sans.iter().any(|s| s == name),
                "the reissued cert does not cover {name}: {:?}",
                info.sans
            );
        }

        // Order is not identity.
        let reordered = vec!["shop.test".to_string()];
        let three =
            ensure_site_cert_at_for(&cert, &key, &ca, "mysite.test", &reordered, None).unwrap();
        assert_eq!(two.cert_pem, three.cert_pem, "the same SET must not reissue");

        // Removing the extra reissues too — a cert that still advertises a name
        // the site no longer answers on is a claim we stopped being able to keep.
        let back = ensure_site_cert_at_for(&cert, &key, &ca, "mysite.test", &[], None).unwrap();
        assert_ne!(two.cert_pem, back.cert_pem, "dropping a name must reissue");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reissue_site_cert_replaces_material_and_leaves_no_temp_files() {
        let (ca_cert, ca_key) = generate_ca().unwrap();
        let ca = LocalCa {
            cert_pem: ca_cert,
            key_pem: ca_key,
            cert_path: PathBuf::new(),
            key_path: PathBuf::new(),
        };
        let dir = std::env::temp_dir().join("rexenv-reissue-test");
        let _ = std::fs::remove_dir_all(&dir);
        let cert = dir.join(SITE_CERT_FILE);
        let key = dir.join(SITE_KEY_FILE);

        // Works with no pre-existing files (deleted/corrupted-cert recovery)…
        let a = reissue_site_cert_at(&cert, &key, &ca, "mysite.test", None).unwrap();
        assert!(cert.exists() && key.exists());
        // …and ALWAYS re-issues over an existing pair (unlike ensure_site_cert).
        let b = reissue_site_cert_at(&cert, &key, &ca, "mysite.test", None).unwrap();
        assert_ne!(a.cert_pem, b.cert_pem);
        assert_ne!(a.key_pem, b.key_pem);
        // On-disk pair is the NEW material (rename landed), still a valid signed leaf.
        assert_eq!(std::fs::read_to_string(&cert).unwrap(), b.cert_pem);
        assert_eq!(std::fs::read_to_string(&key).unwrap(), b.key_pem);
        assert_eq!(issuer_cn(&b.cert_pem), "rexenv Local CA");
        assert!(dns_sans(&b.cert_pem).contains(&"*.mysite.test".to_string()));
        // Atomic path cleaned up: no .tmp files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
