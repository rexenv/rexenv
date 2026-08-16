//! core::php_upstream — "a newer PHP patch exists upstream", read-only.
//!
//! # What this is, and the line it must not cross
//!
//! rexenv pins every binary it runs, and the pin is compiled in. That is what
//! makes compromising a download host unable to reach an installed user, and
//! `docs/PLAN-binary-updates.md` §12 rules that it stays that way: **there is no
//! signed manifest and no Update button.** The complaint that motivated all of
//! that analysis was smaller than the machinery it would have needed — *am I on
//! something stale?* — and it can be answered without moving the security model
//! at all.
//!
//! So this module fetches ONE document and derives ONE thing from it: a version
//! STRING, per minor, that the UI may render beside the pin. It never reads a
//! URL, never reads a digest, and nothing downstream of it selects bytes. A
//! document that can only make the UI say "8.3.33 exists" cannot make the app
//! run anything, which is why it needs none of §1–§3's trust machinery.
//!
//! **If that ever stops being true — if anything here starts choosing what to
//! download — the whole trust analysis applies again in full, starting at key
//! custody.**
//!
//! # Why php.net, and the honest defect in doing so
//!
//! php.net's release feed is the authority on what PHP has released. It is not
//! the authority on what rexenv can install: 8.x builds come from
//! static-php.dev, which publishes no index and lags. Measured 16 Aug 2026:
//! php.net listed 8.4.24 and 8.5.9 (released 30 Jul) while static-php.dev's bulk
//! directory still topped out at 8.4.23 and 8.5.8 — **which are exactly rexenv's
//! pins**. So for those minors this module reports a newer version that rexenv
//! could not ship even if it wanted to, and had been able to for 17 days.
//!
//! That is survivable ONLY because there is no button. "8.4.24 exists · this
//! build pins 8.4.23" stays true when rexenv cannot install 8.4.24; "update
//! available" would not, and neither would "up to date" — which is also
//! unprovable before the first successful check. The wording is load-bearing,
//! not decoration, and `core::copy_scan` guards it.

use crate::core::php;
use crate::error::{Error, Result};
use crate::state::store;
use rusqlite::Connection;
use std::collections::BTreeMap;

/// php.net's machine-readable list of ACTIVE branches and their newest release.
/// One GET, ~2.3KB, no auth, no rate limit, CDN-cached.
///
/// `active.php` omits end-of-life branches, and that omission is a feature: an
/// EOL minor cannot gain a patch, so the rows that are not in this document are
/// exactly the rows that never need it.
const UPSTREAM_URL: &str = "https://www.php.net/releases/active.php?json";

/// Settings key holding the whole cached answer — timestamp AND versions, in one
/// value.
///
/// **One key, not two.** A separate timestamp key means a crash between the two
/// writes leaves "checked just now" sitting over yesterday's versions, which is
/// precisely the dishonesty the `checked N ago` line exists to prevent.
const CACHE_KEY: &str = "php_upstream_check";

/// Refuse a body larger than this before parsing. `active.php` is ~2.3KB;
/// nothing legitimate approaches 64KiB, and this is a read-only poll rather than
/// a download — it has no business streaming.
const MAX_BODY: usize = 64 * 1024;

/// What the cache holds: when it was taken, and what it found.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamCheck {
    /// `db_now()` at the moment of a SUCCESSFUL fetch. Never written on failure,
    /// so `checked N ago` can never age a failure into a success.
    pub checked_at: String,
    /// Minor → newest upstream patch, for minors rexenv actually ships.
    pub latest: BTreeMap<String, String>,
}

