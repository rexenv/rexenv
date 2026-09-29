//! Writes that must survive a power cut — the files the OS reads at the NEXT boot or login.
//!
//! Why this exists (29 Sep 2026, the Ubuntu 22.04 VM, 0.8.10): Start all wrote
//! `/etc/systemd/system/rexenv-edge.service`, the VM lost power ~30 s later, and after the boot
//! the unit was ZERO bytes — ext4's delayed allocation had never flushed its data, while the
//! `multi-user.target.wants` symlink (metadata, journaled) survived. systemd reads an empty unit
//! as masked, so the edge never started and every login said "the HTTPS edge needs Start all".
//! A written file is in the page cache until something flushes it; the only writes that reach the
//! disk in time are the ones that ask.
//!
//! Two shapes, one module, shared by the two OSes that write such files themselves:
//! - [`flushed_script`] — every privileged step on macOS and Linux (osascript / pkexec run a POSIX
//!   shell) ends with `/bin/sync`, whatever the script wrote and whether or not it succeeded, so a
//!   root write — a unit, a LaunchDaemon plist, a resolver file, a CA — is on disk before the
//!   prompt returns. One wrapper at the one door, not a `sync` remembered in every script.
//! - [`write_durable`] — the boot-time files the app writes as the user (a LaunchAgent plist, a
//!   systemd user unit, an XDG autostart entry): temp file, fsync, rename over, fsync the folder.
//!   A crash leaves the old file or the new one, never an empty one.
//!
//! Windows needs neither: its privileged step changes the registry, NRPT and the certificate
//! store through their own APIs, and the DNS agent is a Task Scheduler registration — all kept
//! durable by the OS. The task's XML input file is read once by `schtasks /Create` and never at boot.
#![cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]

use std::io::Write;
use std::path::Path;

/// `script` as it is handed to the privileged shell: run in a subshell, then `/bin/sync`, then
/// the script's own exit status. The newline before `)` keeps a script that ends in a comment
/// from swallowing the paren. `/bin/sync` is absolute because `pkexec` strips `PATH`.
pub(crate) fn flushed_script(script: &str) -> String {
    format!("( {script}\n) ; rexenv_rc=$? ; /bin/sync ; exit $rexenv_rc")
}

/// Write `bytes` to `path` so that a power cut leaves the old content or the new, never neither:
/// a sibling temp file, `fsync`, `rename` over `path`, then `fsync` of the folder so the rename
/// itself is on disk.
pub(crate) fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other(format!("{} names no file", path.display())))?;
    let tmp = path.with_file_name(format!(".{}.rexenv-tmp", name.to_string_lossy()));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        std::fs::File::open(if dir.as_os_str().is_empty() { Path::new(".") } else { dir })?.sync_all()?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(script: &str) -> std::process::Output {
        std::process::Command::new("/bin/sh").arg("-c").arg(flushed_script(script)).output().expect("/bin/sh")
    }

    /// The wrapper changes nothing a caller reads — stdout and the exit status are the script's —
    /// and the flush runs on success AND failure (a failed batch may have written half its files).
    #[test]
    fn a_flushed_script_keeps_its_output_and_status_and_always_syncs() {
        let ok = run("/bin/echo one && /bin/echo two");
        assert_eq!(String::from_utf8_lossy(&ok.stdout), "one\ntwo\n");
        assert_eq!(ok.status.code(), Some(0));
        assert_eq!(run("exit 3").status.code(), Some(3), "an `exit` inside ends the subshell, not the flush");
        assert_eq!(run("[ 1 = 2 ] && /bin/echo never").status.code(), Some(1));
        assert_eq!(run("/bin/echo last # a trailing comment").status.code(), Some(0));
        let wrapped = flushed_script("exit 3");
        assert!(wrapped.contains("; /bin/sync ;"), "{wrapped}");
        assert!(wrapped.find("/bin/sync").unwrap() > wrapped.find("exit 3").unwrap(), "sync runs after the script");
    }

    /// **Both privileged doors flush, and no boot-time file goes back to a bare write** (ledger
    /// #740). TEXT, not behaviour (the #175 bound): `run_privileged` on macOS and Linux shadows its
    /// `script` with [`flushed_script`] before anything uses it, and the five files the app writes
    /// for the next boot or login — the macOS login item and DNS agent plists and the DNS agent's
    /// launcher script (#764: a 0-byte launcher would leave KeepAlive respawning a job that runs
    /// nothing), the Linux autostart entry and DNS user unit — are written with [`write_durable`].
    /// What a power cut does to them is a VM run (SMOKE, Linux).
    #[test]
    fn every_privileged_door_flushes_and_no_boot_file_is_a_bare_write() {
        for (os, src) in [("macOS", include_str!("macos/mod.rs")), ("Linux", include_str!("linux/mod.rs"))] {
            let src = crate::core::copy_scan::production_source(src);
            let start = src.find("fn run_privileged(").unwrap_or_else(|| panic!("{os}: run_privileged is gone"));
            let body = &src[start..];
            let wrap = body.find("let script = &crate::platform::durable::flushed_script(script);");
            let first_use = body.find("(script").or_else(|| body.find(", script"));
            assert!(wrap.is_some(), "{os}: run_privileged no longer flushes what the step wrote");
            assert!(wrap < first_use, "{os}: the script is used before it is wrapped");
        }
        let linux = crate::core::copy_scan::production_source(include_str!("linux/mod.rs"));
        let macos = crate::core::copy_scan::production_source(include_str!("macos/mod.rs"));
        for bare in ["std::fs::write(&entry", "std::fs::write(&unit"] {
            assert!(!linux.contains(bare), "Linux writes a boot file bare again: {bare}");
        }
        assert!(!macos.contains("std::fs::write(&plist"), "macOS writes a launchd plist bare again");
        assert_eq!(linux.matches("durable::write_durable(").count(), 2, "Linux: the autostart entry and the DNS unit");
        assert_eq!(macos.matches("durable::write_durable(").count(), 3, "macOS: the login item plist, the DNS agent plist and its launcher");
    }

    /// The file ends up with exactly the new bytes, an existing file is replaced, and no temp file
    /// is left beside it.
    #[test]
    fn a_durable_write_replaces_the_file_and_leaves_no_temp_behind() {
        let dir = std::env::temp_dir().join(format!("rexenv-durable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rexenv-dns.service");
        write_durable(&f, b"first\n").unwrap();
        write_durable(&f, b"second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "second\n");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("rexenv-dns.service")], "a temp file was left: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
