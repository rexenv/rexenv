//! core::tld — the development-TLD policy (configurable-TLD v1).
//!
//! A site's TLD becomes an `/etc/resolver/<tld>` file, which shadows that
//! ENTIRE top-level domain system-wide on the developer's machine — every
//! `*.<tld>` lookup, in every app, is answered with 127.0.0.1. So which TLDs
//! we accept is a POLICY decision enforced in the backend (`validate_domain`
//! calls [`ensure_allowed`] for both create and change-domain): a blocked TLD
//! is refused even via a direct IPC invoke, not just hidden in the UI.
//!
//! Three tiers:
//! - **Blocked** — refused outright: TLDs that break the OS or shadow real
//!   infrastructure (`.local` fights Bonjour/mDNS; ICANN-owned gTLDs like
//!   `.dev`/`.app`; every 2-letter country code; popular gTLDs).
//! - **Warn** — allowed, but the UI shows a "may shadow a real internet TLD"
//!   notice: anything outside the safe set (e.g. `.rex`).
//! - **Safe** — reserved for local/testing use by RFC 2606/6761 (`test`,
//!   `localhost`, `example`, `invalid`): never delegated on the internet.

use crate::error::{Error, Result};

/// The backbone TLD. `.test` stays PERMANENTLY active regardless of the
/// configured default: the internal Adminer vhost (`adminer.rexenv.test`) and
/// existing sites depend on it, and it's the RFC 6761 TLD reserved for exactly
/// this. It is always in the safe set and can never be blocked.
pub const BACKBONE_TLD: &str = "test";

/// TLDs reserved for local/testing use (RFC 2606 / RFC 6761) — the only ones
/// that can never collide with a real internet TLD, so no shadow warning.
pub const SAFE_TLDS: &[&str] = &["test", "localhost", "example", "invalid"];

/// Hard-blocked TLDs with the reason each is refused. An `/etc/resolver/<tld>`
/// file shadows the whole real TLD system-wide, so anything with existing
/// OS-level or internet-level meaning is refused outright. All 2-letter TLDs
/// (country codes) are blocked by a length rule in [`blocked_reason`], on top
/// of this list.
const HARD_BLOCKED: &[(&str, &str)] = &[
    (
        "local",
        ".local is used by Bonjour/mDNS — shadowing it breaks printers, AirDrop and other local-network discovery",
    ),
    ("dev", ".dev is a real Google-owned TLD (with preloaded HTTPS) — shadowing it hides every real .dev site"),
    ("app", ".app is a real Google-owned TLD — shadowing it hides every real .app site"),
    ("page", ".page is a real Google-owned TLD — shadowing it hides every real .page site"),
    ("home", ".home is reserved by ICANN for home networks — routers and ISPs use it"),
    ("corp", ".corp is reserved by ICANN for corporate intranets — VPN and AD setups use it"),
    ("mail", ".mail is reserved because mail systems treat it specially"),
    ("com", ".com is the most-used real TLD — shadowing it would break most of the internet on this machine"),
    ("net", ".net is a major real TLD — shadowing it hides every real .net site"),
    ("org", ".org is a major real TLD — shadowing it hides every real .org site"),
    ("cloud", ".cloud is a popular real TLD — shadowing it hides every real .cloud site"),
    ("site", ".site is a popular real TLD — shadowing it hides every real .site site"),
    ("online", ".online is a popular real TLD — shadowing it hides every real .online site"),
];

/// Policy classification of a TLD, for the UI (Settings picker, dialogs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TldPolicy {
    /// Whether the backend accepts domains under this TLD at all.
    pub allowed: bool,
    /// Allowed but outside the safe set — "may shadow a real internet TLD".
    pub warn: bool,
    /// Why a blocked TLD is refused (empty when allowed).
    pub reason: String,
}

/// Syntax check for a bare TLD label: 1–63 chars, lowercase letters only.
/// Real TLDs are alphabetic; digits/hyphens in a TLD are a typo or an attempt
/// to smuggle something odd into a resolver filename / config token.
fn syntax_error(tld: &str) -> Option<String> {
    if tld.is_empty() || tld.len() > 63 {
        return Some("a TLD must be 1–63 characters".into());
    }
    if !tld.bytes().all(|b| b.is_ascii_lowercase()) {
        return Some("a TLD may contain only lowercase letters a–z".into());
    }
    None
}

