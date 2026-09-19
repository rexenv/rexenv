//! Windows: replacing the app's own install directory.
//!
//! The Windows half of `docs/archive/PLAN-self-update.md` T3, shaped like the
//! macOS one: syscalls and OS facts only — every decision about what the facts
//! MEAN lives in `core::app_update`, and the path/number rules live in
//! `app_bundle_rules.rs` so the macOS test host can run them.
//!
//! # The shape, and what was measured to choose it
//!
//! Stage a whole new install directory beside the installed one, verify it, then
//! rename the installed one aside and the staged one in.
//!
//! - **A running `.exe` cannot be deleted or overwritten, but it — and the
//!   directory holding it — CAN be renamed, and the process keeps running from
//!   the renamed file.** Measured on the Dell, 19 Sep 2026 (`docs/PLAN-windows-port.md`
//!   D5). The plan had assumed the opposite and would have built a relauncher
//!   that swapped after exit; it now only restarts, as on macOS.
//! - **Staged as a SIBLING**, in the install directory's parent, for the reason
//!   macOS does it: a cross-volume rename is then impossible by construction.
//! - **A rename pair PER FILE, never of the directory.** The install directory is
//!   also the ROOT of the app-data tree (`%LOCALAPPDATA%\rexenv\rexenv\data`: the
//!   database, logs, every binary), and a directory with an open handle anywhere
//!   beneath it cannot be renamed — ACCESS_DENIED, measured 19 Sep 2026 on the
//!   first real update (0.7.9 → 0.8.1, clean VM and the Dell alike): the swap
//!   failed with "the folder is not writable" while the Dell fixture, whose
//!   install directory held nothing open, had passed. A running `.exe` CAN be
//!   renamed, so each staged file is moved aside and the new one moved in, with
//!   every move undone if one fails; the data tree is never touched. The window
//!   between moves is real and is why the DNS agent is re-launched afterwards
//!   rather than expected to survive.
//! - **The uninstaller travels.** `uninstall.exe` is written by the INSTALLER, not
//!   the build, so a staged directory extracted from the update archive has none;
//!   it is copied in from the installed directory before the swap, and the HKCU
//!   uninstall entry's `DisplayVersion` is rewritten after it — Apps & Features
//!   is the one place a user sees a version without opening the app.
//! - **Nothing is deleted here, ever.** The previous directory stays until the
//!   NEW app has launched and confirmed its own version — and on Windows it could
//!   not be deleted anyway while the old process runs, which is the measured fact
//!   behind the rule.
//! - **No privileged path.** A per-machine install is a refusal with the
//!   per-user reinstall named (`core::app_update::Refusal::PerMachineInstall`),
//!   never a UAC prompt.

use super::app_bundle_rules as rules;
use super::pe;
use crate::core::app_update::RelaunchArgs;
use crate::error::{Error, Result};
use crate::platform::traits::{
    AppBundle, Arch, BundleFacts, InstallKind, Leftover, StagedBundle, StagedExpect, SwapFailure,
    SwapMethod, SwapReceipt,
};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_NOT_SAME_DEVICE, ERROR_SHARING_VIOLATION,
    ERROR_WRITE_PROTECT, FILETIME, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetFileVersionInfoSizeW, GetFileVersionInfoW, GetVolumeInformationW, VerQueryValueW,
    VS_FIXEDFILEINFO,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

/// `FILE_READ_ONLY_VOLUME` from winnt.h — the one flag this file reads off
/// `GetVolumeInformationW`. Spelled here rather than pulled from windows-sys's
/// `Win32_System_SystemServices`, which would switch on a whole feature for one
/// number.
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

