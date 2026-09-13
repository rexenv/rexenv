//! The owner-only security descriptor, as SDDL text.
//!
//! Pure string work with no Windows API, so the macOS test host runs these tests
//! too: `platform/mod.rs` includes this file under `cfg(test)` there. The Win32
//! calls that apply the descriptor live in `platform/windows/mod.rs`
//! (`WindowsPermissions`), and only a Windows run can show them working (ledger #597).

/// SDDL for a file only the user `user_sid` may touch.
///
/// `D:P` — a PROTECTED DACL, so nothing is inherited from the folder above (a
/// file in `%LOCALAPPDATA%` would otherwise pick up SYSTEM and Administrators
/// entries). One ACE: `A;;FA;;;<sid>`, full file access for that user. No SYSTEM,
/// no Administrators, no Everyone — the Windows reading of Unix `0600`, where the
/// owner alone has an entry. An administrator can still take ownership, as root
/// can still read a `0600` file; that is the same boundary, not a weaker one.
/// No SYSTEM entry is an owner ruling (13 Sep 2026, ledger #597): if something
/// Windows needs turns out to fail without one, bring the finding back rather than
/// quietly adding an ACE here.
///
/// `None` unless `user_sid` is a well-formed SID string (`S-1-` then dash-separated
/// decimal parts), so nothing but a SID can ever be spliced into the descriptor.
pub fn owner_only_sddl(user_sid: &str) -> Option<String> {
    let parts = user_sid.strip_prefix("S-1-")?;
    let well_formed = !parts.is_empty()
        && parts
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    well_formed.then(|| format!("D:P(A;;FA;;;{user_sid})"))
}

#[cfg(test)]
mod tests {
    use super::owner_only_sddl;

    #[test]
    fn a_user_sid_becomes_one_protected_full_access_entry() {
        assert_eq!(
            owner_only_sddl("S-1-5-21-3623811015-3361044348-30300820-1013").as_deref(),
            Some("D:P(A;;FA;;;S-1-5-21-3623811015-3361044348-30300820-1013)")
        );
    }

    #[test]
    fn anything_but_a_sid_is_refused_rather_than_spliced_in() {
        for bad in [
            "",
            "WD",
            "S-1-",
            "S-1-5--21",
            "S-1-5-21-1)(A;;FA;;;WD",
            "S-1-5-21-1 ",
            "s-1-5-21-1",
            "S-2-5-21-1",
        ] {
            assert_eq!(owner_only_sddl(bad), None, "{bad:?} must not become a descriptor");
        }
    }
}
