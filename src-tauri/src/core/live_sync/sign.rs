//! rexsync1 pairing keys and request signatures (`docs/rexsync-protocol.md` §2–§3).
//!
//! Pure: no clock, no randomness, no I/O. The caller supplies `ts` and `nonce`,
//! so the same inputs always sign the same way, and the vectors in
//! `companion/rexenv-sync/tests/vectors.json` pin the result on both sides of
//! the wire.
//!
//! HMAC-SHA256 is the RFC 2104 construction over the `sha2` crate the tree
//! already has. It is fifteen lines, and adding a dependency (plus its
//! THIRD-PARTY-NOTICES row) for fifteen lines is not a trade worth making. RFC
//! 4231's own vectors pin it below.

use crate::error::{Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

/// The protocol tag: the first line of every canonical string, the prefix of
/// every pairing key.
pub const PROTOCOL: &str = "rexsync1";

/// HMAC-SHA256 (RFC 2104, block size 64).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// Lowercase hex of a SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// RFC 3986 percent-encoding: everything but the unreserved set.
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The canonical string (§3): seven lines, the query SORTED by key (then value),
/// no trailing newline.
pub fn canonical(method: &str, path: &str, query: &[(&str, &str)], ts: &str, nonce: &str, body: &[u8]) -> String {
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (pct(k), pct(v))).collect();
    q.sort();
    let q = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    [PROTOCOL, &method.to_ascii_uppercase(), path, &q, ts, nonce, &sha256_hex(body)].join("\n")
}

/// `X-Rexsync-Sig` for a canonical string.
pub fn signature(secret: &[u8], canonical: &str) -> String {
    URL_SAFE_NO_PAD.encode(hmac_sha256(secret, canonical.as_bytes()))
}

/// What a pasted pairing key carries (§2). `Debug` never prints the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingKey {
    pub site_url: String,
    pub key_id: String,
    pub secret: Vec<u8>,
}

impl std::fmt::Debug for PairingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingKey").field("site_url", &self.site_url).field("key_id", &self.key_id).finish_non_exhaustive()
    }
}

/// Parse a pasted key, refusing as a whole anything that is not exactly the
/// §2 shape. An `http://` site is refused with the reason: the protocol carries
/// database dumps.
pub fn parse_key(text: &str) -> Result<PairingKey> {
    let bad = || Error::Other("that is not a rexenv Sync connection key — copy it again from the plugin's page".into());
    let body = text.trim().strip_prefix("rexsync1:").ok_or_else(bad)?;
    let json = URL_SAFE_NO_PAD.decode(body).map_err(|_| bad())?;
    let v: serde_json::Value = serde_json::from_slice(&json).map_err(|_| bad())?;
    let obj = v.as_object().filter(|o| o.len() == 3).ok_or_else(bad)?;
    let field = |k: &str| obj.get(k).and_then(|x| x.as_str()).ok_or_else(bad);
    let (url, key_id, secret) = (field("u")?, field("k")?, field("s")?);
    let secret = URL_SAFE_NO_PAD.decode(secret).map_err(|_| bad())?;
    let key_ok = key_id.len() == 10
        && key_id.starts_with("k_")
        && key_id[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !key_ok || secret.len() != 32 {
        return Err(bad());
    }
    if !url.starts_with("https://") {
        return Err(Error::Other(format!(
            "{url} is not served over HTTPS — rexenv will not pull a database over a plain \
             connection. Turn on HTTPS for the live site first."
        )));
    }
    Ok(PairingKey { site_url: url.trim_end_matches('/').to_string(), key_id: key_id.to_string(), secret })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../companion/rexenv-sync/tests/vectors.json"));

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2 and test case 6 (a key longer than the block).
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// **Both sides sign the same string the same way** — the vectors file the
    /// plugin's test reads too (ledger #824). Plant: drop the `q.sort()` and
    /// vector 1, whose query arrives out of order, fails.
    #[test]
    fn requests_sign_as_the_shared_vectors_say() {
        let v: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        let secret = URL_SAFE_NO_PAD.decode(v["secret_b64u"].as_str().unwrap()).unwrap();
        let (ts, nonce) = (v["ts"].as_str().unwrap(), v["nonce"].as_str().unwrap());
        for r in v["requests"].as_array().unwrap() {
            let q: Vec<(String, String)> = r["query"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_str().unwrap().to_string(), p[1].as_str().unwrap().to_string()))
                .collect();
            let q: Vec<(&str, &str)> = q.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let body = r["body"].as_str().unwrap().as_bytes();
            assert_eq!(sha256_hex(body), r["body_sha256"].as_str().unwrap());
            let c = canonical(r["method"].as_str().unwrap(), r["path"].as_str().unwrap(), &q, ts, nonce, body);
            assert_eq!(c, r["canonical"].as_str().unwrap());
            assert_eq!(signature(&secret, &c), r["sig"].as_str().unwrap());
        }
    }

    #[test]
    fn keys_parse_exactly_or_not_at_all() {
        let v: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        let k = parse_key(v["key"]["text"].as_str().unwrap()).unwrap();
        assert_eq!(k.site_url, v["key"]["site_url"].as_str().unwrap());
        assert_eq!(k.key_id, v["key"]["key_id"].as_str().unwrap());
        assert_eq!(URL_SAFE_NO_PAD.encode(&k.secret), v["secret_b64u"].as_str().unwrap());
        assert!(!format!("{k:?}").contains(v["secret_b64u"].as_str().unwrap()), "Debug must not print the secret");
        for bad in v["bad_keys"].as_array().unwrap() {
            assert!(parse_key(bad.as_str().unwrap()).is_err(), "{bad}");
        }
        let http = parse_key(v["bad_keys"][2].as_str().unwrap()).unwrap_err().to_string();
        assert!(http.contains("HTTPS"), "the http refusal says why: {http}");
    }
}
