//! Where a site's live-sync pairing lives on THIS machine (L2 of
//! `docs/PLAN-wp-live-sync.md`, §11 Q4 decided 10 Oct 2026).
//!
//! ONE mechanism on every OS: an owner-only file under app data, written through
//! `PermissionManager::write_private` — 0600 on macOS and Linux, the owner-only
//! ACL on Windows (#597). Not the keychain: rexenv is ad-hoc signed, so every
//! update changes the code identity a keychain item is bound to and would prompt
//! after each one. Not Secret Service: a Linux box without one (headless, a
//! minimal WM) still has to work. The file holds exactly what the pasted key
//! held, so it is as private as the key, and the key was pasted from the plugin's
//! own admin page.
//!
//! Never in SQLite (`sites` carries the site_id → nothing), never logged, never
//! sent to the webview after the paste (the UI gets `Pairing` without the secret).
//!
//! What plaintext-at-rest costs (the security review, 10 Oct 2026): the file is
//! exactly as private as the file ACL. A backup tool or any process running as
//! this user can read it, and with it the live site's full sync credentials —
//! the same as the key on the plugin's page. That is the accepted posture; the
//! alternative (the keychain) prompts after every update of an ad-hoc-signed app.

use super::sign::PairingKey;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What the file holds — the key, plus the optional HTTP basic auth (§11 Q5).
#[derive(Serialize, Deserialize)]
struct Stored {
    site_url: String,
    key_id: String,
    secret: String,
    #[serde(default)]
    basic_auth: Option<BasicAuth>,
}

/// HTTP basic auth in front of the live site: `(user, password)`.
pub type BasicAuth = (String, String);
/// A stored pairing: the key, and the basic auth if any.
pub type StoredPairing = (PairingKey, Option<BasicAuth>);

/// The pairing a local site holds, without its secret — what the UI may see.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub site_url: String,
    pub key_id: String,
    pub basic_auth_user: Option<String>,
}

use crate::platform::traits::PermissionManager;

/// The folder the pairing files live in.
pub fn dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("live-sync"))
}

fn file(dir: &std::path::Path, site_id: &str) -> Result<PathBuf> {
    // A site id is a UUID the app made; refuse anything else before it joins a path.
    if site_id.is_empty() || !site_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(Error::Other("not a site id".into()));
    }
    Ok(dir.join(format!("{site_id}.json")))
}

/// Store the pairing for `site_id` in `dir`, replacing any earlier one.
pub fn put_in(dir: &std::path::Path, perms: &dyn PermissionManager, site_id: &str, key: &PairingKey, basic_auth: Option<BasicAuth>) -> Result<()> {
    let path = file(dir, site_id)?;
    // The folder is NOT `set_private`: that is 0600, and a directory without `x`
    // cannot be entered (the first run of the test). App data is per-user already;
    // the FILE is what is born owner-only.
    std::fs::create_dir_all(dir)?;
    let body = serde_json::to_vec(&Stored {
        site_url: key.site_url.clone(),
        key_id: key.key_id.clone(),
        secret: URL_SAFE_NO_PAD.encode(&key.secret),
        basic_auth,
    })
    .expect("json");
    perms.write_private(&path, &body)
}

/// The key and basic auth for `site_id`, or `None` when it has no pairing.
pub fn get_in(dir: &std::path::Path, site_id: &str) -> Result<Option<StoredPairing>> {
    let path = file(dir, site_id)?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let s: Stored = serde_json::from_slice(&bytes).map_err(|_| Error::Other(format!("{} is not a pairing file", path.display())))?;
    let secret = URL_SAFE_NO_PAD.decode(&s.secret).map_err(|_| Error::Other(format!("{} holds no usable secret", path.display())))?;
    Ok(Some((PairingKey { site_url: s.site_url, key_id: s.key_id, secret }, s.basic_auth)))
}

/// Forget `site_id`'s pairing. `false` when there was none.
pub fn delete_in(dir: &std::path::Path, site_id: &str) -> Result<bool> {
    match std::fs::remove_file(file(dir, site_id)?) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub fn put(platform: &dyn Platform, site_id: &str, key: &PairingKey, basic_auth: Option<BasicAuth>) -> Result<()> {
    put_in(&dir(platform)?, platform.permissions(), site_id, key, basic_auth)
}

pub fn get(platform: &dyn Platform, site_id: &str) -> Result<Option<StoredPairing>> {
    get_in(&dir(platform)?, site_id)
}

/// What the UI may know about `site_id`'s pairing.
pub fn pairing(platform: &dyn Platform, site_id: &str) -> Result<Option<Pairing>> {
    Ok(get(platform, site_id)?.map(|(k, auth)| Pairing { site_url: k.site_url, key_id: k.key_id, basic_auth_user: auth.map(|(u, _)| u) }))
}

pub fn delete(platform: &dyn Platform, site_id: &str) -> Result<bool> {
    delete_in(&dir(platform)?, site_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pairing round-trips through an owner-only file, the UI view never
    /// carries the secret, and a site id that is not one never reaches a path**
    /// (ledger #828).
    #[test]
    fn pairing_round_trips_owner_only_and_the_view_has_no_secret() {
        let plat = crate::platform::current();
        let perms = plat.permissions();
        let dir = std::env::temp_dir().join(format!("rexenv-live-sync-secrets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let key = PairingKey { site_url: "https://example.com".into(), key_id: "k_0123abcd".into(), secret: vec![7u8; 32] };
        let id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        assert!(get_in(&dir, id).unwrap().is_none());
        put_in(&dir, perms, id, &key, Some(("staging".into(), "hunter2".into()))).unwrap();
        let (back, auth) = get_in(&dir, id).unwrap().unwrap();
        assert_eq!(back, key);
        assert_eq!(auth, Some(("staging".into(), "hunter2".into())));
        let view = Pairing { site_url: back.site_url.clone(), key_id: back.key_id.clone(), basic_auth_user: auth.map(|(u, _)| u) };
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("hunter2") && !json.contains(&URL_SAFE_NO_PAD.encode(&key.secret)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&dir, id).unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "owner-only");
        }
        for bad in ["", "../x", "a/b", "x;y"] {
            assert!(put_in(&dir, perms, bad, &key, None).is_err(), "{bad:?}");
        }
        assert!(delete_in(&dir, id).unwrap());
        assert!(!delete_in(&dir, id).unwrap());
        assert!(get_in(&dir, id).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
