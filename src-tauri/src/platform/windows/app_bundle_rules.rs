//! Windows: the PURE half of replacing the app's own install directory — the
//! decisions `app_bundle.rs` makes over paths and numbers, with no Windows API in
//! them, so the macOS test host runs these tests too (`platform/mod.rs` includes
//! this file under `cfg(test)` there, like `pe.rs`).
//!
//! What a Windows "bundle" IS: the install directory the NSIS installer makes —
//! `rexenv.exe`, the `rex.exe` sidecar and `uninstall.exe`, nothing else (there is
//! no `resources` tree; read off tauri-bundler's `installer.nsi`, 19 Sep 2026). The
//! update swaps that directory whole, the way macOS swaps the `.app`, because a
//! running `.exe` cannot be deleted or overwritten but it and its directory can be
//! renamed, and the process keeps running from the renamed file (measured on the
//! Dell, 19 Sep 2026 — `docs/PLAN-windows-port.md` D5).

use crate::platform::traits::InstallKind;
use std::path::Path;

/// Staging directories are dot-prefixed, as on macOS: Explorer does not hide them
/// (Windows hides by attribute, not by name), but the prefix is what the sweep
/// matches on, and one name across both platforms is one rule to remember.
pub const STAGE_PREFIX: &str = ".rexenv-update-";

/// `${PRODUCTNAME}` in the installer: the install directory's name under
/// `%LOCALAPPDATA%`, the uninstall registry key's leaf, and the `ProductName`
/// string the build stamps into `rexenv.exe`'s VERSIONINFO.
pub const PRODUCT_NAME: &str = "rexenv";

/// The uninstall entry the installer writes — `${UNINSTKEY}` with `SHCTX` = HKCU
/// for a per-user install. After a swap its `DisplayVersion` still says the
/// version that was installed, so the swap rewrites it: Apps & Features is the
/// one place a user sees a version without opening the app.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\rexenv";

/// A path as one comparable string: backslashes, no trailing separator, no case.
/// Windows paths are case-insensitive and arrive with either separator, and the
/// macOS test host sees `C:\…` as a single opaque component — so the comparison
/// is on text, deliberately, rather than on `Path` components.
fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase()
}

/// Where the install directory `bundle` lives, given where this user's Local
/// AppData and the machine's Program Files folders are.
///
/// Measured off the installer rather than assumed: `currentUser` installs to
/// `$LOCALAPPDATA\${PRODUCTNAME}`, `perMachine` to `$PROGRAMFILES64\${PRODUCTNAME}`
/// (or `$PROGRAMFILES` on x86). A `target\debug` or `target\release` parent is a
/// cargo build — the `cargo run` case, with nothing installed to replace.
pub fn install_kind_of(bundle: &Path, local_app_data: Option<&Path>, program_files: &[&Path]) -> InstallKind {
    let b = norm(bundle);
    if let Some(lad) = local_app_data {
        if b == format!("{}\\{}", norm(lad), PRODUCT_NAME) {
            return InstallKind::ProgramsPerUser;
        }
    }
    if program_files.iter().any(|pf| {
        let pf = norm(pf);
        !pf.is_empty() && (b == pf || b.starts_with(&format!("{pf}\\")))
    }) {
        return InstallKind::ProgramFiles;
    }
    let is_cargo_dir = b.ends_with("\\target\\debug") || b.ends_with("\\target\\release");
    if is_cargo_dir {
        return InstallKind::DevBuild;
    }
    InstallKind::Elsewhere
}

/// Whether a directory name is one of ours to sweep.
pub fn is_stage_dir(name: &str) -> bool {
    name.starts_with(STAGE_PREFIX)
}

/// `VS_FIXEDFILEINFO`'s two product-version words as the `a.b.c` the descriptor
/// names. The build stamps `major.minor.patch.0`; the fourth field is dropped
/// because no rexenv version has one and `version_segments` compares three.
pub fn version_from_fixed(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16)
}

