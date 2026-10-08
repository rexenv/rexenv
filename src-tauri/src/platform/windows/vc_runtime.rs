//! Reads what `vc_runtime_rules::judge` decides on: the Visual C++ Redistributable's registry entry
//! (64-bit view, `HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64`) and its DLLs in
//! System32. Read-only, no elevation (#798).

use super::vc_runtime_rules::{judge, Registered, DLLS, KEY};
use crate::platform::traits::RuntimeProblem;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RRF_RT_REG_DWORD,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The entry when it exists with `Installed = 1`.
fn registered() -> Option<Registered> {
    let name = wide(KEY);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated name and a valid out-pointer; closed below.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, name.as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut key) }
        != ERROR_SUCCESS
    {
        return None;
    }
    let dword = |value: &str| -> Option<u32> {
        let value = wide(value);
        let mut data = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        // SAFETY: an open key, a NUL-terminated value name, a 4-byte buffer and its size.
        let rc = unsafe {
            RegGetValueW(
                key,
                std::ptr::null(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut len,
            )
        };
        (rc == ERROR_SUCCESS).then_some(data)
    };
    let found = (dword("Installed") == Some(1)).then(|| Registered {
        major: dword("Major").unwrap_or(0),
        minor: dword("Minor").unwrap_or(0),
        build: dword("Bld").unwrap_or(0),
    });
    // SAFETY: opened above, closed once.
    unsafe { RegCloseKey(key) };
    found
}

/// Whether every one of the redistributable's DLLs is in System32.
fn dlls_present() -> bool {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let system32 = std::path::Path::new(&root).join("System32");
    DLLS.iter().all(|dll| system32.join(dll).is_file())
}

pub(crate) fn problem() -> Option<RuntimeProblem> {
    judge(registered(), dlls_present())
}
