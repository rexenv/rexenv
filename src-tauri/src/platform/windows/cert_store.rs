//! Windows: rexenv's local CA in THIS user's Root certificate store (ledger #613).
//!
//! The CurrentUser `Root` store, never LocalMachine's: trust is per-user by design, as on macOS
//! (login keychain), and the machine store needs elevation. Adding a certificate to that store makes
//! Windows itself ask the user — "Security Warning: You are about to install a certificate from a
//! certification authority…" — and removing one asks too; the store call waits on the answer, which
//! is why its callers wait through `core::prompt::while_prompting`. A No reads as a cancel
//! (`cert_rules::trust_error`).
//!
//! `is_trusted` looks the certificate up in the LOGICAL CurrentUser Root store, which also shows the
//! machine's roots — so a CA an administrator trusted machine-wide counts, as Windows' own chain
//! building would. No prompt.

use super::cert_rules;
use crate::error::{Error, Result};
use std::ffi::c_void;
use std::path::Path;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Security::Cryptography::{
    CertAddEncodedCertificateToStore, CertCloseStore, CertCreateCertificateContext, CertDeleteCertificateFromStore,
    CertFindCertificateInStore, CertFreeCertificateContext, CertOpenStore, CERT_CONTEXT, CERT_FIND_EXISTING,
    CERT_FIND_SUBJECT_STR_W,
    CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_USE_EXISTING, CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W,
    CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};

const ENCODING: CERT_QUERY_ENCODING_TYPE = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;

/// An open certificate store, closed once.
struct Store(HCERTSTORE);

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: opened by `open_root`, closed exactly once.
        unsafe { CertCloseStore(self.0, 0) };
    }
}

/// A certificate context we own, freed once.
struct Context(*mut CERT_CONTEXT);

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: returned by a Create/Find call, freed exactly once.
        unsafe { CertFreeCertificateContext(self.0) };
    }
}

fn open_root(read_only: bool) -> Result<Store> {
    let name: Vec<u16> = "Root".encode_utf16().chain(Some(0)).collect();
    let flags = CERT_SYSTEM_STORE_CURRENT_USER
        | CERT_STORE_OPEN_EXISTING_FLAG
        | if read_only { CERT_STORE_READONLY_FLAG } else { 0 };
    // SAFETY: `name` is NUL-terminated and outlives the call; a NULL return is handled.
    let handle = unsafe { CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, flags, name.as_ptr().cast()) };
    if handle.is_null() {
        // SAFETY: a plain call.
        let code = unsafe { GetLastError() };
        return Err(Error::Other(format!(
            "could not open this user's Root certificate store (Windows error {code:#x})"
        )));
    }
    Ok(Store(handle))
}

fn der_len(der: &[u8]) -> Result<u32> {
    u32::try_from(der.len()).map_err(|_| Error::Other("the CA certificate is too large".into()))
}

/// The store's copy of the certificate `der`, if it holds one.
fn find(store: &Store, der: &[u8]) -> Result<Option<Context>> {
    // SAFETY: `der` outlives the call; the context copies the bytes; NULL is handled.
    let probe = unsafe { CertCreateCertificateContext(ENCODING, der.as_ptr(), der_len(der)?) };
    if probe.is_null() {
        return Err(Error::Other("the CA file is not a certificate Windows can read".into()));
    }
    let probe = Context(probe);
    // SAFETY: `store` is open and `probe` live for the call; a NULL return means not found.
    let found = unsafe {
        CertFindCertificateInStore(store.0, ENCODING, 0, CERT_FIND_EXISTING, probe.0 as *const c_void, std::ptr::null())
    };
    Ok((!found.is_null()).then_some(Context(found)))
}

fn read_der(path: &Path) -> Result<Vec<u8>> {
    cert_rules::der_from_pem(&std::fs::read(path)?)
}

/// Add the CA to this user's Root store. Already there → Ok with no prompt; otherwise Windows asks.
pub(crate) fn trust(ca_cert_path: &Path) -> Result<()> {
    let der = read_der(ca_cert_path)?;
    let store = open_root(false)?;
    if find(&store, &der)?.is_some() {
        return Ok(());
    }
    // SAFETY: `store` is open for writing; `der` outlives the call; no context is requested back.
    let added = unsafe {
        CertAddEncodedCertificateToStore(
            store.0,
            ENCODING,
            der.as_ptr(),
            der_len(&der)?,
            CERT_STORE_ADD_USE_EXISTING,
            std::ptr::null_mut(),
        )
    };
    if added == 0 {
        // SAFETY: a plain call, straight after the failure.
        return Err(cert_rules::trust_error("trust", unsafe { GetLastError() }));
    }
    Ok(())
}

