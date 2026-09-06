//! macOS: replacing the app's own bundle.
//!
//! T3 of `docs/PLAN-self-update.md`. Syscalls and OS tools only — every decision
//! about what the facts MEAN lives in `core::app_update`, so it can be driven
//! over fixture facts by a test that has no Mac, no installed app and no
//! `/Volumes`.
//!
//! # The shape, and why it is this one
//!
//! Stage a whole new bundle beside the installed one, verify it, then replace
//! the old one with a single `renamex_np(RENAME_SWAP)`.
//!
//! - **Never a write inside the launched bundle.** Not because macOS stops us —
//!   T0 measured that it does not stop an ad-hoc bundle at all — but because
//!   replacing a running Mach-O in place invalidates the pages the kernel is
//!   executing and the process is SIGKILLed, and a copy-over leaves a hybrid of
//!   two builds (`docs/PUBLISH-TESTING.md`). Apple's own guidance is to build a
//!   complete new copy and replace the old one.
//! - **Staged as a SIBLING**, in the bundle's own parent directory. That is what
//!   makes a cross-device rename impossible by construction rather than by a
//!   check — the failure that costs the Tauri plugin its backup (its staging is
//!   in `$TMPDIR`, and `EXDEV` there is upstream issue #3505's data loss).
//! - **One syscall.** After `RENAME_SWAP` the install path holds the new bundle
//!   and the staging path holds the previous one. There is no instant where the
//!   path holds nothing, which is the state a KeepAlive LaunchAgent would
//!   respawn into.
//! - **Nothing is deleted here, ever.** The previous bundle stays until the NEW
//!   app has launched and confirmed its own version. The process that swapped it
//!   is gone by then and could not honestly watch.
//! - **No privileged path.** An unwritable folder is a refusal with a copy-paste
//!   fix, never an `osascript` admin prompt: root does not bypass App Management,
//!   a prompt outside `PrivilegeManager` is against this project's rule, and the
//!   plugin's version of this branch is a root `rm -rf` with no backup.

use crate::core::macho;
use crate::error::{Error, Result};
use crate::platform::traits::{
    AppBundle, Arch, BundleFacts, InstallKind, Leftover, StagedBundle, StagedExpect, SwapFailure,
    SwapMethod, SwapReceipt,
};
use std::path::{Path, PathBuf};

/// Staging directories are dot-prefixed so Finder and Spotlight skip them, and
/// so a user glancing at /Applications during an update does not see a second
/// rexenv and wonder which one is real.
const STAGE_PREFIX: &str = ".rexenv-update-";

pub struct MacosAppBundle;

impl MacosAppBundle {
    /// `…/X.app/Contents/MacOS/rexenv` → `…/X.app`. `None` when the executable
    /// is not inside a bundle at all, which is the `cargo run` case.
    fn bundle_of(exe: &Path) -> Option<PathBuf> {
        let macos_dir = exe.parent()?;
        if macos_dir.file_name()? != "MacOS" {
            return None;
        }
        let contents = macos_dir.parent()?;
        if contents.file_name()? != "Contents" {
            return None;
        }
        let app = contents.parent()?;
        if app.extension()? != "app" {
            return None;
        }
        Some(app.to_path_buf())
    }

    fn install_kind(bundle: &Path, parent: &Path) -> InstallKind {
        let b = bundle.to_string_lossy();
        if b.contains("/AppTranslocation/") {
            return InstallKind::Translocated;
        }
        if b.starts_with("/Volumes/") {
            return InstallKind::DiskImage;
        }
        if parent == Path::new("/Applications") {
            return InstallKind::Applications;
        }
        if let Some(home) = std::env::var_os("HOME") {
            if parent == PathBuf::from(home).join("Applications") {
                return InstallKind::UserApplications;
            }
        }
        InstallKind::Elsewhere
    }

    /// Is this install managed by a Homebrew cask? A directory probe, not a
    /// `brew` call: shelling out to a tool that may not exist, to answer a
    /// question about a directory, is slower and can fail in more ways.
    fn homebrew_managed() -> bool {
        ["/opt/homebrew/Caskroom/rexenv", "/usr/local/Caskroom/rexenv"]
            .iter()
            .any(|p| Path::new(p).is_dir())
    }

