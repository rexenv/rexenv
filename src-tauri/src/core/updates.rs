//! core::updates — the signed PHP patch manifest.
//!
//! # What this moves, and what it must not
//!
//! Every binary rexenv runs is checksum-pinned into the app. That is not a
//! statement about knowing which bytes run — it is the reason **compromising a
//! download host cannot reach an installed user**: eight independent upstreams
//! each face a separately compiled-in digest, and a mismatch fails closed.
//!
//! An in-app PHP patch update needs a digest for bytes the app was built before.
//! So the anchor moves — and the ONE acceptable shape is that it moves from a
//! `const` in the binary to **a signed document whose public key is a `const` in
//! the binary**. It never moves to TLS: `binaries.rs`' digest gate verifies the
//! bytes against *whoever supplied the digest*, so an attacker-chosen `url` with
//! an attacker-chosen `sha256` matches perfectly and passes in silence.
//!
//! # What the signature does and does not buy
//!
//! Stated here rather than implied, because the honest limits are the reason this
//! was argued about for two days (`docs/PLAN-binary-updates.md` §1–§3):
//!
//! - It DOES defend against the manifest host being compromised on its own — a
//!   CDN, a mirror, a stolen upload token — and against tampering past TLS.
//! - It does NOT defend against whoever can sign. With the key in CI, one
//!   account compromise takes the app, the manifest and the key together.
//! - It attests **"this is the byte string rexenv's maintainer saw at pin
//!   time"** — never "this build is safe". Upstream shipping a compromised
//!   8.3.32 is faithfully hashed, signed and distributed. That is already
//!   today's posture; this path must not claim more.
//!
//! **Custody can improve without touching a line of this file.** The app holds
//! only the public key, so moving the private half from a CI secret to a hardware
//! token costs a key rotation, not a redesign.
//!
//! # Inert until a key is pinned
//!
//! [`RELEASE_PUBKEY`] is empty, so [`verify`] refuses everything and
//! [`cached`] returns an empty catalog: the app resolves exactly what it resolves
//! today. Same deliberate shape as the debug build's empty digests — wired,
//! tested, and unable to trust anything until a maintainer pins the real value.

use crate::core::php;
use crate::error::{Error, Result};
use crate::state::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// The ed25519 public key whose signature this app accepts on a manifest, hex.
///
/// **Compiled in, and that is the whole point**: a compromised manifest host
/// cannot hand out a new key, because rotating it requires an app release.
///
/// EMPTY = no manifest is ever accepted (see the module doc). Pin the real value
/// only together with the release-side signing step, never before — a key in the
/// binary with no signing procedure behind it invites someone to sign by hand.
const RELEASE_PUBKEY: &str = "";

/// Hosts a manifest artifact may be downloaded from.
///
/// The second structural limit. There is **no scheme or host constraint anywhere
/// on the download path today** — `binaries::http_client` sets a user-agent and a
/// connect timeout and nothing else — an absence that has never mattered because
/// every URL is a compiled-in `format!`. A manifest makes `http://attacker/` a
/// valid entry with no code change required to accept it.
const ALLOWED_HOSTS: &[&str] = &[
    "https://dl.static-php.dev/",
    "https://github.com/rexenv/",
];

/// Artifact names a manifest may describe.
///
/// The first structural limit, and the sharpest. `binaries::resolve` is
/// name-generic: `("caddy", …)` resolves through the identical path as PHP, and
/// the resolved cache path is then handed to `proxy::start_edge_daemon`, which
/// copies it to `/Library/…`, chowns it `root:wheel` and bootstraps it into the
/// system launchd domain. **A manifest that can name a binary can name `caddy`,
/// and `caddy` is a root LaunchDaemon.** So it cannot.
const ALLOWED_NAMES: &[&str] = &["php", "php-fpm"];

