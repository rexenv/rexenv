//! macOS: trusting rexenv's local CA from THIS process, so the keychain dialog
//! names rexenv.
//!
//! # Why not `security add-trusted-cert`
//!
//! The trust dialog is drawn by SecurityAgent and titled after the process that
//! called the trust-settings API. Through `/usr/bin/security` that process is
//! `security`: the dialog read "security" over a plain lock, beside an admin
//! dialog that already said "rexenv" (#577). Measured on macOS 26.6.2, 12 Sep
//! 2026, each dialog cancelled by the owner:
//!
//! - `security add-trusted-cert` from a terminal → titled "security";
//! - the same command run inside the rexenv-named applet that fixes the admin
//!   dialog → still "security" (the caller of the API names it, not its parent);
//! - a bundle named rexenv calling `SecTrustSettingsSetTrustSettings` itself →
//!   "rexenv" with rexenv's logo.
//!
//! So the calls are made here, in-process. The packaged app is `rexenv.app`, so
//! its dialog carries the name and the icon; an unbundled `cargo run` gets the
//! executable's name and a generic icon.
//!
//! A dismissed dialog returns `errAuthorizationCanceled` (-60006) — measured, the
//! same run.

use crate::error::{Error, Result};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;

type OSStatus = i32;
type CFTypeRef = *const c_void;

/// `errAuthorizationCanceled`: what a dismissed trust dialog returns (measured).
const ERR_AUTHORIZATION_CANCELED: OSStatus = -60006;
/// `errSecUserCanceled`, the other spelling of a cancel the Security APIs use.
const ERR_SEC_USER_CANCELED: OSStatus = -128;
/// `errSecDuplicateItem`: the CA is already in the keychain — the ordinary re-trust.
const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;
/// `errSecItemNotFound`: no trust setting to remove for this certificate.
const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
/// `kSecTrustSettingsDomainUser`: this user's trust settings. Never the admin or
/// system domain — those need root, and trust is per-user by design.
const TRUST_DOMAIN_USER: u32 = 0;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFStringGetCString(s: CFTypeRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCertificateCreateWithData(allocator: CFTypeRef, data: CFTypeRef) -> CFTypeRef;
    fn SecKeychainOpen(path: *const c_char, keychain: *mut CFTypeRef) -> OSStatus;
    fn SecCertificateAddToKeychain(certificate: CFTypeRef, keychain: CFTypeRef) -> OSStatus;
    fn SecTrustSettingsSetTrustSettings(certificate: CFTypeRef, domain: u32, settings: CFTypeRef) -> OSStatus;
    fn SecTrustSettingsRemoveTrustSettings(certificate: CFTypeRef, domain: u32) -> OSStatus;
    fn SecCopyErrorMessageString(status: OSStatus, reserved: *const c_void) -> CFTypeRef;
}

/// A Core Foundation object we own, released once.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: every `Owned` holds a +1 reference from a Create/Copy call.
            unsafe { CFRelease(self.0) }
        }
    }
}

/// The DER bytes of the first CERTIFICATE block in `pem`.
pub(super) fn der_from_pem(pem: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let body = pem
        .split("-----BEGIN CERTIFICATE-----")
        .nth(1)
        .and_then(|rest| rest.split("-----END CERTIFICATE-----").next())
        .ok_or_else(|| Error::Other("the CA file holds no PEM certificate".into()))?;
    let b64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| Error::Other(format!("the CA file's certificate is not valid base64: {e}")))
}

/// A `SecCertificate` for the PEM file at `path`. No dialog.
fn certificate(path: &Path) -> Result<Owned> {
    certificate_from_der(&der_from_pem(&std::fs::read_to_string(path)?)?)
}

/// A `SecCertificate` for DER bytes. No dialog.
fn certificate_from_der(der: &[u8]) -> Result<Owned> {
    let len = isize::try_from(der.len()).map_err(|_| Error::Other("the CA certificate is too large".into()))?;
    // SAFETY: `der` outlives the call; CFDataCreate copies the bytes.
    let data = Owned(unsafe { CFDataCreate(std::ptr::null(), der.as_ptr(), len) });
    if data.0.is_null() {
        return Err(Error::Other("could not allocate the CA certificate's data".into()));
    }
    // SAFETY: `data.0` is a live CFData; a NULL return means "not a certificate".
    let cert = Owned(unsafe { SecCertificateCreateWithData(std::ptr::null(), data.0) });
    if cert.0.is_null() {
        return Err(Error::Other("the CA file is not a certificate macOS can read".into()));
    }
    Ok(cert)
}

