//! Linux: replacing the running app (docs/PLAN-linux-port.md L7, owner ruling 24 Sep 2026).
//!
//! Two installs, two swaps — the rules in `app_bundle_rules.rs`, the decisions in
//! `core::app_update` as on the other two OSes:
//!
//! - **`.deb`** (`/usr/bin/rexenv`, root-owned): the downloaded package is staged under the
//!   user's app-data (`updates/.rexenv-update-<pid>/`), verified with `dpkg-deb` (name, version,
//!   architecture, the `rex` sidecar), then installed by `dpkg -i` through the polkit step —
//!   ONE prompt, the same dialog every other privileged change shows. Linux does not lock a
//!   running executable, so the old process keeps its inode and the new one starts from the
//!   new file; `dpkg` keeps no previous version, so the "previous" in the receipt is the
//!   staged package itself, swept later.
//! - **AppImage** (`$APPIMAGE`, one user-owned file): staged beside itself, verified by
//!   running it with `--print-version` (`APPIMAGE_EXTRACT_AND_RUN=1`, so a machine without
//!   libfuse2 can still verify), then exchanged in one syscall — `renameat2(RENAME_EXCHANGE)`,
//!   Linux's `renamex_np(RENAME_SWAP)` — or a rename pair where the filesystem refuses.
//! - A `cargo` binary is a dev build; anything else is `Elsewhere`, refused with the words.
//!
//! **Never a write inside the running copy, nothing deleted here, no privilege outside
//! `PrivilegeManager`** — the macOS rules, kept.

use super::app_bundle_rules as rules;
use crate::error::{Error, Result};
use crate::platform::traits::{
    AppBundle, BundleFacts, InstallKind, Leftover, PrivilegeManager, PromptReason, StagedBundle, StagedExpect,
    SwapFailure, SwapMethod, SwapReceipt,
};
use std::path::{Path, PathBuf};

/// The flag a staged copy is asked its version through (`core::app_update::PRINT_VERSION_FLAG`,
/// printed by `main.rs` before Tauri boots on every OS) — the NEW build answers for itself.
const PRINT_VERSION_FLAG: &str = crate::core::app_update::PRINT_VERSION_FLAG;

pub struct LinuxAppBundle;

impl LinuxAppBundle {
    fn appimage() -> Option<PathBuf> {
        std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
    }

