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
use crate::platform::traits::{CliInstall, Platform};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The sidecar's file name on an OS whose executables carry `exe_suffix` — `rex` on macOS, `rex.exe` on
/// Windows (W8 S4, ledger #633). Tauri bundles `binaries/rex-<target triple><suffix>` next to the app under
/// this name.
pub fn sidecar_file_name(exe_suffix: &str) -> String {
    format!("rex{exe_suffix}")
}

/// The bundled `rex` sidecar next to the running app binary. Errors when it
/// was never staged (bare `cargo run` without `scripts/build-cli.sh`).
pub fn bundled_rex() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let rex = exe
        .parent()
        .ok_or_else(|| Error::Other("app binary has no parent directory".into()))?
        .join(sidecar_file_name(std::env::consts::EXE_SUFFIX));
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
    /// A copy install only (Windows): whether its folder is on the user's own `Path`. `None` for a symlink
    /// install, whose place on PATH is the folder's convention (W8 S5, ledger #634).
    pub on_path: Option<bool>,
}

pub fn status(platform: &dyn Platform) -> Result<CliStatus> {
    match platform.paths().cli_install()? {
        CliInstall::Symlink(link) => Ok(status_from(&link, bundled_rex().ok())),
        CliInstall::CopyOnUserPath(dir) => {
            let on_path = platform.shell().user_path_has(&dir).ok();
            Ok(status_of_copy(&copy_in(&dir), bundled_rex().ok(), on_path))
        }
    }
}

/// Where a copy install keeps `rex` inside its folder.
fn copy_in(dir: &Path) -> PathBuf {
    dir.join(sidecar_file_name(std::env::consts::EXE_SUFFIX))
}

/// The card's state for a copy install (W8 S5, ledger #634): installed = the copy exists; current = it is this
/// app's `rex` byte for byte — a copy points nowhere, so its bytes are what "current" can mean.
fn status_of_copy(copy: &Path, bundled: Option<PathBuf>, on_path: Option<bool>) -> CliStatus {
    let installed = copy.is_file();
    let current = installed && bundled.as_deref().is_some_and(|b| same_bytes(b, copy));
    CliStatus {
        available: bundled.is_some(),
        installed,
        current,
        link_path: copy.display().to_string(),
        bundled_path: bundled.map(|p| p.display().to_string()),
        on_path,
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => {
            std::fs::read(a).ok().zip(std::fs::read(b).ok()).is_some_and(|(x, y)| x == y)
        }
        _ => false,
    }
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
        on_path: None,
    }
}

/// Install/refresh the PATH symlink. Unprivileged first; otherwise ONE admin
/// prompt (callers run this off the UI thread, foreground — the prompt rule).
/// Verified after either path: the link must resolve to the bundled rex.
pub fn install(platform: &dyn Platform) -> Result<()> {
    let src = bundled_rex()?;
    let dst = match platform.paths().cli_install()? {
        CliInstall::Symlink(link) => link,
        CliInstall::CopyOnUserPath(dir) => return install_copy(platform, &src, &dir),
    };
    if try_symlink_unprivileged(platform, &src, &dst).is_err() {
        platform.privileges().run_privileged(
            &install_script(&src, &dst)?,
            &crate::platform::traits::PromptReason::new(format!(
                "add the rex command at {}",
                dst.display()
            )),
        )?;
    }
    match std::fs::read_link(&dst) {
        Ok(t) if t == src => Ok(()),
        _ => Err(Error::Other(format!(
            "the CLI symlink at {} was not created",
            dst.display()
        ))),
    }
}

