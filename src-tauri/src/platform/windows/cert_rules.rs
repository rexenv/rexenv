//! The pure half of Windows' CA trust (ledger #613): the certificate's bytes out of rexenv's PEM file,
//! and what a failed store call tells the user. No Win32 here — `cert_store.rs` makes the calls — so
//! this file is compiled into the macOS test build and runs in `verify.sh`.

use crate::error::{Error, Result};

/// `ERROR_CANCELLED`: the user answered No to Windows' "install this certificate?" prompt.
pub(crate) const ERROR_CANCELLED: u32 = 1223;
/// The same cancel as an HRESULT (`HRESULT_FROM_WIN32(ERROR_CANCELLED)`), the spelling CryptoAPI
/// also reports it in.
pub(crate) const HRESULT_ERROR_CANCELLED: u32 = 0x8007_04C7;
/// `ERROR_NOT_SUPPORTED`: what adding to the CurrentUser Root store answered, at once and with no
/// prompt, from a session with no interactive desktop — measured over SSH on the Dell, 14 Sep 2026.
/// Windows will not change this store without asking, and there was nowhere to ask.
pub(crate) const ERROR_NOT_SUPPORTED: u32 = 50;

/// The DER bytes of the certificate in `pem` — the first PEM block, which must be a CERTIFICATE.
pub(crate) fn der_from_pem(pem: &[u8]) -> Result<Vec<u8>> {
    let (_, block) = x509_parser::pem::parse_x509_pem(pem)
        .map_err(|e| Error::Other(format!("the CA file holds no PEM certificate: {e}")))?;
    if block.label != "CERTIFICATE" {
        return Err(Error::Other(format!(
            "the CA file's first PEM block is a {}, not a certificate",
            block.label
        )));
    }
    Ok(block.contents)
}

/// The error a failed trust step reads as. An answered-No prompt is a recoverable cancel, never a
/// raw error number.
pub(crate) fn trust_error(action: &str, code: u32) -> Error {
    if code == ERROR_CANCELLED || code == HRESULT_ERROR_CANCELLED {
        Error::Other(
            "Windows' certificate prompt was cancelled — rexenv's local certificate authority needs it for \
             HTTPS. Try again and choose Yes."
                .into(),
        )
    } else if code == ERROR_NOT_SUPPORTED {
        Error::Other(format!(
            "Windows would not {action} rexenv's local certificate authority from here: it asks the signed-in \
             user to confirm, and this session has no desktop to ask on (Windows error {code:#x}). Run it from \
             rexenv on the desktop."
        ))
    } else {
        Error::Other(format!(
            "could not {action} rexenv's local certificate authority in this user's Root certificate store \
             (Windows error {code:#x})"
        ))
    }
}

/// Whether `der` is one of rexenv's OWN local CAs: its subject common name is EXACTLY
/// [`crate::core::ssl::CA_COMMON_NAME`].
///
/// The store search that finds the sweep's candidates is `CERT_FIND_SUBJECT_STR_W`, a
/// SUBSTRING match on the subject's display string -- so a root somebody else installed as
/// "rexenv Local CA Inc", or an "Old rexenv Local CA", comes back from it too. What this
/// sweep does with its candidates is DELETE them from the user's Root store, so the name has
/// to match exactly rather than contain: the cost of being wrong here is deleting a root
/// somebody depends on.
pub(crate) fn is_rexenv_ca(der: &[u8]) -> bool {
    let Ok((_, cert)) = x509_parser::parse_x509_certificate(der) else { return false };
    let mut names = cert.subject().iter_common_name().filter_map(|cn| cn.as_str().ok());
    names.any(|cn| cn == crate::core::ssl::CA_COMMON_NAME)
}

