//! core::firefox — make the local CA visible to Firefox (QA P0-2).
//!
//! Firefox does NOT read the OS trust store directly: it ships its own (NSS).
//! Since Firefox 120 (Nov 2023) it IMPORTS OS roots — including our login-
//! keychain CA — when `security.enterprise_roots.enabled` is on, and that pref
//! defaults ON. So a modern default Firefox works with rexenv out of the box
//! (verified live: fresh profile 152.0.5 loads a rexenv site; the same profile
//! with the pref forced off hangs on the TLS error). What breaks: Firefox 68–119
//! (pref exists, default off) or any profile where the pref was flipped off.
//!
//! Fix: force the pref per profile via `user.js` — Firefox's sanctioned
//! override file, read at every startup (a running Firefox picks it up on
//! restart). The mkcert route (inject the CA into each profile's `cert9.db`
//! with NSS `certutil`) would cover even ESR builds with enterprise policies
//! disabled, but needs a Homebrew `nss` install we can't assume — the Settings
//! UI offers manual import instructions for that long tail instead.
//!
//! Platform-agnostic: the per-OS profiles location comes from
//! `CertTrustManager::firefox_profiles_root`; everything here is file I/O on it.

use crate::error::Result;
use std::path::{Path, PathBuf};

/// The Firefox pref that imports OS trust-store roots (our CA) into NSS.
pub const ENTERPRISE_ROOTS_PREF: &str = "security.enterprise_roots.enabled";

/// The exact `user.js` line we enforce.
const USER_JS_LINE: &str = r#"user_pref("security.enterprise_roots.enabled", true);"#;

/// Marker comment so users (and `enable_in_profiles` re-runs) can see what
/// wrote the line and safely remove it.
const USER_JS_MARKER: &str =
    "// Added by rexenv: trust the OS root store (rexenv Local CA). Safe to delete.";

/// Firefox trust state for the Settings UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirefoxTrust {
    /// Firefox has a profiles root with a `profiles.ini` for this user.
    pub installed: bool,
    /// Profiles found in `profiles.ini`.
    pub profiles: usize,
    /// Profiles whose `user.js` already forces the pref on.
    pub forced: usize,
}

/// Profile directories listed in `<root>/profiles.ini` (absolute paths).
/// Minimal INI walk: every `Path=` inside a `[Profile*]` section, honoring
/// `IsRelative` (defaults to relative when absent).
pub fn profiles(root: &Path) -> Vec<PathBuf> {
    let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut in_profile = false;
    let mut path: Option<String> = None;
    let mut relative = true;
    let mut flush = |in_profile: bool, path: &mut Option<String>, relative: bool| {
        if in_profile {
            if let Some(p) = path.take() {
                let dir = if relative { root.join(&p) } else { PathBuf::from(&p) };
                if dir.is_dir() {
                    out.push(dir);
                }
            }
        }
        *path = None;
    };
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            flush(in_profile, &mut path, relative);
            in_profile = line.trim_start_matches('[').starts_with("Profile");
            relative = true;
        } else if let Some(v) = line.strip_prefix("Path=") {
            path = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("IsRelative=") {
            relative = v.trim() != "0";
        }
    }
    flush(in_profile, &mut path, relative);
    out
}

/// Whether a profile's `user.js` already forces the pref ON.
fn user_js_forces_pref(profile: &Path) -> bool {
    std::fs::read_to_string(profile.join("user.js"))
        .map(|s| s.lines().any(|l| is_pref_line(l, true)))
        .unwrap_or(false)
}

/// Is `line` a `user_pref` for OUR pref with the given value? (Whitespace-tolerant.)
fn is_pref_line(line: &str, value: bool) -> bool {
    let squashed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    !squashed.starts_with("//")
        && squashed.contains(&format!("\"{ENTERPRISE_ROOTS_PREF}\",{value}"))
}