/// The Windows install (plan §5 W8 rulings Q3, Q4, ledger #634): this app's `rex.exe` copied into rexenv's own
/// folder, the folder added to the user's `Path`, both verified. No prompt — the folder and the value are the
/// user's own.
fn install_copy(platform: &dyn Platform, src: &Path, dir: &Path) -> Result<()> {
    let dst = copy_in(dir);
    replace_copy(src, &dst)?;
    platform.shell().add_to_user_path(dir)?;
    if !same_bytes(src, &dst) {
        return Err(Error::Other(format!("the copy at {} is not this app's rex", dst.display())));
    }
    if !platform.shell().user_path_has(dir)? {
        return Err(Error::Other(format!("{} was not added to your Path", dir.display())));
    }
    Ok(())
}

/// `<name>.old` beside `dst` — where a replaced copy goes.
fn aside_path(dst: &Path) -> PathBuf {
    let mut name = dst.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    dst.with_file_name(name)
}

/// Put `src` at `dst`. A running program's file cannot be copied over or deleted on Windows, but it can be
/// renamed (measured on the Dell, 15 Sep 2026) — and a `rex mcp` an agent keeps open is a running `rex.exe`. So
/// a `dst` that differs is renamed aside to `<name>.old` and the new copy goes in; a leftover `.old` from an
/// earlier replace is swept first, best-effort (it stays while its program runs); an identical `dst` is left
/// alone; a failed copy puts the old file back.
fn replace_copy(src: &Path, dst: &Path) -> Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let aside = aside_path(dst);
    let _ = std::fs::remove_file(&aside);
    let mut moved = false;
    if dst.exists() {
        if same_bytes(src, dst) {
            return Ok(());
        }
        std::fs::rename(dst, &aside).map_err(|e| {
            Error::Other(format!(
                "could not move the old {} aside to replace it (is a rex from an earlier update still running?): {e}",
                dst.display()
            ))
        })?;
        moved = true;
    }
    if let Err(e) = std::fs::copy(src, dst) {
        if moved {
            let _ = std::fs::rename(&aside, dst);
        }
        return Err(e.into());
    }
    Ok(())
}

/// What a launch does to a copy install.
#[derive(Debug, PartialEq, Eq)]
enum CopyRefresh {
    Nothing,
    Replace,
    KeepForDevBuild,
}

fn copy_refresh(installed: bool, current: bool, dev_build: bool) -> CopyRefresh {
    if !installed || current {
        CopyRefresh::Nothing
    } else if dev_build {
        CopyRefresh::KeepForDevBuild
    } else {
        CopyRefresh::Replace
    }
}

/// Whether `exe` is a build out of a cargo `target` folder — the binary the next `cargo clean` deletes. Split on
/// both separators, so the rule reads a Windows path on every host.
fn is_dev_build(exe: &Path) -> bool {
    let text = exe.to_string_lossy().to_ascii_lowercase();
    let parts: Vec<&str> = text.split(['\\', '/']).collect();
    parts.windows(2).any(|w| {
        w[0] == "target" && ["debug", "release", "xwin", "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"].contains(&w[1])
    })
}

/// At launch, a copy install (Windows, ruling Q3, ledger #634) is brought up to this app's `rex` — never made
/// (Q4: only the Settings button installs), never the user's `Path` touched, and never replaced with a dev
/// build's `rex` (the autostart rule, #623). A leftover `.old` is swept every launch. A symlink install has
/// nothing to refresh: its link follows the bundle.
pub fn refresh_at_launch(platform: &dyn Platform) -> Result<()> {
    let CliInstall::CopyOnUserPath(dir) = platform.paths().cli_install()? else {
        return Ok(());
    };
    let copy = copy_in(&dir);
    let _ = std::fs::remove_file(aside_path(&copy));
    let Ok(bundled) = bundled_rex() else { return Ok(()) };
    let installed = copy.is_file();
    let current = installed && same_bytes(&bundled, &copy);
    match copy_refresh(installed, current, is_dev_build(&std::env::current_exe()?)) {
        CopyRefresh::Nothing => Ok(()),
        CopyRefresh::KeepForDevBuild => {
            log::info!("cli: this launch is a dev build — keeping the installed rex at {}", copy.display());
            Ok(())
        }
        CopyRefresh::Replace => replace_copy(&bundled, &copy),
    }
}