/// The Security framework's own sentence for `status`.
fn status_message(status: OSStatus) -> String {
    // SAFETY: a NULL return is handled; the buffer is sized and NUL-terminated by CF.
    unsafe {
        let s = Owned(SecCopyErrorMessageString(status, std::ptr::null()));
        let mut buf = [0 as c_char; 512];
        if !s.0.is_null() && CFStringGetCString(s.0, buf.as_mut_ptr(), buf.len() as isize, CF_STRING_ENCODING_UTF8) != 0
        {
            return CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
        }
    }
    format!("OSStatus {status}")
}

/// The error a failed step reads as. A dismissed dialog is a recoverable
/// cancel, never a raw status code.
pub(super) fn trust_error(action: &str, status: OSStatus, message: &str) -> Error {
    if status == ERR_AUTHORIZATION_CANCELED || status == ERR_SEC_USER_CANCELED {
        Error::Other(
            "Keychain permission was cancelled — rexenv's local certificate authority needs it for HTTPS. \
             Try again and approve the prompt."
                .into(),
        )
    } else {
        Error::Other(format!("could not {action} rexenv's local certificate authority: {message} ({status})"))
    }
}

/// Put the CA into `login_keychain` (no dialog) and trust it as a root for this
/// user (the dialog).
pub(super) fn trust(ca_cert_path: &Path, login_keychain: &str) -> Result<()> {
    let cert = certificate(ca_cert_path)?;
    let path = CString::new(login_keychain)
        .map_err(|_| Error::Other("the login keychain path contains a NUL byte".into()))?;
    let mut keychain: CFTypeRef = std::ptr::null();
    // SAFETY: `path` is NUL-terminated and outlives the call; the out-pointer is ours.
    let opened = unsafe { SecKeychainOpen(path.as_ptr(), &mut keychain) };
    let keychain = Owned(keychain);
    if opened != 0 {
        return Err(trust_error("open the login keychain for", opened, &status_message(opened)));
    }
    // SAFETY: both references are live for the call.
    let added = unsafe { SecCertificateAddToKeychain(cert.0, keychain.0) };
    if added != 0 && added != ERR_SEC_DUPLICATE_ITEM {
        return Err(trust_error("add to the login keychain", added, &status_message(added)));
    }
    // SAFETY: `cert.0` is live; NULL settings = always trust this root.
    let trusted = unsafe { SecTrustSettingsSetTrustSettings(cert.0, TRUST_DOMAIN_USER, std::ptr::null()) };
    if trusted != 0 {
        return Err(trust_error("trust", trusted, &status_message(trusted)));
    }
    Ok(())
}

/// Remove this user's trust setting for the CA (the dialog), then delete the CA from the login
/// keychain. Removing the trust alone left an inert `rexenv Local CA` item behind after "Remove
/// system changes", and the cask's `--zap` cannot reach a keychain (the 15.8 VM, 9 Oct 2026,
/// #813) — on Windows and Linux the same call already takes the certificate out of the store.
pub(super) fn untrust(ca_cert_path: &Path, login_keychain: &str) -> Result<()> {
    let cert = certificate(ca_cert_path)?;
    // SAFETY: `cert.0` is live for the call.
    let removed = unsafe { SecTrustSettingsRemoveTrustSettings(cert.0, TRUST_DOMAIN_USER) };
    // `errSecItemNotFound`: no trust to remove — the item an older rexenv's untrust left behind.
    // Still ours to delete.
    if removed != 0 && removed != ERR_SEC_ITEM_NOT_FOUND {
        return Err(trust_error("remove the trust of", removed, &status_message(removed)));
    }
    let current = der_from_pem(&std::fs::read_to_string(ca_cert_path)?)?;
    for sha1 in current_in_listing(&ca_listing(login_keychain)?, &current) {
        // Enumeration and deletion raise no dialog (as in `untrust_stale`).
        let out = std::process::Command::new("security")
            .args(["delete-certificate", "-Z", &sha1])
            .arg(login_keychain)
            .output()?;
        if !out.status.success() {
            return Err(Error::Other(format!(
                "rexenv removed the trust of its local certificate authority but could not delete it \
                 from your login keychain ({}). Delete \"{}\" in Keychain Access.",
                String::from_utf8_lossy(&out.stderr).trim(),
                crate::core::ssl::CA_COMMON_NAME
            )));
        }
    }
    Ok(())
}

