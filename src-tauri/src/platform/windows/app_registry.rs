//! Reading what installers register, for `app_catalog.rs` (W7 S3, plan §5 W7, ledger #622): App Paths, the
//! uninstall entries, `StartMenuInternet` and the `https` handler's ProgId. Read-only, string values only
//! (`REG_SZ`, `REG_EXPAND_SZ` — not expanded: a `%ProgramFiles%` in a value is a path that does not exist, so
//! the catalog's existence check drops it rather than guessing). A key that cannot be opened is simply
//! absent. Readable by the desktop user without elevation (the Dell's inventory ran as that user's
//! registry).

use super::app_catalog::{Installed, Program};
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_EXPAND_SZ, REG_SZ,
};

const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const START_MENU_INTERNET: &str = r"SOFTWARE\Clients\StartMenuInternet";
const START_MENU_INTERNET_32: &str = r"SOFTWARE\WOW6432Node\Clients\StartMenuInternet";
const HTTPS_CHOICE: &str = r"SOFTWARE\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by `open`, closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open(parent: HKEY, sub: &str) -> Option<Key> {
    let name = wide(sub);
    let mut raw: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated name and a valid out-pointer.
    (unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, KEY_READ, &mut raw) } == ERROR_SUCCESS).then_some(Key(raw))
}

fn subkeys(key: &Key) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        // SAFETY: the name buffer's capacity is passed in characters; the optional outputs are null.
        let rc = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        index += 1;
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        if rc == ERROR_SUCCESS {
            names.push(String::from_utf16_lossy(&name[..len as usize]));
        }
    }
    names
}

/// A string value (`""` is the key's default value), or `None` when absent or not a string.
fn string(key: &Key, value: &str) -> Option<String> {
    let name = wide(value);
    let (mut kind, mut len) = (0u32, 0u32);
    // SAFETY: a size-and-type query — no data buffer.
    let rc = unsafe {
        RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut len)
    };
    if rc != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
        return None;
    }
    let mut data = vec![0u8; len as usize];
    // SAFETY: `data` holds `len` bytes, as the call is told.
    let rc = unsafe {
        RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), data.as_mut_ptr(), &mut len)
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let units: Vec<u16> = data[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
}

/// Everything `app_catalog` detects from, read now.
pub(super) fn installed() -> Installed {
    let mut out = Installed::default();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Some(paths) = open(root, APP_PATHS) else { continue };
        for exe in subkeys(&paths) {
            if let Some(entry) = open(paths.0, &exe) {
                out.app_paths.push((exe.to_ascii_lowercase(), string(&entry, "").unwrap_or_default()));
            }
        }
    }
    for (root, path) in [(HKEY_CURRENT_USER, UNINSTALL), (HKEY_LOCAL_MACHINE, UNINSTALL), (HKEY_LOCAL_MACHINE, UNINSTALL_32)] {
        let Some(list) = open(root, path) else { continue };
        for id in subkeys(&list) {
            let Some(entry) = open(list.0, &id) else { continue };
            let Some(name) = string(&entry, "DisplayName").filter(|n| !n.trim().is_empty()) else { continue };
            out.programs.push(Program {
                name,
                location: string(&entry, "InstallLocation").unwrap_or_default(),
                icon: string(&entry, "DisplayIcon").unwrap_or_default(),
            });
        }
    }
    for (root, path) in [
        (HKEY_CURRENT_USER, START_MENU_INTERNET),
        (HKEY_LOCAL_MACHINE, START_MENU_INTERNET),
        (HKEY_LOCAL_MACHINE, START_MENU_INTERNET_32),
    ] {
        let Some(clients) = open(root, path) else { continue };
        for client in subkeys(&clients) {
            if let Some(command) = open(clients.0, &format!(r"{client}\shell\open\command")).and_then(|k| string(&k, "")) {
                out.browser_commands.push(command);
            }
        }
    }
    out.https_prog_id = open(HKEY_CURRENT_USER, HTTPS_CHOICE).and_then(|k| string(&k, "ProgId"));
    out.local_app_data = std::env::var("LOCALAPPDATA").ok();
    out.program_files = ["ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|v| std::env::var(v).ok()).collect();
    out.system_root = std::env::var("SystemRoot").ok();
    out
}