/// Whether `upstream` is a strictly newer patch than `have` **within the same
/// minor**. Numeric per segment, so `8.3.9` < `8.3.10` — a lexical compare gets
/// that backwards, and PHP has shipped double-digit patches on every branch.
///
/// Cross-minor comparisons answer `false` by construction: this exists to fill
/// one row, and a minor's row must never be moved by another minor's release.
pub fn is_newer(upstream: &str, have: &str) -> bool {
    if php::minor_of(upstream) != php::minor_of(have) {
        return false;
    }
    let nums = |v: &str| -> Option<Vec<u32>> { v.split('.').map(|p| p.parse().ok()).collect() };
    match (nums(upstream), nums(have)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Pull minor → newest-patch out of php.net's document.
///
/// Pure, so the contract is testable without a network. Deliberately narrow:
///
/// - only minors rexenv actually ships are kept, so an upstream branch we do not
///   offer can never appear in the UI;
/// - only strict `x.y.z` numeric versions are kept, so an RC/alpha/beta string
///   is dropped rather than rendered as a release;
/// - **`source`, `sha256`, `announcement` and `museum` are never read.** They are
///   present in the document and they are exactly what this module must not
///   touch: reading a URL or a digest here would turn a UI hint into a byte
///   selector, which is the line §12's ruling draws.
pub fn parse(body: &[u8]) -> BTreeMap<String, String> {
    let Ok(doc) = serde_json::from_slice::<serde_json::Value>(body) else {
        return BTreeMap::new();
    };
    let shipped: Vec<String> = php::all_minors();
    let mut out = BTreeMap::new();
    // Shape: { "8": { "8.3": { "version": "8.3.33", … }, … }, … }
    for major in doc.as_object().into_iter().flat_map(|m| m.values()) {
        for branch in major.as_object().into_iter().flat_map(|m| m.values()) {
            let Some(version) = branch.get("version").and_then(|v| v.as_str()) else {
                continue;
            };
            let parts: Vec<&str> = version.split('.').collect();
            if parts.len() != 3
                || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
            {
                continue; // an RC/alpha/beta is not a release
            }
            let minor = php::minor_of(version);
            if shipped.contains(&minor) {
                out.insert(minor, version.to_string());
            }
        }
    }
    out
}

/// The cached answer, or a default one. **Never touches the network**, so every
/// UI read is local and an offline install behaves exactly as it does today.
///
/// A cache that fails to parse reads as "never checked" rather than as an error:
/// this is a hint beside a version number, and it must not be able to fail a
/// screen. (The value lives in the settings KV, which the frontend can write
/// through the generic setter — harmless while nothing but a comparison and a
/// render consumes it, and a reason to keep it that way.)
pub fn cached(conn: &Connection) -> UpstreamCheck {
    store::get_setting(conn, CACHE_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Fetch php.net's list. **Takes no `Connection` on purpose** — the write is
/// [`store_check`], so no caller can hold the database lock across this await
/// (the house rule, and this is a network call nobody is waiting on).
///
/// Best-effort by contract: every failure path returns `Err` and writes nothing,
/// so a flaky network degrades to "checked a while ago" rather than to a blank
/// row, an error screen, or a fresh timestamp over stale data.
pub async fn fetch() -> Result<BTreeMap<String, String>> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // A hard TOTAL deadline, not the download path's retry-and-backoff:
            // this is a poll nobody is waiting on, and it must never be the
            // reason something else is slow.
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(concat!("rexenv/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("php upstream client")
    });
    let res = client
        .get(UPSTREAM_URL)
        .send()
        .await
        .map_err(|e| Error::Other(format!("php.net didn't answer: {e}")))?;
    if !res.status().is_success() {
        return Err(Error::Other(format!("php.net answered {}", res.status())));
    }
    let body = res.bytes().await.map_err(|e| Error::Other(format!("php.net: {e}")))?;
    if body.len() > MAX_BODY {
        return Err(Error::Other(format!(
            "php.net returned {} bytes, more than this poll will read",
            body.len()
        )));
    }
    let latest = parse(&body);
    if latest.is_empty() {
        return Err(Error::Other("php.net's release list named none of our minors".into()));
    }
    Ok(latest)
}

/// Persist a successful [`fetch`], stamping it with the time it landed.
///
/// The timestamp is written HERE, in the same value as the data, so the pair can
/// never disagree — a separate timestamp key would let a crash between two
/// writes leave "checked just now" over yesterday's versions.
pub fn store_check(conn: &Connection, latest: BTreeMap<String, String>) -> Result<UpstreamCheck> {
    let check = UpstreamCheck { checked_at: store::db_now(conn)?, latest };
    let blob = serde_json::to_string(&check)
        .map_err(|e| Error::Other(format!("could not cache the php.net answer: {e}")))?;
    store::set_setting(conn, CACHE_KEY, &blob)?;
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// php.net's real shape, trimmed. Includes the three things the parse must
    /// reject: a minor rexenv does not ship, a non-release version string, and
    /// the `source`/`sha256` keys that must never be read.
    const SAMPLE: &str = r#"{
      "8": {
        "8.3": {"version": "8.3.33", "date": "30 Jul 2026",
                "source": [{"filename": "php-8.3.33.tar.gz",
                            "sha256": "deadbeef"}]},
        "8.4": {"version": "8.4.24", "date": "30 Jul 2026"},
        "8.9": {"version": "8.9.0RC1"}
      },
      "9": { "9.0": {"version": "9.0.1"} }
    }"#;

    #[test]
    fn the_parse_keeps_only_shipped_minors_and_real_releases() {
        let got = parse(SAMPLE.as_bytes());
        assert_eq!(got.get("8.3").map(String::as_str), Some("8.3.33"));
        assert_eq!(got.get("8.4").map(String::as_str), Some("8.4.24"));
        // A minor rexenv does not ship never reaches the UI, even if php.net
        // lists it — the row it would fill does not exist.
        assert_eq!(got.get("8.9"), None);
        assert_eq!(got.get("9.0"), None);
        // An RC is not a release.
        assert!(!got.values().any(|v| v.contains("RC")));
    }

    /// **Nothing but a version string comes out of that document.**
    ///
    /// This is the line the whole read-only ruling rests on: a URL or a digest
    /// read from here would turn a UI hint into a byte selector, and the trust
    /// analysis (`docs/PLAN-binary-updates.md` §1–§3) would apply again in full.
    /// The sample deliberately carries `source` and `sha256` so the assertion is
    /// about a document that HAS them.
    #[test]
    fn nothing_but_a_version_string_survives_the_parse() {
        assert!(SAMPLE.contains("sha256"), "the fixture must carry what we refuse to read");
        assert!(SAMPLE.contains("filename"));
        for v in parse(SAMPLE.as_bytes()).values() {
            assert!(
                v.split('.').count() == 3 && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
                "{v} is not a bare version — something else escaped the parse"
            );
        }
        // And the source text says so: no field of that document but `version`
        // is ever named here.
        // Scanned over the NON-TEST source only: this assertion's own literals
        // are in the file, and counting them is the scanner-counts-itself trap
        // the copy-scan guards already record.
        const SRC: &str = include_str!("php_upstream.rs");
        let code = SRC.split("mod tests").next().unwrap();
        let reads: Vec<&str> = code.match_indices(".get(\"").map(|(i, _)| &code[i..i + 20]).collect();
        assert_eq!(
            reads.len(),
            1,
            "exactly one field may be read out of php.net's document; found {reads:?}"
        );
        assert!(reads[0].starts_with(".get(\"version\")"), "{}", reads[0]);
    }

    /// Garbage in must be an empty answer, never a panic and never an error the
    /// UI has to render.
    #[test]
    fn an_unparseable_document_is_simply_no_answer() {
        assert!(parse(b"").is_empty());
        assert!(parse(b"not json").is_empty());
        assert!(parse(b"[1,2,3]").is_empty());
        assert!(parse(br#"{"8":{"8.3":{}}}"#).is_empty());
    }

    /// Patch comparison is NUMERIC and same-minor only. A lexical compare says
    /// `8.3.9 > 8.3.10`, and PHP has shipped double-digit patches on every
    /// branch it has ever had.
    #[test]
    fn newer_is_numeric_and_never_crosses_a_minor() {
        assert!(is_newer("8.3.10", "8.3.9"), "lexically '10' < '9' — this must not be lexical");
        assert!(is_newer("8.3.33", "8.3.31"));
        assert!(!is_newer("8.3.31", "8.3.31"), "equal is not newer");
        assert!(!is_newer("8.3.30", "8.3.31"), "older is not newer");
        // Another minor's release must never move this row.
        assert!(!is_newer("8.4.24", "8.3.31"));
        assert!(!is_newer("8.4.0", "8.3.99"));
        // Junk compares as "no".
        assert!(!is_newer("8.3.x", "8.3.31"));
        assert!(!is_newer("", "8.3.31"));
    }
}
