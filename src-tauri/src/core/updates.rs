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
const RELEASE_PUBKEY: &str = "faa52f961af3e0542d836ab539823f598ef88b976055809f73247f88af13cb12";

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

/// An [`Arch`] in the MANIFEST's vocabulary (`arm64` / `x86_64`).
///
/// Here, in the module that owns the document's schema, because this project
/// already carries six per-source arch spellings in `binaries.rs` and a seventh
/// written inline at each use is how a machine gets offered the other Mac's
/// binaries. Every arch string that crosses into the catalog comes from here.
pub fn catalog_arch(arch: crate::platform::traits::Arch) -> &'static str {
    match arch {
        crate::platform::traits::Arch::Arm64 => "arm64",
        crate::platform::traits::Arch::X86_64 => "x86_64",
    }
}

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
    /// The newest patch this catalog offers for `minor` **on `arch`, with BOTH
    /// binaries present**, if it is newer than `have`.
    ///
    /// Numeric per segment — `8.3.9 < 8.3.10`, which a lexical compare gets
    /// backwards, and PHP has shipped double-digit patches on every branch.
    ///
    /// The completeness rule is not the publisher's job to get right. An apply
    /// resolves `php` AND `php-fpm` for the machine it is on, so a version
    /// carrying three of those four offers a button that downloads ~100 MB and
    /// then fails at the last resolve — and, worse, does it on one developer's
    /// Mac while working on another's. `scripts/publish-manifest.sh` already
    /// drops half-published versions whole; this is the same rule enforced where
    /// it is load-bearing, because a manifest is data and data is exactly the
    /// thing that must not be trusted to have been generated correctly.
    pub fn newer_than(&self, minor: &str, have: &str, arch: &str) -> Option<String> {
        self.entries
            .iter()
            .filter(|a| php::minor_of(&a.version) == minor && a.arch == arch)
            .map(|a| a.version.clone())
            .filter(|v| newer(v, have))
            .filter(|v| {
                ALLOWED_NAMES
                    .iter()
                    .all(|n| self.artifact(n, v, arch).is_some())
            })
            .max_by(|a, b| segments(a).cmp(&segments(b)))
    }

    /// The digest and URL this catalog holds for one artifact, if any.
    ///
    /// `arch` is the MANIFEST's spelling — use [`catalog_arch`], never a literal.
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
///
/// # Equal is not stale
///
/// `serial == highest` is **the document we already have** — the ordinary state
/// on every launch after the first, since the manifest only changes when a new
/// PHP patch is published. It returns the catalog and writes nothing.
///
/// It used to be refused with "refusing a replay", which put that sentence in a
/// user's log at every single launch about rexenv's own current manifest. A log
/// line that cries wolf daily is worse than no log line: it trains the reader to
/// scroll past the one that means it. **A replay is an OLDER document being
/// served in place of a newer one** — a host holding users on a known-CVE patch —
/// and that is `<`, which is still refused, loudly, and is the case the rule
/// exists for.
///
/// Two different documents sharing one serial would be a publisher error, not an
/// attack (both carry our signature, and `publish-manifest.sh` reads the
/// published serial and increments). Equal therefore writes NOTHING: the stored
/// pair stays whatever was last accepted at that serial, rather than letting a
/// same-serial variant quietly displace it.
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
    if m.serial < highest {
        return Err(Error::Other(format!(
            "this manifest's serial ({}) is OLDER than the highest already accepted \
             ({highest}) — refusing a replay, which is how a host would hold this machine \
             on a superseded PHP patch",
            m.serial
        )));
    }
    if m.serial == highest {
        // Already ours at this serial. Nothing to write, nothing to report.
        return Ok(VersionCatalog { entries: m.artifacts });
    }
    let text = std::str::from_utf8(doc)
        .map_err(|_| Error::Other("the manifest is not valid UTF-8".into()))?;
    store::set_setting(conn, DOC_KEY, text)?;
    store::set_setting(conn, SIG_KEY, sig_hex.trim())?;
    store::set_setting(conn, SERIAL_KEY, &m.serial.to_string())?;
    Ok(VersionCatalog { entries: m.artifacts })
}

