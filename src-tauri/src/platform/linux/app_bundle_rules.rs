//! The Linux self-update's pure rules (docs/PLAN-linux-port.md L7, owner ruling 24 Sep 2026):
//! which kind of install this is, what a staged package must say about itself, and the
//! root command that installs one. Text only, tested on every host; the syscalls, the
//! `dpkg-deb` calls and the `pkexec` step are in `app_bundle.rs`.
//!
//! Two kinds of install, two swaps:
//! - **a `.deb`** puts `/usr/bin/rexenv` in place, root-owned — replaced through the package
//!   manager (`dpkg -i` in the polkit step, one prompt), never by a rename;
//! - **an AppImage** is ONE user-owned file — replaced by the macOS-shaped exchange
//!   (`renameat2(RENAME_EXCHANGE)`) beside itself, no prompt.
//!
//! A `cargo` binary in `target/` is a dev build; anything else is "elsewhere" and refused with
//! the words for it.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::platform::traits::{InstallKind, StagedExpect};
use std::path::{Path, PathBuf};

/// Staging directories are dot-prefixed, as on macOS: a file manager skips them, and a user
/// glancing at the folder mid-update does not see a second rexenv.
pub(crate) const STAGE_PREFIX: &str = ".rexenv-update-";
/// Where the `.deb` lands: `/usr/bin/rexenv`, and nothing else is a package install.
pub(crate) const PACKAGE_EXE: &str = "/usr/bin/rexenv";
/// The package's name in `dpkg`, and the AppImage's identity — `StagedExpect::identifier` on Linux.
pub(crate) const PACKAGE_NAME: &str = "rexenv";

/// The in-app update's own polkit door. pkexec picks an action by the program's PATH alone, so
/// "install the update" needs a program of its own beside `/usr/bin/rexenv`'s generic step:
/// `linux/rexenv-privileged-update` (a `/bin/sh` script that runs only `/usr/bin/dpkg -i` on an
/// absolute `.deb`), installed by the deb at `UPDATE_PROGRAM` and annotated by
/// `UPDATE_ACTION_ID` in `linux/dev.rexenv.rexenv.policy`. The 27 Sep 2026 update (0.8.7 →
/// 0.8.8 on the VM) asked with the generic sentence — DNS route, edge, CA — for a package install.
pub(crate) const UPDATE_PROGRAM: &str = "/usr/libexec/rexenv/privileged-update";
pub(crate) const UPDATE_ACTION_ID: &str = "dev.rexenv.rexenv.privileged-update";
/// The generic step's action, for the tests that read the policy file.
pub(crate) const STEP_ACTION_ID: &str = "dev.rexenv.rexenv.privileged-step";

/// Whether the update may go through its own door: the INSTALLED action file declares the update
/// action for `UPDATE_PROGRAM` AND that program is an executable file (`program_mode`, `None`
/// when absent). Otherwise the generic step installs the package — an install whose action file
/// predates the update action (any deb before 0.8.11) keeps the generic sentence for that one
/// update and is never refused.
pub(crate) fn update_program_usable(policy: Option<&str>, program_mode: Option<u32>) -> bool {
    let declared = policy.is_some_and(|p| {
        p.contains(&format!("<action id=\"{UPDATE_ACTION_ID}\">"))
            && p.contains(&format!("<annotate key=\"org.freedesktop.policykit.exec.path\">{UPDATE_PROGRAM}</annotate>"))
    });
    declared && program_mode.is_some_and(|m| m & 0o111 != 0)
}

/// What this process is, from the executable and `$APPIMAGE`: the bundle path (what gets
/// replaced) and its kind.
pub(crate) fn classify(exe: &Path, appimage: Option<&Path>) -> (PathBuf, InstallKind) {
    if let Some(image) = appimage {
        return (image.to_path_buf(), InstallKind::PortableFile);
    }
    if exe == Path::new(PACKAGE_EXE) {
        return (exe.to_path_buf(), InstallKind::SystemPackage);
    }
    if exe.components().any(|c| c.as_os_str() == "target") {
        return (exe.to_path_buf(), InstallKind::DevBuild);
    }
    (exe.to_path_buf(), InstallKind::Elsewhere)
}