fn wide(p: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    p.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// What the build stamps into an executable: its product version and name.
struct VersionInfo {
    version: String,
    product_name: String,
}

/// Read `rexenv.exe`'s own record of itself — the Windows counterpart of reading
/// `Info.plist`. Derived from the bytes the build produced, never from a file
/// written beside them, so a stale or hand-edited marker cannot vouch for an
/// executable.
fn version_info(exe: &Path) -> Option<VersionInfo> {
    let path = wide(exe);
    // SAFETY: a valid NUL-terminated path; the handle out-param is unused (0).
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` is `size` bytes, exactly what the call was told.
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let query = |sub: &str| -> Option<(*mut std::ffi::c_void, u32)> {
        let sub = wide(sub);
        let mut out: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        // SAFETY: `block` outlives every pointer handed back, which points INTO it.
        let ok = unsafe { VerQueryValueW(block.as_ptr().cast(), sub.as_ptr(), &mut out, &mut len) };
        (ok != 0 && !out.is_null() && len > 0).then_some((out, len))
    };

    let (fixed, len) = query("\\")?;
    if (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }
    // SAFETY: the root query answers a VS_FIXEDFILEINFO of at least that size.
    let fixed = unsafe { std::ptr::read_unaligned(fixed as *const VS_FIXEDFILEINFO) };
    let version = rules::version_from_fixed(fixed.dwProductVersionMS, fixed.dwProductVersionLS);

    // The string table is keyed by language + code page; ask which one the build
    // wrote rather than guessing 040904b0.
    let (translation, tlen) = query("\\VarFileInfo\\Translation")?;
    if (tlen as usize) < 4 {
        return None;
    }
    // SAFETY: two u16s, checked for length above.
    let (lang, cp) = unsafe { (*(translation as *const u16), *(translation as *const u16).add(1)) };
    let (name, nlen) = query(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\ProductName"))?;
    // SAFETY: `nlen` is a count of u16s including the terminator, inside `block`.
    let chars = unsafe { std::slice::from_raw_parts(name as *const u16, nlen as usize) };
    let product_name = String::from_utf16_lossy(chars).trim_end_matches('\0').trim().to_string();
    Some(VersionInfo { version, product_name })
}

/// A process's creation time in FILETIME ticks — the start token the relaunch
/// argv carries (`rules::relaunch_token`). `None` when the pid is gone or may not
/// be opened.
pub fn creation_ticks(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    // SAFETY: plain calls; the handle is checked before use and closed after.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(h);
        (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

/// A process's start-time token as the relaunch argv carries it — the same name
/// and contract as macOS's, so a live check can spawn the relauncher by hand on
/// either OS. `None` when the pid is gone.
pub fn process_start_token(pid: u32) -> Option<String> {
    creation_ticks(pid).map(rules::relaunch_token)
}

pub struct WindowsAppBundle;

impl WindowsAppBundle {
    /// The install directory is the executable's own directory: the installer puts
    /// `rexenv.exe`, `rex.exe` and `uninstall.exe` straight into `$INSTDIR`.
    fn bundle_of(exe: &Path) -> Option<PathBuf> {
        exe.parent().map(Path::to_path_buf)
    }

    fn install_kind(bundle: &Path) -> InstallKind {
        let lad = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let pfs: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|k| std::env::var_os(k).map(PathBuf::from))
            .collect();
        let pf_refs: Vec<&Path> = pfs.iter().map(PathBuf::as_path).collect();
        rules::install_kind_of(bundle, lad.as_deref(), &pf_refs)
    }

    /// Can THIS account create and remove a file here? Asked by doing it: a
    /// static read of the ACL would have to reproduce Windows' access check to
    /// be right, and a probe file costs nothing.
    fn writable(dir: &Path) -> bool {
        let probe = dir.join(format!(".rexenv-write-probe-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    /// The owner SID of `path` is this account's — OR the SID this account's token
    /// stamps as owner on what it creates (`BUILTIN\Administrators` for an admin
    /// account, which is what an installer run by that account leaves on the install
    /// directory; measured 19 Sep 2026, ledger #693). Comparing against the user SID
    /// alone called every admin user's own install foreign and refused the update.
    fn owned_by_me(path: &Path) -> bool {
        let Ok(me) = super::acl::current_user_sid() else { return false };
        let default_owner = super::acl::current_token_owner_sid().unwrap_or_default();
        let wpath = wide(path);
        let mut owner: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut descriptor: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: valid path, owner-only query; the descriptor is LocalFree'd
        // exactly once, and the SID pointer points into it.
        unsafe {
            let rc = GetNamedSecurityInfoW(
                wpath.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            );
            if rc != 0 || owner.is_null() {
                return false;
            }
            let mut text: *mut u16 = std::ptr::null_mut();
            let same = if ConvertSidToStringSidW(owner, &mut text) != 0 && !text.is_null() {
                let mut n = 0;
                while *text.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
                LocalFree(text.cast());
                s.eq_ignore_ascii_case(&me) || (!default_owner.is_empty() && s.eq_ignore_ascii_case(&default_owner))
            } else {
                false
            };
            LocalFree(descriptor);
            same
        }
    }

    /// `(read_only, free_bytes)` for the volume `path` sits on.
    fn volume(path: &Path) -> (bool, u64) {
        let dir = wide(path);
        let mut free: u64 = 0;
        // SAFETY: valid path; the two unused out-params may be null per the docs.
        let got_free = unsafe { GetDiskFreeSpaceExW(dir.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
        let free = if got_free != 0 { free } else { 0 };

        // The volume flags need the ROOT (`C:\`), not a directory.
        let root = path
            .components()
            .next()
            .map(|c| PathBuf::from(format!("{}\\", c.as_os_str().to_string_lossy())))
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        let root_w = wide(&root);
        let mut flags: u32 = 0;
        // SAFETY: valid root path; every name buffer is null with length 0, which
        // the call documents as "not wanted".
        let ok = unsafe {
            GetVolumeInformationW(
                root_w.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            )
        };
        let read_only = ok != 0 && (flags & FILE_READ_ONLY_VOLUME) != 0;
        (read_only, free)
    }

    /// No symlink or junction anywhere on the way to `exe`. Walked with
    /// `symlink_metadata` rather than compared against `canonicalize`, which on
    /// Windows answers in `\\?\` form and would refuse every install.
    fn canonical(exe: &Path) -> bool {
        exe.ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .all(|p| std::fs::symlink_metadata(p).map(|m| !m.file_type().is_symlink()).unwrap_or(false))
    }

    fn read_version_info(dir: &Path, executable: &str) -> Result<VersionInfo> {
        version_info(&dir.join(executable)).ok_or_else(|| {
            Error::Other(format!(
                "the staged {executable} carries no version information — it was discarded and \
                 nothing was changed"
            ))
        })
    }

    /// Everything that must be true of a staged directory before it is allowed to
    /// become the installed app. Each failure names what was wrong.
    fn verify_staged(dir: &Path, expect: &StagedExpect) -> Result<()> {
        let info = Self::read_version_info(dir, &expect.executable)?;
        if info.version != expect.version {
            return Err(Error::Other(format!(
                "the downloaded build says it is {}, but the signed release names {} — it was \
                 discarded and nothing was changed",
                info.version, expect.version
            )));
        }
        if info.product_name != expect.identifier {
            return Err(Error::Other(format!(
                "the downloaded build is {:?}, not {:?}",
                info.product_name, expect.identifier
            )));
        }

        let mut required: Vec<String> = vec![expect.executable.clone()];
        required.extend(expect.required_binaries.iter().cloned());
        for name in &required {
            let path = dir.join(name);
            if !path.is_file() {
                return Err(Error::Other(format!("the downloaded build is missing {name}")));
            }
            if !expect.archs.is_empty() {
                let mut head = vec![0u8; pe::HEAD_BYTES];
                let n = std::fs::File::open(&path)
                    .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                    .map_err(|e| Error::Other(format!("could not read {name}: {e}")))?;
                let kind = pe::classify(&head[..n]);
                let ok = match kind {
                    pe::PeKind::X64 => expect.archs.contains(&Arch::X86_64),
                    pe::PeKind::OtherMachine(0xaa64) => expect.archs.contains(&Arch::Arm64),
                    _ => false,
                };
                if !ok {
                    return Err(Error::Other(format!(
                        "{name} is not a build for this machine ({kind:?}) — installing it would \
                         leave rexenv unable to start"
                    )));
                }
            }
        }
        // `expect.codesign` is false on Windows by ruling (D5): there is no
        // signature to verify and a check that "passed" would be a lie.
        Ok(())
    }

    fn classify(e: &std::io::Error) -> SwapFailure {
        match e.raw_os_error().map(|c| c as u32) {
            Some(ERROR_ACCESS_DENIED) => SwapFailure::NotWritable,
            Some(ERROR_NOT_SAME_DEVICE) => SwapFailure::CrossDevice,
            Some(ERROR_WRITE_PROTECT) => SwapFailure::ReadOnly,
            Some(ERROR_SHARING_VIOLATION) => SwapFailure::Other(
                "a file in the install folder is open in another program".into(),
            ),
            _ => SwapFailure::Other(e.to_string()),
        }
    }

    /// Rewrite the uninstall entry's version so Apps & Features agrees with the
    /// binary. Best-effort and logged: the app is already swapped, and a stale
    /// number in a settings page is not worth failing an update over.
    ///
    /// Only if the entry EXISTS. `RegSetKeyValueW` would create it, which is
    /// wrong twice: a dev build or a fixture swap has no uninstall entry to keep
    /// in step, and creating one would put a half-filled "rexenv" row into the
    /// user's Apps & Features — on the developer's own machine, from a test.
    fn record_installed_version(installed: &Path, executable: &str) {
        let Some(info) = version_info(&installed.join(executable)) else { return };
        let key = wide(rules::UNINSTALL_KEY);
        let name = wide("DisplayVersion");
        let value = wide(&info.version);
        // SAFETY: valid NUL-terminated strings; the handle is closed on every
        // path after a successful open; the byte length counts the terminator.
        unsafe {
            let mut h = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut h) != 0 {
                log::info!("app update: no uninstall entry to update (not an installed copy)");
                return;
            }
            let rc = RegSetValueExW(h, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32);
            RegCloseKey(h);
            if rc != 0 {
                log::warn!("app update: could not record {} in the uninstall entry (rc {rc})", info.version);
            }
        }
    }
}

impl AppBundle for WindowsAppBundle {
    fn facts(&self, exe: &Path) -> Result<BundleFacts> {
        let canonical = Self::canonical(exe);
        let Some(bundle) = Self::bundle_of(exe) else {
            return Ok(BundleFacts {
                bundle: exe.to_path_buf(),
                parent: PathBuf::from("C:\\"),
                kind: InstallKind::DevBuild,
                homebrew: false,
                parent_writable: false,
                owned_by_me: false,
                read_only: false,
                canonical,
                free_parent_bytes: 0,
            });
        };
        let kind = Self::install_kind(&bundle);
        let parent = bundle.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("C:\\"));
        let (read_only, free_parent_bytes) = Self::volume(&parent);
        Ok(BundleFacts {
            kind,
            homebrew: false,
            parent_writable: Self::writable(&parent),
            owned_by_me: Self::owned_by_me(&bundle),
            read_only,
            canonical,
            free_parent_bytes,
            bundle,
            parent,
        })
    }

    fn stage(&self, facts: &BundleFacts, archive: &Path, expect: &StagedExpect) -> Result<StagedBundle> {
        let stage_dir = facts.parent.join(format!("{}{}", rules::STAGE_PREFIX, std::process::id()));
        if stage_dir.exists() {
            let _ = std::fs::remove_dir_all(&stage_dir);
        }
        std::fs::create_dir(&stage_dir)
            .map_err(|e| Error::Other(format!("could not create {}: {e}", stage_dir.display())))?;
        let dir = stage_dir.join(rules::PRODUCT_NAME);

        let staged = (|| -> Result<()> {
            // The same guarded extractor the binary cache uses for zip trees:
            // it refuses `..`, absolute paths, drive prefixes and symlink entries,
            // so a signed-but-hostile archive cannot write outside `dir`. The
            // archive holds the install directory's contents at its root.
            crate::core::binaries::extract_zip_tree(archive, &dir, 0)?;
            Self::verify_staged(&dir, expect)?;
            // The uninstaller is the installer's, not the build's: carry the
            // installed one across so the swapped-in directory can still be
            // removed from Apps & Features.
            let uninstaller = facts.bundle.join("uninstall.exe");
            if uninstaller.is_file() {
                std::fs::copy(&uninstaller, dir.join("uninstall.exe"))
                    .map_err(|e| Error::Other(format!("could not carry uninstall.exe across: {e}")))?;
            }
            Ok(())
        })();

        if let Err(e) = staged {
            let _ = std::fs::remove_dir_all(&stage_dir);
            return Err(e);
        }
        Ok(StagedBundle { path: dir, stage_dir })
    }

    fn swap(&self, installed: &Path, staged: &StagedBundle) -> std::result::Result<SwapReceipt, SwapFailure> {
        // Per FILE (module doc): the install directory cannot be renamed while
        // the app-data tree under it holds an open handle, but a running .exe
        // can be. Each staged file: the installed one aside, the new one in.
        let aside = staged.stage_dir.join("previous");
        std::fs::create_dir_all(&aside).map_err(|e| Self::classify(&e))?;
        let names: Vec<std::ffi::OsString> = std::fs::read_dir(&staged.path)
            .map_err(|e| Self::classify(&e))?
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name())
            .collect();
        let mut moved_aside: Vec<std::ffi::OsString> = Vec::new();
        let mut moved_in: Vec<std::ffi::OsString> = Vec::new();
        let restore = |moved_in: &[std::ffi::OsString], moved_aside: &[std::ffi::OsString]| -> Vec<String> {
            let mut stuck = Vec::new();
            for n in moved_in {
                if std::fs::rename(installed.join(n), staged.path.join(n)).is_err() {
                    stuck.push(installed.join(n).display().to_string());
                }
            }
            for n in moved_aside {
                if std::fs::rename(aside.join(n), installed.join(n)).is_err() {
                    stuck.push(aside.join(n).display().to_string());
                }
            }
            stuck
        };
        for name in &names {
            let current = installed.join(name);
            let step = (|| -> std::io::Result<()> {
                if current.exists() {
                    std::fs::rename(&current, aside.join(name))?;
                    moved_aside.push(name.clone());
                }
                std::fs::rename(staged.path.join(name), &current)?;
                moved_in.push(name.clone());
                Ok(())
            })();
            if let Err(e) = step {
                let failure = Self::classify(&e);
                let stuck = restore(&moved_in, &moved_aside);
                if !stuck.is_empty() {
                    return Err(SwapFailure::Other(format!(
                        "{failure}; the previous rexenv's files are under {} and could not all be moved back ({}) — move them back by hand",
                        aside.display(),
                        stuck.join(", ")
                    )));
                }
                return Err(failure);
            }
        }
        Self::record_installed_version(installed, "rexenv.exe");
        Ok(SwapReceipt { installed: installed.to_path_buf(), previous: aside, method: SwapMethod::RenamePair })
    }

    fn spawn_relauncher(&self, bundle: &Path) -> Result<()> {
        use std::os::windows::process::CommandExt;
        // The NEW build's own binary, in relauncher mode — the same self-exec
        // shape the DNS agent uses. Spawned from the new directory on purpose:
        // after the rename this process's own `current_exe()` names the path that
        // now holds the new file anyway (measured), so there is no old binary to
        // prefer, and the flag is a cross-version contract either way.
        let me = std::process::id();
        let token = rules::relaunch_token(creation_ticks(me).ok_or_else(|| {
            Error::Other(format!("could not read this process's creation time (pid {me})"))
        })?);
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        std::process::Command::new(bundle.join("rexenv.exe"))
            .arg(crate::core::app_update::RELAUNCH_FLAG)
            .arg(me.to_string())
            .arg(token)
            .arg(bundle)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }

    fn sweep_leftovers(&self, parent: &Path, my_version: &str, delete_previous: bool) -> Result<Vec<Leftover>> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(parent) else { return Ok(out) };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { continue };
            if !rules::is_stage_dir(name) || !dir.is_dir() {
                continue;
            }
            // Classify by the VERSION inside, read off the executable itself —
            // never a marker file.
            let version = [rules::PRODUCT_NAME, "previous"]
                .iter()
                .find_map(|sub| version_info(&dir.join(sub).join("rexenv.exe")).map(|v| v.version));
            let is_previous = version.as_deref().map(|v| rules::is_previous(v, my_version)).unwrap_or(false);
            let deleted = if is_previous && !delete_previous {
                false
            } else {
                std::fs::remove_dir_all(&dir).is_ok()
            };
            out.push(Leftover { path: dir, version, deleted });
        }
        Ok(out)
    }
}

/// How long the relauncher waits before giving up — the same bound, for the
/// same reason, as macOS: a "Keep sharing" answer at the quit gate leaves the
/// app running with the new directory already in place, and this process must
/// not sit on that pid for the rest of the session. Giving up is safe; the next
/// ordinary launch runs the new build.
const CAP_MS: u32 = 120_000;

/// The detached relauncher (`main.rs`, before Tauri boots): wait for the process
/// that swapped the directory to be gone, then start the new `rexenv.exe`.
///
/// The parent's start token is checked BEFORE the wait, as on macOS: a pid the
/// kernel recycled belongs to a stranger, and a mismatch means "already gone".
pub fn run_relauncher(args: RelaunchArgs) -> i32 {
    let RelaunchArgs { parent, parent_start, bundle } = args;
    if creation_ticks(parent).map(rules::relaunch_token).as_deref() == Some(parent_start.as_str()) {
        // SAFETY: a handle opened for SYNCHRONIZE, waited on once, closed once.
        unsafe {
            let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, parent);
            if !h.is_null() {
                let rc = WaitForSingleObject(h, CAP_MS);
                CloseHandle(h);
                if rc != WAIT_OBJECT_0 {
                    eprintln!("relauncher: pid {parent} did not exit within {}s; starting anyway", CAP_MS / 1000);
                }
            }
        }
    }
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    match std::process::Command::new(bundle.join("rexenv.exe"))
        .creation_flags(DETACHED_PROCESS)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("relauncher: could not start {}: {e}", bundle.join("rexenv.exe").display());
            1
        }
    }
}