/// Merge the pref into existing `user.js` content. `None` = already forced on,
/// nothing to write. A conflicting `false` line is commented out, not deleted.
fn merged_user_js(existing: &str) -> Option<String> {
    if existing.lines().any(|l| is_pref_line(l, true)) {
        return None;
    }
    let mut out = String::new();
    for line in existing.lines() {
        if is_pref_line(line, false) {
            out.push_str("// disabled by rexenv (needs the OS root store): ");
        }
        out.push_str(line);
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(USER_JS_MARKER);
    out.push('\n');
    out.push_str(USER_JS_LINE);
    out.push('\n');
    Some(out)
}

/// Force the pref in every profile under `root`. Returns how many profiles
/// were WRITTEN (already-forced ones are skipped). Errors on the first
/// unwritable profile — partial success is fine, a re-run is idempotent.
pub fn enable_in_profiles(root: &Path) -> Result<usize> {
    let mut written = 0;
    for profile in profiles(root) {
        let user_js = profile.join("user.js");
        let existing = std::fs::read_to_string(&user_js).unwrap_or_default();
        if let Some(merged) = merged_user_js(&existing) {
            std::fs::write(&user_js, merged)?;
            written += 1;
        }
    }
    Ok(written)
}

/// Current Firefox trust state (`root` = `CertTrustManager::firefox_profiles_root`).
pub fn status(root: Option<&Path>) -> FirefoxTrust {
    let Some(root) = root else {
        return FirefoxTrust { installed: false, profiles: 0, forced: 0 };
    };
    let profiles = profiles(root);
    FirefoxTrust {
        installed: !profiles.is_empty(),
        profiles: profiles.len(),
        forced: profiles.iter().filter(|p| user_js_forces_pref(p)).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_root(name: &str, profiles_ini: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-firefox-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("profiles.ini"), profiles_ini).unwrap();
        dir
    }

    #[test]
    fn parses_profiles_ini_relative_and_absolute() {
        let root = fake_root(
            "ini",
            "[Install5]\nDefault=Profiles/a.default\nLocked=1\n\n\
             [Profile1]\nName=default\nIsRelative=1\nPath=Profiles/a.default\nDefault=1\n\n\
             [Profile0]\nName=other\nIsRelative=0\nPath=/tmp\n\n\
             [General]\nVersion=2\n",
        );
        std::fs::create_dir_all(root.join("Profiles/a.default")).unwrap();
        let got = profiles(&root);
        // Relative resolved under root; absolute kept; Install/General ignored;
        // nonexistent dirs dropped.
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], root.join("Profiles/a.default"));
        assert_eq!(got[1], PathBuf::from("/tmp"));
    }

    #[test]
    fn merge_appends_flips_and_skips() {
        // Fresh file: marker + pref.
        let fresh = merged_user_js("").unwrap();
        assert!(fresh.contains(USER_JS_LINE));
        assert!(fresh.contains(USER_JS_MARKER));
        // Existing content preserved, conflicting false line commented out.
        let flipped = merged_user_js(
            "user_pref(\"foo\", 1);\nuser_pref(\"security.enterprise_roots.enabled\", false);\n",
        )
        .unwrap();
        assert!(flipped.contains("user_pref(\"foo\", 1);"));
        assert!(flipped.contains("// disabled by rexenv"));
        assert!(flipped.lines().any(|l| is_pref_line(l, true)));
        assert!(!flipped.lines().any(|l| is_pref_line(l, false)));
        // Already forced → nothing to write (idempotent re-runs).
        assert!(merged_user_js(&fresh).is_none());
        assert!(merged_user_js(" user_pref( \"security.enterprise_roots.enabled\" , true ); ")
            .is_none());
    }

    #[test]
    fn enable_writes_once_and_status_counts() {
        let root = fake_root("enable", "[Profile0]\nName=default\nIsRelative=1\nPath=p0\n");
        std::fs::create_dir_all(root.join("p0")).unwrap();
        assert_eq!(enable_in_profiles(&root).unwrap(), 1);
        assert_eq!(enable_in_profiles(&root).unwrap(), 0); // idempotent
        let st = status(Some(&root));
        assert!(st.installed);
        assert_eq!((st.profiles, st.forced), (1, 1));
        // No Firefox at all.
        let none = status(None);
        assert!(!none.installed && none.profiles == 0);
    }
}