/// Where the signed manifest lives.
///
/// A GitHub release asset on `rexenv/runtimes` — the public repo the 7.4 build
/// already comes from, so this adds no infrastructure and no new host: it is
/// already on [`ALLOWED_HOSTS`]. The `manifest` tag is MOVED by each publish,
/// which is safe here and nowhere else in this codebase: the bytes are not
/// trusted for being at a URL, they are trusted for carrying a signature, so a
/// moved tag is the one case where re-upload cannot hurt.
const MANIFEST_URL: &str =
    "https://github.com/rexenv/runtimes/releases/download/manifest/manifest.json";
const MANIFEST_SIG_URL: &str =
    "https://github.com/rexenv/runtimes/releases/download/manifest/manifest.json.sig";

/// Refuse a document larger than this before parsing. A manifest of every patch
/// of every minor is a few KB; this is a poll, not a download.
const MAX_DOC: usize = 256 * 1024;

/// Fetch the manifest and its detached signature. **Takes no `Connection`**, so
/// no caller can hold the database lock across this await — the house rule, made
/// structural.
///
/// Returns `(document, signature_hex)`. Verification happens in [`accept`]; this
/// function is deliberately dumb about trust so there is exactly one place that
/// decides it.
pub async fn fetch() -> Result<(Vec<u8>, String)> {
    if !enabled() {
        return Err(Error::Other(
            "this build has no PHP update key pinned, so it does not fetch a manifest".into(),
        ));
    }
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // A hard TOTAL deadline, not the download path's retry-and-backoff:
            // nobody is waiting on this and it must never be why something else
            // is slow.
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("rexenv/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("update manifest client")
    });
    let get = |url: &'static str| async move {
        let res = client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Other(format!("could not reach {url}: {e}")))?;
        if !res.status().is_success() {
            return Err(Error::Other(format!("{url} answered {}", res.status())));
        }
        let body = res.bytes().await.map_err(|e| Error::Other(format!("{url}: {e}")))?;
        if body.len() > MAX_DOC {
            return Err(Error::Other(format!("{url} returned {} bytes", body.len())));
        }
        Ok(body.to_vec())
    };
    let doc = get(MANIFEST_URL).await?;
    let sig = get(MANIFEST_SIG_URL).await?;
    let sig = String::from_utf8(sig)
        .map_err(|_| Error::Other("the signature file is not text".into()))?;
    Ok((doc, sig))
}

