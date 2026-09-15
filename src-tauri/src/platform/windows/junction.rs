//! Windows' linked folders (W7 S6, plan §5 W7, ledger #625) — the Win32 half of `junction_rules.rs`: a
//! directory junction made through `FSCTL_SET_REPARSE_POINT`, and a link removed without touching its target.

use super::junction_rules::{junction_target, mount_point_buffer};
use crate::error::{Error, Result};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
use windows_sys::Win32::System::IO::DeviceIoControl;

/// Make `link` a junction to the folder `target` (a canonical path). `link` must not exist; on any failure the
/// empty folder this call created is removed again, and nothing else is.
pub(super) fn create(target: &Path, link: &Path) -> Result<()> {
    let target = junction_target(&target.to_string_lossy()).map_err(Error::Other)?;
    std::fs::create_dir(link)?;
    if let Err(e) = make_mount_point(link, &target) {
        let _ = std::fs::remove_dir(link);
        return Err(e);
    }
    Ok(())
}

fn make_mount_point(link: &Path, target: &str) -> Result<()> {
    let wide: Vec<u16> = link.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path; no security attributes or template. Backup semantics opens a directory,
    // and open-reparse-point opens the folder itself rather than anything it might point at.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let e = std::io::Error::last_os_error();
        return Err(Error::Other(format!("could not open {} to link it: {e}", link.display())));
    }
    let buffer = mount_point_buffer(target);
    let mut returned = 0u32;
    // SAFETY: an open directory handle, an input buffer of the length passed, no output buffer, synchronous.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    let err = std::io::Error::last_os_error();
    // SAFETY: the handle opened above, closed once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return Err(Error::Other(format!("could not link {} to {target}: {err}", link.display())));
    }
    Ok(())
}

/// Remove `link` — only if it IS a link (a junction or a symbolic link), and only the link: `remove_dir` on a
/// junction leaves the target's files (measured on the Dell), where `remove_file` is refused.
pub(super) fn remove(link: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(link)?;
    if !meta.file_type().is_symlink() {
        return Err(Error::Other(format!("{} is not a link — refusing to remove it here", link.display())));
    }
    std::fs::remove_dir(link)?;
    Ok(())
}
