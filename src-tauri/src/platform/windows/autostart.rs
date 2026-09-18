//! Windows' "Open rexenv at login" (W7 S4, plan §5 W7, ledger #623) — the Win32 half of `autostart_rules.rs`:
//! the per-user Run value and Task Manager's `StartupApproved` value, read and written under
//! `HKEY_CURRENT_USER` with no elevation. A key that does not exist is simply "not enabled".

use super::autostart_rules::{disabled_by_user, refresh as decide, run_value, Refresh, APPROVED_KEY, RUN_KEY, VALUE_NAME};
use crate::error::{Error, Result};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened or created below, closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn failed(rc: u32, what: &str) -> Error {
    Error::Other(format!("could not {what}: {}", std::io::Error::from_raw_os_error(rc as i32)))
}

fn open(sub: &str, access: u32) -> Option<Key> {
    let name = wide(sub);
    let mut raw: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated name and a valid out-pointer.
    (unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, access, &mut raw) } == ERROR_SUCCESS).then_some(Key(raw))
}

/// A value's raw bytes, or `None` when it is absent.
fn read(key: &Key, value: &str) -> Option<(u32, Vec<u8>)> {
    let name = wide(value);
    let (mut kind, mut len) = (0u32, 0u32);
    // SAFETY: a size-and-type query — no data buffer.
    if unsafe { RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut len) }
        != ERROR_SUCCESS
    {
        return None;
    }
    let mut data = vec![0u8; len as usize];
    // SAFETY: `data` holds `len` bytes, as the call is told.
    let rc = unsafe {
        RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), data.as_mut_ptr(), &mut len)
    };
    (rc == ERROR_SUCCESS).then(|| {
        data.truncate(len as usize);
        (kind, data)
    })
}

/// The Run value as it is now, when it is a string.
fn current_run_value() -> Option<String> {
    let key = open(RUN_KEY, KEY_READ)?;
    let (kind, bytes) = read(&key, VALUE_NAME)?;
    (kind == REG_SZ).then(|| {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    })
}

fn write_run_value(value: &str) -> Result<()> {
    let sub = wide(RUN_KEY);
    let mut raw: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated key name, no class or security attributes, a valid out-pointer.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_READ,
            std::ptr::null(),
            &mut raw,
            std::ptr::null_mut(),
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(failed(rc, "open the Run key"));
    }
    let key = Key(raw);
    let name = wide(VALUE_NAME);
    let data = wide(value);
    // SAFETY: a NUL-terminated name, and `data` as bytes with its length in bytes (terminator included).
    let rc = unsafe {
        RegSetValueExW(key.0, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
    };
    if rc != ERROR_SUCCESS {
        return Err(failed(rc, "write the login item"));
    }
    Ok(())
}

/// Delete `VALUE_NAME` under `sub`; an absent key or value is already the wanted state.
fn delete_value(sub: &str, what: &str) -> Result<()> {
    let Some(key) = open(sub, KEY_SET_VALUE) else { return Ok(()) };
    let name = wide(VALUE_NAME);
    // SAFETY: a NUL-terminated value name on an open key.
    let rc = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        return Err(failed(rc, what));
    }
    Ok(())
}

/// Enabled: the Run value is there and Task Manager has not disabled it.
pub(super) fn is_enabled() -> Result<bool> {
    // `map_or(true, ..)`, not `is_none_or`: that one is stable since Rust 1.82 and the crate
    // declares `rust-version = "1.77.2"` -- clippy's `incompatible_msrv` caught it the first
    // time the bar ran on Windows (W12); the Mac never compiles this file.
    if current_run_value().map_or(true, |v| v.trim().is_empty()) {
        return Ok(false);
    }
    let approved = open(APPROVED_KEY, KEY_READ).and_then(|k| read(&k, VALUE_NAME)).map(|(_, bytes)| bytes);
    Ok(!disabled_by_user(approved.as_deref()))
}

/// The user's choice: THIS build, hidden — and an older Task Manager "Disabled" cleared.
pub(super) fn enable() -> Result<()> {
    let exe = std::env::current_exe()?;
    write_run_value(&run_value(&exe))?;
    delete_value(APPROVED_KEY, "clear Task Manager's startup state")
}

pub(super) fn disable() -> Result<()> {
    delete_value(RUN_KEY, "remove the login item")?;
    delete_value(APPROVED_KEY, "clear Task Manager's startup state")
}

/// A launch's refresh (`autostart_rules::refresh`): rewrite only a changed value, never move the item to a
/// dev build while the recorded program exists, never touch Task Manager's state.
pub(super) fn refresh() -> Result<()> {
    let exe = std::env::current_exe()?;
    let have = current_run_value();
    match decide(have.as_deref(), &exe, &|p| p.exists()) {
        Refresh::Unchanged => Ok(()),
        Refresh::KeepRecorded => {
            log::info!(
                "autostart: this launch is {} (a dev build) — keeping the login item on {}",
                exe.display(),
                have.as_deref().unwrap_or("")
            );
            Ok(())
        }
        Refresh::Write => write_run_value(&run_value(&exe)),
    }
}