/// Older than me = the bundle I replaced; newer or equal = a staged copy some
/// interrupted apply left behind, which is safe to drop now. Same rule as macOS.
pub fn is_previous(found: &str, mine: &str) -> bool {
    crate::core::updates::version_segments(found) < crate::core::updates::version_segments(mine)
}

/// The parent's start token that rides the relaunch argv: its creation time in
/// FILETIME ticks, as decimal. A pid the kernel recycled between the spawn and
/// the wait wears a different one.
pub fn relaunch_token(creation_ticks: u64) -> String {
    creation_ticks.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Where the installer puts things is the whole classification, and it is
    /// measured, not remembered: `$LOCALAPPDATA\rexenv` per user, Program Files
    /// per machine, and a cargo `target` dir is a dev build.
    #[test]
    fn the_install_kind_follows_where_the_installer_puts_things() {
        let lad = PathBuf::from(r"C:\Users\DELL\AppData\Local");
        let pf = [PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)")];
        let pfs: Vec<&Path> = pf.iter().map(PathBuf::as_path).collect();
        let kind = |p: &str| install_kind_of(Path::new(p), Some(&lad), &pfs);

        assert_eq!(kind(r"C:\Users\DELL\AppData\Local\rexenv"), InstallKind::ProgramsPerUser);
        // Case and separator do not change the answer: Windows paths ignore both.
        assert_eq!(kind(r"c:/users/dell/appdata/local/REXENV/"), InstallKind::ProgramsPerUser);
        assert_eq!(kind(r"C:\Program Files\rexenv"), InstallKind::ProgramFiles);
        assert_eq!(kind(r"C:\Program Files (x86)\rexenv"), InstallKind::ProgramFiles);
        assert_eq!(kind(r"C:\Users\DELL\rexenv-src\src-tauri\target\debug"), InstallKind::DevBuild);
        assert_eq!(kind(r"C:\Users\DELL\rexenv-src\src-tauri\target\release"), InstallKind::DevBuild);
        assert_eq!(kind(r"C:\Users\DELL\Downloads\rexenv"), InstallKind::Elsewhere);
        // A sibling of the product dir is NOT the product dir.
        assert_eq!(kind(r"C:\Users\DELL\AppData\Local\rexenv-old"), InstallKind::Elsewhere);
        // A prefix that merely shares text with Program Files is not under it.
        assert_eq!(kind(r"C:\Program Filesystem\rexenv"), InstallKind::Elsewhere);
        // No Local AppData known at all: nothing is per-user, and nothing panics.
        assert_eq!(install_kind_of(Path::new(r"C:\x\rexenv"), None, &[]), InstallKind::Elsewhere);
    }

    #[test]
    fn the_product_version_is_read_off_the_fixed_info_words_and_drops_the_fourth_field() {
        // 0.7.1.0 → ms = 0<<16 | 7, ls = 1<<16 | 0
        assert_eq!(version_from_fixed(7, 1 << 16), "0.7.1");
        assert_eq!(version_from_fixed((1 << 16) | 12, (3 << 16) | 9), "1.12.3");
    }

    #[test]
    fn a_leftover_older_than_me_is_the_previous_bundle_and_a_newer_one_is_a_stale_stage() {
        assert!(is_previous("0.7.0", "0.7.1"));
        assert!(!is_previous("0.7.1", "0.7.1"));
        assert!(!is_previous("0.8.0", "0.7.1"));
    }

    #[test]
    fn stage_dirs_are_named_for_the_sweep_and_the_token_is_the_creation_time() {
        assert!(is_stage_dir(".rexenv-update-4242"));
        assert!(!is_stage_dir("rexenv"));
        assert!(!is_stage_dir("rexenv-update-4242"), "the dot is the prefix");
        assert_eq!(relaunch_token(133_000_000_000_000_000), "133000000000000000");
    }
}
