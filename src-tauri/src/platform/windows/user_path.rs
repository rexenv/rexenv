//! The user's own `Path` (W8 S5, plan §5 W8 rulings Q3 and Q4, ledger #634) — the Win32 half of
//! `user_path_rules.rs`: `HKEY_CURRENT_USER\Environment`'s `Path`, read and written back in the kind it already
//! has, then `WM_SETTINGCHANGE("Environment")` so what Explorer starts afterwards sees it.
//!
//! Measured on the Dell, 15 Sep 2026: its user `Path` is `REG_SZ`, not `REG_EXPAND_SZ` — so the kind is kept,
//! never assumed; and a folder appended there plus this broadcast, sent from the desktop session, was found by a
//! PowerShell opened from the Start menu afterwards. A value that does not exist yet is created as
//! `REG_EXPAND_SZ`, the kind Windows itself gives a new user `Path`. A value of any non-text kind is refused.

use super::user_path_rules::{contains, with_dir, without_dir};
use crate::error::{Error, Result};
use std::path::Path;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
    REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

const ENVIRONMENT: &str = "Environment";
const PATH: &str = "Path";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: created below, closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn failed(rc: u32, what: &str) -> Error {
    Error::Other(format!("could not {what}: {}", std::io::Error::from_raw_os_error(rc as i32)))
}

/// `HKEY_CURRENT_USER\Environment`, for reading and writing (a profile always has it; created if not).
fn environment() -> Result<Key> {
    let sub = wide(ENVIRONMENT);
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
        return Err(failed(rc, "open the user's environment"));
    }
    Ok(Key(raw))
}

/// The `Path` value and its kind, the data as stored (an unexpanded `%VAR%` stays unexpanded); `None` when absent.
fn read_path(key: &Key) -> Result<Option<(u32, String)>> {
    let name = wide(PATH);
    let (mut kind, mut len) = (0u32, 0u32);
    // SAFETY: a size-and-type query — no data buffer.
    let rc = unsafe { RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut len) };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if rc != ERROR_SUCCESS {
        return Err(failed(rc, "read the user's Path"));
    }
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Err(Error::Other(format!(
            "the user's Path is not a text value (registry kind {kind}) — rexenv will not rewrite it"
        )));
    }
    let mut data = vec![0u8; len as usize];
    // SAFETY: `data` holds `len` bytes, as the call is told.
    let rc = unsafe {
        RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), data.as_mut_ptr(), &mut len)
    };
    if rc != ERROR_SUCCESS {
        return Err(failed(rc, "read the user's Path"));
    }
    data.truncate(len as usize);
    let units: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(Some((kind, String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())))
}

fn write_path(key: &Key, kind: u32, value: &str) -> Result<()> {
    let name = wide(PATH);
    let data = wide(value);
    // SAFETY: a NUL-terminated name, and `data` as bytes with its length in bytes (terminator included).
    let rc = unsafe { RegSetValueExW(key.0, name.as_ptr(), 0, kind, data.as_ptr().cast(), (data.len() * 2) as u32) };
    if rc != ERROR_SUCCESS {
        return Err(failed(rc, "write the user's Path"));
    }
    Ok(())
}

/// Tell every top-level window the environment changed; Explorer re-reads it, so what it starts next sees the new
/// `Path`. A window that does not answer is skipped after 5 s rather than holding the install.
fn broadcast() {
    let what = wide(ENVIRONMENT);
    let mut result: usize = 0;
    // SAFETY: a NUL-terminated string that outlives the call, passed as the message's lParam, and a valid out-pointer.
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 5000, &mut result)
    };
}

pub(super) fn has(dir: &Path) -> Result<bool> {
    let key = environment()?;
    Ok(read_path(&key)?.is_some_and(|(_, value)| contains(&value, &dir.to_string_lossy())))
}

pub(super) fn add(dir: &Path) -> Result<()> {
    let key = environment()?;
    let (kind, value) = read_path(&key)?.unwrap_or((REG_EXPAND_SZ, String::new()));
    let next = with_dir(&value, &dir.to_string_lossy());
    if next != value {
        write_path(&key, kind, &next)?;
        broadcast();
    }
    Ok(())
}

pub(super) fn remove(dir: &Path) -> Result<()> {
    let key = environment()?;
    let Some((kind, value)) = read_path(&key)? else { return Ok(()) };
    let next = without_dir(&value, &dir.to_string_lossy());
    if next != value {
        write_path(&key, kind, &next)?;
        broadcast();
    }
    Ok(())
}