    fn writable(path: &Path) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: a valid NUL-terminated path and a documented mode constant.
        unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
    }

    fn owned_by_me(path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: getuid takes no arguments and cannot fail.
        let me = unsafe { libc::getuid() };
        std::fs::metadata(path).map(|m| m.uid() == me).unwrap_or(false)
    }

    /// `(read_only, free_bytes)` for the volume `path` sits on.
    ///
    /// Both come from one `statfs`, because they are one question — "can this
    /// volume take the update" — and two calls could answer it about two
    /// different moments.
    fn volume(path: &Path) -> (bool, u64) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return (false, 0);
        };
        // SAFETY: `statfs` is zeroed before the call and only read after it
        // returns 0; the path is a valid NUL-terminated string.
        unsafe {
            let mut st: libc::statfs = std::mem::zeroed();
            if libc::statfs(c.as_ptr(), &mut st) != 0 {
                return (false, 0);
            }
            let read_only = st.f_flags & libc::MNT_RDONLY as u32 != 0;
            let free = st.f_bavail.saturating_mul(st.f_bsize as u64);
            (read_only, free)
        }
    }

    /// Read one top-level string value out of an `Info.plist`.
    ///
    /// A scan rather than a plist parser: the two values this needs are XML
    /// strings in a file the app itself produced, and a dependency that can
    /// parse binary plists buys nothing here. Deliberately literal about the
    /// shape — `<key>NAME</key>` followed by the next `<string>` — so a file
    /// that does not look like that reads as `None` instead of as a lucky match.
    fn plist_string(plist: &str, key: &str) -> Option<String> {
        let at = plist.find(&format!("<key>{key}</key>"))?;
        let rest = &plist[at..];
        let open = rest.find("<string>")? + "<string>".len();
        let close = rest[open..].find("</string>")?;
        Some(rest[open..open + close].trim().to_string())
    }

    fn read_plist(bundle: &Path) -> Result<String> {
        std::fs::read_to_string(bundle.join("Contents/Info.plist"))
            .map_err(|e| Error::Other(format!("the staged bundle has no readable Info.plist: {e}")))
    }

    /// Everything that must be true of a staged tree before it is allowed to
    /// become the installed app.
    ///
    /// Each failure names what was wrong rather than "verification failed": this
    /// message reaches a user who has to decide whether to try again or report
    /// something, and "the download says it is 0.5.9" is the difference.
    fn verify_staged(app: &Path, expect: &StagedExpect) -> Result<()> {
        let plist = Self::read_plist(app)?;
        let got = |k: &str| Self::plist_string(&plist, k).unwrap_or_default();

        let version = got("CFBundleShortVersionString");
        if version != expect.version {
            return Err(Error::Other(format!(
                "the downloaded bundle says it is {version}, but the signed release names \
                 {} — it was discarded and nothing was changed",
                expect.version
            )));
        }
        let identifier = got("CFBundleIdentifier");
        if identifier != expect.identifier {
            return Err(Error::Other(format!(
                "the downloaded bundle is {identifier}, not {}",
                expect.identifier
            )));
        }
        let executable = got("CFBundleExecutable");
        if executable != expect.executable {
            return Err(Error::Other(format!(
                "the downloaded bundle runs {executable}, not {} — the relaunch resolves \
                 the binary through that key and would start the wrong thing",
                expect.executable
            )));
        }

        let mut required: Vec<String> = vec![expect.executable.clone()];
        required.extend(expect.required_binaries.iter().cloned());
        for name in &required {
            let path = app.join("Contents/MacOS").join(name);
            if !path.is_file() {
                return Err(Error::Other(format!(
                    "the downloaded bundle is missing Contents/MacOS/{name}"
                )));
            }
            if !expect.archs.is_empty() {
                let got = macho::archs(&path).ok_or_else(|| {
                    Error::Other(format!("Contents/MacOS/{name} is not a Mach-O binary"))
                })?;
                let missing: Vec<&Arch> =
                    expect.archs.iter().filter(|a| !got.contains(a)).collect();
                if !missing.is_empty() {
                    return Err(Error::Other(format!(
                        "Contents/MacOS/{name} does not run on every Mac this build \
                         supports (missing {missing:?}) — installing it would leave half \
                         the userbase unable to launch rexenv"
                    )));
                }
            }
        }

        if expect.codesign {
            let ok = std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(app)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                return Err(Error::Other(
                    "the downloaded bundle fails its own code signature — it was discarded \
                     and nothing was changed"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    /// Classify `errno` from a rename. EACCES and EPERM are DIFFERENT here even
    /// though Rust folds both into `PermissionDenied`: EPERM on macOS is what a
    /// policy refusal looks like, root does not bypass it, and treating it as a
    /// permission problem is how the Tauri plugin ends up running a privileged
    /// `rm -rf` against a refusal that privileges cannot fix.
    fn classify(e: &std::io::Error) -> SwapFailure {
        match e.raw_os_error() {
            Some(libc::EACCES) => SwapFailure::NotWritable,
            Some(libc::EPERM) => SwapFailure::PolicyBlocked,
            Some(libc::EXDEV) => SwapFailure::CrossDevice,
            Some(libc::EROFS) => SwapFailure::ReadOnly,
            Some(libc::ENOTSUP) | Some(libc::EINVAL) => SwapFailure::Unsupported,
            _ => SwapFailure::Other(e.to_string()),
        }
    }

    /// `renamex_np(a, b, RENAME_SWAP)` — atomically exchange two paths.
    fn rename_swap(a: &Path, b: &Path) -> std::io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(a.as_os_str().as_bytes())?;
        let to = CString::new(b.as_os_str().as_bytes())?;
        // SAFETY: two valid NUL-terminated paths, a documented flag, return checked.
        let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

impl AppBundle for MacosAppBundle {
    fn facts(&self, exe: &Path) -> Result<BundleFacts> {
        let canonical = std::fs::canonicalize(exe).map(|c| c == exe).unwrap_or(false);
        let Some(bundle) = Self::bundle_of(exe) else {
            // No bundle: every other fact is about a bundle, so they are the
            // neutral values and `preflight` refuses on the kind alone.
            return Ok(BundleFacts {
                bundle: exe.to_path_buf(),
                parent: exe.parent().unwrap_or(Path::new("/")).to_path_buf(),
                kind: InstallKind::DevBuild,
                homebrew: false,
                parent_writable: false,
                owned_by_me: false,
                read_only: false,
                canonical,
                free_parent_bytes: 0,
            });
        };
        let parent = bundle.parent().unwrap_or(Path::new("/")).to_path_buf();
        let (read_only, free_parent_bytes) = Self::volume(&parent);
        Ok(BundleFacts {
            kind: Self::install_kind(&bundle, &parent),
            homebrew: Self::homebrew_managed(),
            parent_writable: Self::writable(&parent),
            owned_by_me: Self::owned_by_me(&bundle),
            read_only,
            canonical,
            free_parent_bytes,
            bundle,
            parent,
        })
    }

    fn stage(
        &self,
        facts: &BundleFacts,
        archive: &Path,
        expect: &StagedExpect,
    ) -> Result<StagedBundle> {
        let name = facts
            .bundle
            .file_name()
            .ok_or_else(|| Error::Other("the bundle path has no name".into()))?
            .to_owned();
        let stage_dir = facts.parent.join(format!("{STAGE_PREFIX}{}", std::process::id()));
        // A leftover from an interrupted apply is not a reason to refuse — it is
        // a reason to start clean.
        if stage_dir.exists() {
            let _ = std::fs::remove_dir_all(&stage_dir);
        }
        std::fs::create_dir(&stage_dir).map_err(|e| {
            Error::Other(format!("could not create {}: {e}", stage_dir.display()))
        })?;
        let app = stage_dir.join(&name);

        // Extract through the same guarded extractor the binary cache uses: it
        // refuses absolute paths, `..`, and links that resolve outside the
        // destination, so a signed-but-hostile archive cannot write anywhere but
        // here. Strip 1 because the archive holds `rexenv.app/…` at its root.
        let staged = (|| -> Result<()> {
            let file = std::fs::File::open(archive)
                .map_err(|e| Error::Other(format!("could not open {}: {e}", archive.display())))?;
            crate::core::binaries::extract_tar_gz_tree(std::io::BufReader::new(file), &app)?;
            // The bundle directory's own mode comes from the archive; set it
            // explicitly so no umask or odd tar entry can leave it unreadable
            // for anyone but this user.
            #[allow(clippy::permissions_set_readonly_false)]
            std::fs::set_permissions(&app, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .map_err(|e| Error::Other(format!("could not set the bundle's mode: {e}")))?;
            Self::verify_staged(&app, expect)
        })();

        if let Err(e) = staged {
            // Everything this created goes away, so a failed stage leaves the
            // installed bundle and its folder exactly as they were.
            let _ = std::fs::remove_dir_all(&stage_dir);
            return Err(e);
        }
        Ok(StagedBundle { path: app, stage_dir })
    }

    fn swap(
        &self,
        installed: &Path,
        staged: &StagedBundle,
    ) -> std::result::Result<SwapReceipt, SwapFailure> {
        match Self::rename_swap(installed, &staged.path) {
            Ok(()) => Ok(SwapReceipt {
                installed: installed.to_path_buf(),
                // After an exchange the PREVIOUS bundle is where the staged one
                // was — the receipt says so rather than the caller assuming it.
                previous: staged.path.clone(),
                method: SwapMethod::AtomicSwap,
            }),
            Err(e) if matches!(Self::classify(&e), SwapFailure::Unsupported) => {
                // No atomic exchange on this filesystem. Two renames, and a
                // restore if the second one fails — the window between them is
                // the reason this is the fallback and not the default.
                let aside = staged.stage_dir.join("previous.app");
                std::fs::rename(installed, &aside).map_err(|e| Self::classify(&e))?;
                if let Err(e) = std::fs::rename(&staged.path, installed) {
                    let failure = Self::classify(&e);
                    // Put it back. If even this fails the bundle is still on
                    // disk at `aside`, and the message says where.
                    if std::fs::rename(&aside, installed).is_err() {
                        return Err(SwapFailure::Other(format!(
                            "{failure}; the previous rexenv is at {} and can be moved back \
                             by hand",
                            aside.display()
                        )));
                    }
                    return Err(failure);
                }
                Ok(SwapReceipt {
                    installed: installed.to_path_buf(),
                    previous: aside,
                    method: SwapMethod::RenamePair,
                })
            }
            Err(e) => Err(Self::classify(&e)),
        }
    }

    fn spawn_relauncher(&self, bundle: &Path) -> Result<()> {
        // Our own binary, re-executed in relauncher mode — the same self-exec
        // shape the DNS agent and the tunnel guard use, so there is no second
        // artifact to ship, sign and keep in step.
        //
        // It is spawned from the OLD inode (this process is the one being
        // replaced), which is why `--relaunch-after` is a cross-version
        // contract: the version being replaced starts the version replacing it.
        let exe = std::env::current_exe()?;
        let me = std::process::id();
        let start = super::process_start_token(me).ok_or_else(|| {
            Error::Other(format!("could not read this process's start time (pid {me})"))
        })?;
        // Detached and silent, and NOT reaped: unlike the tunnel guard this
        // helper is meant to outlive us by design — we are about to exit, and
        // launchd reaps it.
        std::process::Command::new(exe)
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

    fn sweep_leftovers(
        &self,
        parent: &Path,
        my_version: &str,
        delete_previous: bool,
    ) -> Result<Vec<Leftover>> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(parent) else { return Ok(out) };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { continue };
            if !name.starts_with(STAGE_PREFIX) || !dir.is_dir() {
                continue;
            }
            // Classify by the VERSION inside, never by a marker file. A crash
            // between the swap and any write cannot make a bundle lie about its
            // own Info.plist, and "derived beats typed" is the rule this project
            // keeps re-learning.
            let app = std::fs::read_dir(&dir)
                .ok()
                .and_then(|mut e| {
                    e.find_map(|x| {
                        let p = x.ok()?.path();
                        (p.extension()? == "app").then_some(p)
                    })
                })
                .unwrap_or_else(|| dir.join("previous.app"));
            let version = Self::read_plist(&app)
                .ok()
                .and_then(|p| Self::plist_string(&p, "CFBundleShortVersionString"));

            // Older than me = the bundle I replaced; newer or equal = a staged
            // copy some interrupted apply left behind, which is safe to drop now.
            let is_previous = version
                .as_deref()
                .map(|v| {
                    crate::core::updates::version_segments(v)
                        < crate::core::updates::version_segments(my_version)
                })
                .unwrap_or(false);
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