/// Remove the CA from this user's Root store. Not there → Ok with no prompt; otherwise Windows asks.
pub(crate) fn untrust(ca_cert_path: &Path) -> Result<()> {
    let der = read_der(ca_cert_path)?;
    let store = open_root(false)?;
    let Some(found) = find(&store, &der)? else {
        return Ok(());
    };
    let raw = found.0;
    // `CertDeleteCertificateFromStore` frees the context whether or not it succeeds.
    std::mem::forget(found);
    // SAFETY: `raw` came from `CertFindCertificateInStore` on the open `store`, and is not used again.
    if unsafe { CertDeleteCertificateFromStore(raw) } == 0 {
        // SAFETY: a plain call, straight after the failure.
        return Err(cert_rules::trust_error("untrust", unsafe { GetLastError() }));
    }
    Ok(())
}

/// Every certificate in `store` whose subject CONTAINS rexenv's CA common name, as DER.
///
/// A SUBSTRING match is all CryptoAPI offers here, so this is the CANDIDATE list; the exact-name
/// filter that decides what may be deleted is `cert_rules::is_rexenv_ca`, applied by the caller.
fn subject_matches(store: &Store) -> Result<Vec<Vec<u8>>> {
    let needle: Vec<u16> = crate::core::ssl::CA_COMMON_NAME.encode_utf16().chain(Some(0)).collect();
    let mut out = Vec::new();
    // `CertFindCertificateInStore` FREES the context handed to it as `prev` -- including the last
    // one, on the call that returns NULL. So this loop owns nothing and must NOT wrap `prev` in
    // `Context`: that would free it a second time.
    let mut prev: *mut CERT_CONTEXT = std::ptr::null_mut();
    loop {
        // SAFETY: `store` is open and `needle` outlives the call; `prev` is NULL or the context
        // the previous iteration received from this same call, which this call takes over.
        let found = unsafe {
            CertFindCertificateInStore(store.0, ENCODING, 0, CERT_FIND_SUBJECT_STR_W, needle.as_ptr().cast(), prev)
        };
        if found.is_null() {
            return Ok(out);
        }
        // SAFETY: `found` is a live context; its encoded bytes are valid while it is.
        unsafe {
            let ctx = &*found;
            out.push(std::slice::from_raw_parts(ctx.pbCertEncoded, ctx.cbCertEncoded as usize).to_vec());
        }
        prev = found;
    }
}

/// Remove every rexenv local CA from this user's Root store EXCEPT the one at `current_ca`, and
/// answer how many went (#678). A hand-wiped app-data folder mints a new CA and leaves the old
/// root trusted, anchoring keys that no longer exist.
///
/// **One Windows confirmation per certificate.** macOS sweeps under the authorization its trust
/// dialog just granted; this store has no such warmth -- every change asks on its own, so four
/// stale CAs mean four dialogs. That is Windows' rule for the Root store, not a choice rexenv
/// makes, and it is why `ssl::trust_ca` treats the sweep as best-effort.
pub(crate) fn untrust_stale(current_ca: &Path) -> Result<usize> {
    let current = read_der(current_ca)?;
    let store = open_root(false)?;
    let mut swept = 0;
    for der in cert_rules::stale_ders(subject_matches(&store)?, &current) {
        let Some(found) = find(&store, &der)? else { continue };
        let raw = found.0;
        // `CertDeleteCertificateFromStore` frees the context whether or not it succeeds.
        std::mem::forget(found);
        // SAFETY: `raw` came from `CertFindCertificateInStore` on the open `store`, and is not used again.
        if unsafe { CertDeleteCertificateFromStore(raw) } == 0 {
            // SAFETY: a plain call, straight after the failure.
            return Err(cert_rules::trust_error("remove a stale copy of", unsafe { GetLastError() }));
        }
        swept += 1;
    }
    Ok(swept)
}

/// Whether this user's Root store (the logical view, machine roots included) holds the CA. No prompt;
/// any failure reads as not trusted, so first-run setup offers the step again.
pub(crate) fn is_trusted(ca_cert_path: &Path) -> bool {
    let Ok(der) = read_der(ca_cert_path) else { return false };
    let Ok(store) = open_root(true) else { return false };
    matches!(find(&store, &der), Ok(Some(_)))
}