/// Load the cached catalog into the resolve path. Called at launch, before
/// anything resolves, so a selected patch is resolvable offline.
pub fn install_cached(conn: &Connection) {
    crate::core::binaries::install_catalog(cached(conn));
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

/// Build a catalog directly, for tests in OTHER modules that need to exercise the
/// resolve path's precedence rather than the verification.
///
/// `#[cfg(test)]`-gated on purpose: in a shipping build the only way to obtain a
/// [`VersionCatalog`] is through [`verify`], and that is the property the private
/// field exists to enforce. A production constructor here would delete it.
#[cfg(test)]
pub fn catalog_for_tests(rows: &[(&str, &str, &str, &str, &str)]) -> VersionCatalog {
    VersionCatalog {
        entries: rows
            .iter()
            .map(|(name, version, arch, url, sha256)| Artifact {
                name: (*name).to_string(),
                version: (*version).to_string(),
                arch: (*arch).to_string(),
                url: (*url).to_string(),
                sha256: (*sha256).to_string(),
            })
            .collect(),
    }
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
    /// **The pinned key is a real ed25519 public key, and nothing else verifies
    /// against it.**
    ///
    /// Replaces the inert-state assertion the moment a key exists — a test that
    /// says "no key is pinned" would otherwise fail the day the feature became
    /// real, and the tempting fix is to delete it rather than to write this.
    #[test]
    fn the_pinned_key_is_real_and_only_it_verifies() {
        assert!(enabled(), "the update key was un-pinned — the button silently disappears");
        assert_eq!(RELEASE_PUBKEY.len(), 64, "an ed25519 public key is 32 bytes of hex");
        assert!(RELEASE_PUBKEY.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(unhex(RELEASE_PUBKEY).is_some(), "the pinned key does not decode");

        // A document signed by SOMEBODY ELSE's key must not verify against ours.
        // The whole compiled-in-pubkey design rests on exactly this.
        let (other_pub, other_kp) = keypair();
        assert_ne!(other_pub, RELEASE_PUBKEY, "the pinned key is a generated test key");
        let d = br#"{"serial":1,"generatedAt":"x","minAppVersion":"0.0.1","artifacts":[]}"#;
        assert!(
            verify(d, &sign(&other_kp, d)).is_err(),
            "a manifest signed by an unrelated key verified against the pinned one"
        );
        // Garbage still refuses, and the read side still degrades rather than errors.
        assert!(verify(d, "00").is_err());
        let conn = crate::state::db::open_in_memory().unwrap();
        assert!(cached(&conn).is_empty(), "an empty cache must read as no catalog");
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

    /// **An `openssl`-signed manifest verifies in `ring`.** The interop leg, and
    /// the one that would otherwise fail on release day.
    ///
    /// `scripts/publish-php-manifest.sh` signs with `openssl pkeyutl -rawin`; the
    /// app verifies with `ring`'s ED25519. Both are "ed25519" and that proves
    /// nothing about the wire format — key encoding, signature encoding and
    /// whether the tool pre-hashes are all places two correct implementations
    /// disagree. Every other test here signs with ring and verifies with ring,
    /// which cannot see a mismatch at all.
    ///
    /// The fixture is REAL output from that script's exact commands, pasted in.
    /// If openssl's format ever changes, or the script's flags drift, this fails
    /// here rather than as "the manifest signature does not verify" on a user's
    /// machine with nobody able to tell whose fault it is.
    #[test]
    fn a_manifest_signed_by_openssl_verifies_in_ring() {
        // Generated 18 Aug 2026 by: openssl genpkey -algorithm ed25519,
        // then `openssl pkeyutl -sign -rawin`, exactly as the publish script does.
        const PUB: &str = "380f17a77f4d9b976c0c0c33f2588ef9f2c6c0086c3c4cdeb56765e2f4f14556";
        const SIG: &str = "b996ad973ac3722ed4691f9944bef02e48916be5b15e152cdf15fcf1022d46f6c2a864d98cb10e1c65379c5ae1cda7140fbbf7d09f20688959c22d12edfd6a0e";
        // Byte-exact: a single added space changes the signature, which is the
        // property being relied on.
        const DOC: &str = r#"{"serial":42,"generatedAt":"2026-08-18T00:00:00Z","minAppVersion":"0.3.0","artifacts":[{"name":"php","version":"8.3.9999","arch":"arm64","url":"https://dl.static-php.dev/static-php-cli/bulk/x.tar.gz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#;

        assert_eq!(PUB.len(), 64, "an ed25519 public key is 32 bytes");
        assert_eq!(SIG.len(), 128, "an ed25519 signature is 64 bytes");

        let m = verify_with(PUB, DOC.as_bytes(), SIG)
            .expect("openssl's signature must verify in ring — the release path depends on it");
        assert_eq!(m.serial, 42);
        // …and the document survived the structural limits, so the shape the
        // script emits is a shape the app accepts. A verified document whose
        // every entry is then dropped would be a silent no-op release.
        assert_eq!(m.artifacts.len(), 1, "the script's entry shape was rejected");
        assert_eq!(m.artifacts[0].version, "8.3.9999");

        // And the tamper direction, on the real fixture: one flipped character.
        let bad = DOC.replacen("42", "43", 1);
        assert!(verify_with(PUB, bad.as_bytes(), SIG).is_err());
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
        // And nothing was displaced: the stored document is still serial 7's.
        assert_eq!(store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(), Some("7"));
        let stored = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        assert!(stored.contains(&newv), "the replay overwrote the newer document");
        assert!(!stored.contains(&oldv));
    }

    /// **The SAME serial is the document we already have, not an incident.**
    ///
    /// The manifest only changes when a new PHP patch is published, so on every
    /// launch after the first the app re-fetches the identical document. That
    /// used to be refused with "refusing a replay", and the sentence landed in a
    /// user's log at every launch, about rexenv's own current manifest — a log
    /// line that cries wolf daily is worse than none, because it trains the
    /// reader to scroll past the one that means it.
    ///
    /// Equal ACCEPTS (returning the catalog, so the ordinary path just works) and
    /// writes NOTHING: two documents sharing a serial is a publisher error rather
    /// than an attack — both carry our signature — and not writing means a
    /// same-serial variant cannot quietly displace what was stored.
    #[test]
    fn the_same_serial_is_accepted_as_a_no_op_and_never_called_a_replay() {
        let (pub_hex, kp) = keypair();
        let conn = crate::state::db::open_in_memory().unwrap();
        let minor = php::minor_of(binaries::PHP_VERSION);
        let newv = format!("{minor}.9999");
        let oldv = format!("{minor}.9998");

        let d7 = doc(7, &[&newv]);
        accept_with(&pub_hex, &conn, &d7, &sign(&kp, &d7)).expect("serial 7 accepted");

        // Re-fetching the IDENTICAL document: accepted, catalog returned.
        let again = accept_with(&pub_hex, &conn, &d7, &sign(&kp, &d7))
            .expect("re-reading our own current manifest must not be an error");
        assert_eq!(again.newer_than(&minor, binaries::PHP_VERSION, "arm64"), None);
        assert!(again.versions().iter().any(|v| v == &newv));

        // A DIFFERENT document at the same serial changes nothing on disk.
        let variant = doc(7, &[&oldv]);
        accept_with(&pub_hex, &conn, &variant, &sign(&kp, &variant)).expect("also ours");
        let stored = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        assert!(
            stored.contains(&newv) && !stored.contains(&oldv),
            "a same-serial variant displaced the stored document"
        );
        assert_eq!(store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(), Some("7"));
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

        // A COMPLETE catalog: both binaries, both arches, exactly as a published
        // manifest carries them. An earlier fixture listed only `php`/`arm64` and
        // made the completeness rule below untestable — the friendlier-fixture
        // family this project keeps paying for.
        let full = |vs: &[&str]| VersionCatalog {
            entries: vs
                .iter()
                .flat_map(|v| {
                    ["php", "php-fpm"].into_iter().flat_map(move |n| {
                        ["arm64", "x86_64"].map(move |a| Artifact {
                            name: n.into(),
                            version: (*v).to_string(),
                            arch: a.into(),
                            url: "https://dl.static-php.dev/x".into(),
                            sha256: "a".repeat(64),
                        })
                    })
                })
                .collect(),
        };
        let cat = full(&["8.3.32", "8.3.9", "8.3.40", "8.4.99"]);
        assert_eq!(cat.newer_than("8.3", "8.3.31", "arm64").as_deref(), Some("8.3.40"));
        assert_eq!(cat.newer_than("8.3", "8.3.40", "arm64"), None, "nothing newer than the newest");
        assert_eq!(cat.newer_than("8.5", "8.5.8", "arm64"), None, "a minor with no entries offers nothing");
        assert_eq!(
            cat.newer_than("8.3", "8.3.31", "x86_64").as_deref(),
            Some("8.3.40"),
            "a complete manifest serves both Macs"
        );

        // HALF-PUBLISHED, three ways. An apply resolves php AND php-fpm for the
        // machine it is on, so each of these would offer a button that downloads
        // ~100 MB and fails at the last resolve — and the arch case would do it
        // on one developer's Mac while working on another's.
        let drop_where = |f: fn(&Artifact) -> bool| VersionCatalog {
            entries: full(&["8.3.40"]).entries.into_iter().filter(|a| !f(a)).collect(),
        };
        assert_eq!(
            drop_where(|a| a.name == "php-fpm").newer_than("8.3", "8.3.31", "arm64"),
            None,
            "cli only: the pool binary is missing, so there is nothing to restart onto"
        );
        assert_eq!(
            drop_where(|a| a.name == "php").newer_than("8.3", "8.3.31", "arm64"),
            None,
            "fpm only: the terminal and wp-cli would have no interpreter"
        );
        let one_arch = drop_where(|a| a.arch == "x86_64");
        assert_eq!(
            one_arch.newer_than("8.3", "8.3.31", "arm64").as_deref(),
            Some("8.3.40"),
            "the arch it WAS published for is still offered"
        );
        assert_eq!(
            one_arch.newer_than("8.3", "8.3.31", "x86_64"),
            None,
            "…and the arch it was not published for is offered nothing"
        );
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
