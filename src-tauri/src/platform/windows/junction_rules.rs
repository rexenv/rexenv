//! The pure half of Windows' linked folders (W7 S6, plan §5 W7, ledger #625): what a junction may point at,
//! and the mount-point reparse data that makes one. No Win32 here — `junction.rs` makes the calls — so this is
//! compiled into the macOS test build.
//!
//! A directory SYMBOLIC link needs Developer Mode or a privilege (`symlink_dir` was refused with 1314 on the
//! Dell, Developer Mode off — measured), so "Link folder" makes a directory JUNCTION, which any user may
//! create. Measured the same day (`windows_desktop_probe`): `symlink_metadata(..).file_type().is_symlink()` is
//! TRUE for a junction — so the delete guard that must never let `wp plugin delete` walk into a linked
//! checkout (`core::repo::partition_symlink_deletes`) sees one — and `remove_dir` removes the junction and
//! leaves the target's files, while `remove_file` is refused.
//!
//! A junction can only point at a folder on a LOCAL volume, by an absolute path.

/// `IO_REPARSE_TAG_MOUNT_POINT` — a junction.
pub(crate) const MOUNT_POINT_TAG: u32 = 0xA000_0003;

/// The path a junction to `canonical` stores, from a canonical path (`std::fs::canonicalize` gives a verbatim
/// `\\?\C:\…`): the prefix dropped, a drive letter required, no trailing separator except a drive's root. A
/// network target (`\\?\UNC\…`, `\\server\share`) is refused — a junction cannot point there.
pub(crate) fn junction_target(canonical: &str) -> Result<String, String> {
    let s = canonical.replace('/', "\\");
    if s.starts_with(r"\\?\UNC\") || (s.starts_with(r"\\") && !s.starts_with(r"\\?\")) {
        return Err(format!(
            "{canonical} is on a network share — a linked folder must be on this computer's own drives"
        ));
    }
    let path = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !drive {
        return Err(format!("{canonical} is not an absolute path on a drive"));
    }
    let trimmed = path.trim_end_matches('\\');
    Ok(if trimmed.len() == 2 { format!("{trimmed}\\") } else { trimmed.to_string() })
}

/// The `REPARSE_DATA_BUFFER` for `FSCTL_SET_REPARSE_POINT` that makes a junction to `target` (from
/// [`junction_target`]): the mount-point tag, then the substitute name `\??\<target>` and the print name
/// `<target>`, each NUL-terminated UTF-16, with lengths in bytes that exclude the terminators.
pub(crate) fn mount_point_buffer(target: &str) -> Vec<u8> {
    let substitute: Vec<u16> = format!(r"\??\{target}").encode_utf16().collect();
    let print: Vec<u16> = target.encode_utf16().collect();
    let sub_len = (substitute.len() * 2) as u16;
    let print_len = (print.len() * 2) as u16;
    // Four u16 fields, then both names with their terminators.
    let data_len = 8 + sub_len + 2 + print_len + 2;
    let mut buf = Vec::with_capacity(8 + data_len as usize);
    buf.extend_from_slice(&MOUNT_POINT_TAG.to_le_bytes());
    buf.extend_from_slice(&data_len.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // SubstituteNameOffset
    buf.extend_from_slice(&sub_len.to_le_bytes());
    buf.extend_from_slice(&(sub_len + 2).to_le_bytes()); // PrintNameOffset, past the substitute's NUL
    buf.extend_from_slice(&print_len.to_le_bytes());
    for unit in substitute.iter().chain([0u16].iter()).chain(print.iter()).chain([0u16].iter()) {
        buf.extend_from_slice(&unit.to_le_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn name(b: &[u8], offset: u16, len: u16) -> String {
        let start = 16 + offset as usize;
        let units: Vec<u16> = b[start..start + len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    }

    /// Ledger #625 — a junction points at a local drive path, never a share.
    #[test]
    fn a_junction_points_at_a_local_drive_path() {
        assert_eq!(junction_target(r"\\?\C:\Users\A B\code\my-plugin").as_deref(), Ok(r"C:\Users\A B\code\my-plugin"));
        assert_eq!(junction_target(r"C:\Users\A B\code\my-plugin\").as_deref(), Ok(r"C:\Users\A B\code\my-plugin"));
        assert_eq!(junction_target(r"\\?\D:\").as_deref(), Ok(r"D:\"));
        assert_eq!(junction_target("C:/code/theme").as_deref(), Ok(r"C:\code\theme"));
        // A share is named as one — the drive check alone refuses it too, but with words that send the user
        // looking for a typo (a first version of this test passed with the share guard planted out).
        for share in [r"\\?\UNC\nas\share\plugin", r"\\nas\share\plugin"] {
            let why = junction_target(share).unwrap_err();
            assert!(why.contains("network share"), "{share}: {why}");
        }
        for bad in [r"relative\plugin", r"\\?\Volume{1234}\x"] {
            assert!(junction_target(bad).is_err(), "{bad}");
        }
    }

    /// Ledger #625 — the reparse data names the target the way a junction must: `\??\` substitute, plain print.
    #[test]
    fn the_mount_point_buffer_carries_both_names_with_their_lengths() {
        let target = r"C:\Users\A B\code\my-plugin";
        let b = mount_point_buffer(target);
        assert_eq!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), MOUNT_POINT_TAG);
        let data_len = u16_at(&b, 4);
        assert_eq!(b.len(), 8 + data_len as usize, "the header length covers everything after it");
        let (sub_off, sub_len, print_off, print_len) = (u16_at(&b, 8), u16_at(&b, 10), u16_at(&b, 12), u16_at(&b, 14));
        assert_eq!(name(&b, sub_off, sub_len), format!(r"\??\{target}"));
        assert_eq!(name(&b, print_off, print_len), target);
        assert_eq!(u16_at(&b, 16 + (sub_off + sub_len) as usize), 0, "the substitute name is NUL-terminated");
        assert_eq!(u16_at(&b, 16 + (print_off + print_len) as usize), 0, "the print name is NUL-terminated");
    }
}
