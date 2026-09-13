//! Owner-only files on Windows — the ACL counterpart of Unix `0600`.
//!
//! `PermissionManager::set_private` and `write_private` land here. The descriptor is
//! built from the SDDL in `owner_only.rs` (tested on every host); the Win32 calls
//! that apply it are compile-checked from macOS and have NOT run on Windows yet
//! (ledger #597). Every call that can fail returns the OS error, never a silent
//! success: a key file that stayed readable must not look private.

use crate::error::{Error, Result};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_ALWAYS, READ_CONTROL, WRITE_DAC,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Restrict an existing file to its owner: replace its DACL with the protected
/// owner-only one. Through a handle, so the check and the change are the same file.
pub(super) fn set_private(path: &Path) -> Result<()> {
    let descriptor = OwnerOnlyDescriptor::for_current_user()?;
    let file = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .open(path)?;
    descriptor.apply(&file)
}

/// Write `contents` to `path` with the file BORN owner-only.
///
/// `CreateFileW` receives the descriptor, so a new file never exists with inherited
/// access — the Windows form of `OpenOptions.mode(0o600)` at create. An EXISTING file
/// keeps its old DACL through `OPEN_ALWAYS`, so the handle is re-hardened BEFORE it is
/// truncated and the new secret written, as the macOS implementation re-chmods first.
pub(super) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let descriptor = OwnerOnlyDescriptor::for_current_user()?;
    let mut file = descriptor.create_or_open(path)?;
    descriptor.apply(&file)?;
    file.set_len(0)?;
    file.write_all(contents)?;
    Ok(())
}

/// A security descriptor built from [`super::owner_only::owner_only_sddl`] for the
/// user this process runs as. Owns the `LocalAlloc`ed buffer and frees it on drop.
struct OwnerOnlyDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for OwnerOnlyDescriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer came from ConvertStringSecurityDescriptorToSecurityDescriptorW,
        // whose contract is that the caller frees it with LocalFree, exactly once.
        unsafe {
            LocalFree(self.0);
        }
    }
}

impl OwnerOnlyDescriptor {
    fn for_current_user() -> Result<Self> {
        let sid = current_user_sid()?;
        let sddl = super::owner_only::owner_only_sddl(&sid)
            .ok_or_else(|| Error::Other(format!("this process's user SID is not well-formed: {sid:?}")))?;
        let wide = wide_nul(sddl.encode_utf16());
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and
        // `descriptor` is a valid out-pointer; the size out-parameter is optional.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 || descriptor.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(descriptor))
    }

    fn dacl(&self) -> Result<*mut ACL> {
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = null_mut();
        // SAFETY: `self.0` is a valid descriptor for this struct's lifetime; the
        // out-pointers are valid locals. The returned DACL points INTO the descriptor.
        let ok = unsafe { GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if present == 0 || dacl.is_null() {
            // A missing DACL means "everyone may do anything" — the opposite of private.
            return Err(Error::Other("the owner-only descriptor carries no DACL".into()));
        }
        Ok(dacl)
    }

    /// Replace `file`'s DACL with this one, PROTECTED so nothing is inherited.
    fn apply(&self, file: &std::fs::File) -> Result<()> {
        let dacl = self.dacl()?;
        // SAFETY: the handle is open with WRITE_DAC for the duration of the call, and
        // `dacl` lives inside `self`, which outlives it. Owner, group and SACL are not
        // being set, so their pointers are null.
        let code = unsafe {
            SetSecurityInfo(
                file.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        };
        if code != 0 {
            return Err(std::io::Error::from_raw_os_error(code as i32).into());
        }
        Ok(())
    }

    /// Open `path` for writing, creating it with this descriptor if it does not exist.
    fn create_or_open(&self, path: &Path) -> Result<std::fs::File> {
        let wide = wide_nul(path.as_os_str().encode_wide());
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        };
        // SAFETY: `wide` is a NUL-terminated path, `attributes` points at a descriptor
        // that outlives the call, and no template handle is passed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE | READ_CONTROL | WRITE_DAC,
                FILE_SHARE_READ,
                &attributes,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: `handle` is a valid, owned file handle that nothing else will close.
        Ok(unsafe { std::fs::File::from_raw_handle(handle) })
    }
}

/// The string SID of the user this process runs as, e.g. `S-1-5-21-…-1001`.
fn current_user_sid() -> Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; `token`
    // is a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let sid = token_user_sid(token);
    // SAFETY: `token` was opened above and is closed exactly once, here.
    unsafe {
        CloseHandle(token);
    }
    sid
}

fn token_user_sid(token: HANDLE) -> Result<String> {
    let mut needed = 0u32;
    // SAFETY: a size query — null buffer, zero length; it is expected to fail with
    // ERROR_INSUFFICIENT_BUFFER and report the size in `needed`.
    unsafe {
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` is `needed` bytes long, as the size query asked for.
    if unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: the buffer now holds a TOKEN_USER. A Vec<u8> is not aligned for it, so
    // the struct is copied out unaligned; its SID pointer points into `buffer`, which
    // stays alive until this function returns.
    let user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let mut text: PWSTR = null_mut();
    // SAFETY: the SID is valid (inside `buffer`); `text` receives a LocalAlloc'ed string.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 || text.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `text` is a NUL-terminated UTF-16 string from the call above, read up
    // to its terminator and then freed exactly once with LocalFree, as documented.
    let sid = unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        sid
    };
    Ok(sid)
}

fn wide_nul(units: impl Iterator<Item = u16>) -> Vec<u16> {
    units.chain(std::iter::once(0)).collect()
}