/// Of the certificates `present` in the store, the ones the stale sweep removes: rexenv's own
/// CAs, minus the current one. Every fresh app-data folder mints a new CA, and the roots the
/// earlier folders trusted stay behind as anchors for keys that no longer exist (#678).
///
/// Pure, so the RULE is testable off Windows -- the enumeration and the deletes are Win32 in
/// `cert_store.rs`, and this is the decision they apply.
pub(crate) fn stale_ders(present: Vec<Vec<u8>>, current: &[u8]) -> Vec<Vec<u8>> {
    present.into_iter().filter(|der| der.as_slice() != current && is_rexenv_ca(der)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A self-signed CA with `cn` as its common name, as DER.
    fn ca_named(cn: &str) -> Vec<u8> {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name.push(rcgen::DnType::CommonName, cn);
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key = rcgen::KeyPair::generate().unwrap();
        params.self_signed(&key).unwrap().der().to_vec()
    }

    /// #678 on Windows -- the sweep takes every rexenv CA but the current one, and NEVER a
    /// root that merely looks like one.
    ///
    /// The look-alike is the half that matters: the store is searched by subject SUBSTRING,
    /// so "rexenv Local CA Inc" is a candidate the Win32 side hands to this rule, and a rule
    /// that only compared "is it not the current one" would delete a stranger's root.
    #[test]
    fn the_stale_sweep_takes_rexenv_cas_but_never_the_current_one_or_a_look_alike() {
        let current = ca_named(crate::core::ssl::CA_COMMON_NAME);
        let stale = ca_named(crate::core::ssl::CA_COMMON_NAME);
        let look_alike = ca_named(&format!("{} Inc", crate::core::ssl::CA_COMMON_NAME));
        let stranger = ca_named("Some Company Root");

        assert!(is_rexenv_ca(&current) && is_rexenv_ca(&stale), "ours by exact common name");
        assert!(!is_rexenv_ca(&look_alike), "a longer name that CONTAINS ours is not ours");
        assert!(!is_rexenv_ca(&stranger));
        assert!(!is_rexenv_ca(b"not a certificate"), "unparsable is not ours");

        let present = vec![current.clone(), stale.clone(), look_alike.clone(), stranger.clone()];
        assert_eq!(stale_ders(present, &current), vec![stale], "only the stale rexenv CA");
        // Nothing to sweep is the ordinary case, and it is not an error.
        assert!(stale_ders(vec![current.clone()], &current).is_empty());
        assert!(stale_ders(vec![look_alike, stranger], &current).is_empty());
    }

    /// rexenv's own CA file, as `ssl` writes it, gives the certificate's DER; its KEY file is refused
    /// by name rather than handed to the store as if it were a certificate.
    #[test]
    fn the_ca_pem_gives_its_certificate_and_a_key_file_is_refused() {
        let dir = std::env::temp_dir().join("rexenv-windows-cert-rules");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ca = crate::core::ssl::load_or_create_at(&dir.join("ca.pem"), &dir.join("ca.key"), None).unwrap();
        let der = der_from_pem(&std::fs::read(&ca.cert_path).unwrap()).expect("the CA's certificate");
        let (_, cert) = x509_parser::parse_x509_certificate(&der).expect("the DER parses as X.509");
        assert!(cert.is_ca(), "the local CA's certificate is a CA");
        let key = der_from_pem(&std::fs::read(dir.join("ca.key")).unwrap());
        assert!(key.as_ref().is_err_and(|e| e.to_string().contains("not a certificate")), "{key:?}");
        assert!(der_from_pem(b"not pem at all").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_prompt_reads_as_a_cancel_in_both_spellings_and_anything_else_names_the_error() {
        for code in [ERROR_CANCELLED, HRESULT_ERROR_CANCELLED] {
            let m = trust_error("trust", code).to_string();
            assert!(m.contains("cancelled") && m.contains("Try again"), "{m}");
        }
        let m = trust_error("untrust", 5).to_string();
        assert!(m.contains("untrust") && m.contains("0x5") && !m.contains("cancelled"), "{m}");
        // No desktop to prompt on (measured over SSH): said as that, never as a cancel or a bare number.
        let m = trust_error("trust", ERROR_NOT_SUPPORTED).to_string();
        assert!(m.contains("no desktop") && m.contains("0x32") && !m.contains("cancelled"), "{m}");
    }
}