/// The executable that runs the relauncher after a swap. On macOS and Windows
/// `current_exe()` is a PATH, and after the bundle rename / RenamePair that path names the
/// NEW build — so the old process starts the new one through its own path. On Linux
/// `current_exe()` is `/proc/self/exe`, which names the INODE: once `dpkg -i` has unlinked
/// `/usr/bin/rexenv` it reads `/usr/bin/rexenv (deleted)`, and spawning that is ENOENT —
/// measured 27 Sep 2026 on the 22.04 VM, the first in-app deb update: "could not spawn the
/// relauncher (No such file or directory)", the app closed and did not come back (the honest
/// fallback ran; the next open was 0.8.8). So a package install — or any executable the
/// kernel already calls deleted — starts the relauncher from `bundle`, the new
/// `/usr/bin/rexenv`, whose `--relaunch-after` contract is cross-version. An AppImage keeps
/// `current_exe()`: its mount lives as long as this process, and the new image at `bundle`
/// may need FUSE this process was not started with.
pub(crate) fn relauncher_exe(current_exe: &Path, bundle: &Path, kind: InstallKind) -> PathBuf {
    if kind == InstallKind::SystemPackage || current_exe.to_string_lossy().ends_with(" (deleted)") {
        return bundle.to_path_buf();
    }
    current_exe.to_path_buf()
}

/// The descriptor a Linux build reads: one per package kind and arch, the same schema as the
/// macOS and Windows documents (`app-manifest-linux-<kind>-<arch>.json`).
pub(crate) fn descriptor_variant(kind: InstallKind, arch: &str) -> Option<String> {
    let kind = match kind {
        InstallKind::SystemPackage => "deb",
        InstallKind::PortableFile => "appimage",
        _ => return None,
    };
    Some(format!("{kind}-{arch}"))
}

/// `dpkg`'s spelling of an arch, as `dpkg --print-architecture` answers it.
pub(crate) fn dpkg_arch(rust_arch: &str) -> &'static str {
    match rust_arch {
        "aarch64" => "arm64",
        _ => "amd64",
    }
}

/// One field of `dpkg-deb -f <deb>` output (`Key: value` lines).
pub(crate) fn control_field(control: &str, key: &str) -> Option<String> {
    control.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(':')).map(|v| v.trim().to_string()))
}

/// Everything a staged `.deb` must say about itself before `dpkg -i` may run: the package
/// name, the version the signed release named, and this machine's architecture. Each refusal
/// names what was wrong — the user decides whether to retry or report.
pub(crate) fn verify_deb_control(control: &str, expect: &StagedExpect, host_dpkg_arch: &str) -> Result<(), String> {
    let got = |k: &str| control_field(control, k).unwrap_or_default();
    let package = got("Package");
    if package != expect.identifier {
        return Err(format!("the downloaded package is {package}, not {}", expect.identifier));
    }
    let version = got("Version");
    if version != expect.version {
        return Err(format!(
            "the downloaded package says it is {version}, but the signed release names {} — it was discarded and nothing was changed",
            expect.version
        ));
    }
    let arch = got("Architecture");
    if arch != host_dpkg_arch {
        return Err(format!("the downloaded package is built for {arch}, and this machine is {host_dpkg_arch}"));
    }
    Ok(())
}

/// The listing (`dpkg-deb -c`) must carry the executable and every required sibling under
/// `usr/bin/` — a package without `rex` would silently break every terminal after the update.
/// Members are matched with or without a leading `./`: `dpkg-deb -b` writes `./usr/bin/rexenv`,
/// Tauri's own bundler writes `usr/bin/rexenv` — measured on the VM-built 0.8.7 deb, 25 Sep
/// 2026, where the `./`-only match would have refused the REAL package.
pub(crate) fn verify_deb_listing(listing: &str, expect: &StagedExpect) -> Result<(), String> {
    let mut required: Vec<&str> = vec![expect.executable.as_str()];
    required.extend(expect.required_binaries.iter().map(String::as_str));
    let members: Vec<&str> = listing
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .map(|m| m.strip_prefix("./").unwrap_or(m))
        .collect();
    for name in required {
        let member = format!("usr/bin/{name}");
        if !members.iter().any(|m| *m == member) {
            return Err(format!("the downloaded package is missing {member}"));
        }
    }
    Ok(())
}

/// Single-quote for `/bin/sh` (the polkit step runs `sh -c`); an embedded `'` becomes `'\''`.
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// The one privileged command an update runs: install the staged package. Absolute path
/// (`pkexec` strips `PATH`); `dpkg` itself refuses a package for another architecture.
pub(crate) fn dpkg_install_command(deb: &Path) -> String {
    format!("/usr/bin/dpkg -i {}", sh_quote(deb))
}

/// The stage directory for THIS process, beside `parent`.
pub(crate) fn stage_dir(parent: &Path, pid: u32) -> PathBuf {
    parent.join(format!("{STAGE_PREFIX}{pid}"))
}