/// Every rexenv CA in the login keychain, as `security find-certificate -a -c <cn> -Z -p` lists
/// them. No match exits non-zero with an empty listing — nothing to delete, not an error.
fn ca_listing(login_keychain: &str) -> Result<String> {
    let out = std::process::Command::new("security")
        .args(["find-certificate", "-a", "-c", crate::core::ssl::CA_COMMON_NAME, "-Z", "-p"])
        .arg(login_keychain)
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The SHA-1 of every copy of `current_der` in a listing — what `untrust` deletes. Pure, like
/// [`stale_in_listing`], so the shape of the listing is tested without a keychain.
pub(super) fn current_in_listing(listing: &str, current_der: &[u8]) -> Vec<String> {
    listed_certs(listing).into_iter().filter(|c| c.der == current_der).map(|c| c.sha1).collect()
}

/// One certificate as `security find-certificate -a -Z -p` lists it: its SHA-1
/// (the handle `security delete-certificate -Z` takes) and its DER.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ListedCert {
    pub sha1: String,
    pub der: Vec<u8>,
}

/// The rexenv CAs in a `find-certificate -a -c <cn> -Z -p` listing that are
/// NOT `current_der` — the stale ones. Pure, so the shape of the listing is
/// tested without a keychain.
pub(super) fn stale_in_listing(listing: &str, current_der: &[u8]) -> Vec<ListedCert> {
    listed_certs(listing).into_iter().filter(|c| c.der != current_der).collect()
}

/// Every certificate in a `find-certificate -a -Z -p` listing, paired with its SHA-1.
fn listed_certs(listing: &str) -> Vec<ListedCert> {
    let mut out = Vec::new();
    let mut sha1: Option<String> = None;
    let mut pem = String::new();
    let mut in_pem = false;
    for line in listing.lines() {
        if let Some(h) = line.strip_prefix("SHA-1 hash: ") {
            sha1 = Some(h.trim().to_string());
        }
        if line.starts_with("-----BEGIN CERTIFICATE-----") {
            in_pem = true;
            pem.clear();
        }
        if in_pem {
            pem.push_str(line);
            pem.push('\n');
        }
        if line.starts_with("-----END CERTIFICATE-----") {
            in_pem = false;
            if let (Some(h), Ok(der)) = (sha1.take(), der_from_pem(&pem)) {
                out.push(ListedCert { sha1: h, der });
            }
        }
    }
    out
}