    fn writable(path: &Path) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
        // SAFETY: a valid NUL-terminated path and a documented mode constant.
        unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
    }

    fn owned_by_me(path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: getuid takes no arguments and cannot fail.
        let me = unsafe { libc::getuid() };
        std::fs::metadata(path).map(|m| m.uid() == me).unwrap_or(false)
    }

    /// `(read_only, free_bytes)` for the filesystem `path` is on — one `statvfs`, one moment
    /// (macOS reads `statfs.f_flags`; glibc's `statfs` has no such field, `statvfs.f_flag`
    /// carries `ST_RDONLY` on Linux).
    fn volume(path: &Path) -> (bool, u64) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return (false, 0) };
        // SAFETY: `statvfs` is zeroed before the call and read only after it returns 0.
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut st) != 0 {
                return (false, 0);
            }
            let read_only = (st.f_flag as libc::c_ulong) & (libc::ST_RDONLY as libc::c_ulong) != 0;
            // The field widths are the libc's (u64 on glibc x86_64/aarch64, narrower on 32-bit
            // and musl); the cast is a no-op on the shipping targets and a widening elsewhere.
            #[allow(clippy::unnecessary_cast)]
            let free = (st.f_bavail as u64).saturating_mul(st.f_frsize as u64);
            (read_only, free)
        }
    }

    /// Where a package install stages its download: the user's own app-data, never `/usr`.
    fn package_stage_parent() -> Result<PathBuf> {
        use crate::platform::traits::Paths;
        let dir = super::LinuxPaths.app_data_dir()?.join("updates");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn host_dpkg_arch() -> String {
        let out = crate::platform::command("dpkg").arg("--print-architecture").output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => rules::dpkg_arch(std::env::consts::ARCH).to_string(),
        }
    }

    fn dpkg_deb(args: &[&str], deb: &Path) -> Result<String> {
        let out = crate::platform::command("dpkg-deb").args(args).arg(deb).output().map_err(|e| {
            Error::Other(format!("dpkg-deb is not available, so the downloaded package cannot be checked: {e}"))
        })?;
        if !out.status.success() {
            return Err(Error::Other(format!(
                "the downloaded package is not a readable Debian package: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// What a binary says it is — `<file> --print-version`, extracted rather than mounted so
    /// the check needs no FUSE. `None` when it cannot run or prints nothing version-shaped.
    pub(crate) fn printed_version(file: &Path) -> Option<String> {
        let out = crate::platform::command(file)
            .arg(PRINT_VERSION_FLAG)
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && crate::core::app_update::well_formed_version(&v)).then_some(v)
    }

    fn verify_staged_deb(deb: &Path, expect: &StagedExpect) -> Result<()> {
        let control = Self::dpkg_deb(&["-f"], deb)?;
        rules::verify_deb_control(&control, expect, &Self::host_dpkg_arch()).map_err(Error::Other)?;
        let listing = Self::dpkg_deb(&["-c"], deb)?;
        rules::verify_deb_listing(&listing, expect).map_err(Error::Other)
    }

    fn verify_staged_image(image: &Path, expect: &StagedExpect) -> Result<()> {
        match Self::printed_version(image) {
            Some(v) if v == expect.version => Ok(()),
            Some(v) => Err(Error::Other(format!(
                "the downloaded rexenv says it is {v}, but the signed release names {} — it was discarded and nothing was changed",
                expect.version
            ))),
            None => Err(Error::Other(
                "the downloaded file did not run as rexenv (it printed no version) — it was discarded and nothing was changed".into(),
            )),
        }
    }

    fn classify_errno(e: &std::io::Error) -> SwapFailure {
        match e.raw_os_error() {
            Some(libc::EACCES) => SwapFailure::NotWritable,
            Some(libc::EPERM) => SwapFailure::PolicyBlocked,
            Some(libc::EXDEV) => SwapFailure::CrossDevice,
            Some(libc::EROFS) => SwapFailure::ReadOnly,
            Some(libc::ENOTSUP) | Some(libc::EINVAL) | Some(libc::ENOSYS) => SwapFailure::Unsupported,
            _ => SwapFailure::Other(e.to_string()),
        }
    }

    /// `renameat2(a, b, RENAME_EXCHANGE)` — atomically exchange two paths (Linux 3.15).
    fn rename_exchange(a: &Path, b: &Path) -> std::io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(a.as_os_str().as_bytes())?;
        let to = CString::new(b.as_os_str().as_bytes())?;
        // SAFETY: two valid NUL-terminated paths, `AT_FDCWD` for both, a documented flag.
        let rc = unsafe { libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_EXCHANGE) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

impl AppBundle for LinuxAppBundle {
    fn facts(&self, exe: &Path) -> Result<BundleFacts> {
        let canonical = std::fs::canonicalize(exe).map(|c| c == exe).unwrap_or(false);
        let appimage = Self::appimage();
        let (bundle, kind) = rules::classify(exe, appimage.as_deref());
        match kind {
            InstallKind::PortableFile => {
                let parent = bundle.parent().unwrap_or(Path::new("/")).to_path_buf();
                let (read_only, free_parent_bytes) = Self::volume(&parent);
                Ok(BundleFacts {
                    kind,
                    homebrew: false,
                    parent_writable: Self::writable(&parent),
                    owned_by_me: Self::owned_by_me(&bundle),
                    read_only,
                    // The AppImage is what runs; the mounted exe under /tmp is never it.
                    canonical: true,
                    free_parent_bytes,
                    bundle,
                    parent,
                })
            }
            InstallKind::SystemPackage => {
                // The swap is the package manager's, run as root: the parent that matters is
                // where the download is STAGED — the user's app-data — so writability, ownership
                // and free space are asked of that.
                let parent = Self::package_stage_parent()?;
                let (read_only, free_parent_bytes) = Self::volume(&parent);
                Ok(BundleFacts {
                    kind,
                    homebrew: false,
                    parent_writable: Self::writable(&parent),
                    owned_by_me: Self::owned_by_me(&parent),
                    read_only,
                    canonical,
                    free_parent_bytes,
                    bundle,
                    parent,
                })
            }
            _ => Ok(BundleFacts {
                parent: bundle.parent().unwrap_or(Path::new("/")).to_path_buf(),
                bundle,
                kind,
                homebrew: false,
                parent_writable: false,
                owned_by_me: false,
                read_only: false,
                canonical,
                free_parent_bytes: 0,
            }),
        }
    }

    fn stage(&self, facts: &BundleFacts, archive: &Path, expect: &StagedExpect) -> Result<StagedBundle> {
        let stage_dir = rules::stage_dir(&facts.parent, std::process::id());
        if stage_dir.exists() {
            let _ = std::fs::remove_dir_all(&stage_dir);
        }
        std::fs::create_dir(&stage_dir).map_err(|e| Error::Other(format!("could not create {}: {e}", stage_dir.display())))?;
        let staged = match facts.kind {
            InstallKind::SystemPackage => stage_dir.join(format!("{}_{}.deb", rules::PACKAGE_NAME, expect.version)),
            _ => stage_dir.join(facts.bundle.file_name().unwrap_or_else(|| std::ffi::OsStr::new("rexenv.AppImage"))),
        };
        let done = (|| -> Result<()> {
            std::fs::copy(archive, &staged).map_err(|e| Error::Other(format!("could not stage the download: {e}")))?;
            match facts.kind {
                InstallKind::SystemPackage => Self::verify_staged_deb(&staged, expect),
                _ => {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&staged, PermissionsExt::from_mode(0o755))?;
                    Self::verify_staged_image(&staged, expect)
                }
            }
        })();
        if let Err(e) = done {
            let _ = std::fs::remove_dir_all(&stage_dir);
            return Err(e);
        }
        Ok(StagedBundle { path: staged, stage_dir })
    }

    fn swap(&self, installed: &Path, staged: &StagedBundle) -> std::result::Result<SwapReceipt, SwapFailure> {
        if installed == Path::new(rules::PACKAGE_EXE) {
            // The package manager's swap, as root through the polkit step: one prompt.
            let reason = PromptReason::new("install the downloaded rexenv update");
            return match super::LinuxPrivileges.run_privileged(&rules::dpkg_install_command(&staged.path), &reason) {
                Ok(_) => Ok(SwapReceipt {
                    installed: installed.to_path_buf(),
                    // dpkg keeps no previous package; the staged file is what the sweep removes.
                    previous: staged.path.clone(),
                    method: SwapMethod::RenamePair,
                }),
                Err(e) => Err(SwapFailure::Other(e.to_string())),
            };
        }
        match Self::rename_exchange(installed, &staged.path) {
            Ok(()) => Ok(SwapReceipt { installed: installed.to_path_buf(), previous: staged.path.clone(), method: SwapMethod::AtomicSwap }),
            Err(e) if matches!(Self::classify_errno(&e), SwapFailure::Unsupported) => {
                let aside = staged.stage_dir.join("previous.AppImage");
                std::fs::rename(installed, &aside).map_err(|e| Self::classify_errno(&e))?;
                if let Err(e) = std::fs::rename(&staged.path, installed) {
                    let failure = Self::classify_errno(&e);
                    if std::fs::rename(&aside, installed).is_err() {
                        return Err(SwapFailure::Other(format!(
                            "{failure}; the previous rexenv is at {} and can be moved back by hand",
                            aside.display()
                        )));
                    }
                    return Err(failure);
                }
                Ok(SwapReceipt { installed: installed.to_path_buf(), previous: aside, method: SwapMethod::RenamePair })
            }
            Err(e) => Err(Self::classify_errno(&e)),
        }
    }

    fn spawn_relauncher(&self, bundle: &Path) -> Result<()> {
        // Our own binary in relauncher mode — the DNS agent's and the tunnel guard's self-exec
        // shape. Spawned from the OLD inode: the version being replaced starts the replacement.
        let exe = std::env::current_exe()?;
        let me = std::process::id();
        let start = super::process_start_token(me)
            .ok_or_else(|| Error::Other(format!("could not read this process's start time (pid {me})")))?;
        crate::platform::command(exe)
            .arg(crate::core::app_update::RELAUNCH_FLAG)
            .arg(me.to_string())
            .arg(start)
            .arg(bundle)
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
            // The version INSIDE, never a marker: a `.deb` answers through its control file, an
            // AppImage by running (extracted, no FUSE). A file that answers nothing is a
            // leftover of unknown age, dropped like a newer one.
            let version = std::fs::read_dir(&dir).ok().and_then(|mut e| {
                e.find_map(|x| {
                    let p = x.ok()?.path();
                    if p.extension().is_some_and(|x| x == "deb") {
                        Self::dpkg_deb(&["-f"], &p).ok().and_then(|c| rules::control_field(&c, "Version"))
                    } else if p.is_file() {
                        Self::printed_version(&p)
                    } else {
                        None
                    }
                })
            });
            let is_previous = version
                .as_deref()
                .map(|v| crate::core::updates::version_segments(v) < crate::core::updates::version_segments(my_version))
                .unwrap_or(false);
            let deleted = if is_previous && !delete_previous { false } else { std::fs::remove_dir_all(&dir).is_ok() };
            out.push(Leftover { path: dir, version, deleted });
        }
        Ok(out)
    }

    fn descriptor_variant(&self) -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let (_, kind) = rules::classify(&exe, Self::appimage().as_deref());
        rules::descriptor_variant(kind, std::env::consts::ARCH)
    }
}

/// The relauncher: wait for the swapping process to be gone (pidfd, capped), then start the
/// bundle — `/usr/bin/rexenv` or the AppImage — detached in its own session.
pub fn run_relauncher(args: crate::core::app_update::RelaunchArgs) -> i32 {
    use crate::core::app_update::RelaunchArgs;
    let RelaunchArgs { parent, parent_start, bundle } = args;
    wait_for_parent(parent, &parent_start);
    let mut cmd = crate::platform::command(&bundle);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    match cmd.spawn() {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("relauncher: could not start {}: {e}", bundle.display());
            1
        }
    }
}

/// Block until `parent` exits, turns out to be somebody else, or two minutes pass — the
/// macOS cap, for the macOS reason (a "Keep sharing" at the quit gate keeps the pid alive).
fn wait_for_parent(parent: u32, parent_start: &str) {
    let ours = || super::process_start_token(parent).is_some_and(|t| t == parent_start);
    if !ours() {
        return;
    }
    // SAFETY: a raw syscall with integer arguments; the descriptor is closed below.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, parent as libc::pid_t, 0 as libc::c_uint) };
    if fd < 0 {
        return;
    }
    let fd = fd as libc::c_int;
    if !ours() {
        // SAFETY: closing a descriptor this function opened.
        unsafe { libc::close(fd) };
        return;
    }
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    // SAFETY: `pfd` is a live local; the timeout is the cap in milliseconds.
    unsafe {
        libc::poll(&mut pfd, 1, 120_000);
        libc::close(fd);
    }
}