/// Is `name` one of our stage directories?
pub(crate) fn is_stage_dir(name: &str) -> bool {
    name.starts_with(STAGE_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect() -> StagedExpect {
        StagedExpect {
            version: "0.8.8".into(),
            identifier: "rexenv".into(),
            executable: "rexenv".into(),
            archs: vec![],
            required_binaries: vec!["rex".into()],
            codesign: false,
        }
    }

    #[test]
    fn the_kind_comes_from_the_executable_and_the_appimage_variable() {
        assert_eq!(classify(Path::new("/tmp/.mount_rexenvX/usr/bin/rexenv"), Some(Path::new("/home/u/Apps/rexenv.AppImage"))), (PathBuf::from("/home/u/Apps/rexenv.AppImage"), InstallKind::PortableFile));
        assert_eq!(classify(Path::new("/usr/bin/rexenv"), None).1, InstallKind::SystemPackage);
    }

    #[test]
    fn the_relauncher_runs_from_the_new_package_exe_never_from_a_deleted_inode() {
        // The 27 Sep 2026 VM update: `/proc/self/exe` read "(deleted)" after dpkg and the spawn
        // was ENOENT. A package install always uses the bundle; so does anything deleted.
        let deb = Path::new("/usr/bin/rexenv");
        assert_eq!(relauncher_exe(Path::new("/usr/bin/rexenv (deleted)"), deb, InstallKind::Elsewhere), deb);
        assert_eq!(relauncher_exe(Path::new("/usr/bin/rexenv"), deb, InstallKind::SystemPackage), deb);
        // An AppImage keeps its own mounted executable — the mount outlives the swap.
        let mount = Path::new("/tmp/.mount_rexenvX/usr/bin/rexenv");
        let image = Path::new("/home/u/Apps/rexenv.AppImage");
        assert_eq!(relauncher_exe(mount, image, InstallKind::PortableFile), mount);
        assert_eq!(classify(Path::new("/home/u/rexenv/src-tauri/target/release/rexenv"), None).1, InstallKind::DevBuild);
        assert_eq!(classify(Path::new("/home/u/Downloads/rexenv"), None).1, InstallKind::Elsewhere);
        assert_eq!(descriptor_variant(InstallKind::SystemPackage, "x86_64").as_deref(), Some("deb-x86_64"));
        assert_eq!(descriptor_variant(InstallKind::PortableFile, "aarch64").as_deref(), Some("appimage-aarch64"));
        assert_eq!(descriptor_variant(InstallKind::DevBuild, "x86_64"), None, "a dev build reads no descriptor");
        assert_eq!(dpkg_arch("aarch64"), "arm64");
        assert_eq!(dpkg_arch("x86_64"), "amd64");
    }

    #[test]
    fn a_staged_deb_must_be_this_package_this_version_this_arch_with_the_sidecar() {
        let ok = "Package: rexenv\nVersion: 0.8.8\nArchitecture: arm64\nMaintainer: rexenv\n";
        assert_eq!(verify_deb_control(ok, &expect(), "arm64"), Ok(()));
        let wrong_v = ok.replace("0.8.8", "0.8.7");
        assert!(verify_deb_control(&wrong_v, &expect(), "arm64").unwrap_err().contains("says it is 0.8.7"));
        assert!(verify_deb_control(ok, &expect(), "amd64").unwrap_err().contains("built for arm64"));
        let other = ok.replace("Package: rexenv", "Package: rexenv-evil");
        assert!(verify_deb_control(&other, &expect(), "arm64").unwrap_err().contains("is rexenv-evil"));
        let listing = "drwxr-xr-x root/root 0 2026-09-24 ./usr/bin/\n-rwxr-xr-x root/root 42 2026-09-24 ./usr/bin/rexenv\n-rwxr-xr-x root/root 9 2026-09-24 ./usr/bin/rex\n";
        assert_eq!(verify_deb_listing(listing, &expect()), Ok(()));
        let no_rex = listing.lines().filter(|l| !l.ends_with("/rex")).collect::<Vec<_>>().join("\n");
        assert!(verify_deb_listing(&no_rex, &expect()).unwrap_err().contains("missing usr/bin/rex"));
        // Tauri's bundler writes members WITHOUT `./` (the VM-built 0.8.7 deb, 25 Sep 2026).
        let tauri = "-rwxr-xr-x 0/0 42875800 2026-09-24 09:39 usr/bin/rexenv\n-rwxr-xr-x 0/0 984720 2026-09-24 09:39 usr/bin/rex\n";
        assert_eq!(verify_deb_listing(tauri, &expect()), Ok(()), "the real package's listing shape");
    }

    /// The update's own polkit door, as the tree ships it: the action file declares BOTH actions
    /// for DISTINCT programs (pkexec picks by path), the update's message names the update, both
    /// are `auth_admin` three times with no `_keep`; the program is a `/bin/sh` script that runs
    /// only `/usr/bin/dpkg -i` on an absolute `.deb` and ends in `sync`; the deb ships both; and
    /// the door opens only with both halves present (an older install's file → the generic step).
    #[test]
    fn the_update_has_its_own_polkit_action_program_and_sentence() {
        const POLICY: &str = include_str!("../../../linux/dev.rexenv.rexenv.policy");
        const PROGRAM: &str = include_str!("../../../linux/rexenv-privileged-update");
        const CONF: &str = include_str!("../../../tauri.conf.json");
        let action = |id: &str| {
            let i = POLICY.find(&format!("<action id=\"{id}\">")).unwrap_or_else(|| panic!("no action {id}"));
            &POLICY[i..POLICY[i..].find("</action>").expect("closed") + i]
        };
        let (step, update) = (action(STEP_ACTION_ID), action(UPDATE_ACTION_ID));
        assert!(step.contains("<annotate key=\"org.freedesktop.policykit.exec.path\">/usr/bin/rexenv</annotate>"));
        assert!(update.contains(&format!("<annotate key=\"org.freedesktop.policykit.exec.path\">{UPDATE_PROGRAM}</annotate>")));
        assert!(update.contains("<message>rexenv needs administrator permission to install the update it downloaded"), "{update}");
        assert!(!update.contains("DNS route") && !update.contains("HTTPS edge"), "the update's sentence is its own");
        for a in [step, update] {
            assert_eq!(a.matches(">auth_admin<").count(), 3, "auth_admin every time");
            assert!(!a.contains("_keep"), "no cached grant");
        }
        assert!(PROGRAM.starts_with("#!/bin/sh\n"));
        assert!(PROGRAM.contains("/usr/bin/dpkg -i \"$1\""), "absolute dpkg on the one argument");
        assert!(PROGRAM.contains("/*.deb) ;;"), "anything but an absolute .deb is refused");
        assert!(PROGRAM.contains("\n/bin/sync\n"), "flushed like every privileged step (#740)");
        assert!(PROGRAM.lines().all(|l| !l.trim_start().starts_with("dpkg ")), "pkexec strips PATH");
        assert!(CONF.contains(&format!("\"{UPDATE_PROGRAM}\": \"linux/rexenv-privileged-update\"")), "the deb ships the program");
        assert!(CONF.contains("\"/usr/share/polkit-1/actions/dev.rexenv.rexenv.policy\": \"linux/dev.rexenv.rexenv.policy\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/linux/rexenv-privileged-update")).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "the script is executable in git — the deb copies its mode ({mode:o})");
        }
        // TEXT: the swap takes the door, never the generic step directly.
        const SWAP: &str = include_str!("app_bundle.rs");
        assert!(SWAP.contains("run_privileged_update(&staged.path, &reason)"), "the swap goes through the update's door");
        assert!(!SWAP.contains("run_privileged(&rules::dpkg_install_command"), "the swap bypasses the door");
        assert!(update_program_usable(Some(POLICY), Some(0o100755)));
        assert!(!update_program_usable(Some(POLICY), Some(0o100644)), "not executable → the generic step");
        assert!(!update_program_usable(Some(POLICY), None), "no program → the generic step");
        assert!(!update_program_usable(None, Some(0o100755)), "no action file → the generic step");
        let older = POLICY.replace(&format!("<action id=\"{UPDATE_ACTION_ID}\">"), "<action id=\"dev.rexenv.rexenv.other\">");
        assert!(!update_program_usable(Some(&older), Some(0o100755)), "an older deb's action file → the generic step");
    }

    #[test]
    fn the_install_command_is_absolute_and_quoted() {
        assert_eq!(dpkg_install_command(Path::new("/home/u/.local/share/rexenv/updates/.rexenv-update-7/rexenv_0.8.8_arm64.deb")), "/usr/bin/dpkg -i '/home/u/.local/share/rexenv/updates/.rexenv-update-7/rexenv_0.8.8_arm64.deb'");
        assert!(dpkg_install_command(Path::new("/a/it's.deb")).contains("'/a/it'\\''s.deb'"));
        assert_eq!(stage_dir(Path::new("/home/u/Apps"), 42), PathBuf::from("/home/u/Apps/.rexenv-update-42"));
        assert!(is_stage_dir(".rexenv-update-42") && !is_stage_dir("rexenv.AppImage"));
    }
}
