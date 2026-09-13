//! Which of this process's handles a child would inherit — the pure parse of
//! `NtQueryInformationProcess(ProcessHandleSnapshotInformation)`, with no Win32 in it, so
//! it is also compiled into the macOS test build (`platform/mod.rs`) and tested on every
//! host (ledger #600).
//!
//! # Why rexenv clears them before spawning a service
//!
//! Rust's `Command` on Windows calls `CreateProcessW` with `bInheritHandles = TRUE`, so a
//! child receives EVERY handle already marked inheritable in the parent — not only the
//! stdio it was given. Measured on the Dell 13 Sep 2026: phase 1 of
//! `windows_supervision_check`, started over SSH, exited, and its session stayed open.
//! Reading the whole handle table while it hung showed `mysqld` and `mailpit` each holding
//! 20–25 inheritable File handles with the SAME values and objects as the session's
//! `sshd.exe` and `powershell.exe` — handles the check process had itself been born with
//! from sshd and passed on. The moment phase 2 stopped the two services, the stuck session
//! closed by itself. Clearing only the three standard handles (the first fix) changed
//! nothing, because most of those handles were not standard ones.

/// `OBJ_INHERIT` in a handle entry's attributes.
pub(crate) const OBJ_INHERIT: u32 = 0x2;

/// `PROCESS_HANDLE_SNAPSHOT_INFORMATION`: `NumberOfHandles`, `Reserved` (8 bytes each on
/// x64), then entries.
const HEADER_LEN: usize = 16;

/// `PROCESS_HANDLE_TABLE_ENTRY_INFO` on x64: `HandleValue` (8), `HandleCount` (8),
/// `PointerCount` (8), `GrantedAccess` (4), `ObjectTypeIndex` (4), `HandleAttributes` (4),
/// `Reserved` (4).
const ENTRY_LEN: usize = 40;
const ATTRIBUTES_AT: usize = 32;

/// The handle values in a snapshot buffer that are marked inheritable. A count larger
/// than the buffer holds is clamped to the entries actually present.
pub(crate) fn inheritable_handles(buf: &[u8]) -> Vec<usize> {
    let Some(count) = buf.get(0..8).map(|b| u64::from_le_bytes(b.try_into().unwrap())) else {
        return Vec::new();
    };
    let present = buf.len().saturating_sub(HEADER_LEN) / ENTRY_LEN;
    let entries = usize::try_from(count).unwrap_or(usize::MAX).min(present);
    (0..entries)
        .filter_map(|i| {
            let e = &buf[HEADER_LEN + i * ENTRY_LEN..HEADER_LEN + (i + 1) * ENTRY_LEN];
            let value = u64::from_le_bytes(e[0..8].try_into().unwrap());
            let attributes = u32::from_le_bytes(e[ATTRIBUTES_AT..ATTRIBUTES_AT + 4].try_into().unwrap());
            (attributes & OBJ_INHERIT != 0).then_some(value as usize)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(entries: &[(u64, u32)], claimed: u64) -> Vec<u8> {
        let mut buf = claimed.to_le_bytes().to_vec();
        buf.extend(0u64.to_le_bytes());
        for &(value, attributes) in entries {
            let mut e = vec![0xEEu8; ENTRY_LEN]; // noise in the counts, access and type
            e[0..8].copy_from_slice(&value.to_le_bytes());
            e[ATTRIBUTES_AT..ATTRIBUTES_AT + 4].copy_from_slice(&attributes.to_le_bytes());
            e[36..40].copy_from_slice(&0u32.to_le_bytes());
            buf.extend(e);
        }
        buf
    }

    /// The shapes the Dell's handle table showed: inherited File handles carry attribute
    /// 2; the process's own carry 0; a protected-and-inheritable one carries 3.
    #[test]
    fn only_entries_marked_inherit_are_returned() {
        let buf = snapshot(&[(0x34, 0), (0x174, 2), (0x178, 2), (0x1dc, 3), (0x200, 1)], 5);
        assert_eq!(inheritable_handles(&buf), vec![0x174, 0x178, 0x1dc]);
    }

    #[test]
    fn a_claimed_count_past_the_buffer_is_clamped_and_short_buffers_are_empty() {
        let buf = snapshot(&[(0x10, 2), (0x14, 2)], 9_999);
        assert_eq!(inheritable_handles(&buf), vec![0x10, 0x14]);
        assert!(inheritable_handles(&[]).is_empty());
        assert!(inheritable_handles(&snapshot(&[], 0)[..12]).is_empty());
    }
}