/// The refusal reason for `tld`, or `None` when it's allowed. Checks syntax,
/// the 2-letter (country-code) rule, and the hard-block list. The safe set is
/// never blocked — `.test` in particular must always stay usable.
pub fn blocked_reason(tld: &str) -> Option<String> {
    if let Some(err) = syntax_error(tld) {
        return Some(err);
    }
    if SAFE_TLDS.contains(&tld) {
        return None;
    }
    if tld.len() == 2 {
        return Some(format!(
            ".{tld} is a 2-letter TLD (a real or future country code) — shadowing it hides every real .{tld} site"
        ));
    }
    HARD_BLOCKED
        .iter()
        .find(|(t, _)| *t == tld)
        .map(|(_, why)| (*why).to_string())
}

/// Classify a TLD for the UI: blocked (with reason), warn, or safe.
pub fn classify(tld: &str) -> TldPolicy {
    match blocked_reason(tld) {
        Some(reason) => TldPolicy { allowed: false, warn: false, reason },
        None => TldPolicy {
            allowed: true,
            warn: !SAFE_TLDS.contains(&tld),
            reason: String::new(),
        },
    }
}

/// Refuse a blocked TLD with a clear message. The backend trust boundary:
/// called by `validate_domain` (create + change-domain) and `set_default_tld`.
pub fn ensure_allowed(tld: &str) -> Result<()> {
    match blocked_reason(tld) {
        Some(reason) => Err(Error::Other(format!("TLD '.{tld}' can't be used: {reason}"))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tlds_are_allowed_without_warning() {
        for t in ["test", "localhost", "example", "invalid"] {
            let p = classify(t);
            assert!(p.allowed, ".{t} must be allowed");
            assert!(!p.warn, ".{t} is in the safe set — no shadow warning");
            assert!(ensure_allowed(t).is_ok());
        }
    }

    /// `.test` can never be blocked — the Adminer vhost and existing sites
    /// depend on it staying active regardless of the configured default.
    #[test]
    fn backbone_tld_is_permanently_allowed() {
        assert_eq!(BACKBONE_TLD, "test");
        assert!(SAFE_TLDS.contains(&BACKBONE_TLD));
        assert!(blocked_reason(BACKBONE_TLD).is_none());
    }

    #[test]
    fn hard_blocked_tlds_are_refused_with_a_reason() {
        for t in [
            "local", "dev", "app", "page", "home", "corp", "mail", // OS / ICANN-reserved
            "com", "net", "org", "cloud", "site", "online", // popular gTLDs
        ] {
            let p = classify(t);
            assert!(!p.allowed, ".{t} must be blocked");
            assert!(!p.reason.is_empty(), ".{t} must carry a reason");
            let err = ensure_allowed(t).unwrap_err().to_string();
            assert!(err.contains(&format!(".{t}")), "message names the TLD: {err}");
        }
    }

    #[test]
    fn all_two_letter_tlds_are_refused() {
        for t in ["io", "co", "uk", "de", "ai", "sh", "me", "us"] {
            let err = ensure_allowed(t).unwrap_err().to_string();
            assert!(err.contains("2-letter"), ".{t}: {err}");
        }
    }

    #[test]
    fn unknown_tlds_are_allowed_with_a_shadow_warning() {
        for t in ["rex", "wip", "mycompany", "internal"] {
            let p = classify(t);
            assert!(p.allowed, ".{t} must be allowed");
            assert!(p.warn, ".{t} is outside the safe set — must warn");
            assert!(p.reason.is_empty());
        }
    }

    #[test]
    fn bad_syntax_is_refused() {
        for t in ["", "t3st", "my-tld", "REX", "a.b", "1", &"x".repeat(64)] {
            assert!(ensure_allowed(t).is_err(), "should refuse TLD {t:?}");
        }
    }
}
