//! Lock files a server left behind — `postmaster.pid`, `mysql.sock.lock` — and the one
//! question that decides whether a start may remove them: **is the pid inside still THAT
//! server?**
//!
//! Why this exists (28 Sep 2026, the Ubuntu 22.04 VM): a reboot ended MySQL and PostgreSQL
//! with SIGKILL after systemd's stop timeout, so both left their lock files with the old
//! pids. After the boot those pids belonged to WebKit threads of rexenv's own window
//! (`ReceiveQueue`, `EventDispatcher`). PostgreSQL saw a live pid in `postmaster.pid` and
//! refused with "lock file already exists — is another postmaster (PID 2165) running?";
//! mysqld saw the same in `mysql.sock.lock` and aborted with "Unable to setup unix socket
//! lock file". The health watchdog respawned each three times and gave up; every site was
//! down until a hand removed two files. Low pids come back after every boot on Linux and
//! macOS alike, so the servers' own liveness checks — "does this pid exist?" — are wrong
//! exactly when a reboot was unclean.
//!
//! The rule is stricter than the servers' own: the pid must be alive AND its command line
//! must name what the lock guards (the socket path, the datadir). A live pid of some other
//! program is stale. A live pid that IS the server is left alone — the start then fails the
//! honest way, on a server that really is running.

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::Path;

/// What a lock file's pid turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub enum LockVerdict {
    /// A live process whose command line names `expect` — the server itself. Keep the file.
    Live,
    /// Nothing the lock can mean any more, with the reason a log line shows.
    Stale(String),
}

/// The pure decision: `pid` from the file's first line, `command` from the OS (None = no
/// such process), `expect` = what the server's command line must mention (the socket path
/// for mysqld, the datadir for postgres).
pub fn verdict(pid: Option<u32>, command: Option<&str>, expect: &str) -> LockVerdict {
    let Some(pid) = pid else {
        return LockVerdict::Stale("it carries no pid".into());
    };
    match command {
        None => LockVerdict::Stale(format!("pid {pid} is gone")),
        Some(cmd) if cmd.contains(expect) => LockVerdict::Live,
        Some(cmd) => LockVerdict::Stale(format!(
            "pid {pid} is now `{}`, not the server that wrote it",
            cmd.split_whitespace().next().unwrap_or(cmd)
        )),
    }
}

/// The pid on a lock file's first line, if the file exists and starts with one.
pub fn pid_in(path: &Path) -> Option<Option<u32>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().next().and_then(|l| l.trim().parse::<u32>().ok()))
}

/// Remove `lock` (and `also`, the socket a lock guards) when its pid is not a live process
/// whose command line mentions `expect`. Returns the reason when something was removed,
/// `None` when the file is absent or names a live server. Never an error for a file that
/// cannot be read: an unreadable lock is the server's problem to report.
pub fn clear_if_stale(platform: &dyn Platform, lock: &Path, expect: &str, also: &[&Path]) -> Result<Option<String>> {
    let Some(pid) = pid_in(lock) else {
        return Ok(None);
    };
    let command = pid.and_then(|p| platform.supervisor().pid_command(p));
    match verdict(pid, command.as_deref(), expect) {
        LockVerdict::Live => Ok(None),
        LockVerdict::Stale(why) => {
            std::fs::remove_file(lock)?;
            for extra in also {
                let _ = std::fs::remove_file(extra);
            }
            Ok(Some(format!("{}: {why}", lock.display())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pid_that_is_gone_or_belongs_to_another_program_is_stale_and_the_server_itself_is_live() {
        assert_eq!(verdict(None, None, "/x/mysql.sock"), LockVerdict::Stale("it carries no pid".into()));
        assert_eq!(verdict(Some(2163), None, "/x/mysql.sock"), LockVerdict::Stale("pid 2163 is gone".into()));
        // The VM's case: the pid came back as a WebKit thread of rexenv's own window.
        assert_eq!(
            verdict(Some(2163), Some("/usr/lib/aarch64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess 4 26 28"), "/x/mysql.sock"),
            LockVerdict::Stale("pid 2163 is now `/usr/lib/aarch64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess`, not the server that wrote it".into())
        );
        assert_eq!(verdict(Some(2163), Some("/x/bin/mysqld --socket=/x/mysql.sock --port=13306"), "/x/mysql.sock"), LockVerdict::Live);
        assert_eq!(verdict(Some(2165), Some("/x/bin/postgres -D /home/u/.local/share/rexenv/postgres/data -p 15432"), "/home/u/.local/share/rexenv/postgres/data"), LockVerdict::Live);
    }

    #[test]
    fn clear_removes_a_stale_lock_and_its_socket_and_leaves_a_live_one_alone() {
        let platform = crate::platform::current();
        let dir = std::env::temp_dir().join(format!("rexenv-stale-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("mysql.sock.lock");
        let sock = dir.join("mysql.sock");
        // A pid no OS hands out: gone → removed, and the socket with it.
        std::fs::write(&lock, "4000000\n").unwrap();
        std::fs::write(&sock, "").unwrap();
        let why = clear_if_stale(&*platform, &lock, "/nowhere", &[&sock]).unwrap().unwrap();
        assert!(why.contains("pid 4000000 is gone"), "{why}");
        assert!(!lock.exists() && !sock.exists());
        // This very process, and an `expect` every command line contains: live → kept.
        std::fs::write(&lock, format!("{}\n", std::process::id())).unwrap();
        assert_eq!(clear_if_stale(&*platform, &lock, "", &[]).unwrap(), None);
        assert!(lock.exists());
        // No file at all: nothing to say.
        std::fs::remove_file(&lock).unwrap();
        assert_eq!(clear_if_stale(&*platform, &lock, "", &[]).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
