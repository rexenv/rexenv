//! core::cli — status + PATH install for the bundled `rex` CLI.
//!
//! `rex` ships as a Tauri sidecar next to the app binary (packaged:
//! `Contents/MacOS/rex`; `tauri dev`: `target/debug/rex`). Install = ONE
//! symlink at `Paths::cli_symlink_path` (macOS `/usr/local/bin/rex` — the
//! Docker/Herd convention): tried unprivileged first (Intel-Homebrew machines
//! have a user-writable `/usr/local/bin`), one foreground admin prompt via
//! `PrivilegeManager` otherwise. The symlink tracks the app bundle, so an app
//! update needs no re-install; a MOVED bundle shows as stale in `status` and
//! Install refreshes it.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The bundled `rex` sidecar next to the running app binary. Errors when it
/// was never staged (bare `cargo run` without `scripts/build-cli.sh`).
pub fn bundled_rex() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let rex = exe
        .parent()
        .ok_or_else(|| Error::Other("app binary has no parent directory".into()))?
        .join("rex");
    if rex.is_file() {
        Ok(rex)
    } else {
        Err(Error::Other(format!("bundled rex not found at {}", rex.display())))
    }
}

/// Install state for the Settings card (mirrors the frontend `CliStatus`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    /// The sidecar exists next to the app binary — install is possible.
    pub available: bool,
    /// Something is symlinked at the install path.
    pub installed: bool,
    /// That symlink resolves to THIS app's bundled rex (false = stale copy
    /// from a moved/old bundle, or a foreign `rex`).
    pub current: bool,
    pub link_path: String,
    pub bundled_path: Option<String>,
}

pub fn status(platform: &dyn Platform) -> Result<CliStatus> {
    let link = platform.paths().cli_symlink_path()?;
    Ok(status_from(&link, bundled_rex().ok()))
}

fn status_from(link: &Path, bundled: Option<PathBuf>) -> CliStatus {
    let target = std::fs::read_link(link).ok();
    let current = matches!((&bundled, &target), (Some(b), Some(t)) if b == t);
    CliStatus {
        available: bundled.is_some(),
        installed: target.is_some(),
        current,
        link_path: link.display().to_string(),
        bundled_path: bundled.map(|p| p.display().to_string()),
    }
}

/// Install/refresh the PATH symlink. Unprivileged first; otherwise ONE admin
/// prompt (callers run this off the UI thread, foreground — the prompt rule).
/// Verified after either path: the link must resolve to the bundled rex.
pub fn install(platform: &dyn Platform) -> Result<()> {
    let src = bundled_rex()?;
    let dst = platform.paths().cli_symlink_path()?;
    if try_symlink_unprivileged(&src, &dst).is_err() {
        platform.privileges().run_privileged(&install_script(&src, &dst)?)?;
    }
    match std::fs::read_link(&dst) {
        Ok(t) if t == src => Ok(()),
        _ => Err(Error::Other(format!(
            "the CLI symlink at {} was not created",
            dst.display()
        ))),
    }
}

/// Direct symlink attempt — idempotent (an existing link/file is replaced).
#[cfg(unix)]
fn try_symlink_unprivileged(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::remove_file(dst) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(not(unix))]
fn try_symlink_unprivileged(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("not implemented on this platform"))
}

/// The privileged fallback script. Paths are single-quoted (app-data and
/// bundle paths contain spaces); a path containing a quote is refused rather
/// than escaped — no rexenv-controlled path ever has one.
fn install_script(src: &Path, dst: &Path) -> Result<String> {
    let (src, dst) = (src.display().to_string(), dst.display().to_string());
    let dir = Path::new(&dst)
        .parent()
        .map(|p| p.display().to_string())
        .ok_or_else(|| Error::Other("CLI symlink path has no parent".into()))?;
    if src.contains('\'') || dst.contains('\'') {
        return Err(Error::Other("refusing a path containing a quote".into()));
    }
    Ok(format!("mkdir -p '{dir}' && ln -sf '{src}' '{dst}'"))
}

/// Teardown: remove the symlink only when it is OURS (its target ends in the
/// sidecar name — content-checked like the resolver sweep, so a foreign
/// `rex` on PATH is never touched). Best-effort and unprivileged: teardown
/// must not add a prompt, and a root-owned leftover link is harmless litter
/// that the next install overwrites.
pub fn remove_symlink_best_effort(platform: &dyn Platform) {
    let Ok(link) = platform.paths().cli_symlink_path() else { return };
    let Ok(target) = std::fs::read_link(&link) else { return };
    let ours_current = bundled_rex().is_ok_and(|b| b == target);
    let dangling_rex = target.file_name().is_some_and(|n| n == "rex") && !target.exists();
    if ours_current || dangling_rex {
        let _ = std::fs::remove_file(&link);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-cli-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn symlink_install_is_idempotent_and_replaces_a_stale_link() {
        let dir = scratch("link");
        let src_a = dir.join("rex-a");
        let src_b = dir.join("rex-b");
        std::fs::write(&src_a, "a").unwrap();
        std::fs::write(&src_b, "b").unwrap();
        let dst = dir.join("bin").join("rex");
        try_symlink_unprivileged(&src_a, &dst).expect("first install (creates parent)");
        assert_eq!(std::fs::read_link(&dst).unwrap(), src_a);
        try_symlink_unprivileged(&src_a, &dst).expect("re-install over itself");
        try_symlink_unprivileged(&src_b, &dst).expect("replace a stale link");
        assert_eq!(std::fs::read_link(&dst).unwrap(), src_b);
    }

    #[test]
    fn status_reads_missing_stale_and_current_links() {
        let dir = scratch("status");
        let bundled = dir.join("rex");
        std::fs::write(&bundled, "x").unwrap();
        let link = dir.join("bin").join("rex");

        let s = status_from(&link, Some(bundled.clone()));
        assert!(s.available && !s.installed && !s.current, "no link yet: {s:?}");

        try_symlink_unprivileged(&dir.join("elsewhere"), &link).unwrap();
        let s = status_from(&link, Some(bundled.clone()));
        assert!(s.installed && !s.current, "stale link: {s:?}");

        try_symlink_unprivileged(&bundled, &link).unwrap();
        let s = status_from(&link, Some(bundled));
        assert!(s.installed && s.current, "current link: {s:?}");
    }

    #[test]
    fn install_script_quotes_paths_and_refuses_quotes() {
        let script = install_script(
            Path::new("/Applications/My App.app/Contents/MacOS/rex"),
            Path::new("/usr/local/bin/rex"),
        )
        .unwrap();
        assert_eq!(
            script,
            "mkdir -p '/usr/local/bin' && \
             ln -sf '/Applications/My App.app/Contents/MacOS/rex' '/usr/local/bin/rex'"
        );
        assert!(install_script(Path::new("/tmp/a'b/rex"), Path::new("/usr/local/bin/rex")).is_err());
    }
}