/// Settings keys holding the verified document and its detached signature.
///
/// **Both, always.** Storing the document with a "verified" flag would put the
/// verdict in a file the user's own tools can edit — `rexenv.db` is
/// `-rw-r--r--` — so [`cached`] re-runs the signature check on every read
/// instead of trusting a boolean.
const DOC_KEY: &str = "php_update_manifest";
const SIG_KEY: &str = "php_update_manifest_sig";
/// Highest serial ever accepted. Stored separately from the document because it
/// must survive a document being replaced by an older one — that is its job.
const SERIAL_KEY: &str = "php_update_manifest_serial";

/// One artifact a manifest describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// `php` or `php-fpm` — see [`ALLOWED_NAMES`].
    pub name: String,
    /// A full `x.y.z`, whose minor must already be one rexenv ships.
    pub version: String,
    /// `arm64` or `x86_64`.
    pub arch: String,
    pub url: String,
    /// Lowercase 64-hex SHA-256 of the artifact at `url`.
    pub sha256: String,
}

/// The document, as signed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Monotonic. A document whose serial is not GREATER than the highest ever
    /// accepted is refused — otherwise a host that keeps serving an older,
    /// validly-signed manifest can hold a user on a known-CVE patch forever. The
    /// signature alone does not stop that.
    pub serial: u64,
    pub generated_at: String,
    /// The oldest app whose extract/prepare logic these entries assume. macOS
    /// `prepare_binary` order has changed before; an entry needing handling an
    /// older app lacks must not be offered to it.
    pub min_app_version: String,
    pub artifacts: Vec<Artifact>,
}

/// Verified entries, ready to be consulted alongside the compiled-in pins.
///
/// Empty is the normal degraded state: no key, no network, bad signature, stale
/// serial, or a corrupt cache all land here, and every one of them means the app
/// resolves exactly what it resolves today.
#[derive(Debug, Clone, Default)]
pub struct VersionCatalog {
    entries: Vec<Artifact>,
}

impl VersionCatalog {
    /// The newest patch this catalog offers for `minor`, if it is newer than
    /// `have`. Numeric per segment — `8.3.9 < 8.3.10`, which a lexical compare
    /// gets backwards, and PHP has shipped double-digit patches on every branch.
    pub fn newer_than(&self, minor: &str, have: &str) -> Option<String> {
        self.entries
            .iter()
            .filter(|a| php::minor_of(&a.version) == minor)
            .map(|a| a.version.clone())
            .filter(|v| newer(v, have))
            .max_by(|a, b| segments(a).cmp(&segments(b)))
    }

    /// The digest and URL this catalog holds for one artifact, if any.
    pub fn artifact(&self, name: &str, version: &str, arch: &str) -> Option<&Artifact> {
        self.entries
            .iter()
            .find(|a| a.name == name && a.version == version && a.arch == arch)
    }