/// Untrust and delete every rexenv CA in the login keychain except the one at
/// `current_ca` (#678). Each removal is the trust dialog — in practice one
/// approval, because SecurityAgent keeps the right warm for a short while and
/// this runs right after `trust`/`untrust` of the current CA. A cancel stops
/// the sweep and reads as a cancel; what was already swept stays swept.
///
/// Enumeration and deletion go through `/usr/bin/security` (no dialog for
/// either); only the trust change is made in-process, for the same reason
/// `trust` is: the dialog is titled after the caller.
pub(super) fn untrust_stale(current_ca: &Path, login_keychain: &str) -> Result<usize> {
    let current = der_from_pem(&std::fs::read_to_string(current_ca)?)?;
    let listing = ca_listing(login_keychain)?;
    let mut swept = 0;
    for stale in stale_in_listing(&listing, &current) {
        let cert = certificate_from_der(&stale.der)?;
        // SAFETY: `cert.0` is live for the call.
        let removed = unsafe { SecTrustSettingsRemoveTrustSettings(cert.0, TRUST_DOMAIN_USER) };
        // `errSecItemNotFound`: it was in the keychain but never trusted (or
        // already untrusted) — nothing to remove, still ours to delete.
        if removed != 0 && removed != ERR_SEC_ITEM_NOT_FOUND {
            return Err(trust_error("remove the trust of a stale copy of", removed, &status_message(removed)));
        }
        // Best-effort: a cert that will not delete is untrusted litter, not a failure.
        let _ = std::process::Command::new("security")
            .args(["delete-certificate", "-Z", &stale.sha1])
            .arg(login_keychain)
            .output();
        swept += 1;
    }
    Ok(swept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #678 — the listing parser keeps every rexenv CA that is not the current
    /// one, pairs each with the SHA-1 `delete-certificate -Z` needs, and never
    /// lists the current CA as stale.
    #[test]
    fn the_stale_sweep_names_every_ca_but_the_current_one() {
        let (a_pem, a_der) = ca_pem();
        let (b_pem, b_der) = ca_pem();
        let (c_pem, _) = ca_pem();
        let listing = format!(
            "SHA-256 hash: AA\nSHA-1 hash: 1111\n{a_pem}SHA-256 hash: BB\nSHA-1 hash: 2222\n{b_pem}SHA-1 hash: 3333\n{c_pem}"
        );
        let stale = stale_in_listing(&listing, &b_der);
        assert_eq!(stale.len(), 2, "{stale:?}");
        assert_eq!((stale[0].sha1.as_str(), &stale[0].der), ("1111", &a_der));
        assert_eq!(stale[1].sha1, "3333");
        assert!(stale.iter().all(|s| s.der != b_der), "the current CA is never stale");
        assert!(stale_in_listing("", &b_der).is_empty(), "no match, nothing to sweep");
        assert!(stale_in_listing(&format!("SHA-1 hash: 2222\n{b_pem}"), &b_der).is_empty());
    }

    /// #813 — `untrust` deletes exactly the current CA's copies (every one), never another CA.
    #[test]
    fn untrust_deletes_every_copy_of_the_current_ca_and_nothing_else() {
        let (a_pem, _) = ca_pem();
        let (b_pem, b_der) = ca_pem();
        let listing = format!("SHA-1 hash: 1111\n{a_pem}SHA-1 hash: 2222\n{b_pem}SHA-1 hash: 4444\n{b_pem}");
        assert_eq!(current_in_listing(&listing, &b_der), vec!["2222".to_string(), "4444".to_string()]);
        assert!(current_in_listing(&format!("SHA-1 hash: 1111\n{a_pem}"), &b_der).is_empty());
        assert!(current_in_listing("", &b_der).is_empty(), "no match, nothing to delete");
        // TEXT: the deletion runs for that list, and a never-trusted leftover is not an error.
        let src = crate::core::copy_scan::production_source(include_str!("keychain_trust.rs"));
        let body = &src[src.find("pub(super) fn untrust(").expect("untrust")..];
        let body = &body[..body.find("\n}\n").expect("its end")];
        assert!(body.contains("current_in_listing(&ca_listing(login_keychain)?, &current)"), "untrust lists the current CA");
        assert!(body.contains("\"delete-certificate\", \"-Z\", &sha1"), "and deletes each copy");
        assert!(body.contains("removed != ERR_SEC_ITEM_NOT_FOUND"), "an already-untrusted item is still deleted");
    }

    fn ca_pem() -> (String, Vec<u8>) {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name.push(rcgen::DnType::CommonName, "keychain_trust test CA");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), cert.der().to_vec())
    }

    #[test]
    fn a_pem_certificate_decodes_to_exactly_its_der_bytes() {
        let (pem, der) = ca_pem();
        assert_eq!(der_from_pem(&pem).unwrap(), der);
        // A key file, or anything else, is refused rather than guessed at.
        assert!(der_from_pem("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----").is_err());
        assert!(der_from_pem("-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----").is_err());
    }

    /// The bytes reach the Security framework as a certificate — the half of
    /// `trust` that raises no dialog. A file that is not one is refused there.
    #[test]
    fn the_security_framework_reads_our_ca_as_a_certificate() {
        let dir = std::env::temp_dir().join(format!("rexenv-keychain-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("ca.pem");
        std::fs::write(&good, ca_pem().0).unwrap();
        assert!(certificate(&good).is_ok());
        let bad = dir.join("bad.pem");
        std::fs::write(&bad, "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n").unwrap();
        assert!(certificate(&bad).is_err(), "four zero bytes are not a certificate");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_dismissed_dialog_reads_as_a_cancel_never_a_status_code() {
        for code in [ERR_AUTHORIZATION_CANCELED, ERR_SEC_USER_CANCELED] {
            let m = trust_error("trust", code, "The authorization was canceled by the user.").to_string();
            assert!(m.contains("cancelled") && m.contains("Try again"), "{m}");
            assert!(!m.contains(&code.to_string()), "{m}");
        }
        let m = trust_error("trust", -25293, "The user name or passphrase you entered is not correct.").to_string();
        assert!(m.contains("could not trust") && m.contains("passphrase") && m.contains("-25293"), "{m}");
    }
}
