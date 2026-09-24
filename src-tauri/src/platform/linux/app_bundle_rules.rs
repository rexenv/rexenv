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

    #[test]
    fn the_install_command_is_absolute_and_quoted() {
        assert_eq!(dpkg_install_command(Path::new("/home/u/.local/share/rexenv/updates/.rexenv-update-7/rexenv_0.8.8_arm64.deb")), "/usr/bin/dpkg -i '/home/u/.local/share/rexenv/updates/.rexenv-update-7/rexenv_0.8.8_arm64.deb'");
        assert!(dpkg_install_command(Path::new("/a/it's.deb")).contains("'/a/it'\\''s.deb'"));
        assert_eq!(stage_dir(Path::new("/home/u/Apps"), 42), PathBuf::from("/home/u/Apps/.rexenv-update-42"));
        assert!(is_stage_dir(".rexenv-update-42") && !is_stage_dir("rexenv.AppImage"));
    }
}
