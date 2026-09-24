//! Resolver FILES — the macOS route: one `/etc/resolver/<tld>` per TLD, owned by exact content (ledger
//! #617) — and, since 24 Sep 2026, the Linux route's markers under `/etc/rexenv/dns.d`, the same bytes. Pure file reads over a path or a directory, so the classification is testable against fixture
//! files, and the platform's `DnsManager` and the core's test platforms share one definition rather than
//! a copy each. Moved here from `core/dns.rs` when the trait stopped being file-shaped (W6 R1).
#![cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]

use crate::platform::traits::ResolverOwner;

/// The classification itself, over a path + signature so it is unit-testable
/// against fixture files (same reason [`tlds_matching_signature`] takes a dir):
/// the dev machine has no foreign resolver file to exercise this against, and
/// creating a root-owned one to test would be worse than a fixture.
///
/// A file we cannot READ counts as foreign, never absent — refusing to touch
/// what we can't inspect is the safe direction.
pub(crate) fn owner_of(path: &std::path::Path, signature: &str) -> ResolverOwner {
    match std::fs::read_to_string(path) {
        Ok(c) if c == signature => ResolverOwner::Ours,
        Ok(content) => ResolverOwner::Foreign { content: Some(content) },
        Err(_) if path.exists() => ResolverOwner::Foreign { content: None },
        Err(_) => ResolverOwner::Absent,
    }
}

/// The TLDs whose resolver files under `dir` are OURS — file content equals
/// `signature` (`resolver_contents(port)`, i.e. loopback + our fixed port —
/// the same ownership test as service adoption's port+marker). Pure directory
/// scan, factored out of [`installed_tlds`] for testability. Non-UTF8 names
/// and unreadable/foreign files are skipped.
///
/// The name is ALSO required to be a syntactically valid TLD label
/// (`tld::is_valid_label`, `[a-z]{1,63}`): every resolver file rexenv writes has
/// that shape (creation passes `ensure_allowed`), so this excludes nothing of
/// ours, but it means a scanned filename that ISN'T ours-by-construction can
/// never reach the privileged `rm` in `uninstall_command` — a foreign file with
/// a shell-metachar name + our signature is dropped here, not interpolated into
/// a root shell (B10).
pub(crate) fn tlds_matching_signature(dir: &std::path::Path, signature: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tlds: Vec<String> = entries
        .flatten()
        .filter(|e| std::fs::read_to_string(e.path()).ok().as_deref() == Some(signature))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| crate::core::tld::is_valid_label(name))
        .collect();
    tlds.sort();
    tlds
}

/// The complement of [`tlds_matching_signature`]: every valid-label file in the
/// resolver directory that is NOT ours — a different port, extra options, or a
/// file we cannot read (refusing to classify what we can't inspect as ours is
/// the safe direction, same as `owner_of`). These are the TLDs another tool
/// answers on this machine.
pub(crate) fn tlds_not_matching_signature(dir: &std::path::Path, signature: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tlds: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| std::fs::read_to_string(e.path()).ok().as_deref() != Some(signature))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| crate::core::tld::is_valid_label(name))
        .collect();
    tlds.sort();
    tlds
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ownership classification, against fixture files.
    ///
    /// Fixtures rather than a live check by necessity: the dev Mac has no
    /// foreign `/etc/resolver/<tld>`, and creating a root-owned one to test
    /// against would be a worse idea than this. The takeover/restore paths are
    /// tracked as a clean-VM item in docs/PUBLISH-TESTING.md §F.
    #[test]
    fn owner_of_classifies_ours_foreign_and_absent() {
        let dir = std::env::temp_dir().join(format!("rexenv-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sig = "nameserver 127.0.0.1\nport 15353\n";

        // Absent.
        assert_eq!(owner_of(&dir.join("nothing"), sig), ResolverOwner::Absent);

        // Ours — byte-exact.
        let ours = dir.join("rex");
        std::fs::write(&ours, sig).unwrap();
        assert_eq!(owner_of(&ours, sig), ResolverOwner::Ours);

        // Valet's real shape: same nameserver, NO port line. This is the one
        // that used to be silently overwritten.
        let valet = dir.join("test");
        std::fs::write(&valet, "nameserver 127.0.0.1\n").unwrap();
        assert_eq!(
            owner_of(&valet, sig),
            ResolverOwner::Foreign { content: Some("nameserver 127.0.0.1\n".into()) },
            "a Valet resolver file must read as FOREIGN, never as ours"
        );

        // Our nameserver but a different port — still theirs.
        let other = dir.join("dev");
        std::fs::write(&other, "nameserver 127.0.0.1\nport 5333\n").unwrap();
        assert!(matches!(owner_of(&other, sig), ResolverOwner::Foreign { .. }));

        // Even a near-miss (trailing newline dropped) is foreign, not ours —
        // equality is the whole ownership notion, so it must not be fuzzy.
        let near = dir.join("near");
        std::fs::write(&near, "nameserver 127.0.0.1\nport 15353").unwrap();
        assert!(matches!(owner_of(&near, sig), ResolverOwner::Foreign { .. }));

        // Present but unreadable counts as FOREIGN (never absent): refusing to
        // touch what we can't inspect is the safe direction. Skipped when the
        // test runs as a user who can read it anyway (e.g. root in CI).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.join("locked");
            std::fs::write(&locked, "whatever\n").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read_to_string(&locked).is_err() {
                assert_eq!(owner_of(&locked, sig), ResolverOwner::Foreign { content: None });
            }
            let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Enumerating our resolver files: exact-content signature match — foreign
    /// files (a developer's own dnsmasq entry, different port) are never touched.
    #[test]
    fn tlds_matching_signature_finds_only_our_files() {
        let dir = std::env::temp_dir().join(format!("rexenv-resolver-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sig = "nameserver 127.0.0.1\nport 15353\n".to_string();

        std::fs::write(dir.join("test"), &sig).unwrap();
        std::fs::write(dir.join("rex"), &sig).unwrap();
        // Foreign: another tool's resolver on a different port, and a
        // same-nameserver file with extra options — neither is ours.
        std::fs::write(dir.join("docker"), "nameserver 127.0.0.1\nport 19999\n").unwrap();
        std::fs::write(dir.join("dev"), "nameserver 127.0.0.1\n").unwrap();
        // Files with OUR signature but a name that isn't a valid TLD label
        // ([a-z]{1,63}) can't be ours-by-construction — and must never reach the
        // privileged `rm`. Shell-metachar / space / uppercase / digit names are
        // dropped here (B10). rexenv could never have created any of these.
        for bad in ["evil;reboot", "a b", "UP", "x9", "back`tick`"] {
            std::fs::write(dir.join(bad), &sig).unwrap();
        }

        // Only the two valid-label files with our signature survive the sweep.
        assert_eq!(tlds_matching_signature(&dir, &sig), vec!["rex", "test"]);
        // Missing dir → empty, not an error (fresh machine, nothing installed).
        assert!(tlds_matching_signature(&dir.join("nope"), &sig).is_empty());

        // The complement the import scan lists: the two foreign files, and NOT
        // ours, and NOT the bad-label files either — a name rexenv could never
        // create is also a name it must never offer to take over (the offer
        // ends in a privileged write to that path).
        assert_eq!(tlds_not_matching_signature(&dir, &sig), vec!["dev", "docker"]);
        assert!(tlds_not_matching_signature(&dir.join("nope"), &sig).is_empty());
        // A subdirectory is not a resolver file.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert_eq!(tlds_not_matching_signature(&dir, &sig), vec!["dev", "docker"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