/// Direct symlink attempt — idempotent (an existing link/file is replaced).
///
/// The link is the platform's (`ShellRunner::symlink_file`). It is tried BEFORE
/// anything at `dst` is removed, and removal happens only when the link failed
/// because something is already there: the `cfg(not(unix))` arm this replaced
/// errored without touching the disk, and a remove-then-link order would have
/// deleted whatever sat at `dst` on a platform that then cannot link at all.
fn try_symlink_unprivileged(platform: &dyn Platform, src: &Path, dst: &Path) -> Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match platform.shell().symlink_file(src, dst) {
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(dst)?;
            platform.shell().symlink_file(src, dst)
        }
        other => other,
    }
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
    // A copy install (Windows, ledger #634): the copy, a leftover `.old`, and the folder's `Path` entry — the
    // folder is rexenv's own, so what is in it is ours.
    if let Ok(CliInstall::CopyOnUserPath(dir)) = platform.paths().cli_install() {
        let copy = copy_in(&dir);
        let _ = std::fs::remove_file(&copy);
        let _ = std::fs::remove_file(aside_path(&copy));
        let _ = platform.shell().remove_from_user_path(&dir);
        return;
    }
    let Ok(link) = platform.paths().cli_symlink_path() else { return };
    let Ok(target) = std::fs::read_link(&link) else { return };
    let ours_current = bundled_rex().is_ok_and(|b| b == target);
    let sidecar = sidecar_file_name(std::env::consts::EXE_SUFFIX);
    let dangling_rex = target.file_name().is_some_and(|n| n == std::ffi::OsStr::new(&sidecar)) && !target.exists();
    if ours_current || dangling_rex {
        let _ = std::fs::remove_file(&link);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture directory, EMPTY on every entry.
    ///
    /// The name is keyed on the pid so two concurrent runs cannot collide — but a
    /// pid is *reused*, and this fixture used to `create_dir_all` over whatever
    /// was already there and leave it behind afterwards. So a run that drew a pid
    /// some earlier run had drawn inherited that run's leftovers, and
    /// `status_reads_missing_stale_and_current_links` — whose FIRST assertion is
    /// "there is no link yet" — failed against the `bin/rex` symlink its own
    /// previous incarnation had created. Caught 15 Aug 2026 in `verify-full`, on a
    /// pid whose leftover `bin/rex` was dated four days earlier; 485 of these
    /// directories had accumulated under `$TMPDIR` by then.
    ///
    /// That is the worst shape a gate can have: green on almost every run, red on
    /// no diff, and red *repeatably* for the one machine that drew the wrong pid —
    /// which reads as "the tree is broken" rather than "the fixture is dirty".
    ///
    /// Removing FIRST rather than cleaning up after is deliberate: a cleanup at the
    /// end is skipped by exactly the runs that matter (a panicking test never
    /// reaches it), and leaves the next run to inherit the mess of the failure it
    /// was trying to diagnose. The path is fixture-owned by construction — built
    /// here from `temp_dir()` and our own prefix, never taken from a caller — so
    /// the recursive delete cannot reach anything this function did not make.
    ///
    /// Stated cost: an EMPTY directory per pid still survives a run. That is
    /// litter, not a hazard — the next run to draw the pid empties it on entry —
    /// and it is the price of the ordering above. `$TMPDIR` is periodically swept
    /// by the OS. (The `cli` crate has one of these too: `soft_request`'s deaf
    /// listener leaves `rexenv-cli-test-deaf-<pid>.sock` behind. Same shape, and
    /// it is NOT fixed here — sockets are keyed on a pid that is not reused within
    /// a run, so it has never gone red.)
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// The fixture's own invariant, asserted rather than assumed.
    ///
    /// Every test below opens with `scratch(...)` and then asserts something about
    /// an ABSENCE — no link yet, no stale target. That only means anything if the
    /// directory starts empty, and nothing checked it, so a dirty one produced a
    /// failure that pointed at the code under test instead of at the fixture.
    /// Planting the leftover here is the whole test: it is what a reused pid does.
    #[test]
    fn the_fixture_directory_starts_empty_even_if_a_previous_run_left_it_dirty() {
        let dir = scratch("selfcheck");
        // Exactly what the status test leaves behind: a file and a bin/rex symlink.
        std::fs::write(dir.join("rex"), "stale").unwrap();
        try_symlink_unprivileged(&*crate::platform::current(), &dir.join("rex"), &dir.join("bin").join("rex")).unwrap();
        assert!(dir.join("bin").join("rex").exists(), "plant did not take");

        // A second entry is a second RUN that drew the same pid.
        let again = scratch("selfcheck");
        assert_eq!(again, dir, "same pid + name must name the same directory");
        assert!(
            std::fs::read_dir(&again).unwrap().next().is_none(),
            "scratch handed back a dirty directory — every absence assertion below is vacuous"
        );
    }

    #[test]
    fn symlink_install_is_idempotent_and_replaces_a_stale_link() {
        let dir = scratch("link");
        let src_a = dir.join("rex-a");
        let src_b = dir.join("rex-b");
        std::fs::write(&src_a, "a").unwrap();
        std::fs::write(&src_b, "b").unwrap();
        let dst = dir.join("bin").join("rex");
        try_symlink_unprivileged(&*crate::platform::current(), &src_a, &dst).expect("first install (creates parent)");
        assert_eq!(std::fs::read_link(&dst).unwrap(), src_a);
        try_symlink_unprivileged(&*crate::platform::current(), &src_a, &dst).expect("re-install over itself");
        try_symlink_unprivileged(&*crate::platform::current(), &src_b, &dst).expect("replace a stale link");
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

        try_symlink_unprivileged(&*crate::platform::current(), &dir.join("elsewhere"), &link).unwrap();
        let s = status_from(&link, Some(bundled.clone()));
        assert!(s.installed && !s.current, "stale link: {s:?}");

        try_symlink_unprivileged(&*crate::platform::current(), &bundled, &link).unwrap();
        let s = status_from(&link, Some(bundled));
        assert!(s.installed && s.current, "current link: {s:?}");
    }

    /// Ledger #633 — the sidecar is looked for under this OS's executable name: `rex.exe` beside `rexenv.exe` on
    /// Windows, where a bare `rex` never exists, the card would stay hidden and nothing could be installed.
    #[test]
    fn the_sidecar_is_looked_for_under_this_oss_executable_name() {
        assert_eq!(sidecar_file_name(""), "rex");
        assert_eq!(sidecar_file_name(".exe"), "rex.exe");
        let src = include_str!("cli.rs");
        let prod = src.split("\n#[cfg(test)]").next().unwrap_or(src);
        assert!(!prod.contains(".join(\"rex\")"), "a path is joined with a bare `rex` again");
        for head in ["pub fn bundled_rex()", "pub fn remove_symlink_best_effort("] {
            let body = &prod[prod.find(head).expect(head)..];
            let body = &body[..body.find("\n}\n").expect("the function's end")];
            assert!(
                body.contains("sidecar_file_name(std::env::consts::EXE_SUFFIX)"),
                "{head} no longer asks for this OS's name"
            );
        }
    }

    /// Ledger #634 — replacing a copy renames the old one aside rather than deleting it (a running `rex.exe` can
    /// only be renamed — measured), sweeps a leftover aside file on the next replace, and leaves an identical copy
    /// alone.
    #[test]
    fn a_copy_is_replaced_by_renaming_the_old_one_aside_and_an_identical_one_is_left_alone() {
        let dir = scratch("copy");
        let src = dir.join("new-rex");
        std::fs::write(&src, "new").unwrap();
        let dst = dir.join("bin").join("rex.exe");
        replace_copy(&src, &dst).expect("a first copy makes the folder");
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        assert!(!aside_path(&dst).exists(), "a first copy puts nothing aside");
        std::fs::write(&dst, "old").unwrap();
        replace_copy(&src, &dst).expect("replace an older copy");
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        assert_eq!(std::fs::read(aside_path(&dst)).unwrap(), b"old", "the old copy went aside, not away");
        assert_eq!(aside_path(&dst).file_name().unwrap(), "rex.exe.old");
        replace_copy(&src, &dst).expect("an identical copy");
        assert!(!aside_path(&dst).exists(), "the leftover aside copy is swept on the next replace");
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
    }

    /// Ledger #634 — a copy install reads missing, older, current, and current but off the user's Path.
    #[test]
    fn a_copy_install_reads_missing_older_current_and_off_the_path() {
        let dir = scratch("copystatus");
        let bundled = dir.join("rex.exe");
        std::fs::write(&bundled, "this build").unwrap();
        let copy = dir.join("bin").join("rex.exe");
        let s = status_of_copy(&copy, Some(bundled.clone()), Some(false));
        assert!(s.available && !s.installed && !s.current, "no copy yet: {s:?}");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(&copy, "an older build").unwrap();
        let s = status_of_copy(&copy, Some(bundled.clone()), Some(true));
        assert!(s.installed && !s.current && s.on_path == Some(true), "an older copy: {s:?}");
        std::fs::write(&copy, "this build").unwrap();
        let s = status_of_copy(&copy, Some(bundled.clone()), Some(false));
        assert!(s.installed && s.current && s.on_path == Some(false), "current, but its folder is off the Path: {s:?}");
        let s = status_of_copy(&copy, None, Some(true));
        assert!(!s.available && !s.current, "no sidecar, nothing to compare: {s:?}");
    }

    /// Ledger #634 — a launch brings an installed copy up to date, never installs one, never adds to the user's
    /// Path, and never replaces the copy with a dev build's `rex`.
    #[test]
    fn a_launch_updates_an_installed_copy_but_never_installs_one_or_serves_a_dev_build() {
        assert_eq!(copy_refresh(false, false, false), CopyRefresh::Nothing, "not installed: a launch never installs (Q4)");
        assert_eq!(copy_refresh(true, true, false), CopyRefresh::Nothing);
        assert_eq!(copy_refresh(true, false, false), CopyRefresh::Replace);
        assert_eq!(copy_refresh(true, false, true), CopyRefresh::KeepForDevBuild);
        assert!(is_dev_build(Path::new(r"C:\code\rexenv\src-tauri\target\debug\rexenv.exe")));
        assert!(is_dev_build(Path::new(r"C:\code\rexenv\src-tauri\TARGET\xwin\x86_64-pc-windows-msvc\debug\rexenv.exe")));
        assert!(is_dev_build(Path::new("/Users/a/rexenv/src-tauri/target/release/rexenv")));
        assert!(!is_dev_build(Path::new(r"C:\Users\A B\AppData\Local\Programs\rexenv\rexenv.exe")));
        assert!(!is_dev_build(Path::new(r"C:\target\rexenv.exe")), "a folder named target alone is not a cargo build");
        let src = include_str!("cli.rs");
        let prod = src.split("\n#[cfg(test)]").next().unwrap_or(src);
        let refresh = &prod[prod.find("pub fn refresh_at_launch(").expect("the launch refresh")..];
        let refresh = &refresh[..refresh.find("\n}\n").expect("its end")];
        assert!(!refresh.contains("add_to_user_path") && !refresh.contains("install_copy"), "a launch installed rex unasked");
        assert!(refresh.contains("is_dev_build("), "the launch refresh no longer asks whether this is a dev build");
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