    /// Every patch the catalog offers, for the GC's keep-set and for tests.
    pub fn versions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in &self.entries {
            if !out.contains(&a.version) {
                out.push(a.version.clone());
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn segments(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Whether `candidate` is a strictly newer patch of the SAME minor as `have`.
fn newer(candidate: &str, have: &str) -> bool {
    php::minor_of(candidate) == php::minor_of(have) && segments(candidate) > segments(have)
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || s.is_empty() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Whether an entry survives the structural limits.
///
/// Entries are DROPPED rather than failing the document, so one malformed row
/// cannot deny a user every other update. A bad SIGNATURE is the opposite — that
/// refuses everything, because it says the document is not ours.
fn acceptable(a: &Artifact) -> bool {
    ALLOWED_NAMES.contains(&a.name.as_str())
        && (a.arch == "arm64" || a.arch == "x86_64")
        && ALLOWED_HOSTS.iter().any(|h| a.url.starts_with(h))
        && a.sha256.len() == 64
        && a.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        && a.sha256.bytes().all(|b| !b.is_ascii_uppercase())
        // A patch of a minor rexenv ALREADY ships. Not cosmetic: `php::fpm_port`
        // gives each major ten slots, and per-minor facts (`eol_since`,
        // `xdebug_supported`) are compile-time tables — a runtime-delivered NEW
        // minor would render with no EOL date and Xdebug silently unavailable.
        && php::patch_for_minor(&php::minor_of(&a.version)).is_some()
        && segments(&a.version).len() == 3
        && a.version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Verify a detached signature over `doc` and return the entries that survive
/// the structural limits.
///
/// Refuses when: no key is pinned, the key or signature is not decodable, the
/// signature does not verify, the document does not parse, or `min_app_version`
/// is newer than this build.
pub fn verify(doc: &[u8], sig_hex: &str) -> Result<Manifest> {
    verify_with(RELEASE_PUBKEY, doc, sig_hex)
}

/// [`verify`] against an explicit key.
///
/// Exists so the SIGNATURE CHECK ITSELF is testable. With `RELEASE_PUBKEY` empty
/// — the shipping state — `verify` refuses before reaching ring, so every test
/// routed through it would prove only that an unkeyed build trusts nothing, and
/// the ed25519 path, the tamper rejection and the serial rule would all ship with
/// zero coverage. Tests generate a real keypair and drive this.
fn verify_with(pubkey_hex: &str, doc: &[u8], sig_hex: &str) -> Result<Manifest> {
    let key = unhex(pubkey_hex).ok_or_else(|| {
        Error::Other(
            "no PHP update key is pinned in this build, so no manifest can be trusted".into(),
        )
    })?;
    let sig = unhex(sig_hex.trim())
        .ok_or_else(|| Error::Other("the manifest signature is not hex".into()))?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key)
        .verify(doc, &sig)
        .map_err(|_| Error::Other("the manifest signature does not verify".into()))?;
    let mut m: Manifest = serde_json::from_slice(doc)
        .map_err(|e| Error::Other(format!("the manifest did not parse: {e}")))?;
    if newer_app_required(&m.min_app_version) {
        return Err(Error::Other(format!(
            "this manifest needs rexenv {} or newer",
            m.min_app_version
        )));
    }
    m.artifacts.retain(acceptable);
    Ok(m)
}

/// Whether `required` is newer than this build's own version.
fn newer_app_required(required: &str) -> bool {
    if required.is_empty() {
        return false;
    }
    segments(required) > segments(env!("CARGO_PKG_VERSION"))
}

/// The catalog from the stored document. **No network, and the signature is
/// re-checked every call.**
///
/// Every failure degrades to an empty catalog rather than an error: this feeds a
/// version row and an optional button, and it must not be able to fail a screen.
pub fn cached(conn: &Connection) -> VersionCatalog {
    let Some(doc) = store::get_setting(conn, DOC_KEY).ok().flatten() else {
        return VersionCatalog::default();
    };
    let Some(sig) = store::get_setting(conn, SIG_KEY).ok().flatten() else {
        return VersionCatalog::default();
    };
    match verify(doc.as_bytes(), &sig) {
        Ok(m) => VersionCatalog { entries: m.artifacts },
        Err(_) => VersionCatalog::default(),
    }
}

/// Accept a fetched document: verify it, enforce the serial rule, and persist
/// the document AND its signature.
///
/// Rejecting a stale serial happens BEFORE the write, so a replayed older
/// manifest cannot displace a newer one it was validly signed alongside.
pub fn accept(conn: &Connection, doc: &[u8], sig_hex: &str) -> Result<VersionCatalog> {
    accept_with(RELEASE_PUBKEY, conn, doc, sig_hex)
}

/// [`accept`] against an explicit key — see [`verify_with`] for why.
fn accept_with(
    pubkey_hex: &str,
    conn: &Connection,
    doc: &[u8],
    sig_hex: &str,
) -> Result<VersionCatalog> {
    let m = verify_with(pubkey_hex, doc, sig_hex)?;
    let highest: u64 = store::get_setting(conn, SERIAL_KEY)
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if m.serial <= highest {
        return Err(Error::Other(format!(
            "this manifest's serial ({}) is not newer than the highest already accepted ({highest}) \
             — refusing a replay",
            m.serial
        )));
    }
    let text = std::str::from_utf8(doc)
        .map_err(|_| Error::Other("the manifest is not valid UTF-8".into()))?;
    store::set_setting(conn, DOC_KEY, text)?;
    store::set_setting(conn, SIG_KEY, sig_hex.trim())?;
    store::set_setting(conn, SERIAL_KEY, &m.serial.to_string())?;
    Ok(VersionCatalog { entries: m.artifacts })
}

/// Whether this build can offer in-app PHP updates at all — i.e. whether a key
/// is pinned. The UI asks so it can leave the button out entirely rather than
/// showing one that always fails.
pub fn enabled() -> bool {
    !RELEASE_PUBKEY.is_empty()
}

/// The compiled-in pin for a minor, which a manifest may never move BELOW.
///
/// The floor is a version comparison, not a flag: with no manifest the answer is
/// today's pin byte for byte, and when the app ships a newer pin than the user's
/// selection the pin wins. There is no boolean anyone can get backwards.
pub fn floored(minor: &str, selected: Option<&str>) -> Option<String> {
    let pin = php::patch_for_minor(minor)?;
    match selected {
        Some(sel) if newer(sel, pin) => Some(sel.to_string()),
        _ => Some(pin.to_string()),
    }
}

/// Whether `name` is an artifact a manifest is allowed to describe — exposed so
/// `binaries` can assert it at the point it consults the catalog rather than
/// trusting this module to have filtered.
pub fn nameable(name: &str) -> bool {
    ALLOWED_NAMES.contains(&name)
}

/// Every host a manifest artifact may come from, for the guard that asserts the
/// list cannot silently widen to a bare domain.
pub fn allowed_hosts() -> &'static [&'static str] {
    ALLOWED_HOSTS
}

#[cfg(test)]
mod tests {
    use super::*;
    // Test-only: the shipping code needs no pin table, it needs `php`'s view of
    // one. Kept out of the module imports so clippy's -D warnings stays clean.
    use crate::core::binaries;

    /// **This build trusts NO manifest, because no key is pinned.**
    ///
    /// The inert state is asserted rather than assumed: everything below tests
    /// the machinery through `verify`, which refuses first, so a test that
    /// accidentally passed a real document would be testing nothing. When a key
    /// IS pinned this test flips to asserting the key's shape, and the rest of
    /// the suite starts exercising real signatures.
    #[test]
    fn a_build_with_no_key_pinned_trusts_nothing() {
        assert!(!enabled(), "a key is pinned — update this test and the ledger row together");
        assert!(RELEASE_PUBKEY.is_empty());
        let err = verify(br#"{"serial":1}"#, "00").unwrap_err().to_string();
        assert!(err.contains("no PHP update key is pinned"), "{err}");
        // …and every read-side entry point degrades rather than erroring.
        let conn = crate::state::db::open_in_memory().unwrap();
        assert!(cached(&conn).is_empty());
        assert!(accept(&conn, br#"{"serial":1}"#, "00").is_err());
    }

    /// A throwaway ed25519 keypair, and a signer, so the SIGNATURE CHECK itself
    /// is exercised rather than short-circuited by the empty shipping key.
    fn keypair() -> (String, ring::signature::Ed25519KeyPair) {
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let hex = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        (hex, kp)
    }

    fn sign(kp: &ring::signature::Ed25519KeyPair, doc: &[u8]) -> String {
        kp.sign(doc).as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn doc(serial: u64, versions: &[&str]) -> Vec<u8> {
        let minor = php::minor_of(binaries::PHP_VERSION);
        let arts: Vec<String> = versions
            .iter()
            .flat_map(|v| ["arm64", "x86_64"].map(move |a| (v, a)))
            .map(|(v, a)| {
                format!(
                    r#"{{"name":"php","version":"{v}","arch":"{a}","url":"https://dl.static-php.dev/x","sha256":"{}"}}"#,
                    "a".repeat(64)
                )
            })
            .collect();
        let _ = minor;
        format!(
            r#"{{"serial":{serial},"generatedAt":"2026-08-17T00:00:00Z","minAppVersion":"0.0.1","artifacts":[{}]}}"#,
            arts.join(",")
        )
        .into_bytes()
    }

    /// **A real signature verifies; a tampered document or signature does not.**
    ///
    /// The core security property, driven through a generated key because the
    /// shipping key is empty by design. Without this the ed25519 call would have
    /// zero coverage and the module would be asserting a check it never ran.
    #[test]
    fn a_valid_signature_verifies_and_any_tamper_refuses() {
        let (pub_hex, kp) = keypair();
        let minor = php::minor_of(binaries::PHP_VERSION);
        let d = doc(1, &[&format!("{minor}.9999")]);
        let sig = sign(&kp, &d);

        let m = verify_with(&pub_hex, &d, &sig).expect("a correctly signed manifest verifies");
        assert_eq!(m.serial, 1);
        assert_eq!(m.artifacts.len(), 2, "both arches survived");

        // A single flipped byte in the DOCUMENT.
        let mut tampered = d.clone();
        let i = tampered.iter().position(|b| *b == b'9').unwrap();
        tampered[i] = b'8';
        assert!(verify_with(&pub_hex, &tampered, &sig).is_err(), "a tampered document verified");

        // A single flipped byte in the SIGNATURE.
        let mut bad_sig: Vec<char> = sig.chars().collect();
        bad_sig[0] = if bad_sig[0] == 'a' { 'b' } else { 'a' };
        let bad_sig: String = bad_sig.into_iter().collect();
        assert!(verify_with(&pub_hex, &d, &bad_sig).is_err(), "a tampered signature verified");

        // A DIFFERENT key's signature — the swapped-signer case.
        let (other_pub, other_kp) = keypair();
        assert_ne!(other_pub, pub_hex);
        assert!(
            verify_with(&pub_hex, &d, &sign(&other_kp, &d)).is_err(),
            "a signature from another key verified"
        );
    }

    /// **A replayed older manifest is refused, and does not displace the newer
    /// one.** The signature alone cannot stop a host from serving a stale but
    /// validly-signed document forever, which is how a user gets held on a
    /// known-CVE patch.
    #[test]
    fn an_older_serial_is_refused_and_leaves_the_stored_one_intact() {
        let (pub_hex, kp) = keypair();
        let conn = crate::state::db::open_in_memory().unwrap();
        let minor = php::minor_of(binaries::PHP_VERSION);
        let newv = format!("{minor}.9999");
        let oldv = format!("{minor}.9998");

        let d7 = doc(7, &[&newv]);
        accept_with(&pub_hex, &conn, &d7, &sign(&kp, &d7)).expect("serial 7 accepted");
        assert_eq!(
            store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(),
            Some("7")
        );

        // Serial 6, validly signed — a replay.
        let d6 = doc(6, &[&oldv]);
        let err = accept_with(&pub_hex, &conn, &d6, &sign(&kp, &d6)).unwrap_err().to_string();
        assert!(err.contains("refusing a replay"), "{err}");
        // The SAME serial is a replay too.
        let again = doc(7, &[&oldv]);
        assert!(accept_with(&pub_hex, &conn, &again, &sign(&kp, &again)).is_err());
        // And nothing was displaced: the stored document is still serial 7's.
        assert_eq!(store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(), Some("7"));
        let stored = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        assert!(stored.contains(&newv), "the replay overwrote the newer document");
        assert!(!stored.contains(&oldv));
    }

    /// **A cache tampered with in place is refused on READ.** `rexenv.db` is
    /// world-readable and user-writable, so a "verified" flag would put the
    /// verdict where the attacker is. `cached` re-runs the signature every call.
    #[test]
    fn a_tampered_cache_is_refused_on_every_read_not_trusted_from_a_flag() {
        let (pub_hex, kp) = keypair();
        let conn = crate::state::db::open_in_memory().unwrap();
        let minor = php::minor_of(binaries::PHP_VERSION);
        let d = doc(1, &[&format!("{minor}.9999")]);
        accept_with(&pub_hex, &conn, &d, &sign(&kp, &d)).unwrap();

        // Sanity: with the right key it reads back. (`cached` uses the SHIPPING
        // key, which is empty, so it must be empty even before tampering — that
        // is asserted separately; here we prove the stored bytes are good.)
        let stored = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        let sig = store::get_setting(&conn, SIG_KEY).unwrap().unwrap();
        assert!(verify_with(&pub_hex, stored.as_bytes(), &sig).is_ok());

        // Now edit the cached document the way `sqlite3` would.
        let swapped = stored.replace(&"a".repeat(64), &"b".repeat(64));
        assert_ne!(swapped, stored, "the fixture did not actually change a digest");
        store::set_setting(&conn, DOC_KEY, &swapped).unwrap();
        let re = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        assert!(
            verify_with(&pub_hex, re.as_bytes(), &sig).is_err(),
            "an in-place digest swap survived verification — the cache is trusted, not checked"
        );
    }

    /// **The floor is the compiled-in pin, and a selection may only raise it.**
    #[test]
    fn a_selection_can_only_move_a_minor_forward_from_its_pin() {
        let minor = php::minor_of(binaries::PHP_VERSION);
        let pin = php::patch_for_minor(&minor).unwrap();

        // No selection → today's answer, byte for byte.
        assert_eq!(floored(&minor, None).as_deref(), Some(pin));
        // An older selection cannot lower it — the pin is a FLOOR.
        assert_eq!(floored(&minor, Some(&format!("{minor}.0"))).as_deref(), Some(pin));
        // The same patch is not a move.
        assert_eq!(floored(&minor, Some(pin)).as_deref(), Some(pin));
        // A newer one is.
        let up = format!("{minor}.9999");
        assert_eq!(floored(&minor, Some(&up)).as_deref(), Some(up.as_str()));
        // Another minor's patch never moves this one.
        let other = php::all_minors().into_iter().find(|m| *m != minor).unwrap();
        assert_eq!(floored(&minor, Some(&format!("{other}.9999"))).as_deref(), Some(pin));
        // An unknown minor has no floor and therefore no answer.
        assert_eq!(floored("6.6", None), None);
    }

    /// **Every structural limit, each asserted on an entry that is otherwise
    /// perfectly well-formed** — so a rejection can only be the limit under test.
    #[test]
    fn the_structural_limits_each_drop_an_otherwise_valid_entry() {
        let minor = php::minor_of(binaries::PHP_VERSION);
        let good = Artifact {
            name: "php".into(),
            version: format!("{minor}.9999"),
            arch: "arm64".into(),
            url: "https://dl.static-php.dev/static-php-cli/bulk/x.tar.gz".into(),
            sha256: "a".repeat(64),
        };
        assert!(acceptable(&good), "the control entry must pass, or nothing below means anything");

        // 1. The name allowlist. `caddy` is the one that matters: it reaches the
        //    root LaunchDaemon install.
        for name in ["caddy", "nginx", "mysql", "php-debug", "PHP", ""] {
            assert!(
                !acceptable(&Artifact { name: name.into(), ..good.clone() }),
                "a manifest must not be able to describe `{name}`"
            );
        }
        // 2. Scheme + host.
        for url in [
            "http://dl.static-php.dev/x.tar.gz",             // not https
            "https://dl.static-php.dev.attacker.test/x.tar", // prefix-adjacent host
            "https://github.com/someone-else/x.tar.gz",      // github, not ours
            "https://attacker.test/x.tar.gz",
            "file:///etc/passwd",
            "",
        ] {
            assert!(
                !acceptable(&Artifact { url: url.into(), ..good.clone() }),
                "a manifest must not be able to point at `{url}`"
            );
        }
        // 3. A patch of a KNOWN minor only.
        for version in ["9.9.1", "8.10.0", "6.6.6", "8.3", "8.3.31.1", "8.3.x", ""] {
            assert!(
                !acceptable(&Artifact { version: version.into(), ..good.clone() }),
                "a manifest must not be able to introduce `{version}`"
            );
        }
        // 4. A real digest, lowercase — the form `binaries` compares against.
        for sha in ["", "abc", &"A".repeat(64), &"z".repeat(64), &"a".repeat(63)] {
            assert!(
                !acceptable(&Artifact { sha256: sha.into(), ..good.clone() }),
                "a manifest must not carry `{sha}` as a digest"
            );
        }
        // 5. A real arch.
        for arch in ["aarch64", "amd64", "arm", ""] {
            assert!(!acceptable(&Artifact { arch: arch.into(), ..good.clone() }));
        }
    }

    /// The host list must never become a bare domain. `github.com` serves Caddy,
    /// cloudflared, WP-CLI, Adminer, nginx and PHP upstream; only
    /// `github.com/rexenv/` is ours — the same distinction ledger #336 turned
    /// into the licence obligation.
    #[test]
    fn the_host_allowlist_is_not_a_bare_domain() {
        for h in allowed_hosts() {
            assert!(h.starts_with("https://"), "{h} is not https");
            assert!(h.ends_with('/'), "{h} must end in / so a sibling domain cannot prefix-match");
            let host = h.trim_start_matches("https://").trim_end_matches('/');
            assert!(
                host != "github.com",
                "a bare github.com would make rexenv the distributor of six other projects"
            );
        }
    }

    /// Patch comparison is numeric and same-minor only, and `newer_than` picks
    /// the HIGHEST rather than the first match.
    #[test]
    fn newer_is_numeric_same_minor_and_picks_the_highest() {
        assert!(newer("8.3.10", "8.3.9"), "lexically '10' < '9' — this must not be lexical");
        assert!(!newer("8.3.9", "8.3.10"));
        assert!(!newer("8.3.31", "8.3.31"));
        assert!(!newer("8.4.0", "8.3.99"), "another minor must never move this row");

        let cat = VersionCatalog {
            entries: ["8.3.32", "8.3.9", "8.3.40", "8.4.99"]
                .iter()
                .map(|v| Artifact {
                    name: "php".into(),
                    version: (*v).to_string(),
                    arch: "arm64".into(),
                    url: "https://dl.static-php.dev/x".into(),
                    sha256: "a".repeat(64),
                })
                .collect(),
        };
        assert_eq!(cat.newer_than("8.3", "8.3.31").as_deref(), Some("8.3.40"));
        assert_eq!(cat.newer_than("8.3", "8.3.40"), None, "nothing newer than the newest");
        assert_eq!(cat.newer_than("8.5", "8.5.8"), None, "a minor with no entries offers nothing");
    }

    /// A `min_app_version` newer than this build refuses the document, because
    /// `prepare_binary`'s behaviour has changed between releases before.
    #[test]
    fn a_manifest_for_a_newer_app_is_refused() {
        assert!(!newer_app_required(""), "absent means no constraint");
        assert!(!newer_app_required(env!("CARGO_PKG_VERSION")), "this build satisfies itself");
        assert!(!newer_app_required("0.0.1"));
        assert!(newer_app_required("999.0.0"));
    }

    #[test]
    fn unhex_rejects_what_is_not_a_byte_string() {
        assert_eq!(unhex("00ff").unwrap(), vec![0x00, 0xff]);
        assert!(unhex("").is_none(), "empty is not a key");
        assert!(unhex("0").is_none(), "odd length");
        assert!(unhex("zz").is_none());
    }
}
